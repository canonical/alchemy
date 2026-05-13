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
