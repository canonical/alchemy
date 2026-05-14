pub mod chunker;
pub mod embedder;
pub mod store;
pub mod retriever;

use anyhow::Result;
use std::path::Path;

pub struct RagPipeline {
    pub chunker: chunker::Chunker,
    pub embedder: Box<dyn embedder::Embedder>,
    pub store: store::VectorStore,
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
    pub store_path: String,
    pub dimensions: usize,
}

impl RagPipeline {
    pub async fn new(config: RagConfig) -> Result<Self> {
        let chunker = chunker::Chunker::new(config.chunk_size, config.chunk_overlap);
        let store = store::VectorStore::new(&config.store_path, config.dimensions).await?;
        let retriever = retriever::Retriever::new(config.top_k);

        // Create embedder based on provider
        let embedder: Box<dyn embedder::Embedder> = match config.embed_provider.as_str() {
            "openai" => Box::new(embedder::OpenAIEmbedder::new(
                config.embed_api_key.unwrap_or_default(),
                config.embed_base_url.clone(),
                config.embed_model.clone(),
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

    pub async fn index_directory(&mut self, path: &Path, glob_pattern: Option<&str>) -> Result<usize> {
        let pattern = glob_pattern.unwrap_or("**/*");
        let full_pattern = format!("{}/{}", path.display(), pattern);
        let mut total = 0;

        for entry in glob::glob(&full_pattern)?.flatten() {
            if entry.is_file() {
                match self.index_file(&entry).await {
                    Ok(n) => total += n,
                    Err(e) => tracing::warn!("Failed to index {}: {}", entry.display(), e),
                }
            }
        }

        Ok(total)
    }

    pub async fn search(&self, query: &str) -> Result<Vec<retriever::SearchResult>> {
        let embedding = self.embedder.embed(query).await?;
        self.retriever.search(&self.store, &embedding).await
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
