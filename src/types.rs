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

/// Request to the LLM provider
#[derive(Debug, Clone)]
pub struct LlmRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDefinition>,
    pub temperature: Option<f64>,
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
