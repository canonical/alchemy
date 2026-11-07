pub mod openai;
pub mod copilot;
pub mod gemini;
pub mod anthropic;

use anyhow::Result;
use async_trait::async_trait;
use crate::types::{LlmRequest, LlmResponse};

#[async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;
    fn default_model(&self) -> &str;
    async fn chat_streaming(
        &self,
        request: LlmRequest,
        tx: tokio::sync::mpsc::Sender<String>,
    ) -> Result<LlmResponse>;
    /// Single-shot call: delegates to the streaming path with a sink so we don't keep two
    /// near-identical request/response paths per provider. Override only if a provider can't
    /// stream.
    #[allow(dead_code)]
    async fn chat(&self, request: LlmRequest) -> Result<LlmResponse> {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(1024);
        let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });
        let result = self.chat_streaming(request, tx).await;
        drain.await.ok();
        result
    }
    #[allow(dead_code)]
    async fn embed(&self, _texts: &[String]) -> Result<Vec<Vec<f32>>> {
        anyhow::bail!("Embeddings not supported by provider {}", self.name())
    }
    #[allow(dead_code)]
    fn default_embed_model(&self) -> &str {
        ""
    }
    fn embed_dimensions(&self) -> usize {
        0
    }
    /// Embedding dimensions for a specific model, when the model is known.
    /// Falls back to the provider-level default for unknown/unset models.
    /// `ALCHEMY_RAG_DIMENSIONS` always overrides this.
    fn embed_dimensions_for_model(&self, model: Option<&str>) -> usize {
        let _ = model;
        self.embed_dimensions()
    }
}

pub fn create_provider(
    provider_name: &str,
    api_key: Option<&str>,
    base_url: Option<&str>,
) -> Result<Box<dyn Provider>> {
    match provider_name {
        "openai" => {
            let key = api_key.ok_or_else(|| anyhow::anyhow!("ALCHEMY_API_KEY required for openai"))?;
            Ok(Box::new(openai::OpenAiProvider::new(
                key.to_string(),
                base_url.unwrap_or("https://api.openai.com/v1").to_string(),
                "openai".to_string(),
            )))
        }
        "openrouter" => {
            let key = api_key.ok_or_else(|| anyhow::anyhow!("ALCHEMY_API_KEY required for openrouter"))?;
            Ok(Box::new(openai::OpenAiProvider::new(
                key.to_string(),
                base_url.unwrap_or("https://openrouter.ai/api/v1").to_string(),
                "openrouter".to_string(),
            )))
        }
        "ollama" => {
            Ok(Box::new(openai::OpenAiProvider::new(
                api_key.unwrap_or("").to_string(),
                base_url.unwrap_or("http://localhost:11434/v1").to_string(),
                "ollama".to_string(),
            )))
        }
        "github-copilot" => {
            let key = api_key.ok_or_else(|| anyhow::anyhow!("ALCHEMY_API_KEY required for github-copilot"))?;
            Ok(Box::new(copilot::CopilotProvider::new(
                key.to_string(),
                base_url.map(|s| s.to_string()),
            )))
        }
        "gemini" => {
            let key = api_key.ok_or_else(|| anyhow::anyhow!("ALCHEMY_API_KEY required for gemini"))?;
            Ok(Box::new(gemini::GeminiProvider::new(
                key.to_string(),
                base_url.map(|s| s.to_string()),
            )))
        }
        "anthropic" => {
            let key = api_key.ok_or_else(|| anyhow::anyhow!("ALCHEMY_API_KEY required for anthropic"))?;
            Ok(Box::new(anthropic::AnthropicProvider::new(
                key.to_string(),
                base_url.map(|s| s.to_string()),
            )))
        }
        _ => anyhow::bail!("Unknown provider: {}", provider_name),
    }
}

/// Accumulates raw SSE response bytes and yields complete, newline-terminated
/// lines, decoding UTF-8 only once a full line is assembled.
///
/// `reqwest`'s `bytes_stream()` yields network chunks that split at arbitrary
/// byte offsets, including the middle of a multi-byte UTF-8 character. Decoding
/// each chunk on its own (e.g. `String::from_utf8_lossy` per chunk) replaces the
/// split bytes on both sides with U+FFFD, corrupting any character that straddles
/// a chunk boundary. Buffering the raw bytes and only decoding once a whole line
/// has arrived keeps every character intact.
#[derive(Default)]
pub(crate) struct SseLineBuffer {
    buf: Vec<u8>,
}

impl SseLineBuffer {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Append a raw network chunk.
    pub(crate) fn push(&mut self, chunk: &[u8]) {
        self.buf.extend_from_slice(chunk);
    }

    /// Pop the next complete line (up to and including the next `\n`), decoded
    /// lossily and trimmed. Returns `None` while only a partial line is buffered,
    /// so a trailing partial line waits for more bytes instead of being decoded
    /// (and corrupted) early.
    pub(crate) fn next_line(&mut self) -> Option<String> {
        let nl = self.buf.iter().position(|&b| b == b'\n')?;
        let line: Vec<u8> = self.buf.drain(..=nl).collect();
        // Drop the trailing '\n'; `trim` also removes any '\r' (CRLF) and spaces.
        Some(
            String::from_utf8_lossy(&line[..line.len() - 1])
                .trim()
                .to_string(),
        )
    }
}

#[cfg(test)]
mod sse_line_buffer_tests {
    use super::SseLineBuffer;

    #[test]
    fn reassembles_two_byte_char_split_across_chunks() {
        // "é" is 0xC3 0xA9; split the line between its two bytes.
        let mut lines = SseLineBuffer::new();
        lines.push(b"data: caf\xc3");
        assert_eq!(lines.next_line(), None, "no complete line yet");
        lines.push(b"\xa9\n");
        assert_eq!(lines.next_line().as_deref(), Some("data: café"));
    }

    #[test]
    fn reassembles_four_byte_emoji_split_across_three_chunks() {
        // "😀" is 0xF0 0x9F 0x98 0x80.
        let mut lines = SseLineBuffer::new();
        lines.push(b"data: hi \xf0\x9f");
        assert_eq!(lines.next_line(), None);
        lines.push(b"\x98");
        assert_eq!(lines.next_line(), None);
        lines.push(b"\x80\n");
        assert_eq!(lines.next_line().as_deref(), Some("data: hi 😀"));
    }

    #[test]
    fn yields_multiple_lines_and_holds_partial() {
        let mut lines = SseLineBuffer::new();
        lines.push(b"data: one\ndata: two\ndata: thr");
        assert_eq!(lines.next_line().as_deref(), Some("data: one"));
        assert_eq!(lines.next_line().as_deref(), Some("data: two"));
        assert_eq!(lines.next_line(), None);
        lines.push(b"ee\n");
        assert_eq!(lines.next_line().as_deref(), Some("data: three"));
    }

    #[test]
    fn trims_crlf_and_decodes_empty_line() {
        let mut lines = SseLineBuffer::new();
        lines.push(b"data: x\r\n\n");
        assert_eq!(lines.next_line().as_deref(), Some("data: x"));
        assert_eq!(lines.next_line().as_deref(), Some(""));
    }
}
