use anyhow::Result;
use crate::tui::TuiMessage;
use crate::types::Message;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct MessageEntry {
    role: String,
    content: String,
}

#[derive(Serialize, Deserialize)]
pub struct SessionMetadata {
    pub model: String,
    pub created_at: String,
    pub updated_at: String,
}


pub async fn load_messages(path: &str) -> Result<Vec<TuiMessage>> {
    let content = tokio::fs::read_to_string(path).await?;
    let mut messages = Vec::new();
    for line in content.lines() {
        if line.trim().is_empty() { continue; }
        if let Ok(entry) = serde_json::from_str::<MessageEntry>(line) {
            messages.push(TuiMessage { role: entry.role, content: entry.content });
        }
    }
    Ok(messages)
}

pub async fn save_messages(path: &str, messages: &[TuiMessage]) -> Result<()> {
    let mut content = String::new();
    for msg in messages {
        let entry = MessageEntry { role: msg.role.clone(), content: msg.content.clone() };
        content.push_str(&serde_json::to_string(&entry)?);
        content.push('\n');
    }
    tokio::fs::write(path, content).await?;
    Ok(())
}

/// Persist the full LLM conversation context so it can be restored next session.
pub async fn save_context(path: &str, messages: &[Message]) -> Result<()> {
    tokio::fs::write(path, serde_json::to_string(messages)?).await?;
    Ok(())
}

/// Load the persisted LLM conversation context.
pub async fn load_context(path: &str) -> Result<Vec<Message>> {
    let content = tokio::fs::read_to_string(path).await?;
    Ok(serde_json::from_str(&content)?)
}

/// Load the global prompt history from `path`.
/// Returns up to `limit` most-recent entries, deduplicated so no two
/// adjacent entries are identical.
pub async fn load_prompt_history(path: &str, limit: usize) -> Vec<String> {
    let Ok(content) = tokio::fs::read_to_string(path).await else {
        return Vec::new();
    };
    let mut entries: Vec<String> = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.to_string())
        .collect();
    // Keep only the tail we care about before dedup.
    if entries.len() > limit * 2 {
        entries = entries.split_off(entries.len() - limit * 2);
    }
    // Deduplicate adjacent identical entries.
    entries.dedup();
    if entries.len() > limit {
        entries = entries.split_off(entries.len() - limit);
    }
    entries
}

/// Append a single prompt to the history file.
/// Skips writing if `entry` is empty or matches the last line already
/// in the file (avoids consecutive duplicates on disk).
pub async fn append_prompt_history(path: &str, entry: &str) {
    if entry.trim().is_empty() {
        return;
    }
    // Avoid writing a duplicate of the last recorded line.
    if let Ok(content) = tokio::fs::read_to_string(path).await {
        if content.lines().next_back() == Some(entry) {
            return;
        }
    }
    let line = format!("{}\n", entry);
    // Append-open; create the file if it doesn't exist.
    use tokio::io::AsyncWriteExt;
    if let Ok(mut f) = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await
    {
        let _ = f.write_all(line.as_bytes()).await;
    }
}

pub async fn load_session_metadata(path: &str) -> Result<SessionMetadata> {
    let content = tokio::fs::read_to_string(path).await?;
    Ok(serde_json::from_str(&content)?)
}

pub async fn save_session_metadata(path: &str, metadata: &SessionMetadata) -> Result<()> {
    tokio::fs::write(path, serde_json::to_string_pretty(metadata)?).await?;
    Ok(())
}
