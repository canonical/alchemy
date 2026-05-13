use anyhow::Result;
use serde_json::json;
use crate::types::{ToolDefinition, FunctionDefinition};

const MAX_READ_BYTES: usize = 32 * 1024;

pub fn is_builtin(name: &str) -> bool {
    matches!(name, "read_file" | "write_file" | "list_dir" | "execute_cmd" | "fetch_url")
}

pub fn builtin_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            r#type: "function".to_string(),
            function: FunctionDefinition {
                name: "read_file".to_string(),
                description: "Read a file's contents (UTF-8). Truncates at 32KB.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "File path to read"}
                    },
                    "required": ["path"]
                }),
            },
        },
        ToolDefinition {
            r#type: "function".to_string(),
            function: FunctionDefinition {
                name: "write_file".to_string(),
                description: "Write content to a file. Creates parent directories. Overwrites existing files.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "File path to write"},
                        "content": {"type": "string", "description": "Content to write"}
                    },
                    "required": ["path", "content"]
                }),
            },
        },
        ToolDefinition {
            r#type: "function".to_string(),
            function: FunctionDefinition {
                name: "list_dir".to_string(),
                description: "List directory contents.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Directory path"}
                    },
                    "required": ["path"]
                }),
            },
        },
        ToolDefinition {
            r#type: "function".to_string(),
            function: FunctionDefinition {
                name: "execute_cmd".to_string(),
                description: "Execute a shell command.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "cmd": {"type": "string", "description": "Command to execute"},
                        "cwd": {"type": "string", "description": "Working directory (optional)"},
                        "timeout_secs": {"type": "integer", "description": "Timeout in seconds (optional)"}
                    },
                    "required": ["cmd"]
                }),
            },
        },
        ToolDefinition {
            r#type: "function".to_string(),
            function: FunctionDefinition {
                name: "fetch_url".to_string(),
                description: "Fetch content from a URL.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "url": {"type": "string", "description": "URL to fetch"}
                    },
                    "required": ["url"]
                }),
            },
        },
    ]
}

pub async fn execute(name: &str, arguments: &str, timeout_secs: u64) -> Result<String> {
    let args: serde_json::Value = serde_json::from_str(arguments)
        .unwrap_or(serde_json::json!({}));

    match name {
        "read_file" => execute_read_file(&args).await,
        "write_file" => execute_write_file(&args).await,
        "list_dir" => execute_list_dir(&args).await,
        "execute_cmd" => execute_cmd(&args, timeout_secs).await,
        "fetch_url" => execute_fetch_url(&args, timeout_secs).await,
        _ => anyhow::bail!("Unknown built-in tool: {}", name),
    }
}

async fn execute_read_file(args: &serde_json::Value) -> Result<String> {
    let path = args["path"].as_str()
        .ok_or_else(|| anyhow::anyhow!("read_file: missing 'path' parameter"))?;

    let content = tokio::fs::read_to_string(path).await
        .map_err(|e| anyhow::anyhow!("read_file error: {}", e))?;

    let truncated = content.len() > MAX_READ_BYTES;
    let content = if truncated {
        content[..MAX_READ_BYTES].to_string()
    } else {
        content
    };

    Ok(json!({"content": content, "truncated": truncated}).to_string())
}

async fn execute_write_file(args: &serde_json::Value) -> Result<String> {
    let path = args["path"].as_str()
        .ok_or_else(|| anyhow::anyhow!("write_file: missing 'path' parameter"))?;
    let content = args["content"].as_str()
        .ok_or_else(|| anyhow::anyhow!("write_file: missing 'content' parameter"))?;

    let path = std::path::Path::new(path);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let bytes = content.len();
    tokio::fs::write(path, content).await?;

    Ok(json!({"ok": true, "bytes_written": bytes}).to_string())
}

async fn execute_list_dir(args: &serde_json::Value) -> Result<String> {
    let path = args["path"].as_str()
        .ok_or_else(|| anyhow::anyhow!("list_dir: missing 'path' parameter"))?;

    let mut entries = Vec::new();
    let mut dir = tokio::fs::read_dir(path).await
        .map_err(|e| anyhow::anyhow!("list_dir error: {}", e))?;

    while let Some(entry) = dir.next_entry().await? {
        let name = entry.file_name().to_string_lossy().to_string();
        let ft = entry.file_type().await?;
        if ft.is_dir() {
            entries.push(format!("{}/", name));
        } else {
            entries.push(name);
        }
    }

    entries.sort();
    Ok(json!({"entries": entries}).to_string())
}

async fn execute_cmd(args: &serde_json::Value, default_timeout: u64) -> Result<String> {
    let cmd = args["cmd"].as_str()
        .ok_or_else(|| anyhow::anyhow!("execute_cmd: missing 'cmd' parameter"))?;
    let cwd = args["cwd"].as_str();
    let timeout = args["timeout_secs"].as_u64().unwrap_or(default_timeout);

    #[cfg(unix)]
    let mut command = {
        let mut c = tokio::process::Command::new("sh");
        c.args(["-c", cmd]);
        c
    };
    #[cfg(windows)]
    let mut command = {
        let mut c = tokio::process::Command::new("cmd");
        c.args(["/C", cmd]);
        c
    };

    if let Some(dir) = cwd {
        command.current_dir(dir);
    }

    command.stdout(std::process::Stdio::piped());
    command.stderr(std::process::Stdio::piped());

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(timeout),
        command.output(),
    )
    .await;

    match result {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let exit_code = output.status.code().unwrap_or(-1);
            Ok(json!({
                "stdout": stdout,
                "stderr": stderr,
                "exit_code": exit_code,
                "timed_out": false
            }).to_string())
        }
        Ok(Err(e)) => {
            Ok(json!({
                "stdout": "",
                "stderr": format!("Failed to execute command: {}", e),
                "exit_code": -1,
                "timed_out": false
            }).to_string())
        }
        Err(_) => {
            Ok(json!({
                "stdout": "",
                "stderr": "Command timed out",
                "exit_code": -1,
                "timed_out": true
            }).to_string())
        }
    }
}

async fn execute_fetch_url(args: &serde_json::Value, timeout_secs: u64) -> Result<String> {
    let url = args["url"].as_str()
        .ok_or_else(|| anyhow::anyhow!("fetch_url: missing 'url' parameter"))?;

    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()?;

    let resp = client.get(url).send().await
        .map_err(|e| anyhow::anyhow!("fetch_url error: {}", e))?;

    let content_type = resp.headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("text/plain")
        .to_string();

    let body = resp.text().await?;
    let truncated = body.len() > MAX_READ_BYTES;
    let body = if truncated {
        body[..MAX_READ_BYTES].to_string()
    } else {
        body
    };

    Ok(json!({
        "url": url,
        "content": body,
        "content_type": content_type,
        "truncated": truncated
    }).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_read_file() {
        let result = execute("read_file", r#"{"path": "Cargo.toml"}"#, 30).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(v["content"].as_str().unwrap().contains("[package]"));
        assert!(!v["truncated"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn test_write_and_read_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        let path_str = path.to_str().unwrap();

        let result = execute("write_file", &format!(r#"{{"path": "{}", "content": "hello"}}"#, path_str), 30).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(v["ok"].as_bool().unwrap());
        assert_eq!(v["bytes_written"].as_u64().unwrap(), 5);

        let result = execute("read_file", &format!(r#"{{"path": "{}"}}"#, path_str), 30).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["content"].as_str().unwrap(), "hello");
    }

    #[tokio::test]
    async fn test_list_dir() {
        let result = execute("list_dir", r#"{"path": "src"}"#, 30).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        let entries = v["entries"].as_array().unwrap();
        assert!(entries.iter().any(|e| e.as_str().unwrap() == "providers/"));
    }

    #[tokio::test]
    async fn test_execute_cmd() {
        let result = execute("execute_cmd", r#"{"cmd": "echo hello"}"#, 30).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(v["stdout"].as_str().unwrap().contains("hello"));
        assert_eq!(v["exit_code"].as_i64().unwrap(), 0);
        assert!(!v["timed_out"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn test_execute_cmd_timeout() {
        let result = execute("execute_cmd", r#"{"cmd": "sleep 10", "timeout_secs": 1}"#, 30).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(v["timed_out"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn test_execute_cmd_nonzero_exit() {
        let result = execute("execute_cmd", r#"{"cmd": "exit 42"}"#, 30).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["exit_code"].as_i64().unwrap(), 42);
    }

    #[tokio::test]
    async fn test_write_file_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a/b/c/test.txt");
        let path_str = path.to_str().unwrap();

        let result = execute("write_file", &format!(r#"{{"path": "{}", "content": "nested"}}"#, path_str), 30).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(v["ok"].as_bool().unwrap());
    }
}
