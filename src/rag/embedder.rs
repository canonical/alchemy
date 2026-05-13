use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait Embedder: Send + Sync {
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;
    fn dimensions(&self) -> usize;
}

/// No-op embedder for when no real embedding provider is configured
pub struct NoopEmbedder;

#[async_trait]
impl Embedder for NoopEmbedder {
    async fn embed(&self, _text: &str) -> Result<Vec<f32>> {
        Ok(vec![0.0; 768])
    }
    fn dimensions(&self) -> usize { 768 }
}

/// Provider-backed embedder
pub struct ProviderEmbedder {
    provider: Box<dyn crate::providers::Provider>,
}

impl ProviderEmbedder {
    pub fn new(provider: Box<dyn crate::providers::Provider>) -> Self {
        Self { provider }
    }
}

#[async_trait]
impl Embedder for ProviderEmbedder {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let results = self.provider.embed(&[text.to_string()]).await?;
        results.into_iter().next().ok_or_else(|| anyhow::anyhow!("No embedding returned"))
    }
    fn dimensions(&self) -> usize {
        self.provider.embed_dimensions()
    }
}
