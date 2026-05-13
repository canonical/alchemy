use anyhow::Result;
use async_trait::async_trait;
use reqwest::Client;
use crate::types::{LlmRequest, LlmResponse, ToolCall, FunctionCall, MessageRole};
use crate::providers::Provider;

pub struct GeminiProvider {
    api_key: String,
    base_url: String,
    client: Client,
}

impl GeminiProvider {
    pub fn new(api_key: String, base_url: Option<String>) -> Self {
        Self {
            api_key,
            base_url: base_url.unwrap_or_else(|| "https://generativelanguage.googleapis.com/v1beta".to_string()),
            client: Client::new(),
        }
    }

    fn convert_messages(&self, request: &LlmRequest) -> (Option<serde_json::Value>, Vec<serde_json::Value>) {
        let mut system_instruction = None;
        let mut contents = Vec::new();

        for msg in &request.messages {
            match msg.role {
                MessageRole::System => {
                    system_instruction = Some(serde_json::json!({
                        "parts": [{"text": msg.content.clone().unwrap_or_default()}]
                    }));
                }
                MessageRole::User => {
                    contents.push(serde_json::json!({
                        "role": "user",
                        "parts": [{"text": msg.content.clone().unwrap_or_default()}]
                    }));
                }
                MessageRole::Assistant => {
                    let mut parts = Vec::new();
                    if let Some(ref content) = msg.content {
                        if !content.is_empty() {
                            parts.push(serde_json::json!({"text": content}));
                        }
                    }
                    if let Some(ref tcs) = msg.tool_calls {
                        for tc in tcs {
                            let args: serde_json::Value = serde_json::from_str(&tc.function.arguments)
                                .unwrap_or(serde_json::json!({}));
                            parts.push(serde_json::json!({
                                "functionCall": {
                                    "name": tc.function.name,
                                    "args": args
                                }
                            }));
                        }
                    }
                    if !parts.is_empty() {
                        contents.push(serde_json::json!({
                            "role": "model",
                            "parts": parts
                        }));
                    }
                }
                MessageRole::Tool => {
                    let content: serde_json::Value = serde_json::from_str(
                        msg.content.as_deref().unwrap_or("{}")
                    ).unwrap_or(serde_json::json!({"result": msg.content.clone().unwrap_or_default()}));

                    contents.push(serde_json::json!({
                        "role": "user",
                        "parts": [{
                            "functionResponse": {
                                "name": msg.tool_call_id.clone().unwrap_or_default(),
                                "response": content
                            }
                        }]
                    }));
                }
            }
        }

        (system_instruction, contents)
    }

    fn convert_tools(&self, request: &LlmRequest) -> Option<serde_json::Value> {
        if request.tools.is_empty() {
            return None;
        }

        let declarations: Vec<serde_json::Value> = request.tools.iter().map(|t| {
            serde_json::json!({
                "name": t.function.name,
                "description": t.function.description,
                "parameters": t.function.parameters,
            })
        }).collect();

        Some(serde_json::json!([{
            "functionDeclarations": declarations
        }]))
    }
}

#[async_trait]
impl Provider for GeminiProvider {
    fn name(&self) -> &str {
        "gemini"
    }

    fn default_model(&self) -> &str {
        "gemini-2.0-flash"
    }

    fn default_embed_model(&self) -> &str {
        "text-embedding-004"
    }

    fn embed_dimensions(&self) -> usize {
        768
    }

    async fn chat_streaming(
        &self,
        request: LlmRequest,
        tx: tokio::sync::mpsc::Sender<String>,
    ) -> Result<LlmResponse> {
        let (system_instruction, contents) = self.convert_messages(&request);
        let url = format!(
            "{}/models/{}:streamGenerateContent?alt=sse&key={}",
            self.base_url, request.model, self.api_key
        );

        let mut body = serde_json::json!({ "contents": contents });
        if let Some(si) = system_instruction {
            body["systemInstruction"] = si;
        }
        if let Some(tools) = self.convert_tools(&request) {
            body["tools"] = tools;
        }

        let resp = self.client.post(&url).json(&body).send().await?;
        let status = resp.status();

        if !status.is_success() {
            let text = resp.text().await?;
            anyhow::bail!("Gemini API error {}: {}", status, text);
        }

        let mut full_content = String::new();
        let mut tool_calls = Vec::new();

        use futures::StreamExt;
        let mut stream = resp.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(line_end) = buffer.find('\n') {
                let line = buffer[..line_end].trim().to_string();
                buffer = buffer[line_end + 1..].to_string();

                if !line.starts_with("data: ") {
                    continue;
                }
                let data_str = &line[6..];
                if let Ok(data) = serde_json::from_str::<serde_json::Value>(data_str) {
                    if let Some(candidates) = data["candidates"].as_array() {
                        for candidate in candidates {
                            if let Some(parts) = candidate["content"]["parts"].as_array() {
                                for part in parts {
                                    if let Some(text) = part["text"].as_str() {
                                        full_content.push_str(text);
                                        let _ = tx.send(text.to_string()).await;
                                    }
                                    if let Some(fc) = part.get("functionCall") {
                                        let name = fc["name"].as_str().unwrap_or("").to_string();
                                        let args = fc["args"].clone();
                                        tool_calls.push(ToolCall {
                                            id: format!("call_{}", tool_calls.len()),
                                            r#type: "function".to_string(),
                                            function: FunctionCall {
                                                name,
                                                arguments: serde_json::to_string(&args)?,
                                            },
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(LlmResponse {
            content: if full_content.is_empty() { None } else { Some(full_content) },
            tool_calls,
            usage: None,
            finish_reason: Some("stop".to_string()),
        })
    }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut results = Vec::new();
        for text in texts {
            let url = format!(
                "{}/models/{}:embedContent?key={}",
                self.base_url, self.default_embed_model(), self.api_key
            );
            let body = serde_json::json!({
                "content": {"parts": [{"text": text}]}
            });
            let resp = self.client.post(&url).json(&body).send().await?;
            let status = resp.status();
            let resp_text = resp.text().await?;
            if !status.is_success() {
                anyhow::bail!("Gemini embed error {}: {}", status, resp_text);
            }
            let data: serde_json::Value = serde_json::from_str(&resp_text)?;
            if let Some(values) = data["embedding"]["values"].as_array() {
                let vec: Vec<f32> = values.iter().filter_map(|v| v.as_f64().map(|f| f as f32)).collect();
                results.push(vec);
            }
        }
        Ok(results)
    }
}

