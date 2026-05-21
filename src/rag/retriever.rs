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
        // Retrieve a larger candidate set, then apply MMR to diversify results.
        let candidate_k = (self.top_k * 3).max(10);
        let scored = store.search(query_embedding, candidate_k).await?;
        let mut candidates = Vec::new();

        for (id, score) in scored {
            let (content, source) = store.get_chunk(id).await?;
            candidates.push(SearchResult { content, source, score });
        }

        Ok(mmr_rerank(candidates, self.top_k))
    }
}

/// Maximal Marginal Relevance reranking.
///
/// Iteratively selects the candidate that maximises:
///   `λ * similarity(c, query) - (1 - λ) * max_similarity(c, selected)`
///
/// `λ = 0.7` balances relevance against diversity. Per-chunk embeddings are not
/// stored on disk, so similarity between candidates is approximated by how close
/// their relevance scores are (chunks with very similar scores are treated as
/// near-duplicates).
fn mmr_rerank(mut candidates: Vec<SearchResult>, top_k: usize) -> Vec<SearchResult> {
    if candidates.is_empty() || top_k == 0 {
        return candidates;
    }

    const LAMBDA: f32 = 0.7;

    let n = candidates.len();
    let scores: Vec<f32> = candidates.iter().map(|c| c.score).collect();

    let mut selected_indices: Vec<usize> = Vec::with_capacity(top_k);
    let mut remaining: Vec<usize> = (0..n).collect();

    while selected_indices.len() < top_k && !remaining.is_empty() {
        let best = remaining.iter().copied().max_by(|&a, &b| {
            let max_sim_a = selected_indices.iter()
                .map(|&s| 1.0_f32 - (scores[a] - scores[s]).abs())
                .fold(0.0_f32, f32::max);
            let mmr_a = LAMBDA * scores[a] - (1.0 - LAMBDA) * max_sim_a;

            let max_sim_b = selected_indices.iter()
                .map(|&s| 1.0_f32 - (scores[b] - scores[s]).abs())
                .fold(0.0_f32, f32::max);
            let mmr_b = LAMBDA * scores[b] - (1.0 - LAMBDA) * max_sim_b;

            mmr_a.partial_cmp(&mmr_b).unwrap_or(std::cmp::Ordering::Equal)
        });

        if let Some(idx) = best {
            selected_indices.push(idx);
            remaining.retain(|&r| r != idx);
        } else {
            break;
        }
    }

    // Collect results in selected order.
    let mut result = Vec::with_capacity(selected_indices.len());
    for &idx in &selected_indices {
        result.push(std::mem::replace(
            &mut candidates[idx],
            SearchResult { content: String::new(), source: String::new(), score: 0.0 },
        ));
    }
    result
}
