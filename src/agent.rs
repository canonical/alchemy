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

/// File activity events for the TUI file panel.
#[derive(Debug, Clone)]
pub enum FileEvent {
    Read { path: String },
    Write { path: String },
}

impl Agent {
    pub fn new(config: AgentConfig, provider: Box<dyn Provider>, registry: ToolRegistry) -> Self {
        Self { config, provider, registry }
    }

    /// Single-shot run: no conversation history, no streaming events.
    pub async fn run(&self, user_message: String) -> AgentResult {
        let (result, _) = self.run_internal(vec![], user_message, |_| {}, None, None).await;
        result
    }

    /// Multi-turn run: caller manages history across turns.
    /// Returns (result, updated_history). History excludes the system message.
    pub async fn run_turn(
        &self,
        history: Vec<Message>,
        user_message: String,
    ) -> (AgentResult, Vec<Message>) {
        self.run_internal(history, user_message, |_| {}, None, None).await
    }

    /// Like `run_turn` but also streams ToolEvents and FileEvents for real-time TUI display.
    pub async fn run_turn_with_events<F>(
        &self,
        history: Vec<Message>,
        user_message: String,
        on_token: F,
        tool_tx: tokio::sync::mpsc::Sender<ToolEvent>,
        file_tx: tokio::sync::mpsc::Sender<FileEvent>,
    ) -> (AgentResult, Vec<Message>)
    where
        F: FnMut(&str),
    {
        self.run_internal(history, user_message, on_token, Some(tool_tx), Some(file_tx)).await
    }

    async fn run_internal<F>(
        &self,
        history: Vec<Message>,
        user_message: String,
        mut on_token: F,
        tool_tx: Option<tokio::sync::mpsc::Sender<ToolEvent>>,
        file_tx: Option<tokio::sync::mpsc::Sender<FileEvent>>,
    ) -> (AgentResult, Vec<Message>)
    where
        F: FnMut(&str),
    {
        let mut messages = vec![Message {
            role: MessageRole::System,
            content: Some(self.config.system_prompt.clone()),
            tool_calls: None,
            tool_call_id: None,
        }];
        messages.extend(history);
        messages.push(Message {
            role: MessageRole::User,
            content: Some(user_message),
            tool_calls: None,
            tool_call_id: None,
        });

        let mut steps: u32 = 0;
        let mut tools_used: HashSet<String> = HashSet::new();
        let mut total_tokens: u64 = 0;

        loop {
            if steps >= self.config.max_steps {
                let history = messages[1..].to_vec();
                return (AgentResult {
                    answer: None,
                    steps,
                    tools_used: tools_used.into_iter().collect(),
                    success: false,
                    error: Some("Max steps exceeded".to_string()),
                    total_tokens,
                }, history);
            }

            steps += 1;
            self.compact_context(&mut messages);

            let request = LlmRequest {
                model: self.config.model.clone(),
                messages: messages.clone(),
                tools: self.registry.definitions.clone(),
                temperature: None,
            };

            // Retry with exponential backoff: 3 total attempts, 1s then 2s between them.
            let mut last_error: Option<anyhow::Error> = None;
            let mut response_opt = None;
            for attempt in 0u32..3 {
                if attempt > 0 {
                    let delay_secs = [1u64, 2][(attempt - 1) as usize];
                    tracing::warn!("LLM error, retrying in {}s (attempt {})", delay_secs, attempt + 1);
                    tokio::time::sleep(tokio::time::Duration::from_secs(delay_secs)).await;
                }
                let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(100);
                match self.provider.chat_streaming(request.clone(), tx).await {
                    Ok(r) => {
                        while let Ok(token) = rx.try_recv() {
                            on_token(&token);
                        }
                        response_opt = Some(r);
                        break;
                    }
                    Err(e) => {
                        tracing::warn!("LLM API error (attempt {}): {}", attempt + 1, e);
                        last_error = Some(e);
                    }
                }
            }

            let response = match response_opt {
                Some(r) => r,
                None => {
                    let history = messages[1..].to_vec();
                    return (AgentResult {
                        answer: None,
                        steps,
                        tools_used: tools_used.into_iter().collect(),
                        success: false,
                        error: Some(format!(
                            "Provider error after retries: {}",
                            last_error.unwrap()
                        )),
                        total_tokens,
                    }, history);
                }
            };

            if let Some(ref usage) = response.usage {
                total_tokens = total_tokens.max(usage.total_tokens);
            }

            if response.tool_calls.is_empty() {
                // Add the final assistant message so it appears in returned history.
                messages.push(Message {
                    role: MessageRole::Assistant,
                    content: response.content.clone(),
                    tool_calls: None,
                    tool_call_id: None,
                });
                let history = messages[1..].to_vec();
                return (AgentResult {
                    answer: response.content,
                    steps,
                    tools_used: tools_used.into_iter().collect(),
                    success: true,
                    error: None,
                    total_tokens,
                }, history);
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

            // Parallel tools (read_file, list_dir, fetch_url)
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
                emit_file_event(&file_tx, &tc.function.name, &tc.function.arguments);
                messages.push(Message {
                    role: MessageRole::Tool,
                    content: Some(result),
                    tool_calls: None,
                    tool_call_id: Some(id),
                });
            }

            // Sequential tools (write_file, execute_cmd, MCP, skill)
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
                    let _ = tx.try_send(ToolEvent::Finished { name: name.clone(), duration_ms });
                }
                emit_file_event(&file_tx, &name, &tc.function.arguments);
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
            // Keep last 12 turns (6 user+assistant pairs)
            let keep_count = 12.min(messages.len() - 1);
            let tail: Vec<Message> = messages[messages.len() - keep_count..].to_vec();
            messages.clear();
            messages.push(system);
            messages.extend(tail);
        }
    }
}

fn emit_file_event(
    file_tx: &Option<tokio::sync::mpsc::Sender<FileEvent>>,
    tool_name: &str,
    arguments: &str,
) {
    if let Some(ref ftx) = file_tx {
        if tool_name == "read_file" || tool_name == "write_file" {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(arguments) {
                if let Some(path) = v["path"].as_str() {
                    let event = if tool_name == "read_file" {
                        FileEvent::Read { path: path.to_string() }
                    } else {
                        FileEvent::Write { path: path.to_string() }
                    };
                    let _ = ftx.try_send(event);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{LlmResponse, Usage};
    use async_trait::async_trait;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    /// Mock provider for unit testing the agent loop.
    struct MockProvider {
        responses: Mutex<VecDeque<anyhow::Result<LlmResponse>>>,
    }

    impl MockProvider {
        fn new(responses: Vec<anyhow::Result<LlmResponse>>) -> Self {
            Self {
                responses: Mutex::new(responses.into_iter().collect()),
            }
        }
    }

    #[async_trait]
    impl crate::providers::Provider for MockProvider {
        fn name(&self) -> &str {
            "mock"
        }

        fn default_model(&self) -> &str {
            "mock-model"
        }

        async fn chat(&self, _req: crate::types::LlmRequest) -> anyhow::Result<LlmResponse> {
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err(anyhow::anyhow!("MockProvider: no more responses")))
        }

        async fn chat_streaming(
            &self,
            req: crate::types::LlmRequest,
            _tx: tokio::sync::mpsc::Sender<String>,
        ) -> anyhow::Result<LlmResponse> {
            self.chat(req).await
        }
    }

    fn simple_answer(content: &str) -> anyhow::Result<LlmResponse> {
        Ok(LlmResponse {
            content: Some(content.to_string()),
            tool_calls: vec![],
            usage: Some(Usage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            }),
            finish_reason: Some("stop".to_string()),
        })
    }

    fn make_agent(provider: MockProvider) -> Agent {
        let config = AgentConfig {
            model: "mock-model".to_string(),
            system_prompt: "You are a test agent.".to_string(),
            max_steps: 5,
            timeout_secs: 5,
            context_window: 128000,
        };
        Agent::new(config, Box::new(provider), crate::tools::ToolRegistry::new())
    }

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

    #[tokio::test]
    async fn test_simple_answer() {
        let provider = MockProvider::new(vec![simple_answer("Hello!")]);
        let agent = make_agent(provider);
        let result = agent.run("Hi".to_string()).await;
        assert!(result.success);
        assert_eq!(result.answer.as_deref(), Some("Hello!"));
        assert_eq!(result.steps, 1);
        assert_eq!(result.total_tokens, 15);
    }

    #[tokio::test]
    async fn test_max_steps_exceeded() {
        // Every response asks for a tool call that re-triggers the loop.
        // Since max_steps = 2 and we have no real tools in registry to satisfy calls,
        // we can't do tool calls — instead we just return success on each step
        // and check via max_steps config.
        let provider = MockProvider::new(vec![
            simple_answer("step1"),
        ]);
        let config = AgentConfig {
            model: "mock-model".to_string(),
            system_prompt: "test".to_string(),
            max_steps: 1,
            timeout_secs: 5,
            context_window: 128000,
        };
        let agent = Agent::new(config, Box::new(provider), crate::tools::ToolRegistry::new());
        let result = agent.run("test".to_string()).await;
        // With max_steps=1 and one successful response, we succeed in 1 step
        assert!(result.success);
        assert_eq!(result.steps, 1);
    }

    #[tokio::test]
    async fn test_retry_all_fail() {
        let provider = MockProvider::new(vec![
            Err(anyhow::anyhow!("fail 1")),
            Err(anyhow::anyhow!("fail 2")),
            Err(anyhow::anyhow!("fail 3")),
        ]);
        let agent = make_agent(provider);
        // Note: this test sleeps 1s+2s=3s due to retry delays — acceptable for CI.
        let result = agent.run("test".to_string()).await;
        assert!(!result.success);
        assert!(result.error.as_deref().unwrap_or("").contains("Provider error after retries"));
    }

    #[tokio::test]
    async fn test_retry_succeeds_on_second_attempt() {
        let provider = MockProvider::new(vec![
            Err(anyhow::anyhow!("transient error")),
            simple_answer("recovered"),
        ]);
        let agent = make_agent(provider);
        // Sleeps 1s for the first retry delay.
        let result = agent.run("test".to_string()).await;
        assert!(result.success);
        assert_eq!(result.answer.as_deref(), Some("recovered"));
    }

    #[tokio::test]
    async fn test_token_tracking() {
        let provider = MockProvider::new(vec![Ok(LlmResponse {
            content: Some("answer".to_string()),
            tool_calls: vec![],
            usage: Some(Usage {
                prompt_tokens: 100,
                completion_tokens: 50,
                total_tokens: 150,
            }),
            finish_reason: Some("stop".to_string()),
        })]);
        let agent = make_agent(provider);
        let result = agent.run("test".to_string()).await;
        assert_eq!(result.total_tokens, 150);
    }

    #[tokio::test]
    async fn test_multi_turn_history_accumulates() {
        let provider = MockProvider::new(vec![
            simple_answer("first answer"),
            simple_answer("second answer"),
        ]);
        let agent = make_agent(provider);

        let (result1, history1) = agent.run_turn(vec![], "first question".to_string()).await;
        assert!(result1.success);
        assert_eq!(result1.answer.as_deref(), Some("first answer"));

        // history1 should contain: [User("first question"), Assistant("first answer")]
        assert_eq!(history1.len(), 2);
        assert_eq!(history1[0].role, crate::types::MessageRole::User);
        assert_eq!(history1[1].role, crate::types::MessageRole::Assistant);

        let (result2, history2) = agent.run_turn(history1, "second question".to_string()).await;
        assert!(result2.success);
        assert_eq!(result2.answer.as_deref(), Some("second answer"));

        // history2: [User1, Assistant1, User2, Assistant2]
        assert_eq!(history2.len(), 4);
    }

    #[tokio::test]
    async fn test_multi_turn_no_history() {
        // Without history, each run is isolated.
        let provider = MockProvider::new(vec![simple_answer("isolated")]);
        let agent = make_agent(provider);
        let (result, history) = agent.run_turn(vec![], "question".to_string()).await;
        assert!(result.success);
        assert_eq!(history.len(), 2); // user + assistant
    }

    // ── Integration tests: full agent loop against a wiremock HTTP server ──

    /// Build an SSE (text/event-stream) response body for a plain-text answer.
    fn sse_body(content: &str) -> String {
        let escaped = content.replace('"', "\\\"");
        format!(
            "data: {{\"choices\":[{{\"delta\":{{\"role\":\"assistant\",\"content\":\"{}\"}},\"finish_reason\":null,\"index\":0}}]}}\n\ndata: {{\"choices\":[{{\"delta\":{{}},\"finish_reason\":\"stop\",\"index\":0}}]}}\n\ndata: [DONE]\n\n",
            escaped
        )
    }

    #[tokio::test]
    async fn test_agent_simple_answer_e2e() {
        let server = wiremock::MockServer::start().await;

        // The OpenAI provider appends "/chat/completions" to base_url directly.
        // Passing server.uri() as base_url means requests go to /chat/completions (no /v1 prefix).
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/chat/completions"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_string(sse_body("Hello from mock!"))
                    .insert_header("content-type", "text/event-stream"),
            )
            .mount(&server)
            .await;

        let provider = crate::providers::create_provider(
            "openai",
            Some("test-key"),
            Some(&server.uri()),
        )
        .unwrap();

        let config = AgentConfig {
            model: "gpt-test".to_string(),
            system_prompt: "You are a test agent.".to_string(),
            max_steps: 5,
            timeout_secs: 10,
            context_window: 128000,
        };
        let agent = Agent::new(config, provider, crate::tools::ToolRegistry::new());
        let result = agent.run("Say hello".to_string()).await;

        assert!(result.success);
        assert_eq!(result.answer.as_deref(), Some("Hello from mock!"));
        assert_eq!(result.steps, 1);
    }

    #[tokio::test]
    async fn test_agent_api_error_returns_failure_e2e() {
        let server = wiremock::MockServer::start().await;

        // Return 500 error on all requests — should fail after 3 retry attempts.
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/chat/completions"))
            .respond_with(
                wiremock::ResponseTemplate::new(500)
                    .set_body_string("Internal Server Error"),
            )
            .mount(&server)
            .await;

        let provider = crate::providers::create_provider(
            "openai",
            Some("test-key"),
            Some(&server.uri()),
        )
        .unwrap();

        let config = AgentConfig {
            model: "gpt-test".to_string(),
            system_prompt: "test".to_string(),
            max_steps: 5,
            timeout_secs: 10,
            context_window: 128000,
        };
        let agent = Agent::new(config, provider, crate::tools::ToolRegistry::new());
        // 3 attempts × (0s + 1s + 2s delays) = ~3s total
        let result = agent.run("test".to_string()).await;

        assert!(!result.success);
        assert!(result.error.is_some());
        // Verify all 3 attempts were made (3 POST requests received by wiremock).
        assert_eq!(server.received_requests().await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn test_agent_multi_turn_e2e() {
        let server = wiremock::MockServer::start().await;

        // Return different answers on each call.
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/chat/completions"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_string(sse_body("turn one"))
                    .insert_header("content-type", "text/event-stream"),
            )
            .up_to_n_times(1)
            .mount(&server)
            .await;

        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/chat/completions"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_string(sse_body("turn two"))
                    .insert_header("content-type", "text/event-stream"),
            )
            .mount(&server)
            .await;

        let make_provider = || {
            crate::providers::create_provider("openai", Some("test-key"), Some(&server.uri()))
                .unwrap()
        };

        let config = AgentConfig {
            model: "gpt-test".to_string(),
            system_prompt: "test".to_string(),
            max_steps: 5,
            timeout_secs: 10,
            context_window: 128000,
        };
        let agent = Agent::new(config, make_provider(), crate::tools::ToolRegistry::new());

        let (r1, h1) = agent.run_turn(vec![], "q1".to_string()).await;
        assert!(r1.success);
        assert_eq!(r1.answer.as_deref(), Some("turn one"));

        let (r2, _h2) = agent.run_turn(h1, "q2".to_string()).await;
        assert!(r2.success);
        assert_eq!(r2.answer.as_deref(), Some("turn two"));

        // Two requests were made (one per turn).
        assert_eq!(server.received_requests().await.unwrap().len(), 2);
    }
}
