use crate::types::PipeOutput;

pub fn format_json(output: &PipeOutput) -> String {
    serde_json::to_string_pretty(output).unwrap_or_else(|_| "{}".to_string())
}

pub fn format_text(output: &PipeOutput) -> String {
    if let Some(ref answer) = output.answer {
        answer.clone()
    } else if let Some(ref error) = output.error {
        format!("Error: {}", error)
    } else {
        String::new()
    }
}
