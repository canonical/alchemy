use anyhow::Result;
use crate::types::{ToolDefinition, FunctionDefinition, SkillMetadata};

#[derive(Debug, Clone)]
pub struct SkillTool {
    pub skill_name: String,
    pub script_name: String,
    pub script_path: std::path::PathBuf,
    pub definition: ToolDefinition,
}

/// Create tool definitions from skill scripts
pub fn create_skill_tools(skills: &[SkillMetadata]) -> Vec<SkillTool> {
    let mut tools = Vec::new();

    for skill in skills {
        for script in &skill.scripts {
            let tool_name = format!("skill_{}_{}", skill.name, script.name.replace('.', "_").replace('-', "_"));
            let tool = SkillTool {
                skill_name: skill.name.clone(),
                script_name: script.name.clone(),
                script_path: script.path.clone(),
                definition: ToolDefinition {
                    r#type: "function".to_string(),
                    function: FunctionDefinition {
                        name: tool_name,
                        description: format!("Execute skill '{}' script '{}'", skill.name, script.name),
                        parameters: serde_json::json!({
                            "type": "object",
                            "properties": {
                                "args": {"type": "string", "description": "Arguments to pass to the script"}
                            },
                            "required": []
                        }),
                    },
                },
            };
            tools.push(tool);
        }
    }

    tools
}

pub async fn execute_skill_tool(tool: &SkillTool, arguments: &str, timeout_secs: u64) -> Result<String> {
    let args: serde_json::Value = serde_json::from_str(arguments).unwrap_or(serde_json::json!({}));
    let extra_args = args["args"].as_str().unwrap_or("");

    let cmd = format!("{} {}", tool.script_path.display(), extra_args);

    // Delegate to execute_cmd
    crate::tools::builtin::execute("execute_cmd", &serde_json::json!({
        "cmd": cmd,
        "timeout_secs": timeout_secs,
    }).to_string(), timeout_secs).await
}
