pub mod builtin;
pub mod mcp;
pub mod skill;

use anyhow::Result;
use crate::types::{ToolDefinition, ToolCall};

/// Tool registry that merges built-in, MCP, and skill tools
pub struct ToolRegistry {
    pub definitions: Vec<ToolDefinition>,
    pub mcp_tools: Vec<mcp::McpTool>,
    pub skill_tools: Vec<skill::SkillTool>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        let defs = builtin::builtin_tool_definitions();
        Self {
            definitions: defs,
            mcp_tools: Vec::new(),
            skill_tools: Vec::new(),
        }
    }

    pub fn add_mcp_tools(&mut self, tools: Vec<mcp::McpTool>) {
        for tool in &tools {
            self.definitions.push(tool.definition.clone());
        }
        self.mcp_tools.extend(tools);
    }

    pub fn add_skill_tools(&mut self, tools: Vec<skill::SkillTool>) {
        for tool in &tools {
            self.definitions.push(tool.definition.clone());
        }
        self.skill_tools.extend(tools);
    }

    pub async fn dispatch(
        &self,
        tool_call: &ToolCall,
        timeout_secs: u64,
    ) -> Result<String> {
        let name = &tool_call.function.name;

        // Check built-in tools
        if builtin::is_builtin(name) {
            return builtin::execute(name, &tool_call.function.arguments, timeout_secs).await;
        }

        // Check MCP tools
        if name.starts_with("mcp_") {
            for mcp_tool in &self.mcp_tools {
                if mcp_tool.definition.function.name == *name {
                    return mcp::execute_mcp_tool(mcp_tool, &tool_call.function.arguments).await;
                }
            }
            anyhow::bail!("Unknown MCP tool: {}", name);
        }

        // Check skill tools
        if name.starts_with("skill_") {
            for skill_tool in &self.skill_tools {
                if skill_tool.definition.function.name == *name {
                    return skill::execute_skill_tool(skill_tool, &tool_call.function.arguments, timeout_secs).await;
                }
            }
            anyhow::bail!("Unknown skill tool: {}", name);
        }

        anyhow::bail!("Unknown tool: {}", name)
    }

    /// Returns true if the tool is safe for parallel execution
    pub fn is_parallel_safe(name: &str) -> bool {
        matches!(name, "read_file" | "list_dir" | "fetch_url")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FunctionCall, ToolCall};

    #[test]
    fn test_registry_has_builtin_tools() {
        let registry = ToolRegistry::new();
        let names: Vec<&str> = registry.definitions.iter()
            .map(|d| d.function.name.as_str())
            .collect();
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"write_file"));
        assert!(names.contains(&"list_dir"));
        assert!(names.contains(&"execute_cmd"));
        assert!(names.contains(&"fetch_url"));
        assert_eq!(registry.definitions.len(), 5);
    }

    #[test]
    fn test_parallel_safe() {
        assert!(ToolRegistry::is_parallel_safe("read_file"));
        assert!(ToolRegistry::is_parallel_safe("list_dir"));
        assert!(ToolRegistry::is_parallel_safe("fetch_url"));
        assert!(!ToolRegistry::is_parallel_safe("write_file"));
        assert!(!ToolRegistry::is_parallel_safe("execute_cmd"));
        assert!(!ToolRegistry::is_parallel_safe("mcp_server_tool"));
    }

    #[tokio::test]
    async fn test_dispatch_builtin_read_file() {
        let registry = ToolRegistry::new();
        let tc = ToolCall {
            id: "1".to_string(),
            r#type: "function".to_string(),
            function: FunctionCall {
                name: "read_file".to_string(),
                arguments: r#"{"path": "Cargo.toml"}"#.to_string(),
            },
        };
        let result = registry.dispatch(&tc, 30).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(v["content"].as_str().unwrap().contains("[package]"));
    }

    #[tokio::test]
    async fn test_dispatch_unknown_tool() {
        let registry = ToolRegistry::new();
        let tc = ToolCall {
            id: "1".to_string(),
            r#type: "function".to_string(),
            function: FunctionCall {
                name: "nonexistent_tool".to_string(),
                arguments: "{}".to_string(),
            },
        };
        let result = registry.dispatch(&tc, 30).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_dispatch_unknown_mcp_tool() {
        let registry = ToolRegistry::new();
        let tc = ToolCall {
            id: "1".to_string(),
            r#type: "function".to_string(),
            function: FunctionCall {
                name: "mcp_server_tool".to_string(),
                arguments: "{}".to_string(),
            },
        };
        let result = registry.dispatch(&tc, 30).await;
        assert!(result.is_err());
    }
}
