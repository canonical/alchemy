use anyhow::Result;
use crate::types::*;
use crate::concourse::create_agent_from_source;

pub async fn run(input: ConcourseInInput, dest_dir: &str) -> Result<ConcourseInOutput> {
    let (agent, prompt, model) = create_agent_from_source(
        &input.source, None, None, None, None, None,
    )?;

    let started = std::time::Instant::now();
    let result = agent.run(prompt).await;
    let duration_secs = started.elapsed().as_secs_f64();
    let answer = result.answer.clone().unwrap_or_default();

    let dest = std::path::Path::new(dest_dir);
    tokio::fs::create_dir_all(dest).await?;

    // output_format from source: "json" → only response.json, "text" → only response.txt,
    // absent/anything else → write both (default).
    let fmt = input.source.output_format.as_deref().unwrap_or("both");
    let write_text = fmt != "json";
    let write_json = fmt != "text";

    if write_text {
        tokio::fs::write(dest.join("response.txt"), &answer).await?;
    }
    if write_json {
        let json_output = serde_json::json!({
            "success": result.success,
            "answer": answer,
            "steps": result.steps,
            "tools_used": result.tools_used,
            "model": model,
            "duration_secs": duration_secs,
        });
        tokio::fs::write(dest.join("response.json"), serde_json::to_string_pretty(&json_output)?).await?;
    }

    let metadata_json = serde_json::json!({
        "steps": result.steps,
        "tools_used": result.tools_used,
        "model": model,
        "duration_secs": duration_secs,
    });
    tokio::fs::write(dest.join("metadata.json"), serde_json::to_string_pretty(&metadata_json)?).await?;

    Ok(ConcourseInOutput {
        version: input.version,
        metadata: vec![
            ConcourseMetadataEntry { name: "steps".into(), value: result.steps.to_string() },
            ConcourseMetadataEntry { name: "tools_used".into(), value: result.tools_used.join(",") },
            ConcourseMetadataEntry { name: "model".into(), value: model },
            ConcourseMetadataEntry { name: "duration_secs".into(), value: format!("{:.3}", duration_secs) },
        ],
    })
}
