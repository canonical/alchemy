use anyhow::Result;
use crate::rag::store::VectorStore;

pub struct Retriever {
    pub top_k: usize,
}

pub struct SearchResult {
    pub content: String,
    pub source: String,
    pub score: f32,
}

impl Retriever {
    pub fn new(top_k: usize) -> Self {
        Self { top_k }
    }

    pub async fn search(&self, store: &VectorStore, query_embedding: &[f32]) -> Result<Vec<SearchResult>> {
        let scored = store.search(query_embedding, self.top_k).await?;
        let mut results = Vec::new();

        for (id, score) in scored {
            let (content, source) = store.get_chunk(id).await?;
            results.push(SearchResult { content, source, score });
        }

        Ok(results)
    }
}
