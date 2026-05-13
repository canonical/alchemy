mod agent;
mod concourse;
mod output;
mod providers;
mod rag;
mod skills;
mod tools;
mod tui;
mod types;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::io::Read;
use std::path::PathBuf;

use crate::agent::{Agent, AgentConfig};
use crate::tools::ToolRegistry;
use crate::types::PipeOutput;

const DEFAULT_SYSTEM_PROMPT: &str = "You are Alchemy, a CI/CD AI agent. Use the provided tools to accomplish tasks.\nBe concise. Report results as structured data when possible. If a tool fails,\nanalyze the error and either retry with a corrected approach or report the\nfailure with your analysis.";

#[derive(Parser)]
#[command(name = "alchemy", version = "0.1.0", about = "A cross-platform CI/CD AI agent")]
struct Cli {
    /// Instruction text (if omitted, reads from stdin)
    #[arg()]
    prompt: Option<String>,

    /// Output format: json, text [default: json, or ALCHEMY_OUTPUT env var]
    #[arg(long)]
    output: Option<String>,

    /// System prompt
    #[arg(long)]
    system: Option<String>,

    /// Max agent loop steps
    #[arg(long, alias = "max-steps")]
    max_steps: Option<u32>,

    /// Per-tool timeout in seconds
    #[arg(long)]
    timeout: Option<u64>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// TUI mode
    Tui {
        /// Session name
        #[arg(long, default_value = "default")]
        session: String,
        /// Session storage path
        #[arg(long, alias = "session-dir")]
        session_dir: Option<String>,
        /// System prompt
        #[arg(long)]
        system: Option<String>,
        /// Max steps per turn
        #[arg(long, alias = "max-steps")]
        max_steps: Option<u32>,
        /// Per-tool timeout
        #[arg(long)]
        timeout: Option<u64>,
    },
    /// RAG management
    Rag {
        #[command(subcommand)]
        action: RagAction,
    },
}

#[derive(Subcommand)]
enum RagAction {
    /// Index files for RAG
    Index {
        path: String,
        #[arg(long)]
        glob: Option<String>,
    },
    /// Test RAG retrieval
    Search { query: String },
    /// Show index status
    Status,
    /// Clear RAG index
    Clear,
}

#[tokio::main]
async fn main() {
    let exit_code = run().await;
    std::process::exit(exit_code);
}

async fn run() -> i32 {
    let log_level = std::env::var("ALCHEMY_LOG_LEVEL").unwrap_or_else(|_| "warn".to_string());
    let cli = Cli::parse();

    // In TUI mode write logs to a file so they don't corrupt the terminal display.
    // ALCHEMY_LOG_FILE overrides the default path (~/.alchemy/debug.log).
    let is_tui = matches!(cli.command, Some(Commands::Tui { .. }));
    if is_tui {
        let log_path = std::env::var("ALCHEMY_LOG_FILE")
            .unwrap_or_else(|_| dirs_path("debug.log"));
        if let Ok(file) = std::fs::OpenOptions::new()
            .create(true).append(true).open(&log_path)
        {
            tracing_subscriber::fmt()
                .with_env_filter(&log_level)
                .with_writer(std::sync::Mutex::new(file))
                .with_ansi(false)
                .init();
        } else {
            tracing_subscriber::fmt()
                .with_env_filter(&log_level)
                .with_writer(std::io::sink)
                .init();
        }
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(&log_level)
            .with_writer(std::io::stderr)
            .init();
    }

    match cli.command {
        Some(Commands::Tui { session, session_dir, system, max_steps, timeout }) => {
            match run_tui(session, session_dir, system, max_steps, timeout).await {
                Ok(()) => 0,
                Err(e) => { eprintln!("Error: {}", e); 1 }
            }
        }
        Some(Commands::Rag { action }) => {
            match run_rag(action).await {
                Ok(()) => 0,
                Err(e) => { eprintln!("Error: {}", e); 1 }
            }
        }
        None => {
            match run_pipe(cli).await {
                Ok(code) => code,
                Err(e) => { eprintln!("Error: {}", e); 2 }
            }
        }
    }
}

async fn run_pipe(cli: Cli) -> Result<i32> {
    // Get provider
    let provider_name = std::env::var("ALCHEMY_PROVIDER")
        .map_err(|_| anyhow::anyhow!("ALCHEMY_PROVIDER environment variable is required"))?;

    let api_key = std::env::var("ALCHEMY_API_KEY").ok();
    let base_url = std::env::var("ALCHEMY_BASE_URL").ok();

    let provider = providers::create_provider(
        &provider_name,
        api_key.as_deref(),
        base_url.as_deref(),
    )?;

    let model = std::env::var("ALCHEMY_MODEL")
        .unwrap_or_else(|_| provider.default_model().to_string());

    // Build prompt from args + stdin
    let stdin_content = read_stdin();
    let prompt = match (&cli.prompt, &stdin_content) {
        (Some(p), Some(stdin)) => format!("{}\n\n--- stdin ---\n{}", p, stdin),
        (Some(p), None) => p.clone(),
        (None, Some(stdin)) => stdin.clone(),
        (None, None) => {
            eprintln!("Error: No prompt provided. Pass as argument or pipe via stdin.");
            return Ok(2);
        }
    };

    let system_prompt = cli.system
        .or_else(|| std::env::var("ALCHEMY_SYSTEM_PROMPT").ok())
        .unwrap_or_else(|| DEFAULT_SYSTEM_PROMPT.to_string());

    let max_steps = cli.max_steps
        .or_else(|| std::env::var("ALCHEMY_MAX_STEPS").ok().and_then(|s| s.parse().ok()))
        .unwrap_or(30);

    let timeout_secs = cli.timeout
        .or_else(|| std::env::var("ALCHEMY_TIMEOUT_SECS").ok().and_then(|s| s.parse().ok()))
        .unwrap_or(30);

    let context_window: u64 = std::env::var("ALCHEMY_CONTEXT_WINDOW")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(128000);

    // Build tool registry
    let mut registry = ToolRegistry::new();

    // Load MCP tools
    let mcp_configs = load_mcp_configs();
    if !mcp_configs.is_empty() {
        let mcp_tools = tools::mcp::discover_tools(&mcp_configs).await;
        registry.add_mcp_tools(mcp_tools);
    }

    // Load skills
    let skills_enabled = std::env::var("ALCHEMY_SKILLS_ENABLED")
        .map(|s| s != "false")
        .unwrap_or(true);

    let mut skill_context = String::new();
    if skills_enabled {
        let skills_dir = std::env::var("ALCHEMY_SKILLS_DIR")
            .unwrap_or_else(|_| {
                dirs_path("skills")
            });
        let skills_path = PathBuf::from(&skills_dir);
        let all_skills = skills::load_skills(&skills_path).await;
        let matched = skills::match_skills(&all_skills, &prompt);

        if !matched.is_empty() {
            skill_context = skills::build_skill_context(&all_skills, &matched).await;
            let skill_tools = tools::skill::create_skill_tools(
                &matched.iter().filter_map(|&i| all_skills.get(i)).cloned().collect::<Vec<_>>()
            );
            registry.add_skill_tools(skill_tools);
        }
    }

    // RAG context
    let mut rag_context = String::new();
    let rag_enabled = std::env::var("ALCHEMY_RAG_ENABLED")
        .map(|s| s == "true")
        .unwrap_or(false);

    if rag_enabled {
        // Validate embedding provider
        let embed_provider = std::env::var("ALCHEMY_RAG_EMBED_PROVIDER")
            .unwrap_or_else(|_| provider_name.clone());

        if !matches!(embed_provider.as_str(), "openai" | "gemini" | "ollama") {
            eprintln!("Error: RAG requires embedding provider (openai, gemini, or ollama). Set ALCHEMY_RAG_EMBED_PROVIDER.");
            return Ok(2);
        }

        let store_path = std::env::var("ALCHEMY_RAG_STORE_PATH")
            .unwrap_or_else(|_| dirs_path("rag/vectors.db"));
        let chunk_size: usize = std::env::var("ALCHEMY_RAG_CHUNK_SIZE").ok().and_then(|s| s.parse().ok()).unwrap_or(512);
        let chunk_overlap: usize = std::env::var("ALCHEMY_RAG_CHUNK_OVERLAP").ok().and_then(|s| s.parse().ok()).unwrap_or(64);
        let top_k: usize = std::env::var("ALCHEMY_RAG_TOP_K").ok().and_then(|s| s.parse().ok()).unwrap_or(5);

        let embed_prov = providers::create_provider(&embed_provider, api_key.as_deref(), base_url.as_deref())?;
        let dimensions = embed_prov.embed_dimensions();

        let rag_config = rag::RagConfig {
            embed_provider,
            embed_model: std::env::var("ALCHEMY_RAG_EMBED_MODEL").unwrap_or_default(),
            chunk_size,
            chunk_overlap,
            top_k,
            store_path,
            dimensions,
        };

        if let Ok(pipeline) = rag::RagPipeline::new(rag_config).await {
            if let Ok(ctx) = pipeline.build_context(&prompt).await {
                rag_context = ctx;
            }
        }
    }

    // Build final system prompt
    let full_system = if skill_context.is_empty() && rag_context.is_empty() {
        system_prompt
    } else {
        format!("{}{}\n{}", system_prompt, skill_context, rag_context)
    };

    let config = AgentConfig {
        model,
        system_prompt: full_system,
        max_steps,
        timeout_secs,
        context_window,
    };

    let agent = Agent::new(config, provider, registry);
    let result = agent.run(prompt).await;

    let pipe_output = PipeOutput {
        success: result.success,
        answer: result.answer.clone(),
        steps: result.steps,
        tools_used: result.tools_used,
        error: result.error.clone(),
    };

    let output_format = cli.output
        .or_else(|| std::env::var("ALCHEMY_OUTPUT").ok())
        .unwrap_or_else(|| "json".to_string());

    let output_str = match output_format.as_str() {
        "text" => output::format_text(&pipe_output),
        _ => output::format_json(&pipe_output),
    };

    println!("{}", output_str);

    if result.success { Ok(0) } else { Ok(1) }
}

async fn run_tui(
    session: String,
    session_dir: Option<String>,
    system: Option<String>,
    max_steps: Option<u32>,
    timeout: Option<u64>,
) -> Result<()> {
    let provider_name = std::env::var("ALCHEMY_PROVIDER")
        .map_err(|_| anyhow::anyhow!("ALCHEMY_PROVIDER environment variable is required"))?;
    let api_key = std::env::var("ALCHEMY_API_KEY").ok();
    let base_url = std::env::var("ALCHEMY_BASE_URL").ok();

    let provider = providers::create_provider(&provider_name, api_key.as_deref(), base_url.as_deref())?;
    let model = std::env::var("ALCHEMY_MODEL").unwrap_or_else(|_| provider.default_model().to_string());

    let system_prompt = system
        .or_else(|| std::env::var("ALCHEMY_SYSTEM_PROMPT").ok())
        .unwrap_or_else(|| DEFAULT_SYSTEM_PROMPT.to_string());

    let sess_dir = session_dir
        .or_else(|| std::env::var("ALCHEMY_SESSION_DIR").ok())
        .unwrap_or_else(|| dirs_path("sessions"));

    let config = AgentConfig {
        model: model.clone(),
        system_prompt,
        max_steps: max_steps.unwrap_or(30),
        timeout_secs: timeout.unwrap_or(30),
        context_window: std::env::var("ALCHEMY_CONTEXT_WINDOW").ok().and_then(|s| s.parse().ok()).unwrap_or(128000),
    };

    let registry = ToolRegistry::new();
    let mut app = tui::TuiApp::new(session, sess_dir, model);
    app.run(provider, config, registry).await
}

async fn run_rag(action: RagAction) -> Result<()> {
    let provider_name = std::env::var("ALCHEMY_PROVIDER")
        .map_err(|_| anyhow::anyhow!("ALCHEMY_PROVIDER is required"))?;
    let api_key = std::env::var("ALCHEMY_API_KEY").ok();
    let base_url = std::env::var("ALCHEMY_BASE_URL").ok();

    let embed_provider = std::env::var("ALCHEMY_RAG_EMBED_PROVIDER")
        .unwrap_or_else(|_| provider_name.clone());

    if !matches!(embed_provider.as_str(), "openai" | "gemini" | "ollama") {
        anyhow::bail!("RAG requires embedding provider (openai, gemini, or ollama). Set ALCHEMY_RAG_EMBED_PROVIDER.");
    }

    let store_path = std::env::var("ALCHEMY_RAG_STORE_PATH")
        .unwrap_or_else(|_| dirs_path("rag/vectors.db"));
    let chunk_size: usize = std::env::var("ALCHEMY_RAG_CHUNK_SIZE").ok().and_then(|s| s.parse().ok()).unwrap_or(512);
    let chunk_overlap: usize = std::env::var("ALCHEMY_RAG_CHUNK_OVERLAP").ok().and_then(|s| s.parse().ok()).unwrap_or(64);
    let top_k: usize = std::env::var("ALCHEMY_RAG_TOP_K").ok().and_then(|s| s.parse().ok()).unwrap_or(5);

    let embed_prov = providers::create_provider(&embed_provider, api_key.as_deref(), base_url.as_deref())?;
    let dimensions = embed_prov.embed_dimensions();

    let rag_config = rag::RagConfig {
        embed_provider,
        embed_model: std::env::var("ALCHEMY_RAG_EMBED_MODEL").unwrap_or_default(),
        chunk_size,
        chunk_overlap,
        top_k,
        store_path,
        dimensions,
    };

    let mut pipeline = rag::RagPipeline::new(rag_config).await?;

    match action {
        RagAction::Index { path, glob } => {
            let p = PathBuf::from(&path);
            let count = if p.is_file() {
                pipeline.index_file(&p).await?
            } else {
                pipeline.index_directory(&p, glob.as_deref()).await?
            };
            println!("Indexed {} chunks", count);
        }
        RagAction::Search { query } => {
            let results = pipeline.search(&query).await?;
            for r in &results {
                println!("[{:.3}] {} — {}", r.score, r.source, &r.content[..r.content.len().min(100)]);
            }
            if results.is_empty() {
                println!("No results found.");
            }
        }
        RagAction::Status => {
            let status = pipeline.status().await?;
            println!("Chunks: {}\nSources: {}", status.total_chunks, status.total_sources);
        }
        RagAction::Clear => {
            pipeline.clear().await?;
            println!("RAG index cleared.");
        }
    }

    Ok(())
}

fn read_stdin() -> Option<String> {
    use std::io::IsTerminal;
    if std::io::stdin().is_terminal() {
        return None;
    }
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf).ok()?;
    if buf.is_empty() { None } else { Some(buf) }
}

fn load_mcp_configs() -> Vec<types::McpServerConfig> {
    // Try env var first
    if let Ok(json_str) = std::env::var("ALCHEMY_MCP_SERVERS") {
        if let Ok(configs) = serde_json::from_str::<Vec<types::McpServerConfig>>(&json_str) {
            return configs;
        }
    }

    // Try config file
    let config_path = std::env::var("ALCHEMY_MCP_CONFIG")
        .unwrap_or_else(|_| dirs_path("mcp.json"));

    if let Ok(content) = std::fs::read_to_string(&config_path) {
        if let Ok(config) = serde_json::from_str::<types::McpConfigFile>(&content) {
            return config.servers;
        }
    }

    Vec::new()
}

fn dirs_path(sub: &str) -> String {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    format!("{}/.alchemy/{}", home, sub)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dirs_path() {
        let path = dirs_path("sessions");
        assert!(path.ends_with("/.alchemy/sessions"));
    }

    #[test]
    fn test_provider_create_unknown_fails() {
        let result = providers::create_provider("nonexistent", Some("key"), None);
        assert!(result.is_err());
    }

    #[test]
    fn test_provider_create_ollama_no_key() {
        // ollama doesn't require an API key
        let result = providers::create_provider("ollama", None, None);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().name(), "ollama");
    }

    #[test]
    fn test_provider_create_openai_missing_key() {
        let result = providers::create_provider("openai", None, None);
        assert!(result.is_err());
        assert!(result.err().unwrap().to_string().contains("ALCHEMY_API_KEY"));
    }

    #[test]
    fn test_provider_defaults() {
        let openai = providers::create_provider("openai", Some("k"), None).unwrap();
        assert_eq!(openai.default_model(), "gpt-4o-mini");

        let ollama = providers::create_provider("ollama", None, None).unwrap();
        assert_eq!(ollama.default_model(), "llama3.2");
    }

    #[test]
    fn test_mcp_config_parse_empty() {
        // When no env var or file is set, load_mcp_configs returns empty vec.
        // We can't test without side effects, but we can test the JSON parsing logic.
        let json = r#"[{"name":"test","transport":"stdio","cmd":"echo hello"}]"#;
        let configs: Vec<crate::types::McpServerConfig> =
            serde_json::from_str(json).expect("parse failed");
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].name, "test");
        assert_eq!(configs[0].transport, "stdio");
    }

    #[test]
    fn test_mcp_config_file_parse() {
        let json = r#"{"servers":[{"name":"db","transport":"sse","url":"http://localhost:3001/mcp"}]}"#;
        let config: crate::types::McpConfigFile =
            serde_json::from_str(json).expect("parse failed");
        assert_eq!(config.servers.len(), 1);
        assert_eq!(config.servers[0].name, "db");
        assert_eq!(config.servers[0].transport, "sse");
        assert_eq!(
            config.servers[0].url.as_deref(),
            Some("http://localhost:3001/mcp")
        );
    }
}
