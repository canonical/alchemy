use anyhow::Result;
use async_trait::async_trait;
use reqwest::Client;
use crate::types::{LlmRequest, LlmResponse, ToolCall, FunctionCall, Usage};
use crate::providers::Provider;

pub struct OpenAiProvider {
    api_key: String,
    base_url: String,
    variant: String, // "openai", "openrouter", "ollama"
    client: Client,
}

impl OpenAiProvider {
    pub fn new(api_key: String, base_url: String, variant: String) -> Self {
        Self {
            api_key,
            base_url,
            variant,
            client: Client::new(),
        }
    }

    fn build_request_body(&self, request: &LlmRequest, stream: bool) -> serde_json::Value {
        let messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .map(|m| {
                let mut msg = serde_json::json!({
                    "role": m.role,
                    "content": m.content,
                });
                if let Some(ref tcs) = m.tool_calls {
                    msg["tool_calls"] = serde_json::to_value(tcs).unwrap();
                }
                if let Some(ref id) = m.tool_call_id {
                    msg["tool_call_id"] = serde_json::json!(id);
                }
                msg
            })
            .collect();

        let mut body = serde_json::json!({
            "model": request.model,
            "messages": messages,
            "stream": stream,
        });

        if !request.tools.is_empty() {
            body["tools"] = serde_json::to_value(&request.tools).unwrap();
        }

        if let Some(temp) = request.temperature {
            body["temperature"] = serde_json::json!(temp);
        }

        body
    }
}

#[async_trait]
impl Provider for OpenAiProvider {
    fn name(&self) -> &str {
        &self.variant
    }

    fn default_model(&self) -> &str {
        match self.variant.as_str() {
            "openai" => "gpt-4o-mini",
            "openrouter" => "gpt-4o-mini",
            "ollama" => "llama3.2",
            _ => "gpt-4o-mini",
        }
    }

    fn default_embed_model(&self) -> &str {
        match self.variant.as_str() {
            "openai" => "text-embedding-3-small",
            "ollama" => "nomic-embed-text",
            _ => "",
        }
    }

    fn embed_dimensions(&self) -> usize {
        match self.variant.as_str() {
            "openai" => 1536,
            "ollama" => 768,
            _ => 0,
        }
    }

    async fn chat(&self, request: LlmRequest) -> Result<LlmResponse> {
        let body = self.build_request_body(&request, false);
        let url = format!("{}/chat/completions", self.base_url);

        let mut req_builder = self.client.post(&url)
            .json(&body);

        if !self.api_key.is_empty() {
            req_builder = req_builder.header("Authorization", format!("Bearer {}", self.api_key));
        }

        let resp = req_builder.send().await?;
        let status = resp.status();
        let text = resp.text().await?;

        if !status.is_success() {
            anyhow::bail!("OpenAI API error {}: {}", status, text);
        }

        let data: serde_json::Value = serde_json::from_str(&text)?;
        parse_openai_response(&data)
    }

    async fn chat_streaming(
        &self,
        request: LlmRequest,
        tx: tokio::sync::mpsc::Sender<String>,
    ) -> Result<LlmResponse> {
        let body = self.build_request_body(&request, true);
        let url = format!("{}/chat/completions", self.base_url);

        let mut req_builder = self.client.post(&url)
            .json(&body);

        if !self.api_key.is_empty() {
            req_builder = req_builder.header("Authorization", format!("Bearer {}", self.api_key));
        }

        let resp = req_builder.send().await?;
        let status = resp.status();

        if !status.is_success() {
            let text = resp.text().await?;
            anyhow::bail!("OpenAI API error {}: {}", status, text);
        }

        let mut full_content = String::new();
        let mut tool_calls: Vec<ToolCallAccumulator> = Vec::new();
        let mut finish_reason = None;

        use futures::StreamExt;
        let mut stream = resp.bytes_stream();

        let mut buffer = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(line_end) = buffer.find('\n') {
                let line = buffer[..line_end].trim().to_string();
                buffer = buffer[line_end + 1..].to_string();

                if line.is_empty() || !line.starts_with("data: ") {
                    continue;
                }
                let data_str = &line[6..];
                if data_str == "[DONE]" {
                    continue;
                }

                if let Ok(data) = serde_json::from_str::<serde_json::Value>(data_str) {
                    if let Some(choices) = data["choices"].as_array() {
                        for choice in choices {
                            let delta = &choice["delta"];

                            if let Some(content) = delta["content"].as_str() {
                                full_content.push_str(content);
                                let _ = tx.send(content.to_string()).await;
                            }

                            if let Some(fr) = choice["finish_reason"].as_str() {
                                finish_reason = Some(fr.to_string());
                            }

                            if let Some(tcs) = delta["tool_calls"].as_array() {
                                for tc in tcs {
                                    let idx = tc["index"].as_u64().unwrap_or(0) as usize;
                                    while tool_calls.len() <= idx {
                                        tool_calls.push(ToolCallAccumulator::default());
                                    }
                                    if let Some(id) = tc["id"].as_str() {
                                        tool_calls[idx].id = id.to_string();
                                    }
                                    if let Some(f) = tc.get("function") {
                                        if let Some(name) = f["name"].as_str() {
                                            tool_calls[idx].name = name.to_string();
                                        }
                                        if let Some(args) = f["arguments"].as_str() {
                                            tool_calls[idx].arguments.push_str(args);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        let final_tool_calls: Vec<ToolCall> = tool_calls
            .into_iter()
            .filter(|tc| !tc.id.is_empty())
            .map(|tc| ToolCall {
                id: tc.id,
                r#type: "function".to_string(),
                function: FunctionCall {
                    name: tc.name,
                    arguments: tc.arguments,
                },
            })
            .collect();

        Ok(LlmResponse {
            content: if full_content.is_empty() { None } else { Some(full_content) },
            tool_calls: final_tool_calls,
            usage: None,
            finish_reason,
        })
    }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if self.variant != "openai" && self.variant != "ollama" {
            anyhow::bail!("Embeddings not supported for {}", self.variant);
        }

        let url = format!("{}/embeddings", self.base_url);
        let body = serde_json::json!({
            "model": self.default_embed_model(),
            "input": texts,
        });

        let mut req = self.client.post(&url).json(&body);
        if !self.api_key.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", self.api_key));
        }

        let resp = req.send().await?;
        let status = resp.status();
        let text = resp.text().await?;

        if !status.is_success() {
            anyhow::bail!("Embedding API error {}: {}", status, text);
        }

        let data: serde_json::Value = serde_json::from_str(&text)?;
        let mut results = Vec::new();
        if let Some(items) = data["data"].as_array() {
            for item in items {
                if let Some(embedding) = item["embedding"].as_array() {
                    let vec: Vec<f32> = embedding
                        .iter()
                        .filter_map(|v| v.as_f64().map(|f| f as f32))
                        .collect();
                    results.push(vec);
                }
            }
        }
        Ok(results)
    }
}

#[derive(Default)]
struct ToolCallAccumulator {
    id: String,
    name: String,
    arguments: String,
}

fn parse_openai_response(data: &serde_json::Value) -> Result<LlmResponse> {
    let choice = data["choices"]
        .as_array()
        .and_then(|c| c.first())
        .ok_or_else(|| anyhow::anyhow!("No choices in response"))?;

    let message = &choice["message"];
    let content = message["content"].as_str().map(|s| s.to_string());
    let finish_reason = choice["finish_reason"].as_str().map(|s| s.to_string());

    let tool_calls = if let Some(tcs) = message["tool_calls"].as_array() {
        tcs.iter()
            .map(|tc| ToolCall {
                id: tc["id"].as_str().unwrap_or("").to_string(),
                r#type: "function".to_string(),
                function: FunctionCall {
                    name: tc["function"]["name"].as_str().unwrap_or("").to_string(),
                    arguments: tc["function"]["arguments"].as_str().unwrap_or("{}").to_string(),
                },
            })
            .collect()
    } else {
        Vec::new()
    };

    let usage = data.get("usage").and_then(|u| {
        Some(Usage {
            prompt_tokens: u["prompt_tokens"].as_u64()?,
            completion_tokens: u["completion_tokens"].as_u64()?,
            total_tokens: u["total_tokens"].as_u64()?,
        })
    });

    Ok(LlmResponse {
        content,
        tool_calls,
        usage,
        finish_reason,
    })
}
