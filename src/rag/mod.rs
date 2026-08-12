pub mod chunker;
pub mod embedder;
pub mod store;
pub mod retriever;

use anyhow::Result;
use std::path::Path;

pub struct RagPipeline {
    pub chunker: chunker::Chunker,
    pub embedder: Box<dyn embedder::Embedder>,
    pub store: Box<dyn store::VectorStoreBackend>,
    pub retriever: retriever::Retriever,
}

pub struct RagConfig {
    pub embed_provider: String,
    pub embed_model: Option<String>,
    pub embed_api_key: Option<String>,
    pub embed_base_url: Option<String>,
    pub chunk_size: usize,
    pub chunk_overlap: usize,
    pub top_k: usize,
    // SQLite
    pub store_path: String,
    pub dimensions: usize,
    // External store backend: "sqlite" (default), "qdrant", "chroma"
    pub store_backend: String,
    pub store_url: Option<String>,
    pub store_api_key: Option<String>,
    pub store_collection: Option<String>,
}

impl RagPipeline {
    pub async fn new(config: RagConfig) -> Result<Self> {
        let chunker = chunker::Chunker::new(config.chunk_size, config.chunk_overlap);
        let retriever = retriever::Retriever::new(config.top_k);

        let collection = config.store_collection
            .clone()
            .unwrap_or_else(|| "alchemy".to_string());

        let store: Box<dyn store::VectorStoreBackend> = match config.store_backend.as_str() {
            "qdrant" => {
                let url = config.store_url.clone()
                    .ok_or_else(|| anyhow::anyhow!(
                        "ALCHEMY_RAG_STORE_URL is required for Qdrant backend"
                    ))?;
                Box::new(store::QdrantStore::new(
                    url,
                    config.store_api_key.clone(),
                    collection,
                    config.dimensions,
                ).await?)
            }
            "chroma" => {
                let url = config.store_url.clone()
                    .ok_or_else(|| anyhow::anyhow!(
                        "ALCHEMY_RAG_STORE_URL is required for Chroma backend"
                    ))?;
                Box::new(store::ChromaStore::new(
                    url,
                    config.store_api_key.clone(),
                    collection,
                    config.dimensions,
                ).await?)
            }
            _ => Box::new(store::SqliteStore::new(&config.store_path, config.dimensions).await?),
        };

        // Create embedder based on provider
        let embedder: Box<dyn embedder::Embedder> = match config.embed_provider.as_str() {
            "openai" => Box::new(embedder::OpenAIEmbedder::new(
                config.embed_api_key.unwrap_or_default(),
                config.embed_base_url.clone(),
                config.embed_model.clone(),
            )),
            "github-copilot" => Box::new(embedder::CopilotEmbedder::new(
                config.embed_api_key.unwrap_or_default(),
                config.embed_base_url.clone(),
                config.embed_model.clone(),
            )),
            "openrouter" => Box::new(embedder::OpenAIEmbedder::new(
                config.embed_api_key.unwrap_or_default(),
                Some(config.embed_base_url.clone()
                    .unwrap_or_else(|| "https://openrouter.ai/api/v1".to_string())),
                // OpenRouter requires namespaced model IDs, so the generic
                // OpenAIEmbedder fallback ("text-embedding-3-small") is invalid here.
                Some(config.embed_model.clone()
                    .unwrap_or_else(|| "openai/text-embedding-3-small".to_string())),
            )),
            "gemini" => Box::new(embedder::GeminiEmbedder::new(
                config.embed_api_key.unwrap_or_default(),
                config.embed_base_url.clone(),
                config.embed_model.clone(),
            )),
            "ollama" => Box::new(embedder::OllamaEmbedder::new(
                config.embed_base_url.clone(),
                config.embed_model.clone(),
            )),
            _ => Box::new(embedder::NoopEmbedder),
        };

        Ok(Self { chunker, embedder, store, retriever })
    }

    pub async fn index_file(&mut self, path: &Path) -> Result<usize> {
        let content = tokio::fs::read_to_string(path).await?;
        let chunks = self.chunker.chunk(&content, path);
        let mut count = 0;

        for chunk in &chunks {
            let embedding = self.embedder.embed(&chunk.content).await?;
            self.store.insert(&chunk.content, &embedding, path.to_str().unwrap_or(""), chunk.index).await?;
            count += 1;
        }

        Ok(count)
    }

    pub async fn search(&self, query: &str) -> Result<Vec<retriever::SearchResult>> {
        let embedding = self.embedder.embed(query).await?;
        self.retriever.search(self.store.as_ref(), &embedding).await
    }

    pub async fn status(&self) -> Result<store::StoreStatus> {
        self.store.status().await
    }

    pub async fn clear(&mut self) -> Result<()> {
        self.store.clear().await
    }

    pub async fn build_context(&self, query: &str) -> Result<String> {
        let results = self.search(query).await?;
        if results.is_empty() {
            return Ok(String::new());
        }
        let mut context = String::from("Relevant documents:\n");
        for r in &results {
            context.push_str(&format!("\n[{}]\n{}\n", r.source, r.content));
        }
        Ok(context)
    }
}
