use anyhow::Result;
use sha2::{Sha256, Digest};
use crate::types::*;
use crate::concourse::create_agent_from_source;

pub async fn run(input: ConcourseCheckInput) -> Result<Vec<ConcourseVersion>> {
    let (agent, prompt, _model) = create_agent_from_source(
        &input.source, None, None, None, None, None, None,
    )?;

    if prompt.is_empty() {
        // No prompt configured, return empty versions
        return Ok(Vec::new());
    }

    let result = agent.run(prompt).await;
    let answer = result.answer.unwrap_or_default();

    let mut hasher = Sha256::new();
    hasher.update(answer.as_bytes());
    let hash = hex::encode(hasher.finalize());
    let version_ref = format!("sha256:{}", &hash[..12]);

    let new_version = ConcourseVersion { r#ref: version_ref.clone() };

    // If previous version matches, return empty (no new version)
    if let Some(ref prev) = input.version {
        if prev.r#ref == version_ref {
            return Ok(vec![prev.clone()]);
        }
    }

    Ok(vec![new_version])
}
