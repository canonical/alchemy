use anyhow::Result;
use crate::types::*;
use crate::concourse::create_agent_from_source;

#[allow(dead_code)]
pub async fn run(input: ConcourseInInput, dest_dir: &str) -> Result<ConcourseInOutput> {
    let (agent, prompt) = create_agent_from_source(
        &input.source, None, None, None, None, None,
    )?;

    let result = agent.run(prompt).await;
    let answer = result.answer.clone().unwrap_or_default();

    // Write output files
    let dest = std::path::Path::new(dest_dir);
    tokio::fs::create_dir_all(dest).await?;

    tokio::fs::write(dest.join("response.txt"), &answer).await?;

    let json_output = serde_json::json!({
        "success": result.success,
        "answer": answer,
        "steps": result.steps,
        "tools_used": result.tools_used,
    });
    tokio::fs::write(dest.join("response.json"), serde_json::to_string_pretty(&json_output)?).await?;

    let metadata_json = serde_json::json!({
        "steps": result.steps,
        "tools_used": result.tools_used,
    });
    tokio::fs::write(dest.join("metadata.json"), serde_json::to_string_pretty(&metadata_json)?).await?;

    Ok(ConcourseInOutput {
        version: input.version,
        metadata: vec![
            ConcourseMetadataEntry { name: "steps".into(), value: result.steps.to_string() },
        ],
    })
}
