use anyhow::{anyhow, Result};
use crate::types::{ToolDefinition, FunctionDefinition, SkillMetadata};

#[derive(Debug, Clone)]
pub struct SkillTool {
    #[allow(dead_code)]
    pub skill_name: String,
    pub kind: SkillToolKind,
    pub definition: ToolDefinition,
}

#[derive(Debug, Clone)]
pub enum SkillToolKind {
    /// Execute a script under the skill's `scripts/` directory.
    Script { script_path: std::path::PathBuf },
    /// Read a file under the skill's `references/` or `assets/` directory.
    /// `allowed` is the pre-discovered list of relative paths; reads must
    /// match one of them, preventing path traversal.
    ReadResource {
        skill_dir: std::path::PathBuf,
        allowed: Vec<String>,
    },
}

/// Build tool definitions for the activated skills: one per script, plus a
/// single `read_resource` tool per skill that has references/ or assets/.
pub fn create_skill_tools(skills: &[SkillMetadata]) -> Vec<SkillTool> {
    let mut tools = Vec::new();

    for skill in skills {
        for script in &skill.scripts {
            let tool_name = format!("skill_{}_{}", skill.name, script.name.replace(['.', '-'], "_"));
            tools.push(SkillTool {
                skill_name: skill.name.clone(),
                kind: SkillToolKind::Script { script_path: script.path.clone() },
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
            });
        }

        if !skill.resources.is_empty() {
            let tool_name = format!("skill_{}_read_resource", skill.name);
            let description = format!(
                "Read a reference or asset file from skill '{}'. Available paths: {}",
                skill.name,
                skill.resources.join(", "),
            );
            tools.push(SkillTool {
                skill_name: skill.name.clone(),
                kind: SkillToolKind::ReadResource {
                    skill_dir: skill.path.clone(),
                    allowed: skill.resources.clone(),
                },
                definition: ToolDefinition {
                    r#type: "function".to_string(),
                    function: FunctionDefinition {
                        name: tool_name,
                        description,
                        parameters: serde_json::json!({
                            "type": "object",
                            "properties": {
                                "path": {
                                    "type": "string",
                                    "description": "Relative path under references/ or assets/ (e.g. 'references/api.md')"
                                }
                            },
                            "required": ["path"]
                        }),
                    },
                },
            });
        }
    }

    tools
}

pub async fn execute_skill_tool(tool: &SkillTool, arguments: &str, timeout_secs: u64) -> Result<String> {
    let args: serde_json::Value = serde_json::from_str(arguments).unwrap_or(serde_json::json!({}));

    match &tool.kind {
        SkillToolKind::Script { script_path } => {
            let extra_args = args["args"].as_str().unwrap_or("");
            tracing::info!(
                "skill call: skill={} script={} args={:?}",
                tool.skill_name,
                script_path.file_name().unwrap_or_default().to_string_lossy(),
                extra_args
            );
            let cmd = format!("{} {}", script_path.display(), extra_args);
            let result = crate::tools::builtin::execute("execute_cmd", &serde_json::json!({
                "cmd": cmd,
                "timeout_secs": timeout_secs,
            }).to_string(), timeout_secs).await;
            match &result {
                Ok(out) => tracing::info!(
                    "skill done: skill={} script={} ({} chars) [ok]",
                    tool.skill_name,
                    script_path.file_name().unwrap_or_default().to_string_lossy(),
                    out.len()
                ),
                Err(e) => tracing::info!(
                    "skill done: skill={} script={} [error: {}]",
                    tool.skill_name,
                    script_path.file_name().unwrap_or_default().to_string_lossy(),
                    e
                ),
            }
            result
        }
        SkillToolKind::ReadResource { skill_dir, allowed } => {
            let path = args["path"].as_str()
                .ok_or_else(|| anyhow!("read_resource: missing 'path' parameter"))?;
            tracing::info!("skill read: skill={} path={}", tool.skill_name, path);
            if !allowed.iter().any(|p| p == path) {
                anyhow::bail!(
                    "read_resource: '{}' is not an allowed resource for skill '{}'. Available: {}",
                    path, tool.skill_name, allowed.join(", "),
                );
            }
            let full = skill_dir.join(path);
            let result = tokio::fs::read_to_string(&full).await
                .map_err(|e| anyhow!("read_resource: failed to read '{}': {}", path, e));
            match &result {
                Ok(content) => tracing::info!(
                    "skill read: skill={} path={} ({} chars) [ok]",
                    tool.skill_name, path, content.len()
                ),
                Err(e) => tracing::info!(
                    "skill read: skill={} path={} [error: {}]",
                    tool.skill_name, path, e
                ),
            }
            result
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{SkillMetadata, SkillScript};

    fn skill_with_resources(dir: std::path::PathBuf, resources: Vec<&str>) -> SkillMetadata {
        SkillMetadata {
            name: "demo".into(),
            description: "demo skill".into(),
            path: dir,
            scripts: vec![],
            resources: resources.into_iter().map(String::from).collect(),
        }
    }

    #[tokio::test]
    async fn read_resource_serves_allowed_file() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::create_dir_all(dir.path().join("references")).await.unwrap();
        tokio::fs::write(dir.path().join("references").join("api.md"), "hello").await.unwrap();
        let skills = vec![skill_with_resources(dir.path().to_path_buf(), vec!["references/api.md"])];
        let tools = create_skill_tools(&skills);
        let tool = tools.iter().find(|t| matches!(t.kind, SkillToolKind::ReadResource { .. })).unwrap();
        let out = execute_skill_tool(tool, r#"{"path":"references/api.md"}"#, 5).await.unwrap();
        assert_eq!(out, "hello");
    }

    #[tokio::test]
    async fn read_resource_rejects_path_traversal() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::create_dir_all(dir.path().join("references")).await.unwrap();
        tokio::fs::write(dir.path().join("references").join("api.md"), "hello").await.unwrap();
        let skills = vec![skill_with_resources(dir.path().to_path_buf(), vec!["references/api.md"])];
        let tools = create_skill_tools(&skills);
        let tool = tools.iter().find(|t| matches!(t.kind, SkillToolKind::ReadResource { .. })).unwrap();
        let err = execute_skill_tool(tool, r#"{"path":"../../etc/passwd"}"#, 5).await.unwrap_err();
        assert!(err.to_string().contains("not an allowed resource"));
    }

    #[test]
    fn skill_with_no_resources_emits_no_read_tool() {
        let skills = vec![SkillMetadata {
            name: "scripts-only".into(),
            description: "".into(),
            path: "/tmp".into(),
            scripts: vec![SkillScript {
                name: "do.sh".into(),
                path: "/tmp/do.sh".into(),
            }],
            resources: vec![],
        }];
        let tools = create_skill_tools(&skills);
        assert_eq!(tools.len(), 1);
        assert!(matches!(tools[0].kind, SkillToolKind::Script { .. }));
    }
}
