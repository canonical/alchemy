pub mod openai;
pub mod copilot;
pub mod gemini;
pub mod anthropic;

use anyhow::Result;
use async_trait::async_trait;
use crate::types::{LlmRequest, LlmResponse};

#[async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;
    async fn chat(&self, request: LlmRequest) -> Result<LlmResponse>;
    async fn chat_streaming(
        &self,
        request: LlmRequest,
        tx: tokio::sync::mpsc::Sender<String>,
    ) -> Result<LlmResponse>;
    async fn embed(&self, _texts: &[String]) -> Result<Vec<Vec<f32>>> {
        anyhow::bail!("Embeddings not supported by provider {}", self.name())
    }
    fn default_model(&self) -> &str;
    fn default_embed_model(&self) -> &str {
        ""
    }
    fn embed_dimensions(&self) -> usize {
        0
    }
}

pub fn create_provider(
    provider_name: &str,
    api_key: Option<&str>,
    base_url: Option<&str>,
) -> Result<Box<dyn Provider>> {
    match provider_name {
        "openai" => {
            let key = api_key.ok_or_else(|| anyhow::anyhow!("ALCHEMY_API_KEY required for openai"))?;
            Ok(Box::new(openai::OpenAiProvider::new(
                key.to_string(),
                base_url.unwrap_or("https://api.openai.com/v1").to_string(),
                "openai".to_string(),
            )))
        }
        "openrouter" => {
            let key = api_key.ok_or_else(|| anyhow::anyhow!("ALCHEMY_API_KEY required for openrouter"))?;
            Ok(Box::new(openai::OpenAiProvider::new(
                key.to_string(),
                base_url.unwrap_or("https://openrouter.ai/api/v1").to_string(),
                "openrouter".to_string(),
            )))
        }
        "ollama" => {
            Ok(Box::new(openai::OpenAiProvider::new(
                api_key.unwrap_or("").to_string(),
                base_url.unwrap_or("http://localhost:11434/v1").to_string(),
                "ollama".to_string(),
            )))
        }
        "github-copilot" => {
            let key = api_key.ok_or_else(|| anyhow::anyhow!("ALCHEMY_API_KEY required for github-copilot"))?;
            Ok(Box::new(copilot::CopilotProvider::new(
                key.to_string(),
                base_url.map(|s| s.to_string()),
            )))
        }
        "gemini" => {
            let key = api_key.ok_or_else(|| anyhow::anyhow!("ALCHEMY_API_KEY required for gemini"))?;
            Ok(Box::new(gemini::GeminiProvider::new(
                key.to_string(),
                base_url.map(|s| s.to_string()),
            )))
        }
        "anthropic" => {
            let key = api_key.ok_or_else(|| anyhow::anyhow!("ALCHEMY_API_KEY required for anthropic"))?;
            Ok(Box::new(anthropic::AnthropicProvider::new(
                key.to_string(),
                base_url.map(|s| s.to_string()),
            )))
        }
        _ => anyhow::bail!("Unknown provider: {}", provider_name),
    }
}
