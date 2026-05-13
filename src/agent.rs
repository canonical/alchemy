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

impl Agent {
    pub fn new(config: AgentConfig, provider: Box<dyn Provider>, registry: ToolRegistry) -> Self {
        Self { config, provider, registry }
    }

    pub async fn run(&self, user_message: String) -> AgentResult {
        self.run_with_callback(user_message, |_| {}).await
    }

    pub async fn run_with_callback<F>(&self, user_message: String, mut on_token: F) -> AgentResult
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

            // Context compaction at 85% of context window
            self.compact_context(&mut messages);

            let request = LlmRequest {
                model: self.config.model.clone(),
                messages: messages.clone(),
                tools: self.registry.definitions.clone(),
                temperature: None,
            };

            // Use streaming
            let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(100);
            let response = self.provider.chat_streaming(request, tx).await;

            // Drain any remaining tokens
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

            // No tool calls = final answer
            if response.tool_calls.is_empty() {
                return AgentResult {
                    answer: response.content,
                    steps,
                    tools_used: tools_used.into_iter().collect(),
                    success: true,
                    error: None,
                };
            }

            // Add assistant message with tool calls
            messages.push(Message {
                role: MessageRole::Assistant,
                content: response.content.clone(),
                tool_calls: Some(response.tool_calls.clone()),
                tool_call_id: None,
            });

            // Execute tool calls (parallel for safe tools, sequential otherwise)
            let (parallel, sequential): (Vec<_>, Vec<_>) = response.tool_calls.iter()
                .partition(|tc| ToolRegistry::is_parallel_safe(&tc.function.name));

            // Execute parallel tools concurrently
            let parallel_results: Vec<(String, String)> = if !parallel.is_empty() {
                let futs: Vec<_> = parallel.iter().map(|tc| {
                    let name = tc.function.name.clone();
                    let id = tc.id.clone();
                    let args = tc.function.arguments.clone();
                    let timeout = self.config.timeout_secs;
                    async move {
                        let result = match crate::tools::builtin::execute(&name, &args, timeout).await {
                            Ok(r) => r,
                            Err(e) => format!("{{\"error\": \"{}\"}}", e),
                        };
                        (id, result)
                    }
                }).collect();
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

            // Execute sequential tools
            for tc in sequential {
                tools_used.insert(tc.function.name.clone());
                let result = match self.registry.dispatch(tc, self.config.timeout_secs).await {
                    Ok(r) => r,
                    Err(e) => format!("{{\"error\": \"{}\"}}", e),
                };
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
        // Estimate token count (rough: 4 chars per token)
        let total_chars: usize = messages.iter()
            .map(|m| m.content.as_ref().map(|c| c.len()).unwrap_or(0) + 50)
            .sum();
        let estimated_tokens = total_chars / 4;
        let threshold = (self.config.context_window as f64 * 0.85) as usize;

        if estimated_tokens > threshold {
            // Keep system prompt (first) + last 6 logical turns
            let system = messages[0].clone();
            let keep_count = 12.min(messages.len() - 1); // ~6 turns = 12 messages
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
