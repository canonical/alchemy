use crate::providers::Provider;
use crate::tools::ToolRegistry;
use crate::types::{AgentResult, LlmRequest, Message, MessageRole};
use std::collections::HashSet;

pub struct AgentConfig {
    pub model: String,
    pub system_prompt: String,
    pub max_steps: u32,
    pub timeout_secs: u64,
    pub context_window: u64,
}

pub struct Agent {
    pub config: AgentConfig,
    pub provider: Box<dyn Provider>,
    pub registry: ToolRegistry,
}

/// Real-time tool lifecycle events sent to callers that need live progress (e.g. TUI).
#[derive(Debug, Clone)]
pub enum ToolEvent {
    Started { name: String },
    Finished { name: String, duration_ms: u64 },
}

impl Agent {
    pub fn new(config: AgentConfig, provider: Box<dyn Provider>, registry: ToolRegistry) -> Self {
        Self { config, provider, registry }
    }

    pub async fn run(&self, user_message: String) -> AgentResult {
        self.run_internal(user_message, |_| {}, None).await
    }

    pub async fn run_with_callback<F>(&self, user_message: String, on_token: F) -> AgentResult
    where
        F: FnMut(&str),
    {
        self.run_internal(user_message, on_token, None).await
    }

    /// Like `run` but also streams `ToolEvent`s for real-time display.
    pub async fn run_with_events<F>(
        &self,
        user_message: String,
        on_token: F,
        tool_tx: tokio::sync::mpsc::Sender<ToolEvent>,
    ) -> AgentResult
    where
        F: FnMut(&str),
    {
        self.run_internal(user_message, on_token, Some(tool_tx)).await
    }

    async fn run_internal<F>(
        &self,
        user_message: String,
        mut on_token: F,
        tool_tx: Option<tokio::sync::mpsc::Sender<ToolEvent>>,
    ) -> AgentResult
    where
        F: FnMut(&str),
    {
        let mut messages = vec![
            Message {
                role: MessageRole::System,
                content: Some(self.config.system_prompt.clone()),
                tool_calls: None,
                tool_call_id: None,
            },
            Message {
                role: MessageRole::User,
                content: Some(user_message),
                tool_calls: None,
                tool_call_id: None,
            },
        ];

        let mut steps: u32 = 0;
        let mut tools_used: HashSet<String> = HashSet::new();

        loop {
            if steps >= self.config.max_steps {
                return AgentResult {
                    answer: None,
                    steps,
                    tools_used: tools_used.into_iter().collect(),
                    success: false,
                    error: Some("Max steps exceeded".to_string()),
                };
            }

            steps += 1;
            self.compact_context(&mut messages);

            let request = LlmRequest {
                model: self.config.model.clone(),
                messages: messages.clone(),
                tools: self.registry.definitions.clone(),
                temperature: None,
            };

            let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(100);
            let response = self.provider.chat_streaming(request, tx).await;

            while let Ok(token) = rx.try_recv() {
                on_token(&token);
            }

            let response = match response {
                Ok(r) => r,
                Err(e) => {
                    return AgentResult {
                        answer: None,
                        steps,
                        tools_used: tools_used.into_iter().collect(),
                        success: false,
                        error: Some(format!("Provider error: {}", e)),
                    };
                }
            };

            if response.tool_calls.is_empty() {
                return AgentResult {
                    answer: response.content,
                    steps,
                    tools_used: tools_used.into_iter().collect(),
                    success: true,
                    error: None,
                };
            }

            messages.push(Message {
                role: MessageRole::Assistant,
                content: response.content.clone(),
                tool_calls: Some(response.tool_calls.clone()),
                tool_call_id: None,
            });

            let (parallel, sequential): (Vec<_>, Vec<_>) = response
                .tool_calls
                .iter()
                .partition(|tc| ToolRegistry::is_parallel_safe(&tc.function.name));

            // Parallel tools
            let parallel_results: Vec<(String, String)> = if !parallel.is_empty() {
                let futs: Vec<_> = parallel
                    .iter()
                    .map(|tc| {
                        let name = tc.function.name.clone();
                        let id = tc.id.clone();
                        let args = tc.function.arguments.clone();
                        let timeout = self.config.timeout_secs;
                        let tx = tool_tx.clone();
                        async move {
                            let start = std::time::Instant::now();
                            if let Some(ref tx) = tx {
                                let _ = tx.try_send(ToolEvent::Started { name: name.clone() });
                            }
                            let result =
                                match crate::tools::builtin::execute(&name, &args, timeout).await {
                                    Ok(r) => r,
                                    Err(e) => format!("{{\"error\": \"{}\"}}", e),
                                };
                            let duration_ms = start.elapsed().as_millis() as u64;
                            if let Some(ref tx) = tx {
                                let _ = tx.try_send(ToolEvent::Finished {
                                    name: name.clone(),
                                    duration_ms,
                                });
                            }
                            (id, result)
                        }
                    })
                    .collect();
                futures::future::join_all(futs).await
            } else {
                Vec::new()
            };

            for (id, result) in parallel_results {
                let tc = parallel.iter().find(|t| t.id == id).unwrap();
                tools_used.insert(tc.function.name.clone());
                messages.push(Message {
                    role: MessageRole::Tool,
                    content: Some(result),
                    tool_calls: None,
                    tool_call_id: Some(id),
                });
            }

            // Sequential tools
            for tc in sequential {
                let name = tc.function.name.clone();
                let start = std::time::Instant::now();
                if let Some(ref tx) = tool_tx {
                    let _ = tx.try_send(ToolEvent::Started { name: name.clone() });
                }
                tools_used.insert(name.clone());
                let result = match self.registry.dispatch(tc, self.config.timeout_secs).await {
                    Ok(r) => r,
                    Err(e) => format!("{{\"error\": \"{}\"}}", e),
                };
                let duration_ms = start.elapsed().as_millis() as u64;
                if let Some(ref tx) = tool_tx {
                    let _ = tx.try_send(ToolEvent::Finished { name, duration_ms });
                }
                messages.push(Message {
                    role: MessageRole::Tool,
                    content: Some(result),
                    tool_calls: None,
                    tool_call_id: Some(tc.id.clone()),
                });
            }
        }
    }

    fn compact_context(&self, messages: &mut Vec<Message>) {
        let total_chars: usize = messages
            .iter()
            .map(|m| m.content.as_ref().map(|c| c.len()).unwrap_or(0) + 50)
            .sum();
        let estimated_tokens = total_chars / 4;
        let threshold = (self.config.context_window as f64 * 0.85) as usize;

        if estimated_tokens > threshold {
            let system = messages[0].clone();
            let keep_count = 12.min(messages.len() - 1);
            let tail: Vec<Message> = messages[messages.len() - keep_count..].to_vec();
            messages.clear();
            messages.push(system);
            messages.extend(tail);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_config_defaults() {
        let config = AgentConfig {
            model: "test".to_string(),
            system_prompt: "test".to_string(),
            max_steps: 30,
            timeout_secs: 30,
            context_window: 128000,
        };
        assert_eq!(config.max_steps, 30);
    }
}
