use anyhow::Result;
use rusqlite::Connection;
use std::sync::{Mutex, Once};

pub struct VectorStore {
    conn: Mutex<Connection>,
    #[allow(dead_code)]
    dimensions: usize,
}

pub struct StoreStatus {
    pub total_chunks: usize,
    pub total_sources: usize,
}

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

impl VectorStore {
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

    pub async fn insert(
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

        // sqlite-vec accepts f32 vectors as packed little-endian bytes (same as before).
        let embedding_bytes: Vec<u8> =
            embedding.iter().flat_map(|f| f.to_le_bytes()).collect();

        conn.execute(
            "INSERT INTO vec_chunks(rowid, embedding) VALUES (?1, ?2)",
            rusqlite::params![chunk_id, embedding_bytes],
        )?;

        Ok(())
    }

    /// Returns `(chunk_id, similarity_score)` pairs where 1.0 = identical, 0.0 = orthogonal.
    pub async fn search(
        &self,
        query_embedding: &[f32],
        top_k: usize,
    ) -> Result<Vec<(i64, f32)>> {
        let conn = self.conn.lock().unwrap();
        let query_bytes: Vec<u8> =
            query_embedding.iter().flat_map(|f| f.to_le_bytes()).collect();

        let mut stmt = conn.prepare(
            "SELECT rowid, distance
             FROM vec_chunks
             WHERE embedding MATCH ?1
             ORDER BY distance
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![query_bytes, top_k as i64],
            |row| {
                let id: i64 = row.get(0)?;
                let dist: f64 = row.get(1)?;
                Ok((id, dist as f32))
            },
        )?;

        let mut results = Vec::new();
        for row in rows {
            let (id, dist) = row?;
            // cosine distance ∈ [0, 2]; convert to similarity ∈ [-1, 1] (higher = better).
            let score = 1.0_f32 - dist;
            results.push((id, score));
        }
        Ok(results)
    }

    pub async fn get_chunk(&self, id: i64) -> Result<(String, String)> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT content, source_path FROM chunks WHERE id = ?1")?;
        let result = stmt.query_row(rusqlite::params![id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        Ok(result)
    }

    pub async fn status(&self) -> Result<StoreStatus> {
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

    pub async fn clear(&mut self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch("DELETE FROM vec_chunks; DELETE FROM chunks;")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_store_operations() {
        register_sqlite_vec();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let mut store = VectorStore::new(path.to_str().unwrap(), 3).await.unwrap();

        store.insert("hello world", &[1.0, 0.0, 0.0], "test.txt", 0).await.unwrap();
        store.insert("foo bar", &[0.0, 1.0, 0.0], "test.txt", 1).await.unwrap();

        let status = store.status().await.unwrap();
        assert_eq!(status.total_chunks, 2);
        assert_eq!(status.total_sources, 1);

        let results = store.search(&[1.0, 0.0, 0.0], 1).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, 1); // first inserted chunk wins

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

        // Opening with new VectorStore should migrate without error.
        let store = VectorStore::new(path.to_str().unwrap(), 3).await.unwrap();
        let status = store.status().await.unwrap();
        // Chunk content is preserved; embeddings are gone.
        assert_eq!(status.total_chunks, 1);
    }
}
