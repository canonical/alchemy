pub mod check;
pub mod in_cmd;
pub mod out;

use anyhow::Result;
use crate::types::*;
use crate::agent::{Agent, AgentConfig};
use crate::tools::ToolRegistry;
use crate::providers;

#[allow(dead_code)]
pub async fn run_check(input: ConcourseCheckInput) -> Result<Vec<ConcourseVersion>> {
    check::run(input).await
}

#[allow(dead_code)]
pub async fn run_in(input: ConcourseInInput, dest_dir: &str) -> Result<ConcourseInOutput> {
    in_cmd::run(input, dest_dir).await
}

#[allow(dead_code)]
pub async fn run_out(input: ConcourseOutInput, source_dir: &str) -> Result<ConcourseOutOutput> {
    out::run(input, source_dir).await
}

#[allow(dead_code)]
pub(crate) fn create_agent_from_source(
    source: &ConcourseSource,
    prompt_override: Option<&str>,
    model_override: Option<&str>,
    system_override: Option<&str>,
    max_steps_override: Option<u32>,
    timeout_override: Option<u64>,
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
        .unwrap_or("You are Alchemy, a CI/CD AI agent. Use the provided tools to accomplish tasks. Be concise. Report results as structured data when possible.")
        .to_string();

    let max_steps = max_steps_override.or(source.max_steps).unwrap_or(30);
    let timeout = timeout_override.or(source.timeout_secs).unwrap_or(30);

    let prompt = prompt_override
        .or(source.prompt.as_deref())
        .unwrap_or("")
        .to_string();

    let config = AgentConfig {
        model: model.clone(),
        system_prompt,
        max_steps,
        timeout_secs: timeout,
        context_window: 128000,
    };

    let registry = ToolRegistry::new();
    let agent = Agent::new(config, provider, registry);
    Ok((agent, prompt, model))
}
