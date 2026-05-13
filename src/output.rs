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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_json_success() {
        let output = PipeOutput {
            success: true,
            answer: Some("All tests passed.".to_string()),
            steps: 3,
            tools_used: vec!["execute_cmd".to_string()],
            error: None,
        };
        let json = format_json(&output);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["success"].as_bool().unwrap(), true);
        assert_eq!(v["answer"].as_str().unwrap(), "All tests passed.");
        assert_eq!(v["steps"].as_u64().unwrap(), 3);
        assert!(v["error"].is_null());
    }

    #[test]
    fn test_format_json_failure() {
        let output = PipeOutput {
            success: false,
            answer: None,
            steps: 1,
            tools_used: vec![],
            error: Some("Max steps exceeded".to_string()),
        };
        let json = format_json(&output);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["success"].as_bool().unwrap(), false);
        assert!(v["answer"].is_null());
        assert_eq!(v["error"].as_str().unwrap(), "Max steps exceeded");
    }

    #[test]
    fn test_format_text_with_answer() {
        let output = PipeOutput {
            success: true,
            answer: Some("Hello world".to_string()),
            steps: 1,
            tools_used: vec![],
            error: None,
        };
        assert_eq!(format_text(&output), "Hello world");
    }

    #[test]
    fn test_format_text_with_error() {
        let output = PipeOutput {
            success: false,
            answer: None,
            steps: 0,
            tools_used: vec![],
            error: Some("Something failed".to_string()),
        };
        assert_eq!(format_text(&output), "Error: Something failed");
    }

    #[test]
    fn test_format_text_empty() {
        let output = PipeOutput {
            success: false,
            answer: None,
            steps: 0,
            tools_used: vec![],
            error: None,
        };
        assert_eq!(format_text(&output), "");
    }
}
