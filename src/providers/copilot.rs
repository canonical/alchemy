use anyhow::Result;
use async_trait::async_trait;
use reqwest::Client;
use std::sync::Mutex;
use crate::types::{LlmRequest, LlmResponse};
use crate::providers::{openai::OpenAiProvider, Provider};

/// GitHub Copilot provider: exchanges PAT for a short-lived session token.
/// The token response also carries the actual API endpoint via `endpoints.api`.
pub struct CopilotProvider {
    pat: String,
    base_url: Option<String>,
    client: Client,
    /// Cached (token, api_endpoint, expires_at_unix_secs)
    token_cache: Mutex<Option<(String, String, u64)>>,
}

impl CopilotProvider {
    pub fn new(pat: String, base_url: Option<String>) -> Self {
        Self {
            pat,
            base_url,
            client: crate::http::new_client(),
            token_cache: Mutex::new(None),
        }
    }

    /// Returns (session_token, api_base_url), refreshing when within 60s of expiry.
    async fn ensure_token(&self) -> Result<(String, String)> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        if let Some((token, endpoint, expires_at)) = self.token_cache.lock().unwrap().clone() {
            if now < expires_at.saturating_sub(60) {
                return Ok((token, endpoint));
            }
        }

        tracing::debug!("refreshing GitHub Copilot session token");
        let resp = self
            .client
            .get("https://api.github.com/copilot_internal/v2/token")
            .header("Authorization", format!("token {}", self.pat))
            .header("User-Agent", "alchemy/0.1.0")
            .header("editor-version", "vscode/1.96.0")
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Copilot token refresh failed: {}", &body[..body.len().min(200)]);
        }

        let v: serde_json::Value = serde_json::from_str(&resp.text().await?)?;

        let token = v["token"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("no token in Copilot response"))?
            .to_string();

        let expires_at = v["expires_at"].as_u64().unwrap_or(0);

        // Use the endpoint the server tells us to use; fall back to the known default.
        let endpoint = self.base_url.clone().unwrap_or_else(|| {
            v.get("endpoints")
                .and_then(|e| e.get("api"))
                .and_then(|a| a.as_str())
                .unwrap_or("https://api.githubcopilot.com")
                .to_string()
        });

        tracing::info!(
            endpoint = %endpoint,
            expires_in = expires_at.saturating_sub(now),
            "Copilot token refreshed"
        );

        *self.token_cache.lock().unwrap() = Some((token.clone(), endpoint.clone(), expires_at));

        Ok((token, endpoint))
    }
}

#[async_trait]
impl Provider for CopilotProvider {
    fn name(&self) -> &str {
        "github-copilot"
    }

    fn default_model(&self) -> &str {
        "gpt-5-mini"
    }

    async fn chat_streaming(
        &self,
        request: LlmRequest,
        tx: tokio::sync::mpsc::Sender<String>,
    ) -> Result<LlmResponse> {
        let (token, endpoint) = self.ensure_token().await?;
        OpenAiProvider::new(token, endpoint, "github-copilot".to_string())
            .chat_streaming(request, tx)
            .await
    }
}
