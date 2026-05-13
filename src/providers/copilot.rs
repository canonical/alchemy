use anyhow::Result;
use async_trait::async_trait;
use reqwest::Client;
use std::sync::RwLock;
use crate::types::{LlmRequest, LlmResponse};
use crate::providers::Provider;
use crate::providers::openai::OpenAiProvider;

/// GitHub Copilot provider: refreshes session token from PAT
pub struct CopilotProvider {
    pat: String,
    base_url: Option<String>,
    inner: RwLock<Option<OpenAiProvider>>,
    session_token: RwLock<Option<String>>,
    client: Client,
}

impl CopilotProvider {
    pub fn new(pat: String, base_url: Option<String>) -> Self {
        Self {
            pat,
            base_url,
            inner: RwLock::new(None),
            session_token: RwLock::new(None),
            client: Client::new(),
        }
    }

    async fn ensure_token(&self) -> Result<String> {
        // Check if we have a cached token
        if let Some(ref token) = *self.session_token.read().unwrap() {
            return Ok(token.clone());
        }

        // Get session token from GitHub Copilot API
        let resp = self
            .client
            .get("https://api.github.com/copilot_internal/v2/token")
            .header("Authorization", format!("token {}", self.pat))
            .header("User-Agent", "alchemy/0.1.0")
            .send()
            .await?;

        let status = resp.status();
        let text = resp.text().await?;

        if !status.is_success() {
            anyhow::bail!("Copilot token refresh failed {}: {}", status, text);
        }

        let data: serde_json::Value = serde_json::from_str(&text)?;
        let token = data["token"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("No token in Copilot response"))?
            .to_string();

        *self.session_token.write().unwrap() = Some(token.clone());

        Ok(token)
    }

    async fn get_inner(&self) -> Result<OpenAiProvider> {
        let token = self.ensure_token().await?;
        let base = self
            .base_url
            .clone()
            .unwrap_or_else(|| "https://api.githubcopilot.com".to_string());
        Ok(OpenAiProvider::new(token, base, "github-copilot".to_string()))
    }
}

#[async_trait]
impl Provider for CopilotProvider {
    fn name(&self) -> &str {
        "github-copilot"
    }

    fn default_model(&self) -> &str {
        "gpt-4.1"
    }

    async fn chat(&self, request: LlmRequest) -> Result<LlmResponse> {
        let inner = self.get_inner().await?;
        inner.chat(request).await
    }

    async fn chat_streaming(
        &self,
        request: LlmRequest,
        tx: tokio::sync::mpsc::Sender<String>,
    ) -> Result<LlmResponse> {
        let inner = self.get_inner().await?;
        inner.chat_streaming(request, tx).await
    }
}
