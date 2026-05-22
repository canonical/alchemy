pub mod check;
pub mod in_cmd;
pub mod out;

use anyhow::Result;
use crate::types::*;
use crate::types::ThinkingLevel;
use crate::agent::{Agent, AgentConfig};
use crate::tools::ToolRegistry;
use crate::providers;

const DEFAULT_SYSTEM_PROMPT: &str = "You are Alchemy, a CI/CD AI agent. Use the provided tools to accomplish tasks.\nBe concise. Report results as structured data when possible. If a tool fails,\nanalyze the error and either retry with a corrected approach or report the\nfailure with your analysis.";

pub async fn run_check(input: ConcourseCheckInput) -> Result<Vec<ConcourseVersion>> {
    check::run(input).await
}

pub async fn run_in(input: ConcourseInInput, dest_dir: &str) -> Result<ConcourseInOutput> {
    in_cmd::run(input, dest_dir).await
}

pub async fn run_out(input: ConcourseOutInput, source_dir: &str) -> Result<ConcourseOutOutput> {
    out::run(input, source_dir).await
}

pub(crate) fn create_agent_from_source(
    source: &ConcourseSource,
    prompt_override: Option<&str>,
    model_override: Option<&str>,
    system_override: Option<&str>,
    max_steps_override: Option<u32>,
    timeout_override: Option<u64>,
    thinking_override: Option<&str>,
) -> Result<(Agent, String, String)> {
    let provider_name = source.provider.as_deref()
        .ok_or_else(|| anyhow::anyhow!("source.provider is required"))?;
    let api_key = source.api_key.as_deref();

    let provider = providers::create_provider(provider_name, api_key, None)?;
    let model = model_override
        .or(source.model.as_deref())
        .unwrap_or(provider.default_model())
        .to_string();

    let system_prompt = system_override
        .or(source.system_prompt.as_deref())
        .unwrap_or(DEFAULT_SYSTEM_PROMPT)
        .to_string();

    let max_steps = max_steps_override.or(source.max_steps).unwrap_or(30);
    let timeout = timeout_override.or(source.timeout_secs).unwrap_or(30);

    let prompt = prompt_override
        .or(source.prompt.as_deref())
        .unwrap_or("")
        .to_string();

    // Resolve thinking level: param override > source field > env var > Off.
    let thinking_level = thinking_override
        .or(source.thinking_level.as_deref())
        .and_then(ThinkingLevel::from_str)
        .unwrap_or_else(|| {
            std::env::var("ALCHEMY_THINKING_LEVEL")
                .ok()
                .and_then(|s| ThinkingLevel::from_str(&s))
                .unwrap_or(ThinkingLevel::Off)
        });

    let config = AgentConfig {
        model: model.clone(),
        system_prompt,
        max_steps,
        timeout_secs: timeout,
        context_window: 128000,
        thinking_level,
    };

    let registry = ToolRegistry::new();
    let agent = Agent::new(config, provider, registry);
    Ok((agent, prompt, model))
}
