use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A single message in the conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: MessageRole,
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub r#type: String,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

/// Tool definition sent to the LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub r#type: String,
    pub function: FunctionDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Thinking / reasoning level passed through to the provider.
///
/// Maps to:
///   - Anthropic  → `thinking.budget_tokens`  (+ `anthropic-beta` header)
///   - OpenAI     → `reasoning_effort`
///   - Gemini     → `generationConfig.thinkingConfig.thinkingBudget`
///   - Others     → silently ignored
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingLevel {
    #[default]
    Off,
    Low,
    Medium,
    High,
    XHigh,
}

impl ThinkingLevel {
    /// Advance to the next level, wrapping Off→Low→…→XHigh→Off.
    pub fn cycle(self) -> Self {
        match self {
            ThinkingLevel::Off    => ThinkingLevel::Low,
            ThinkingLevel::Low    => ThinkingLevel::Medium,
            ThinkingLevel::Medium => ThinkingLevel::High,
            ThinkingLevel::High   => ThinkingLevel::XHigh,
            ThinkingLevel::XHigh  => ThinkingLevel::Off,
        }
    }

    /// Short display label shown in the TUI status bar.
    pub fn label(self) -> &'static str {
        match self {
            ThinkingLevel::Off    => "off",
            ThinkingLevel::Low    => "low",
            ThinkingLevel::Medium => "medium",
            ThinkingLevel::High   => "high",
            ThinkingLevel::XHigh  => "xhigh",
        }
    }

    /// Case-insensitive parse from a string (e.g. from env var or Concourse source).
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "off"              => Some(ThinkingLevel::Off),
            "low"              => Some(ThinkingLevel::Low),
            "medium" | "med"   => Some(ThinkingLevel::Medium),
            "high"             => Some(ThinkingLevel::High),
            "xhigh" | "x-high" => Some(ThinkingLevel::XHigh),
            _                  => None,
        }
    }

    /// Anthropic `thinking.budget_tokens`. `None` means thinking is disabled.
    pub fn anthropic_budget(self) -> Option<u32> {
        match self {
            ThinkingLevel::Off    => None,
            ThinkingLevel::Low    => Some(1_024),
            ThinkingLevel::Medium => Some(8_192),
            ThinkingLevel::High   => Some(32_000),
            ThinkingLevel::XHigh  => Some(100_000),
        }
    }

    /// OpenAI `reasoning_effort` string. `None` means do not send the field.
    pub fn openai_effort(self) -> Option<&'static str> {
        match self {
            ThinkingLevel::Off    => None,
            ThinkingLevel::Low    => Some("low"),
            ThinkingLevel::Medium => Some("medium"),
            ThinkingLevel::High   => Some("high"),
            ThinkingLevel::XHigh  => Some("high"), // API maximum
        }
    }

    /// Gemini `thinkingBudget`. `None` means do not send the field.
    /// `-1` means dynamic (let the model decide).
    pub fn gemini_budget(self) -> Option<i32> {
        match self {
            ThinkingLevel::Off    => None,
            ThinkingLevel::Low    => Some(1_024),
            ThinkingLevel::Medium => Some(8_192),
            ThinkingLevel::High   => Some(32_000),
            ThinkingLevel::XHigh  => Some(-1), // dynamic
        }
    }
}

/// Request to the LLM provider
#[derive(Debug, Clone)]
pub struct LlmRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDefinition>,
    pub temperature: Option<f64>,
    /// Thinking / reasoning level. Providers that don't support it ignore this.
    pub thinking_level: ThinkingLevel,
}

/// Response from the LLM provider
#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub usage: Option<Usage>,
    #[allow(dead_code)]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

/// Output format for pipe mode
#[derive(Debug, Serialize, Deserialize)]
pub struct PipeOutput {
    pub success: bool,
    pub answer: Option<String>,
    pub steps: u32,
    pub tools_used: Vec<String>,
    pub error: Option<String>,
}

/// Concourse types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConcourseVersion {
    pub r#ref: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConcourseSource {
    pub api_key: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub prompt: Option<String>,
    pub system_prompt: Option<String>,
    pub max_steps: Option<u32>,
    pub timeout_secs: Option<u64>,
    /// Output format for written files: "json", "text", or omitted (both).
    pub output_format: Option<String>,
    /// Thinking level: "off", "low", "medium", "high", "xhigh".
    pub thinking_level: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConcourseCheckInput {
    pub source: ConcourseSource,
    pub version: Option<ConcourseVersion>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConcourseInInput {
    pub source: ConcourseSource,
    pub version: ConcourseVersion,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConcourseOutInput {
    pub source: ConcourseSource,
    pub params: Option<ConcourseOutParams>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConcourseOutParams {
    pub prompt: Option<String>,
    pub stdin_file: Option<String>,
    pub model: Option<String>,
    pub system_prompt: Option<String>,
    pub max_steps: Option<u32>,
    pub timeout_secs: Option<u64>,
    pub output_format: Option<String>,
    /// Override `source.thinking_level` for this invocation.
    pub thinking_level: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConcourseOutOutput {
    pub version: ConcourseVersion,
    pub metadata: Vec<ConcourseMetadataEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConcourseInOutput {
    pub version: ConcourseVersion,
    pub metadata: Vec<ConcourseMetadataEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConcourseMetadataEntry {
    pub name: String,
    pub value: String,
}

/// MCP configuration types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub transport: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cmd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfigFile {
    pub servers: Vec<McpServerConfig>,
}

/// Skill types
#[derive(Debug, Clone)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub path: std::path::PathBuf,
    pub scripts: Vec<SkillScript>,
    /// Relative paths under `references/` and `assets/` (e.g. "references/api.md").
    /// Surfaced to the LLM and readable via the per-skill `read_resource` tool.
    pub resources: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SkillScript {
    pub name: String,
    pub path: std::path::PathBuf,
}

/// Agent run result
#[derive(Debug)]
pub struct AgentResult {
    pub answer: Option<String>,
    pub steps: u32,
    pub tools_used: Vec<String>,
    pub success: bool,
    pub error: Option<String>,
    pub total_tokens: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── ThinkingLevel::cycle ─────────────────────────────────────────────────

    #[test]
    fn test_cycle_full_sequence() {
        let levels = [
            ThinkingLevel::Off,
            ThinkingLevel::Low,
            ThinkingLevel::Medium,
            ThinkingLevel::High,
            ThinkingLevel::XHigh,
        ];
        for window in levels.windows(2) {
            assert_eq!(window[0].cycle(), window[1],
                "{:?}.cycle() should be {:?}", window[0], window[1]);
        }
        // XHigh wraps back to Off.
        assert_eq!(ThinkingLevel::XHigh.cycle(), ThinkingLevel::Off);
    }

    // ── ThinkingLevel::label ─────────────────────────────────────────────────

    #[test]
    fn test_labels() {
        assert_eq!(ThinkingLevel::Off.label(),    "off");
        assert_eq!(ThinkingLevel::Low.label(),    "low");
        assert_eq!(ThinkingLevel::Medium.label(), "medium");
        assert_eq!(ThinkingLevel::High.label(),   "high");
        assert_eq!(ThinkingLevel::XHigh.label(),  "xhigh");
    }

    // ── ThinkingLevel::from_str ──────────────────────────────────────────────

    #[test]
    fn test_from_str_canonical() {
        assert_eq!(ThinkingLevel::from_str("off"),    Some(ThinkingLevel::Off));
        assert_eq!(ThinkingLevel::from_str("low"),    Some(ThinkingLevel::Low));
        assert_eq!(ThinkingLevel::from_str("medium"), Some(ThinkingLevel::Medium));
        assert_eq!(ThinkingLevel::from_str("high"),   Some(ThinkingLevel::High));
        assert_eq!(ThinkingLevel::from_str("xhigh"),  Some(ThinkingLevel::XHigh));
    }

    #[test]
    fn test_from_str_aliases() {
        // "med" and "x-high" are accepted as aliases.
        assert_eq!(ThinkingLevel::from_str("med"),    Some(ThinkingLevel::Medium));
        assert_eq!(ThinkingLevel::from_str("x-high"), Some(ThinkingLevel::XHigh));
    }

    #[test]
    fn test_from_str_case_insensitive() {
        assert_eq!(ThinkingLevel::from_str("OFF"),    Some(ThinkingLevel::Off));
        assert_eq!(ThinkingLevel::from_str("Low"),    Some(ThinkingLevel::Low));
        assert_eq!(ThinkingLevel::from_str("MEDIUM"), Some(ThinkingLevel::Medium));
        assert_eq!(ThinkingLevel::from_str("HIGH"),   Some(ThinkingLevel::High));
        assert_eq!(ThinkingLevel::from_str("XHIGH"),  Some(ThinkingLevel::XHigh));
    }

    #[test]
    fn test_from_str_unknown_returns_none() {
        assert_eq!(ThinkingLevel::from_str(""),         None);
        assert_eq!(ThinkingLevel::from_str("supermax"), None);
        assert_eq!(ThinkingLevel::from_str("1"),        None);
    }

    // ── Provider mappings ────────────────────────────────────────────────────

    #[test]
    fn test_anthropic_budget_off_is_none() {
        assert_eq!(ThinkingLevel::Off.anthropic_budget(), None);
    }

    #[test]
    fn test_anthropic_budget_increases_with_level() {
        let budgets: Vec<u32> = [
            ThinkingLevel::Low,
            ThinkingLevel::Medium,
            ThinkingLevel::High,
            ThinkingLevel::XHigh,
        ]
        .iter()
        .map(|l| l.anthropic_budget().expect("should have a budget"))
        .collect();

        for w in budgets.windows(2) {
            assert!(w[0] < w[1], "budget should increase: {} < {}", w[0], w[1]);
        }
    }

    #[test]
    fn test_openai_effort_off_is_none() {
        assert_eq!(ThinkingLevel::Off.openai_effort(), None);
    }

    #[test]
    fn test_openai_effort_values() {
        assert_eq!(ThinkingLevel::Low.openai_effort(),    Some("low"));
        assert_eq!(ThinkingLevel::Medium.openai_effort(), Some("medium"));
        assert_eq!(ThinkingLevel::High.openai_effort(),   Some("high"));
        // xhigh is capped at "high" (API maximum).
        assert_eq!(ThinkingLevel::XHigh.openai_effort(),  Some("high"));
    }

    #[test]
    fn test_gemini_budget_off_is_none() {
        assert_eq!(ThinkingLevel::Off.gemini_budget(), None);
    }

    #[test]
    fn test_gemini_budget_xhigh_is_dynamic() {
        // -1 signals "dynamic" to the Gemini API.
        assert_eq!(ThinkingLevel::XHigh.gemini_budget(), Some(-1));
    }

    #[test]
    fn test_gemini_budget_non_xhigh_values_are_positive() {
        for level in [ThinkingLevel::Low, ThinkingLevel::Medium, ThinkingLevel::High] {
            let budget = level.gemini_budget().expect("should have a budget");
            assert!(budget > 0, "{:?} budget should be positive, got {}", level, budget);
        }
    }

    // ── Default ──────────────────────────────────────────────────────────────

    #[test]
    fn test_default_is_off() {
        assert_eq!(ThinkingLevel::default(), ThinkingLevel::Off);
    }
}
