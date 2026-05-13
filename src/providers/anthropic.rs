use anyhow::Result;
use async_trait::async_trait;
use reqwest::Client;
use crate::types::{LlmRequest, LlmResponse, ToolCall, FunctionCall, Usage, MessageRole};
use crate::providers::Provider;

pub struct AnthropicProvider {
    api_key: String,
    base_url: String,
    client: Client,
}

impl AnthropicProvider {
    pub fn new(api_key: String, base_url: Option<String>) -> Self {
        Self {
            api_key,
            base_url: base_url.unwrap_or_else(|| "https://api.anthropic.com".to_string()),
            client: Client::new(),
        }
    }

    fn convert_request(&self, request: &LlmRequest) -> (Option<String>, Vec<serde_json::Value>, Option<serde_json::Value>) {
        let mut system = None;
        let mut messages = Vec::new();

        for msg in &request.messages {
            match msg.role {
                MessageRole::System => {
                    system = msg.content.clone();
                }
                MessageRole::User => {
                    messages.push(serde_json::json!({
                        "role": "user",
                        "content": msg.content.clone().unwrap_or_default(),
                    }));
                }
                MessageRole::Assistant => {
                    let mut content_parts: Vec<serde_json::Value> = Vec::new();
                    if let Some(ref text) = msg.content {
                        if !text.is_empty() {
                            content_parts.push(serde_json::json!({"type": "text", "text": text}));
                        }
                    }
                    if let Some(ref tcs) = msg.tool_calls {
                        for tc in tcs {
                            let input: serde_json::Value = serde_json::from_str(&tc.function.arguments)
                                .unwrap_or(serde_json::json!({}));
                            content_parts.push(serde_json::json!({
                                "type": "tool_use",
                                "id": tc.id,
                                "name": tc.function.name,
                                "input": input,
                            }));
                        }
                    }
                    if !content_parts.is_empty() {
                        messages.push(serde_json::json!({
                            "role": "assistant",
                            "content": content_parts,
                        }));
                    }
                }
                MessageRole::Tool => {
                    messages.push(serde_json::json!({
                        "role": "user",
                        "content": [{
                            "type": "tool_result",
                            "tool_use_id": msg.tool_call_id.clone().unwrap_or_default(),
                            "content": msg.content.clone().unwrap_or_default(),
                        }],
                    }));
                }
            }
        }

        let tools = if request.tools.is_empty() {
            None
        } else {
            let t: Vec<serde_json::Value> = request.tools.iter().map(|t| {
                serde_json::json!({
                    "name": t.function.name,
                    "description": t.function.description,
                    "input_schema": t.function.parameters,
                })
            }).collect();
            Some(serde_json::json!(t))
        };

        (system, messages, tools)
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    fn name(&self) -> &str {
        "anthropic"
    }

    fn default_model(&self) -> &str {
        "claude-3-5-haiku-latest"
    }

    async fn chat_streaming(
        &self,
        request: LlmRequest,
        tx: tokio::sync::mpsc::Sender<String>,
    ) -> Result<LlmResponse> {
        let (system, messages, tools) = self.convert_request(&request);
        let url = format!("{}/v1/messages", self.base_url);

        let mut body = serde_json::json!({
            "model": request.model,
            "messages": messages,
            "max_tokens": 4096,
            "stream": true,
        });
        if let Some(s) = system {
            body["system"] = serde_json::json!(s);
        }
        if let Some(t) = tools {
            body["tools"] = t;
        }

        let resp = self.client.post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await?;
            anyhow::bail!("Anthropic API error {}: {}", status, text);
        }

        let mut full_content = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut current_tool_id = String::new();
        let mut current_tool_name = String::new();
        let mut current_tool_args = String::new();
        let mut in_tool = false;
        let mut usage_data = None;

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
                if let Ok(event) = serde_json::from_str::<serde_json::Value>(data_str) {
                    match event["type"].as_str() {
                        Some("content_block_start") => {
                            if let Some(cb) = event.get("content_block") {
                                if cb["type"].as_str() == Some("tool_use") {
                                    current_tool_id = cb["id"].as_str().unwrap_or("").to_string();
                                    current_tool_name = cb["name"].as_str().unwrap_or("").to_string();
                                    current_tool_args.clear();
                                    in_tool = true;
                                }
                            }
                        }
                        Some("content_block_delta") => {
                            if let Some(delta) = event.get("delta") {
                                if delta["type"].as_str() == Some("text_delta") {
                                    if let Some(text) = delta["text"].as_str() {
                                        full_content.push_str(text);
                                        let _ = tx.send(text.to_string()).await;
                                    }
                                } else if delta["type"].as_str() == Some("input_json_delta") {
                                    if let Some(json) = delta["partial_json"].as_str() {
                                        current_tool_args.push_str(json);
                                    }
                                }
                            }
                        }
                        Some("content_block_stop") => {
                            if in_tool {
                                tool_calls.push(ToolCall {
                                    id: current_tool_id.clone(),
                                    r#type: "function".to_string(),
                                    function: FunctionCall {
                                        name: current_tool_name.clone(),
                                        arguments: if current_tool_args.is_empty() {
                                            "{}".to_string()
                                        } else {
                                            current_tool_args.clone()
                                        },
                                    },
                                });
                                in_tool = false;
                            }
                        }
                        Some("message_delta") => {
                            if let Some(u) = event.get("usage") {
                                usage_data = Some(Usage {
                                    prompt_tokens: 0,
                                    completion_tokens: u["output_tokens"].as_u64().unwrap_or(0),
                                    total_tokens: u["output_tokens"].as_u64().unwrap_or(0),
                                });
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        Ok(LlmResponse {
            content: if full_content.is_empty() { None } else { Some(full_content) },
            tool_calls,
            usage: usage_data,
            finish_reason: Some("end_turn".to_string()),
        })
    }
}

