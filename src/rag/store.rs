use anyhow::Result;
use async_trait::async_trait;
use rusqlite::Connection;
use std::sync::{Mutex, Once};

/// A single result from a vector similarity search.
pub struct SearchHit {
    pub content: String,
    pub source: String,
    pub score: f32,
}

pub struct StoreStatus {
    pub total_chunks: usize,
    pub total_sources: usize,
}

/// Common interface for all vector store backends.
#[async_trait]
pub trait VectorStoreBackend: Send + Sync {
    /// Insert a chunk with its embedding.
    async fn insert(
        &self,
        content: &str,
        embedding: &[f32],
        source: &str,
        chunk_index: usize,
    ) -> Result<()>;

    /// Return the top-k most similar chunks to `query_embedding`.
    async fn search(&self, query_embedding: &[f32], top_k: usize) -> Result<Vec<SearchHit>>;

    /// Return aggregate statistics about the store.
    async fn status(&self) -> Result<StoreStatus>;

    /// Delete all stored chunks and embeddings.
    async fn clear(&mut self) -> Result<()>;
}

// ── SQLite backend ────────────────────────────────────────────────────────────

/// Register the sqlite-vec extension for all future rusqlite Connections.
/// Called once per process via `Once`.
fn register_sqlite_vec() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        unsafe {
            rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute::<
                *const (),
                unsafe extern "C" fn(
                    *mut rusqlite::ffi::sqlite3,
                    *mut *mut i8,
                    *const rusqlite::ffi::sqlite3_api_routines,
                ) -> i32,
            >(sqlite_vec::sqlite3_vec_init as *const ())));
        }
    });
}

pub struct SqliteStore {
    conn: Mutex<Connection>,
    #[allow(dead_code)]
    dimensions: usize,
}

impl SqliteStore {
    pub async fn new(path: &str, dimensions: usize) -> Result<Self> {
        let path = path.to_string();
        if let Some(parent) = std::path::Path::new(&path).parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Register sqlite-vec before opening any connection.
        register_sqlite_vec();

        if dimensions == 0 {
            anyhow::bail!(
                "RAG embedding dimensions must be > 0. \
                 Set ALCHEMY_RAG_DIMENSIONS to the output dimension of your embedding model \
                 (e.g. 1536 for OpenAI text-embedding-3-small, 768 for Gemini/Ollama)."
            );
        }

        let conn = Connection::open(&path)?;

        // Check whether the vec0 virtual table already exists.
        let has_vec_chunks: bool = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='vec_chunks'",
            [],
            |r| r.get::<_, i64>(0),
        )? > 0;

        if !has_vec_chunks {
            // If the old BLOB-based embeddings table is present, drop it.
            // The `chunks` content table is preserved; embeddings must be re-indexed.
            let has_old_embeddings: bool = conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='embeddings'",
                [],
                |r| r.get::<_, i64>(0),
            )? > 0;
            if has_old_embeddings {
                tracing::warn!(
                    "RAG store: migrating from BLOB schema to sqlite-vec vec0. \
                     Stored embeddings are dropped — please re-index your documents."
                );
                conn.execute_batch("DROP TABLE embeddings;")?;
            }
            conn.execute_batch(&format!(
                "CREATE VIRTUAL TABLE vec_chunks USING vec0(
                     embedding float[{dimensions}] distance_metric=cosine
                 );"
            ))?;
        }

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS chunks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source_path TEXT NOT NULL,
                content TEXT NOT NULL,
                chunk_index INTEGER NOT NULL,
                metadata TEXT DEFAULT '{}'
            );
            CREATE INDEX IF NOT EXISTS idx_chunks_source ON chunks(source_path);",
        )?;

        Ok(Self { conn: Mutex::new(conn), dimensions })
    }

    #[allow(dead_code)]
    pub async fn get_chunk(&self, id: i64) -> Result<(String, String)> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT content, source_path FROM chunks WHERE id = ?1")?;
        let result = stmt.query_row(rusqlite::params![id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        Ok(result)
    }
}

#[async_trait]
impl VectorStoreBackend for SqliteStore {
    async fn insert(
        &self,
        content: &str,
        embedding: &[f32],
        source: &str,
        chunk_index: usize,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO chunks (source_path, content, chunk_index) VALUES (?1, ?2, ?3)",
            rusqlite::params![source, content, chunk_index as i64],
        )?;
        let chunk_id = conn.last_insert_rowid();

        let embedding_bytes: Vec<u8> =
            embedding.iter().flat_map(|f| f.to_le_bytes()).collect();

        conn.execute(
            "INSERT INTO vec_chunks(rowid, embedding) VALUES (?1, ?2)",
            rusqlite::params![chunk_id, embedding_bytes],
        )?;

        Ok(())
    }

    async fn search(&self, query_embedding: &[f32], top_k: usize) -> Result<Vec<SearchHit>> {
        let conn = self.conn.lock().unwrap();
        let query_bytes: Vec<u8> =
            query_embedding.iter().flat_map(|f| f.to_le_bytes()).collect();

        // vec0 requires LIMIT directly on the virtual table scan (not on a JOIN).
        // Use a subquery to satisfy the k-constraint, then look up content.
        let mut stmt = conn.prepare(
            "SELECT c.content, c.source_path, v.distance
             FROM (
                 SELECT rowid, distance
                 FROM vec_chunks
                 WHERE embedding MATCH ?1
                 LIMIT ?2
             ) v
             JOIN chunks c ON c.id = v.rowid
             ORDER BY v.distance",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![query_bytes, top_k as i64],
            |row| {
                let content: String = row.get(0)?;
                let source: String = row.get(1)?;
                let dist: f64 = row.get(2)?;
                Ok((content, source, dist as f32))
            },
        )?;

        let mut results = Vec::new();
        for row in rows {
            let (content, source, dist) = row?;
            // cosine distance ∈ [0, 2]; convert to similarity (higher = better).
            let score = 1.0_f32 - dist;
            results.push(SearchHit { content, source, score });
        }
        Ok(results)
    }

    async fn status(&self) -> Result<StoreStatus> {
        let conn = self.conn.lock().unwrap();
        let total_chunks: i64 =
            conn.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))?;
        let total_sources: i64 = conn.query_row(
            "SELECT COUNT(DISTINCT source_path) FROM chunks",
            [],
            |r| r.get(0),
        )?;
        Ok(StoreStatus {
            total_chunks: total_chunks as usize,
            total_sources: total_sources as usize,
        })
    }

    async fn clear(&mut self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch("DELETE FROM vec_chunks; DELETE FROM chunks;")?;
        Ok(())
    }
}

// ── Qdrant backend ────────────────────────────────────────────────────────────

/// Qdrant HTTP backend. Uses the Qdrant REST API.
///
/// Required env vars: `ALCHEMY_RAG_STORE_URL` (e.g. `http://localhost:6333`).
/// Optional: `ALCHEMY_RAG_STORE_API_KEY`, `ALCHEMY_RAG_STORE_COLLECTION`
/// (defaults to `"alchemy"`).
pub struct QdrantStore {
    client: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
    collection: String,
    dimensions: usize,
}

impl QdrantStore {
    pub async fn new(
        base_url: String,
        api_key: Option<String>,
        collection: String,
        dimensions: usize,
    ) -> Result<Self> {
        if dimensions == 0 {
            anyhow::bail!(
                "RAG embedding dimensions must be > 0. \
                 Set ALCHEMY_RAG_DIMENSIONS to the correct value for your embedding model."
            );
        }

        let store = Self {
            client: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            collection,
            dimensions,
        };

        store.ensure_collection().await?;
        Ok(store)
    }

    fn auth_headers(&self) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(key) = &self.api_key {
            headers.insert(
                "api-key",
                reqwest::header::HeaderValue::from_str(key)
                    .unwrap_or_else(|_| reqwest::header::HeaderValue::from_static("")),
            );
        }
        headers
    }

    async fn ensure_collection(&self) -> Result<()> {
        let url = format!("{}/collections/{}", self.base_url, self.collection);
        let resp = self.client.get(&url).headers(self.auth_headers()).send().await?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            // Create the collection.
            let body = serde_json::json!({
                "vectors": {
                    "size": self.dimensions,
                    "distance": "Cosine"
                }
            });
            let create_resp = self.client
                .put(&url)
                .headers(self.auth_headers())
                .json(&body)
                .send()
                .await?;
            if !create_resp.status().is_success() {
                let text = create_resp.text().await.unwrap_or_default();
                anyhow::bail!("Qdrant: failed to create collection '{}': {}", self.collection, truncate_chars(&text, 200));
            }
        } else if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Qdrant: failed to check collection '{}': {}", self.collection, truncate_chars(&text, 200));
        }

        Ok(())
    }
}

#[async_trait]
impl VectorStoreBackend for QdrantStore {
    async fn insert(
        &self,
        content: &str,
        embedding: &[f32],
        source: &str,
        chunk_index: usize,
    ) -> Result<()> {
        // Use a deterministic ID derived from source + chunk_index.
        let id = format!("{:x}", md5_u128(source, chunk_index));
        let body = serde_json::json!({
            "points": [{
                "id": id,
                "vector": embedding,
                "payload": {
                    "content": content,
                    "source": source,
                    "chunk_index": chunk_index
                }
            }]
        });
        let url = format!("{}/collections/{}/points", self.base_url, self.collection);
        let resp = self.client
            .put(&url)
            .headers(self.auth_headers())
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Qdrant insert failed: {}", truncate_chars(&text, 200));
        }
        Ok(())
    }

    async fn search(&self, query_embedding: &[f32], top_k: usize) -> Result<Vec<SearchHit>> {
        let body = serde_json::json!({
            "vector": query_embedding,
            "limit": top_k,
            "with_payload": true
        });
        let url = format!("{}/collections/{}/points/search", self.base_url, self.collection);
        let resp = self.client
            .post(&url)
            .headers(self.auth_headers())
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Qdrant search failed: {}", truncate_chars(&text, 200));
        }
        let json: serde_json::Value = resp.json().await?;
        let result = json["result"].as_array().cloned().unwrap_or_default();
        let hits = result.iter().filter_map(|item| {
            let score = item["score"].as_f64()? as f32;
            let payload = &item["payload"];
            let content = payload["content"].as_str()?.to_string();
            let source = payload["source"].as_str()?.to_string();
            Some(SearchHit { content, source, score })
        }).collect();
        Ok(hits)
    }

    async fn status(&self) -> Result<StoreStatus> {
        let url = format!("{}/collections/{}", self.base_url, self.collection);
        let resp = self.client.get(&url).headers(self.auth_headers()).send().await?;
        if !resp.status().is_success() {
            return Ok(StoreStatus { total_chunks: 0, total_sources: 0 });
        }
        let json: serde_json::Value = resp.json().await?;
        let total_chunks = json["result"]["points_count"].as_u64().unwrap_or(0) as usize;
        Ok(StoreStatus { total_chunks, total_sources: 0 })
    }

    async fn clear(&mut self) -> Result<()> {
        // Delete and recreate the collection to clear all points.
        let url = format!("{}/collections/{}", self.base_url, self.collection);
        self.client.delete(&url).headers(self.auth_headers()).send().await?;
        self.ensure_collection().await?;
        Ok(())
    }
}

// ── Chroma backend ────────────────────────────────────────────────────────────

/// Chroma HTTP backend. Uses the Chroma REST API v1.
///
/// Required env vars: `ALCHEMY_RAG_STORE_URL` (e.g. `http://localhost:8000`).
/// Optional: `ALCHEMY_RAG_STORE_API_KEY`, `ALCHEMY_RAG_STORE_COLLECTION`
/// (defaults to `"alchemy"`).
pub struct ChromaStore {
    client: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
    collection_id: String, // UUID obtained after create/get
    collection_name: String,
}

impl ChromaStore {
    pub async fn new(
        base_url: String,
        api_key: Option<String>,
        collection: String,
        dimensions: usize,
    ) -> Result<Self> {
        if dimensions == 0 {
            anyhow::bail!(
                "RAG embedding dimensions must be > 0. \
                 Set ALCHEMY_RAG_DIMENSIONS to the correct value for your embedding model."
            );
        }

        let base = base_url.trim_end_matches('/').to_string();
        let mut store = Self {
            client: reqwest::Client::new(),
            base_url: base,
            api_key,
            collection_id: String::new(),
            collection_name: collection,
        };

        store.collection_id = store.ensure_collection().await?;
        Ok(store)
    }

    fn auth_headers(&self) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(key) = &self.api_key {
            headers.insert(
                reqwest::header::AUTHORIZATION,
                reqwest::header::HeaderValue::from_str(&format!("Bearer {}", key))
                    .unwrap_or_else(|_| reqwest::header::HeaderValue::from_static("")),
            );
        }
        headers
    }

    async fn ensure_collection(&self) -> Result<String> {
        // Try to get or create the collection, return its UUID.
        let url = format!("{}/api/v1/collections", self.base_url);
        let body = serde_json::json!({ "name": self.collection_name });
        let resp = self.client
            .post(&url)
            .headers(self.auth_headers())
            .json(&body)
            .send()
            .await?;

        if resp.status().is_success() {
            let json: serde_json::Value = resp.json().await?;
            return Ok(json["id"].as_str().unwrap_or("").to_string());
        }

        // 409 Conflict = collection already exists; fetch it.
        let get_url = format!("{}/api/v1/collections/{}", self.base_url, self.collection_name);
        let get_resp = self.client.get(&get_url).headers(self.auth_headers()).send().await?;
        if get_resp.status().is_success() {
            let json: serde_json::Value = get_resp.json().await?;
            return Ok(json["id"].as_str().unwrap_or("").to_string());
        }

        let text = get_resp.text().await.unwrap_or_default();
        anyhow::bail!("Chroma: failed to get/create collection '{}': {}", self.collection_name, truncate_chars(&text, 200));
    }
}

#[async_trait]
impl VectorStoreBackend for ChromaStore {
    async fn insert(
        &self,
        content: &str,
        embedding: &[f32],
        source: &str,
        chunk_index: usize,
    ) -> Result<()> {
        let id = format!("{:x}", md5_u128(source, chunk_index));
        let body = serde_json::json!({
            "ids": [id],
            "embeddings": [embedding],
            "documents": [content],
            "metadatas": [{ "source": source, "chunk_index": chunk_index }]
        });
        let url = format!("{}/api/v1/collections/{}/add", self.base_url, self.collection_id);
        let resp = self.client
            .post(&url)
            .headers(self.auth_headers())
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Chroma insert failed: {}", truncate_chars(&text, 200));
        }
        Ok(())
    }

    async fn search(&self, query_embedding: &[f32], top_k: usize) -> Result<Vec<SearchHit>> {
        let body = serde_json::json!({
            "query_embeddings": [query_embedding],
            "n_results": top_k,
            "include": ["documents", "metadatas", "distances"]
        });
        let url = format!("{}/api/v1/collections/{}/query", self.base_url, self.collection_id);
        let resp = self.client
            .post(&url)
            .headers(self.auth_headers())
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Chroma search failed: {}", truncate_chars(&text, 200));
        }
        let json: serde_json::Value = resp.json().await?;

        // Chroma returns parallel arrays: documents[0], metadatas[0], distances[0]
        let docs = json["documents"][0].as_array().cloned().unwrap_or_default();
        let metas = json["metadatas"][0].as_array().cloned().unwrap_or_default();
        let dists = json["distances"][0].as_array().cloned().unwrap_or_default();

        let hits = docs.iter().enumerate().filter_map(|(i, doc)| {
            let content = doc.as_str()?.to_string();
            let source = metas.get(i)
                .and_then(|m| m["source"].as_str())
                .unwrap_or("")
                .to_string();
            // Chroma returns L2 distance by default; convert to score (lower = more similar).
            let dist = dists.get(i).and_then(|d| d.as_f64()).unwrap_or(1.0) as f32;
            let score = 1.0 / (1.0 + dist);
            Some(SearchHit { content, source, score })
        }).collect();
        Ok(hits)
    }

    async fn status(&self) -> Result<StoreStatus> {
        let url = format!("{}/api/v1/collections/{}/count", self.base_url, self.collection_id);
        let total_chunks = match self.client.get(&url).headers(self.auth_headers()).send().await {
            Ok(resp) if resp.status().is_success() => {
                resp.json::<serde_json::Value>().await
                    .ok()
                    .and_then(|j| j.as_u64())
                    .unwrap_or(0) as usize
            }
            _ => 0,
        };
        Ok(StoreStatus { total_chunks, total_sources: 0 })
    }

    async fn clear(&mut self) -> Result<()> {
        // Delete and recreate.
        let del_url = format!("{}/api/v1/collections/{}", self.base_url, self.collection_name);
        self.client.delete(&del_url).headers(self.auth_headers()).send().await?;
        self.collection_id = self.ensure_collection().await?;
        Ok(())
    }
}

// ── Shared helpers ────────────────────────────────────────────────────────────

/// Deterministic 128-bit ID from source path + chunk index.
/// Used as point ID in Qdrant (UUID string) and document ID in Chroma.
fn md5_u128(source: &str, chunk_index: usize) -> u128 {
    use std::hash::Hash;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut h);
    chunk_index.hash(&mut h);
    let lo = std::hash::Hasher::finish(&h);
    // Second pass with a salt for the high bits.
    "salt".hash(&mut h);
    source.hash(&mut h);
    let hi = std::hash::Hasher::finish(&h);
    ((hi as u128) << 64) | lo as u128
}

fn truncate_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

// ── Backwards-compat type alias ───────────────────────────────────────────────

/// Backwards-compat alias kept for any external code referencing `VectorStore`.
#[allow(dead_code)]
pub type VectorStore = SqliteStore;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_store_operations() {
        register_sqlite_vec();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let mut store = SqliteStore::new(path.to_str().unwrap(), 3).await.unwrap();

        store.insert("hello world", &[1.0, 0.0, 0.0], "test.txt", 0).await.unwrap();
        store.insert("foo bar", &[0.0, 1.0, 0.0], "test.txt", 1).await.unwrap();

        let status = store.status().await.unwrap();
        assert_eq!(status.total_chunks, 2);
        assert_eq!(status.total_sources, 1);

        let results = store.search(&[1.0, 0.0, 0.0], 1).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "hello world");

        store.clear().await.unwrap();
        let status = store.status().await.unwrap();
        assert_eq!(status.total_chunks, 0);
    }

    #[tokio::test]
    async fn test_migration_from_old_schema() {
        register_sqlite_vec();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.db");

        // Simulate old BLOB-based schema.
        {
            let conn = Connection::open(path.to_str().unwrap()).unwrap();
            conn.execute_batch(
                "CREATE TABLE chunks (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     source_path TEXT NOT NULL,
                     content TEXT NOT NULL,
                     chunk_index INTEGER NOT NULL,
                     metadata TEXT DEFAULT '{}'
                 );
                 CREATE TABLE embeddings (
                     chunk_id INTEGER PRIMARY KEY,
                     embedding BLOB NOT NULL
                 );
                 INSERT INTO chunks (source_path, content, chunk_index)
                     VALUES ('doc.txt', 'hello', 0);
                 INSERT INTO embeddings (chunk_id, embedding) VALUES (1, X'000000000000000000000000');",
            ).unwrap();
        }

        // Opening with new SqliteStore should migrate without error.
        let store = SqliteStore::new(path.to_str().unwrap(), 3).await.unwrap();
        let status = store.status().await.unwrap();
        // Chunk content is preserved; embeddings are gone.
        assert_eq!(status.total_chunks, 1);
    }

    #[tokio::test]
    async fn test_store_multibyte_content_roundtrips() {
        register_sqlite_vec();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mb.db");
        let store = SqliteStore::new(path.to_str().unwrap(), 3).await.unwrap();

        // Content containing multi-byte UTF-8 chars (box-drawing, emoji).
        let content = "// ── Panel toggles ───────────────────────────────────────────────────────── 😀 end";
        store.insert(content, &[1.0, 0.0, 0.0], "src/tui/mod.rs", 0).await.unwrap();

        let results = store.search(&[1.0, 0.0, 0.0], 1).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, content);

        // Truncating at 100 chars must not panic even with multi-byte chars.
        let preview: String = results[0].content.chars().take(100).collect();
        assert!(preview.chars().count() <= 100);
        assert!(preview.is_char_boundary(preview.len()));
    }
}
