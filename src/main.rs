mod agent;
mod concourse;
mod http;
mod output;
mod providers;
mod rag;
mod skills;
mod tools;
mod tui;
mod types;

use anyhow::Result;
use clap::{Parser, Subcommand};
use serde::de::DeserializeOwned;
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
    /// Concourse check entrypoint
    Check,
    /// Concourse in entrypoint
    In {
        /// Destination directory for resource output
        dest_dir: String,
    },
    /// Concourse out entrypoint
    Out {
        /// Source directory for resource input
        source_dir: String,
    },
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
    let log_level = std::env::var("ALCHEMY_LOG_LEVEL")
        .ok()
        .and_then(|s| s.parse::<tracing::Level>().ok())
        .unwrap_or(tracing::Level::WARN);
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
                .with_max_level(log_level)
                .with_writer(std::sync::Mutex::new(file))
                .with_ansi(false)
                .init();
        } else {
            tracing_subscriber::fmt()
                .with_max_level(log_level)
                .with_writer(std::io::sink)
                .init();
        }
    } else {
        tracing_subscriber::fmt()
            .with_max_level(log_level)
            .with_writer(std::io::stderr)
            .init();
    }

    match cli.command {
        Some(Commands::Check) => {
            match run_concourse_check().await {
                Ok(()) => 0,
                Err(e) => { eprintln!("Error: {}", e); 1 }
            }
        }
        Some(Commands::In { dest_dir }) => {
            match run_concourse_in(&dest_dir).await {
                Ok(()) => 0,
                Err(e) => { eprintln!("Error: {}", e); 1 }
            }
        }
        Some(Commands::Out { source_dir }) => {
            match run_concourse_out(&source_dir).await {
                Ok(()) => 0,
                Err(e) => { eprintln!("Error: {}", e); 1 }
            }
        }
        Some(Commands::Tui { session, session_dir, system, max_steps, timeout }) => {
            match run_tui(session, session_dir, system, max_steps, timeout).await {
                Ok(code) => code,
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
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let matched = skills::match_skills(&all_skills, &prompt, &cwd);

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
        // Validate store backend — only SQLite is implemented.
        let rag_store = std::env::var("ALCHEMY_RAG_STORE").unwrap_or_else(|_| "sqlite".to_string());
        if !matches!(rag_store.as_str(), "sqlite") {
            eprintln!("Error: ALCHEMY_RAG_STORE={} is not supported. Only 'sqlite' is implemented.", rag_store);
            return Ok(2);
        }

        // Validate embedding provider
        let embed_provider = std::env::var("ALCHEMY_RAG_EMBED_PROVIDER")
            .unwrap_or_else(|_| provider_name.clone());

        if !matches!(embed_provider.as_str(), "openai" | "gemini" | "ollama" | "github-copilot" | "openrouter") {
            eprintln!("Error: RAG embedding provider '{}' does not support embeddings. Use openai, gemini, ollama, github-copilot, or openrouter. Set ALCHEMY_RAG_EMBED_PROVIDER.", embed_provider);
            return Ok(2);
        }

        let store_path = std::env::var("ALCHEMY_RAG_STORE_PATH")
            .unwrap_or_else(|_| dirs_path("rag/vectors.db"));
        let chunk_size: usize = std::env::var("ALCHEMY_RAG_CHUNK_SIZE").ok().and_then(|s| s.parse().ok()).unwrap_or(512);
        let chunk_overlap: usize = std::env::var("ALCHEMY_RAG_CHUNK_OVERLAP").ok().and_then(|s| s.parse().ok()).unwrap_or(64);
        let top_k: usize = std::env::var("ALCHEMY_RAG_TOP_K").ok().and_then(|s| s.parse().ok()).unwrap_or(5);

        let embed_prov = providers::create_provider(&embed_provider, api_key.as_deref(), base_url.as_deref())?;
        let dimensions = embed_prov.embed_dimensions();

        let embed_api_key = api_key.clone();
        let embed_base_url = std::env::var("ALCHEMY_RAG_EMBED_BASE_URL").ok();

        let rag_config = rag::RagConfig {
            embed_provider,
            embed_model: {
                let m = std::env::var("ALCHEMY_RAG_EMBED_MODEL").unwrap_or_default();
                if m.is_empty() { None } else { Some(m) }
            },
            embed_api_key,
            embed_base_url,
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
) -> Result<i32> {
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

    // Validate ALCHEMY_RAG_STORE before any setup.
    let rag_store = std::env::var("ALCHEMY_RAG_STORE").unwrap_or_else(|_| "sqlite".to_string());
    if !matches!(rag_store.as_str(), "sqlite") {
        eprintln!("Error: ALCHEMY_RAG_STORE={} is not supported. Only 'sqlite' is implemented.", rag_store);
        return Ok(2);
    }

    // Build tool registry with MCP + skills (same as pipe mode).
    let mut registry = ToolRegistry::new();

    let mcp_configs = load_mcp_configs();
    let mut mcp_display: Vec<tui::McpEntry> = Vec::new();
    if !mcp_configs.is_empty() {
        let mcp_tools = tools::mcp::discover_tools(&mcp_configs).await;
        // Collect display info grouped by server, preserving tool descriptions.
        let mut server_map: std::collections::HashMap<String, Vec<tui::McpToolEntry>> =
            std::collections::HashMap::new();
        for t in &mcp_tools {
            let prefix = format!("mcp_{}_", t.server_name);
            let display_name = t.definition.function.name
                .strip_prefix(&prefix)
                .unwrap_or(&t.definition.function.name)
                .to_string();
            server_map
                .entry(t.server_name.clone())
                .or_default()
                .push(tui::McpToolEntry {
                    name: display_name,
                    description: t.definition.function.description.clone(),
                });
        }
        let mut servers: Vec<String> = server_map.keys().cloned().collect();
        servers.sort();
        for srv in servers {
            let tools_list = server_map.remove(&srv).unwrap_or_default();
            let cfg = mcp_configs.iter().find(|c| c.name == srv);
            let transport = cfg.map(|c| c.transport.clone()).unwrap_or_default();
            let endpoint = cfg.map(|c| {
                c.cmd.clone()
                    .or_else(|| c.url.clone())
                    .unwrap_or_default()
            }).unwrap_or_default();
            mcp_display.push(tui::McpEntry { server: srv, transport, endpoint, tools: tools_list });
        }
        registry.add_mcp_tools(mcp_tools);
    }

    let skills_enabled = std::env::var("ALCHEMY_SKILLS_ENABLED")
        .map(|s| s != "false")
        .unwrap_or(true);

    let mut skill_context = String::new();
    let mut skills_display: Vec<tui::SkillEntry> = Vec::new();
    if skills_enabled {
        let skills_dir = std::env::var("ALCHEMY_SKILLS_DIR")
            .unwrap_or_else(|_| dirs_path("skills"));
        let skills_path = PathBuf::from(&skills_dir);
        let all_skills = skills::load_skills(&skills_path).await;
        // In TUI mode we have no initial prompt, so match on CWD signals only.
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let matched = skills::match_skills(&all_skills, "", &cwd);

        // Show ALL loaded skills in the info panel so the user can see what's available.
        for s in &all_skills {
            skills_display.push(tui::SkillEntry {
                name: s.name.clone(),
                description: s.description.clone(),
                scripts: s.scripts.iter().map(|sc| sc.name.clone()).collect(),
            });
        }

        if !matched.is_empty() {
            skill_context = skills::build_skill_context(&all_skills, &matched).await;
            let matched_skills: Vec<_> = matched.iter()
                .filter_map(|&i| all_skills.get(i))
                .cloned()
                .collect();
            let skill_tools = tools::skill::create_skill_tools(&matched_skills);
            registry.add_skill_tools(skill_tools);
        }
    }

    let full_system = if skill_context.is_empty() {
        system_prompt
    } else {
        format!("{}{}", system_prompt, skill_context)
    };

    let config = AgentConfig {
        model: model.clone(),
        system_prompt: full_system,
        max_steps: max_steps
            .or_else(|| std::env::var("ALCHEMY_MAX_STEPS").ok().and_then(|s| s.parse().ok()))
            .unwrap_or(30),
        timeout_secs: timeout
            .or_else(|| std::env::var("ALCHEMY_TIMEOUT_SECS").ok().and_then(|s| s.parse().ok()))
            .unwrap_or(30),
        context_window: std::env::var("ALCHEMY_CONTEXT_WINDOW").ok().and_then(|s| s.parse().ok()).unwrap_or(128000),
    };

    let prompt_history_path = dirs_path("prompt_history");
    let mut app = tui::TuiApp::new(session, sess_dir, prompt_history_path, model);
    app.set_skills_info(skills_display);
    app.set_mcp_info(mcp_display);
    app.run(provider, config, registry).await?;
    Ok(0)
}

async fn run_rag(action: RagAction) -> Result<()> {
    let provider_name = std::env::var("ALCHEMY_PROVIDER")
        .map_err(|_| anyhow::anyhow!("ALCHEMY_PROVIDER is required"))?;
    let api_key = std::env::var("ALCHEMY_API_KEY").ok();
    let base_url = std::env::var("ALCHEMY_BASE_URL").ok();

    let embed_provider = std::env::var("ALCHEMY_RAG_EMBED_PROVIDER")
        .unwrap_or_else(|_| provider_name.clone());

    if !matches!(embed_provider.as_str(), "openai" | "gemini" | "ollama" | "github-copilot" | "openrouter") {
        anyhow::bail!("RAG embedding provider '{}' does not support embeddings. Use openai, gemini, ollama, github-copilot, or openrouter. Set ALCHEMY_RAG_EMBED_PROVIDER.", embed_provider);
    }

    let store_path = std::env::var("ALCHEMY_RAG_STORE_PATH")
        .unwrap_or_else(|_| dirs_path("rag/vectors.db"));
    let chunk_size: usize = std::env::var("ALCHEMY_RAG_CHUNK_SIZE").ok().and_then(|s| s.parse().ok()).unwrap_or(512);
    let chunk_overlap: usize = std::env::var("ALCHEMY_RAG_CHUNK_OVERLAP").ok().and_then(|s| s.parse().ok()).unwrap_or(64);
    let top_k: usize = std::env::var("ALCHEMY_RAG_TOP_K").ok().and_then(|s| s.parse().ok()).unwrap_or(5);

    let embed_prov = providers::create_provider(&embed_provider, api_key.as_deref(), base_url.as_deref())?;
    let dimensions = embed_prov.embed_dimensions();

    let embed_api_key = api_key.clone();
    let embed_base_url = std::env::var("ALCHEMY_RAG_EMBED_BASE_URL").ok();

    let rag_config = rag::RagConfig {
        embed_provider,
        embed_model: {
            let m = std::env::var("ALCHEMY_RAG_EMBED_MODEL").unwrap_or_default();
            if m.is_empty() { None } else { Some(m) }
        },
        embed_api_key,
        embed_base_url,
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
                // Single-file case: spinner while indexing.
                let pb = indicatif::ProgressBar::new_spinner();
                pb.set_style(
                    indicatif::ProgressStyle::with_template("{spinner:.cyan} {msg}")
                        .unwrap()
                        .tick_strings(&["⠋","⠙","⠹","⠸","⠼","⠴","⠦","⠧","⠇","⠏"]),
                );
                pb.set_message(format!("Indexing {}", p.display()));
                pb.enable_steady_tick(std::time::Duration::from_millis(80));
                let n = pipeline.index_file(&p).await?;
                pb.finish_and_clear();
                n
            } else {
                // Directory case: collect files first, then show progress bar.
                let pattern = glob.as_deref().unwrap_or("**/*");
                let full_pattern = format!("{}/{}", p.display(), pattern);
                let files: Vec<std::path::PathBuf> = ::glob::glob(&full_pattern)
                    .unwrap_or_else(|_| ::glob::glob("").unwrap())
                    .flatten()
                    .filter(|e| e.is_file())
                    .collect();

                let total = files.len() as u64;
                let pb = indicatif::ProgressBar::new(total);
                pb.set_style(
                    indicatif::ProgressStyle::with_template(
                        "{spinner:.cyan} [{bar:40.cyan/blue}] {pos}/{len} {wide_msg}",
                    )
                    .unwrap()
                    .progress_chars("█▉▊▋▌▍▎▏ ")
                    .tick_strings(&["⠋","⠙","⠹","⠸","⠼","⠴","⠦","⠧","⠇","⠏"]),
                );
                pb.enable_steady_tick(std::time::Duration::from_millis(80));

                let mut total_chunks = 0usize;
                let mut failed = 0usize;
                for file in &files {
                    let display = file.strip_prefix(&p)
                        .unwrap_or(file)
                        .display()
                        .to_string();
                    pb.set_message(display);
                    match pipeline.index_file(file).await {
                        Ok(n) => total_chunks += n,
                        Err(e) => {
                            tracing::warn!("Failed to index {}: {}", file.display(), e);
                            failed += 1;
                        }
                    }
                    pb.inc(1);
                }
                pb.finish_and_clear();
                if failed > 0 {
                    eprintln!("Warning: {} file(s) could not be indexed (check log for details)", failed);
                }
                total_chunks
            };
            println!("Indexed {} chunks from {}", count, path);
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

async fn run_concourse_check() -> Result<()> {
    let input: types::ConcourseCheckInput = read_json_stdin()?;
    let versions = concourse::run_check(input).await?;
    println!("{}", serde_json::to_string(&versions)?);
    Ok(())
}

async fn run_concourse_in(dest_dir: &str) -> Result<()> {
    let input: types::ConcourseInInput = read_json_stdin()?;
    let output = concourse::run_in(input, dest_dir).await?;
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

async fn run_concourse_out(source_dir: &str) -> Result<()> {
    let input: types::ConcourseOutInput = read_json_stdin()?;
    let output = concourse::run_out(input, source_dir).await?;
    println!("{}", serde_json::to_string_pretty(&output)?);
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

fn read_json_stdin<T>() -> Result<T>
where
    T: DeserializeOwned,
{
    let input = read_stdin().ok_or_else(|| anyhow::anyhow!("Expected JSON on stdin"))?;
    Ok(serde_json::from_str(&input)?)
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

    #[test]
    fn test_concourse_check_command_parses() {
        let cli = Cli::try_parse_from(["alchemy", "check"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Check)));
    }

    #[test]
    fn test_concourse_in_command_parses() {
        let cli = Cli::try_parse_from(["alchemy", "in", "resource"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::In { ref dest_dir }) if dest_dir == "resource"));
    }

    #[test]
    fn test_concourse_out_command_parses() {
        let cli = Cli::try_parse_from(["alchemy", "out", "source"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Out { ref source_dir }) if source_dir == "source"));
    }
}
