use anyhow::Result;
use sha2::{Sha256, Digest};
use crate::types::*;
use crate::concourse::create_agent_from_source;

#[allow(dead_code)]
pub async fn run(input: ConcourseOutInput, source_dir: &str) -> Result<ConcourseOutOutput> {
    let params = input.params.as_ref();

    // Read stdin_file if specified
    let mut extra_input = String::new();
    if let Some(stdin_file) = params.and_then(|p| p.stdin_file.as_deref()) {
        let path = std::path::Path::new(source_dir).join(stdin_file);
        if let Ok(content) = tokio::fs::read_to_string(&path).await {
            extra_input = format!("\n\n--- stdin ---\n{}", content);
        }
    }

    let prompt_override = params.and_then(|p| p.prompt.as_deref());
    let model_override = params.and_then(|p| p.model.as_deref());
    let system_override = params.and_then(|p| p.system_prompt.as_deref());
    let max_steps_override = params.and_then(|p| p.max_steps);
    let timeout_override = params.and_then(|p| p.timeout_secs);

    let (agent, mut prompt, model) = create_agent_from_source(
        &input.source, prompt_override, model_override, system_override,
        max_steps_override, timeout_override,
    )?;

    prompt.push_str(&extra_input);

    let started = std::time::Instant::now();
    let result = agent.run(prompt).await;
    let duration_secs = started.elapsed().as_secs_f64();
    let answer = result.answer.unwrap_or_default();

    let mut hasher = Sha256::new();
    hasher.update(answer.as_bytes());
    let hash = hex::encode(hasher.finalize());
    let version_ref = format!("sha256:{}", &hash[..12]);

    Ok(ConcourseOutOutput {
        version: ConcourseVersion { r#ref: version_ref },
        metadata: vec![
            ConcourseMetadataEntry { name: "steps".into(), value: result.steps.to_string() },
            ConcourseMetadataEntry { name: "tools_used".into(), value: result.tools_used.join(",") },
            ConcourseMetadataEntry { name: "model".into(), value: model },
            ConcourseMetadataEntry { name: "duration_secs".into(), value: format!("{:.3}", duration_secs) },
        ],
    })
}
