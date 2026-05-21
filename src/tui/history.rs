use anyhow::Result;
use crate::tui::TuiMessage;
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

pub async fn load_session_metadata(path: &str) -> Result<SessionMetadata> {
    let content = tokio::fs::read_to_string(path).await?;
    Ok(serde_json::from_str(&content)?)
}

pub async fn save_session_metadata(path: &str, metadata: &SessionMetadata) -> Result<()> {
    tokio::fs::write(path, serde_json::to_string_pretty(metadata)?).await?;
    Ok(())
}
