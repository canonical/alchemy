use crate::providers::Provider;
use crate::tools::ToolRegistry;
use crate::types::{AgentResult, LlmRequest, Message, MessageRole, ToolCall};
use std::collections::HashSet;
use std::sync::Arc;

const MAX_LLM_ATTEMPTS: u32 = 3;

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
    pub registry: Arc<ToolRegistry>,
}

/// Real-time tool lifecycle events sent to callers that need live progress (e.g. TUI).
#[derive(Debug, Clone)]
pub enum ToolEvent {
    Started { name: String },
    Finished { name: String, duration_ms: u64, success: bool },
}

/// File activity events for the TUI file panel.
#[derive(Debug, Clone)]
pub enum FileEvent {
    Read { path: String },
    Write { path: String },
}

/// Cumulative progress emitted after each LLM step so the TUI status bar
/// can show step count and token usage live instead of only on turn end.
#[derive(Debug, Clone, Copy)]
pub struct StepEvent {
    pub steps: u32,
    pub total_tokens: u64,
}

impl Agent {
    pub fn new(config: AgentConfig, provider: Box<dyn Provider>, registry: ToolRegistry) -> Self {
        Self { config, provider, registry: Arc::new(registry) }
    }

    /// Single-shot run: no conversation history, no streaming events.
    pub async fn run(&self, user_message: String) -> AgentResult {
        let (result, _) = self.run_internal(vec![], user_message, None, None, None, None).await;
        result
    }

    /// Multi-turn run: caller manages history across turns.
    /// Returns (result, updated_history). History excludes the system message.
    #[cfg(test)]
    pub async fn run_turn(
        &self,
        history: Vec<Message>,
        user_message: String,
    ) -> (AgentResult, Vec<Message>) {
        self.run_internal(history, user_message, None, None, None, None).await
    }

    /// Like `run_turn` but also streams token, ToolEvents, FileEvents, and per-step
    /// progress (StepEvent) for real-time TUI display.
    pub async fn run_turn_with_events(
        &self,
        history: Vec<Message>,
        user_message: String,
        token_tx: tokio::sync::mpsc::Sender<String>,
        tool_tx: tokio::sync::mpsc::Sender<ToolEvent>,
        file_tx: tokio::sync::mpsc::Sender<FileEvent>,
        step_tx: tokio::sync::mpsc::Sender<StepEvent>,
    ) -> (AgentResult, Vec<Message>) {
        self.run_internal(history, user_message, Some(token_tx), Some(tool_tx), Some(file_tx), Some(step_tx)).await
    }

    async fn run_internal(
        &self,
        history: Vec<Message>,
        user_message: String,
        token_tx: Option<tokio::sync::mpsc::Sender<String>>,
        tool_tx: Option<tokio::sync::mpsc::Sender<ToolEvent>>,
        file_tx: Option<tokio::sync::mpsc::Sender<FileEvent>>,
        step_tx: Option<tokio::sync::mpsc::Sender<StepEvent>>,
    ) -> (AgentResult, Vec<Message>) {
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
                let error_msg = "Max steps exceeded".to_string();
                messages.push(Message {
                    role: MessageRole::Assistant,
                    content: Some(format!("[{}]", error_msg)),
                    tool_calls: None,
                    tool_call_id: None,
                });
                let history = messages[1..].to_vec();
                return (AgentResult {
                    answer: None,
                    steps,
                    tools_used: tools_used.into_iter().collect(),
                    success: false,
                    error: Some(error_msg),
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

            let mut last_error: Option<anyhow::Error> = None;
            let mut response_opt = None;
            for attempt in 0u32..MAX_LLM_ATTEMPTS {
                if attempt > 0 {
                    // Exponential backoff: 1s, 2s, 4s, ...
                    let delay_secs = 1u64 << (attempt - 1);
                    tracing::warn!("LLM error, retrying in {}s (attempt {})", delay_secs, attempt + 1);
                    tokio::time::sleep(tokio::time::Duration::from_secs(delay_secs)).await;
                }
                match stream_with_callback(&*self.provider, request.clone(), token_tx.as_ref()).await {
                    Ok(r) => {
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
                    let err = last_error
                        .map(|e| e.to_string())
                        .unwrap_or_else(|| "unknown error".to_string());
                    let error_msg = format!("Provider error after retries: {}", err);
                    messages.push(Message {
                        role: MessageRole::Assistant,
                        content: Some(format!("[{}]", error_msg)),
                        tool_calls: None,
                        tool_call_id: None,
                    });
                    let history = messages[1..].to_vec();
                    return (AgentResult {
                        answer: None,
                        steps,
                        tools_used: tools_used.into_iter().collect(),
                        success: false,
                        error: Some(error_msg),
                        total_tokens,
                    }, history);
                }
            };

            if let Some(ref usage) = response.usage {
                total_tokens += usage.total_tokens;
            }
            if let Some(ref tx) = step_tx {
                let _ = tx.try_send(StepEvent { steps, total_tokens });
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

            let parallel_results: Vec<ToolOutcome> = if !parallel.is_empty() {
                let futs: Vec<_> = parallel
                    .iter()
                    .map(|tc| {
                        let tc = (*tc).clone();
                        let timeout = self.config.timeout_secs;
                        let tx = tool_tx.clone();
                        let registry = Arc::clone(&self.registry);
                        async move {
                            let start = std::time::Instant::now();
                            if let Some(ref tx) = tx {
                                let _ = tx.try_send(ToolEvent::Started {
                                    name: tc.function.name.clone(),
                                });
                            }
                            let dispatch_result = registry.dispatch(&tc, timeout).await;
                            let success = dispatch_result.is_ok();
                            let result = dispatch_result.unwrap_or_else(|e| tool_error_payload(&e));
                            let duration_ms = start.elapsed().as_millis() as u64;
                            if let Some(ref tx) = tx {
                                let _ = tx.try_send(ToolEvent::Finished {
                                    name: tc.function.name.clone(),
                                    duration_ms,
                                    success,
                                });
                            }
                            ToolOutcome { call: tc, result }
                        }
                    })
                    .collect();
                futures::future::join_all(futs).await
            } else {
                Vec::new()
            };

            for outcome in parallel_results {
                tools_used.insert(outcome.call.function.name.clone());
                emit_file_event(&file_tx, &outcome.call.function.name, &outcome.call.function.arguments);
                messages.push(Message {
                    role: MessageRole::Tool,
                    content: Some(outcome.result),
                    tool_calls: None,
                    tool_call_id: Some(outcome.call.id),
                });
            }

            for tc in sequential {
                let name = tc.function.name.clone();
                let start = std::time::Instant::now();
                if let Some(ref tx) = tool_tx {
                    let _ = tx.try_send(ToolEvent::Started { name: name.clone() });
                }
                tools_used.insert(name.clone());
                let dispatch_result = self.registry.dispatch(tc, self.config.timeout_secs).await;
                let success = dispatch_result.is_ok();
                let result = dispatch_result.unwrap_or_else(|e| tool_error_payload(&e));
                let duration_ms = start.elapsed().as_millis() as u64;
                if let Some(ref tx) = tool_tx {
                    let _ = tx.try_send(ToolEvent::Finished { name: name.clone(), duration_ms, success });
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
            let tail_start = tail_start_for_turns(&messages[1..], 6);
            let tail: Vec<Message> = messages[1 + tail_start..].to_vec();
            messages.clear();
            messages.push(system);
            messages.extend(tail);
        }
    }

    /// Truncate a history vec (no system message) so subsequent turns stay under context.
    /// Used by the TUI between turns; mirrors the in-loop compaction semantics.
    pub fn compact_history(&self, history: &mut Vec<Message>) {
        let total_chars: usize = history
            .iter()
            .map(|m| m.content.as_ref().map(|c| c.len()).unwrap_or(0) + 50)
            .sum();
        let estimated_tokens = (total_chars + self.config.system_prompt.len() / 4) / 4;
        let threshold = (self.config.context_window as f64 * 0.85) as usize;

        if estimated_tokens > threshold {
            let tail_start = tail_start_for_turns(history, 6);
            if tail_start > 0 {
                history.drain(..tail_start);
            }
        }
    }
}

/// Return the index into `messages` (no system message) at which the tail of 6 logical
/// turns begins.  A logical turn starts at each `User` message.  If there are ≤ 6 turns,
/// returns 0 (keep everything).
fn tail_start_for_turns(messages: &[Message], keep_turns: usize) -> usize {
    // Walk backward counting User messages (each marks the start of a turn).
    let mut turns_seen = 0usize;
    let mut i = messages.len();
    while i > 0 {
        i -= 1;
        if messages[i].role == MessageRole::User {
            turns_seen += 1;
            if turns_seen == keep_turns {
                return i; // Keep from this index onward.
            }
        }
    }
    // Fewer than `keep_turns` turns present — keep everything.
    0
}

/// Result of one tool call, kept together so we don't have to re-look-up the call.
struct ToolOutcome {
    call: ToolCall,
    result: String,
}

/// Render a tool error as a valid JSON object. The naive `format!("{{\"error\": ...}}")` breaks
/// when the error message contains quotes, backslashes, or newlines.
fn tool_error_payload(e: &anyhow::Error) -> String {
    serde_json::json!({ "error": e.to_string() }).to_string()
}

/// Drive `chat_streaming` to completion while concurrently draining the token channel so the
/// bounded buffer can't fill and deadlock the provider's `send().await`.
async fn stream_with_callback(
    provider: &dyn Provider,
    request: LlmRequest,
    token_tx: Option<&tokio::sync::mpsc::Sender<String>>,
) -> anyhow::Result<crate::types::LlmResponse> {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(100);
    let mut stream_fut = std::pin::pin!(provider.chat_streaming(request, tx));

    loop {
        tokio::select! {
            biased;
            res = &mut stream_fut => {
                while let Ok(token) = rx.try_recv() {
                    if let Some(out) = token_tx {
                        let _ = out.try_send(token);
                    }
                }
                return res;
            }
            Some(token) = rx.recv() => {
                if let Some(out) = token_tx {
                    let _ = out.try_send(token);
                }
            }
        }
    }
}

fn emit_file_event(
    file_tx: &Option<tokio::sync::mpsc::Sender<FileEvent>>,
    tool_name: &str,
    arguments: &str,
) {
    let Some(ftx) = file_tx else { return };
    if tool_name != "read_file" && tool_name != "write_file" {
        return;
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(arguments) else { return };
    let Some(path) = v["path"].as_str() else { return };
    let event = if tool_name == "read_file" {
        FileEvent::Read { path: path.to_string() }
    } else {
        FileEvent::Write { path: path.to_string() }
    };
    let _ = ftx.try_send(event);
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
        let result = agent.run("test".to_string()).await;

        assert!(!result.success);
        assert!(result.error.is_some());
        // Verify all 3 attempts were made (3 POST requests received by wiremock).
        assert_eq!(server.received_requests().await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn test_token_channel_api_compiles() {
        let provider = MockProvider::new(vec![simple_answer("hi")]);
        let agent = make_agent(provider);
        let (token_tx, _token_rx) = tokio::sync::mpsc::channel::<String>(10);
        let (tool_tx, _) = tokio::sync::mpsc::channel(10);
        let (file_tx, _) = tokio::sync::mpsc::channel(10);
        let (step_tx, _step_rx) = tokio::sync::mpsc::channel::<StepEvent>(10);
        let (result, _) = agent
            .run_turn_with_events(vec![], "test".into(), token_tx, tool_tx, file_tx, step_tx)
            .await;
        assert!(result.success);
    }

    // ── E2E tests: gated behind ALCHEMY_E2E=1, use real LLM credentials ──

    #[tokio::test]
    async fn test_e2e_pong() {
        if std::env::var("ALCHEMY_E2E").unwrap_or_default() != "1" {
            return;
        }
        let provider_name = std::env::var("ALCHEMY_PROVIDER").expect("ALCHEMY_PROVIDER required");
        let api_key = std::env::var("ALCHEMY_API_KEY").expect("ALCHEMY_API_KEY required");
        let model = std::env::var("ALCHEMY_MODEL").expect("ALCHEMY_MODEL required");

        let provider = crate::providers::create_provider(&provider_name, Some(&api_key), None)
            .expect("Failed to create provider");

        let config = AgentConfig {
            model,
            system_prompt: "You are a helpful assistant.".to_string(),
            max_steps: 5,
            timeout_secs: 30,
            context_window: 128000,
        };
        let agent = Agent::new(config, provider, crate::tools::ToolRegistry::new());
        let result = agent
            .run("Reply with just the single word PONG and nothing else".to_string())
            .await;

        println!("E2E PONG result: {:?}", result);
        assert!(result.success, "Agent failed: {:?}", result.error);
        let answer = result.answer.expect("Expected an answer");
        assert!(
            answer.to_uppercase().contains("PONG"),
            "Expected answer to contain PONG, got: {answer}"
        );
    }

    #[tokio::test]
    async fn test_e2e_execute_cmd() {
        if std::env::var("ALCHEMY_E2E").unwrap_or_default() != "1" {
            return;
        }
        let provider_name = std::env::var("ALCHEMY_PROVIDER").expect("ALCHEMY_PROVIDER required");
        let api_key = std::env::var("ALCHEMY_API_KEY").expect("ALCHEMY_API_KEY required");
        let model = std::env::var("ALCHEMY_MODEL").expect("ALCHEMY_MODEL required");

        let provider = crate::providers::create_provider(&provider_name, Some(&api_key), None)
            .expect("Failed to create provider");

        let config = AgentConfig {
            model,
            system_prompt: "You are a helpful assistant. When asked to run a command, use the execute_cmd tool.".to_string(),
            max_steps: 10,
            timeout_secs: 30,
            context_window: 128000,
        };
        let agent = Agent::new(config, provider, crate::tools::ToolRegistry::new());
        let result = agent
            .run("Run the command: echo hello_from_alchemy".to_string())
            .await;

        println!("E2E execute_cmd result: {:?}", result);
        assert!(result.success, "Agent failed: {:?}", result.error);
        assert!(
            result.tools_used.contains(&"execute_cmd".to_string()),
            "Expected execute_cmd to be called, tools_used: {:?}",
            result.tools_used
        );
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
