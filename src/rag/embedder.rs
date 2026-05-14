use anyhow::{bail, Result};
use async_trait::async_trait;

#[async_trait]
pub trait Embedder: Send + Sync {
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;
}

/// No-op embedder for when no real embedding provider is configured.
pub struct NoopEmbedder;

#[async_trait]
impl Embedder for NoopEmbedder {
    async fn embed(&self, _text: &str) -> Result<Vec<f32>> {
        Ok(vec![0.0; 768])
    }
}

/// Embedder backed by the OpenAI embeddings API (or any compatible endpoint).
pub struct OpenAIEmbedder {
    api_key: String,
    base_url: String,
    model: String,
    client: reqwest::Client,
}

impl OpenAIEmbedder {
    pub fn new(api_key: String, base_url: Option<String>, model: Option<String>) -> Self {
        Self {
            api_key,
            base_url: base_url.unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
            model: model.unwrap_or_else(|| "text-embedding-3-small".to_string()),
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl Embedder for OpenAIEmbedder {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let url = format!("{}/embeddings", self.base_url);
        let body = serde_json::json!({
            "model": self.model,
            "input": [text],
        });
        let response = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("OpenAI embeddings request failed: {} — {}", status, body);
        }
        let data: serde_json::Value = response.json().await?;
        let embedding = data["data"][0]["embedding"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("missing embedding in OpenAI response"))?
            .iter()
            .map(|v| v.as_f64().unwrap_or(0.0) as f32)
            .collect();
        Ok(embedding)
    }
}

/// Embedder backed by the Google Gemini embedContent API.
pub struct GeminiEmbedder {
    api_key: String,
    base_url: String,
    model: String,
    client: reqwest::Client,
}

impl GeminiEmbedder {
    pub fn new(api_key: String, base_url: Option<String>, model: Option<String>) -> Self {
        Self {
            api_key,
            base_url: base_url.unwrap_or_else(|| {
                "https://generativelanguage.googleapis.com/v1beta".to_string()
            }),
            model: model.unwrap_or_else(|| "text-embedding-004".to_string()),
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl Embedder for GeminiEmbedder {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let url = format!(
            "{}/models/{}:embedContent?key={}",
            self.base_url, self.model, self.api_key
        );
        let body = serde_json::json!({
            "content": {
                "parts": [{"text": text}]
            }
        });
        let response = self.client.post(&url).json(&body).send().await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("Gemini embedContent request failed: {} — {}", status, body);
        }
        let data: serde_json::Value = response.json().await?;
        let embedding = data["embedding"]["values"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("missing embedding in Gemini response"))?
            .iter()
            .map(|v| v.as_f64().unwrap_or(0.0) as f32)
            .collect();
        Ok(embedding)
    }
}

/// Embedder backed by a local Ollama instance.
pub struct OllamaEmbedder {
    base_url: String,
    model: String,
    client: reqwest::Client,
}

impl OllamaEmbedder {
    pub fn new(base_url: Option<String>, model: Option<String>) -> Self {
        Self {
            base_url: base_url.unwrap_or_else(|| "http://localhost:11434".to_string()),
            model: model.unwrap_or_else(|| "nomic-embed-text".to_string()),
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl Embedder for OllamaEmbedder {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let url = format!("{}/api/embed", self.base_url);
        let body = serde_json::json!({
            "model": self.model,
            "input": [text],
        });
        let response = self.client.post(&url).json(&body).send().await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("Ollama embed request failed: {} — {}", status, body);
        }
        let data: serde_json::Value = response.json().await?;
        let embedding = data["embeddings"][0]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("missing embedding in Ollama response"))?
            .iter()
            .map(|v| v.as_f64().unwrap_or(0.0) as f32)
            .collect();
        Ok(embedding)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_embedder_constructs() {
        let _ = OpenAIEmbedder::new("key123".to_string(), None, None);
        let _ = OpenAIEmbedder::new(
            "k".to_string(),
            Some("http://my-proxy/v1".to_string()),
            Some("my-model".to_string()),
        );
    }

    #[test]
    fn gemini_embedder_constructs() {
        let _ = GeminiEmbedder::new("gemini-key".to_string(), None, None);
        let _ = GeminiEmbedder::new(
            "k".to_string(),
            Some("http://fake/v1beta".to_string()),
            Some("my-gemini-model".to_string()),
        );
    }

    #[test]
    fn ollama_embedder_constructs() {
        let _ = OllamaEmbedder::new(None, None);
        let _ = OllamaEmbedder::new(
            Some("http://ollama-server:11434".to_string()),
            Some("mxbai-embed-large".to_string()),
        );
    }

    #[test]
    fn noop_embedder_returns_zeros() {
        let e = NoopEmbedder;
        // Verify the embed call is wired up correctly (sync check on the type)
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(e.embed("hello"));
        let v = result.unwrap();
        assert_eq!(v.len(), 768);
        assert!(v.iter().all(|&x| x == 0.0));
    }
}
