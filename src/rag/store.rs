use anyhow::Result;
use rusqlite::Connection;
use std::sync::Mutex;

pub struct VectorStore {
    conn: Mutex<Connection>,
    dimensions: usize,
}

pub struct StoreStatus {
    pub total_chunks: usize,
    pub total_sources: usize,
}

impl VectorStore {
    pub async fn new(path: &str, dimensions: usize) -> Result<Self> {
        let path = path.to_string();
        // Create parent directories
        if let Some(parent) = std::path::Path::new(&path).parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let conn = Connection::open(&path)?;
        conn.execute_batch(&format!(
            "CREATE TABLE IF NOT EXISTS chunks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source_path TEXT NOT NULL,
                content TEXT NOT NULL,
                chunk_index INTEGER NOT NULL,
                metadata TEXT DEFAULT '{{}}'
            );
            CREATE TABLE IF NOT EXISTS embeddings (
                chunk_id INTEGER PRIMARY KEY,
                embedding BLOB NOT NULL,
                FOREIGN KEY(chunk_id) REFERENCES chunks(id)
            );
            CREATE INDEX IF NOT EXISTS idx_chunks_source ON chunks(source_path);"
        ))?;

        Ok(Self { conn: Mutex::new(conn), dimensions })
    }

    pub async fn insert(&self, content: &str, embedding: &[f32], source: &str, chunk_index: usize) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO chunks (source_path, content, chunk_index) VALUES (?1, ?2, ?3)",
            rusqlite::params![source, content, chunk_index as i64],
        )?;
        let chunk_id = conn.last_insert_rowid();

        let embedding_bytes: Vec<u8> = embedding.iter()
            .flat_map(|f| f.to_le_bytes())
            .collect();

        conn.execute(
            "INSERT INTO embeddings (chunk_id, embedding) VALUES (?1, ?2)",
            rusqlite::params![chunk_id, embedding_bytes],
        )?;

        Ok(())
    }

    pub async fn search(&self, query_embedding: &[f32], top_k: usize) -> Result<Vec<(i64, f32)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT chunk_id, embedding FROM embeddings")?;
        let rows = stmt.query_map([], |row| {
            let id: i64 = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            Ok((id, blob))
        })?;

        let mut scores: Vec<(i64, f32)> = Vec::new();
        for row in rows {
            let (id, blob) = row?;
            let embedding: Vec<f32> = blob.chunks(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            let score = cosine_similarity(query_embedding, &embedding);
            scores.push((id, score));
        }

        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scores.truncate(top_k);
        Ok(scores)
    }

    pub async fn get_chunk(&self, id: i64) -> Result<(String, String)> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT content, source_path FROM chunks WHERE id = ?1")?;
        let result = stmt.query_row(rusqlite::params![id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        Ok(result)
    }

    pub async fn status(&self) -> Result<StoreStatus> {
        let conn = self.conn.lock().unwrap();
        let total_chunks: usize = conn.query_row(
            "SELECT COUNT(*) FROM chunks", [], |r| r.get(0)
        )?;
        let total_sources: usize = conn.query_row(
            "SELECT COUNT(DISTINCT source_path) FROM chunks", [], |r| r.get(0)
        )?;
        Ok(StoreStatus { total_chunks, total_sources })
    }

    pub async fn clear(&mut self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch("DELETE FROM embeddings; DELETE FROM chunks;")?;
        Ok(())
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let mag_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let mag_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if mag_a == 0.0 || mag_b == 0.0 { 0.0 } else { dot / (mag_a * mag_b) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 0.001);

        let c = vec![0.0, 1.0, 0.0];
        assert!(cosine_similarity(&a, &c).abs() < 0.001);
    }

    #[tokio::test]
    async fn test_store_operations() {
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

        store.clear().await.unwrap();
        let status = store.status().await.unwrap();
        assert_eq!(status.total_chunks, 0);
    }
}
