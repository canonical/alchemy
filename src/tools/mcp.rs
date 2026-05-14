use anyhow::{anyhow, bail, Result};
use crate::types::{FunctionDefinition, McpServerConfig, ToolDefinition};
use eventsource_stream::Eventsource;
use futures::StreamExt;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{oneshot, Mutex};

type PendingMap = Arc<Mutex<HashMap<u64, oneshot::Sender<std::result::Result<Value, String>>>>>;

#[derive(Clone)]
pub struct McpTool {
    pub server_name: String,
    pub definition: ToolDefinition,
    pub(crate) client: Arc<McpClient>,
}

impl std::fmt::Debug for McpTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpTool")
            .field("server_name", &self.server_name)
            .field("name", &self.definition.function.name)
            .finish()
    }
}

pub struct McpClient {
    name: String,
    transport: ClientTransport,
}

enum ClientTransport {
    Stdio(Box<Mutex<StdioConn>>),
    Sse(SseConn),
}

struct StdioConn {
    stdin: tokio::process::ChildStdin,
    stdout: BufReader<tokio::process::ChildStdout>,
    id: u64,
    _child: tokio::process::Child,
}

struct SseConn {
    post_url: String,
    http: reqwest::Client,
    id: AtomicU64,
    pending: PendingMap,
}

impl McpClient {
    async fn call(&self, method: &str, params: Value) -> Result<Value> {
        match &self.transport {
            ClientTransport::Stdio(conn) => {
                let mut g = conn.lock().await;
                g.id += 1;
                let id = g.id;
                let msg = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
                let line = serde_json::to_string(&msg)? + "\n";
                g.stdin.write_all(line.as_bytes()).await?;
                g.stdin.flush().await?;
                loop {
                    let mut buf = String::new();
                    let n = g.stdout.read_line(&mut buf).await?;
                    if n == 0 {
                        bail!("MCP server '{}' closed unexpectedly", self.name);
                    }
                    let val: Value = match serde_json::from_str(buf.trim()) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    // Skip notifications (no id) and responses for other ids
                    if val.get("id") != Some(&json!(id)) {
                        continue;
                    }
                    if let Some(err) = val.get("error") {
                        bail!("MCP error from '{}': {}", self.name, err);
                    }
                    return Ok(val["result"].clone());
                }
            }
            ClientTransport::Sse(conn) => {
                let id = conn.id.fetch_add(1, Ordering::SeqCst);
                let (tx, rx) = oneshot::channel();
                conn.pending.lock().await.insert(id, tx);
                let msg = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
                let resp = conn.http.post(&conn.post_url).json(&msg).send().await?;
                if !resp.status().is_success() {
                    conn.pending.lock().await.remove(&id);
                    bail!("MCP server '{}' POST failed: {}", self.name, resp.status());
                }
                match rx.await {
                    Ok(Ok(val)) => Ok(val),
                    Ok(Err(e)) => bail!("MCP error from '{}': {}", self.name, e),
                    Err(_) => bail!("MCP server '{}' disconnected before responding", self.name),
                }
            }
        }
    }

    async fn notify(&self, method: &str, params: Value) -> Result<()> {
        match &self.transport {
            ClientTransport::Stdio(conn) => {
                let mut g = conn.lock().await;
                let msg = json!({"jsonrpc":"2.0","method":method,"params":params});
                let line = serde_json::to_string(&msg)? + "\n";
                g.stdin.write_all(line.as_bytes()).await?;
                g.stdin.flush().await?;
            }
            ClientTransport::Sse(conn) => {
                let msg = json!({"jsonrpc":"2.0","method":method,"params":params});
                conn.http
                    .post(&conn.post_url)
                    .json(&msg)
                    .send()
                    .await?
                    .error_for_status()?;
            }
        }
        Ok(())
    }
}

pub async fn discover_tools(configs: &[McpServerConfig]) -> Vec<McpTool> {
    let mut tools = Vec::new();
    for config in configs {
        match connect_and_discover(config).await {
            Ok(server_tools) => tools.extend(server_tools),
            Err(e) => tracing::warn!("MCP server '{}' failed: {}", config.name, e),
        }
    }
    tools
}

async fn connect_and_discover(config: &McpServerConfig) -> Result<Vec<McpTool>> {
    tracing::info!("Connecting to MCP server '{}' via {}", config.name, config.transport);
    let client = Arc::new(match config.transport.as_str() {
        "stdio" => connect_stdio(config).await?,
        "sse" => connect_sse(config).await?,
        other => bail!("Unknown MCP transport: {}", other),
    });

    let result = client.call("tools/list", json!({})).await?;
    let tools_arr = result
        .get("tools")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut mcp_tools = Vec::new();
    for tool in &tools_arr {
        let name = match tool.get("name").and_then(|v| v.as_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        let description = tool
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let input_schema = tool.get("inputSchema").cloned().unwrap_or_else(|| {
            json!({"type": "object", "properties": {}})
        });
        mcp_tools.push(McpTool {
            server_name: config.name.clone(),
            definition: ToolDefinition {
                r#type: "function".to_string(),
                function: FunctionDefinition {
                    name: format!("mcp_{}_{}", config.name, name),
                    description,
                    parameters: input_schema,
                },
            },
            client: client.clone(),
        });
    }

    tracing::info!(
        "MCP server '{}' provided {} tools",
        config.name,
        mcp_tools.len()
    );
    Ok(mcp_tools)
}

async fn connect_stdio(config: &McpServerConfig) -> Result<McpClient> {
    let cmd_str = config
        .cmd
        .as_deref()
        .ok_or_else(|| anyhow!("stdio MCP server '{}' missing 'cmd'", config.name))?;

    #[cfg(unix)]
    let mut command = {
        let mut c = tokio::process::Command::new("sh");
        c.args(["-c", cmd_str]);
        c
    };
    #[cfg(windows)]
    let mut command = {
        let mut c = tokio::process::Command::new("cmd");
        c.args(["/C", cmd_str]);
        c
    };

    command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());

    if let Some(env) = &config.env {
        for (k, v) in env {
            command.env(k, v);
        }
    }

    let mut child = command
        .spawn()
        .map_err(|e| anyhow!("Failed to spawn MCP server '{}': {}", config.name, e))?;

    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("Failed to get stdin for MCP server '{}'", config.name))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("Failed to get stdout for MCP server '{}'", config.name))?;

    let client = McpClient {
        name: config.name.clone(),
        transport: ClientTransport::Stdio(Box::new(Mutex::new(StdioConn {
            stdin,
            stdout: BufReader::new(stdout),
            id: 0,
            _child: child,
        }))),
    };

    client
        .call(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "alchemy", "version": "0.1.0"}
            }),
        )
        .await
        .map_err(|e| anyhow!("MCP server '{}' initialize failed: {}", config.name, e))?;
    client
        .notify("notifications/initialized", json!({}))
        .await?;

    Ok(client)
}

async fn connect_sse(config: &McpServerConfig) -> Result<McpClient> {
    let base_url = config
        .url
        .as_deref()
        .ok_or_else(|| anyhow!("sse MCP server '{}' missing 'url'", config.name))?;
    let sse_url = format!("{}/sse", base_url.trim_end_matches('/'));

    let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
    let pending_bg = pending.clone();

    // One-shot channel to receive the POST endpoint URL from the first SSE event
    let (endpoint_tx, endpoint_rx) = oneshot::channel::<String>();
    let endpoint_tx = Arc::new(Mutex::new(Some(endpoint_tx)));

    let sse_url_bg = sse_url.clone();
    tokio::spawn(async move {
        let resp = match crate::http::new_client().get(&sse_url_bg).send().await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("SSE connect failed for MCP: {}", e);
                return;
            }
        };
        let mut stream = resp.bytes_stream().eventsource();
        while let Some(event) = stream.next().await {
            match event {
                Ok(msg) if msg.event == "endpoint" => {
                    let mut guard = endpoint_tx.lock().await;
                    if let Some(tx) = guard.take() {
                        let _ = tx.send(msg.data.clone());
                    }
                }
                Ok(msg) => {
                    if let Ok(val) = serde_json::from_str::<Value>(&msg.data) {
                        if let Some(id) = val.get("id").and_then(|v| v.as_u64()) {
                            let mut p = pending_bg.lock().await;
                            if let Some(tx) = p.remove(&id) {
                                if let Some(err) = val.get("error") {
                                    let _ = tx.send(Err(err.to_string()));
                                } else {
                                    let _ = tx.send(Ok(val["result"].clone()));
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("SSE stream error for MCP: {}", e);
                    break;
                }
            }
        }
    });

    let post_path = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        endpoint_rx,
    )
    .await
    .map_err(|_| anyhow!("Timeout waiting for endpoint from SSE MCP server '{}'", config.name))?
    .map_err(|_| anyhow!("SSE MCP server '{}' endpoint channel dropped", config.name))?;

    let post_url = if post_path.starts_with("http") {
        post_path
    } else {
        format!("{}{}", base_url.trim_end_matches('/'), post_path)
    };

    let client = McpClient {
        name: config.name.clone(),
        transport: ClientTransport::Sse(SseConn {
            post_url,
            http: crate::http::new_client(),
            id: AtomicU64::new(1),
            pending,
        }),
    };

    client
        .call(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "alchemy", "version": "0.1.0"}
            }),
        )
        .await
        .map_err(|e| anyhow!("MCP server '{}' initialize failed: {}", config.name, e))?;
    client
        .notify("notifications/initialized", json!({}))
        .await?;

    Ok(client)
}

pub async fn execute_mcp_tool(tool: &McpTool, arguments: &str) -> Result<String> {
    let args: Value = serde_json::from_str(arguments).unwrap_or(json!({}));

    // Strip mcp_<server>_ prefix to recover the original tool name
    let prefix = format!("mcp_{}_", tool.server_name);
    let actual_name = tool
        .definition
        .function
        .name
        .strip_prefix(&prefix)
        .unwrap_or(&tool.definition.function.name);

    let result = tool
        .client
        .call("tools/call", json!({"name": actual_name, "arguments": args}))
        .await?;

    // MCP tool results: {"content":[{"type":"text","text":"..."}],"isError":false}
    if let Some(content) = result.get("content").and_then(|c| c.as_array()) {
        let parts: Vec<&str> = content
            .iter()
            .filter_map(|c| {
                if c.get("type").and_then(|t| t.as_str()) == Some("text") {
                    c.get("text").and_then(|t| t.as_str())
                } else {
                    None
                }
            })
            .collect();
        if result.get("isError").and_then(|v| v.as_bool()) == Some(true) {
            bail!("MCP tool error: {}", parts.join("\n"));
        }
        Ok(parts.join("\n"))
    } else {
        Ok(serde_json::to_string(&result)?)
    }
}
