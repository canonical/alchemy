pub mod types;

use anyhow::Result;
use crate::types::SkillMetadata;
use std::path::Path;

/// Load all skills from the skills directory
pub async fn load_skills(skills_dir: &Path) -> Vec<SkillMetadata> {
    let mut skills = Vec::new();

    if !skills_dir.exists() {
        tracing::debug!("Skills directory does not exist: {}", skills_dir.display());
        return skills;
    }

    let mut dir = match tokio::fs::read_dir(skills_dir).await {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("Failed to read skills directory: {}", e);
            return skills;
        }
    };

    while let Ok(Some(entry)) = dir.next_entry().await {
        let path = entry.path();
        if path.is_dir() {
            let skill_md = path.join("SKILL.md");
            if skill_md.exists() {
                match parse_skill(&skill_md, &path).await {
                    Ok(skill) => {
                        tracing::info!("Loaded skill: {}", skill.name);
                        skills.push(skill);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to parse skill at {}: {}", skill_md.display(), e);
                    }
                }
            }
        }
    }

    skills
}

async fn parse_skill(skill_md: &Path, skill_dir: &Path) -> Result<SkillMetadata> {
    let content = tokio::fs::read_to_string(skill_md).await?;

    // Parse YAML frontmatter
    let (name, description) = if let Some(rest) = content.strip_prefix("---") {
        let end = rest.find("---");
        if let Some(end_idx) = end {
            let frontmatter = &rest[..end_idx];
            let name = extract_yaml_field(frontmatter, "name")
                .unwrap_or_else(|| skill_dir.file_name().unwrap().to_string_lossy().to_string());
            let description = extract_yaml_field(frontmatter, "description").unwrap_or_default();
            (name, description)
        } else {
            (skill_dir.file_name().unwrap().to_string_lossy().to_string(), String::new())
        }
    } else {
        (skill_dir.file_name().unwrap().to_string_lossy().to_string(), String::new())
    };

    // Discover scripts
    let scripts_dir = skill_dir.join("scripts");
    let mut scripts = Vec::new();
    if scripts_dir.exists() {
        let mut dir = tokio::fs::read_dir(&scripts_dir).await?;
        while let Ok(Some(entry)) = dir.next_entry().await {
            let path = entry.path();
            if path.is_file() {
                let script_name = path.file_name().unwrap().to_string_lossy().to_string();
                scripts.push(crate::types::SkillScript {
                    name: script_name,
                    path,
                });
            }
        }
    }

    Ok(SkillMetadata {
        name,
        description,
        path: skill_dir.to_path_buf(),
        scripts,
    })
}

fn extract_yaml_field(yaml: &str, field: &str) -> Option<String> {
    let prefix = format!("{}:", field);
    for line in yaml.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix(&prefix) {
            let value = rest.trim();
            // Remove quotes
            let value = value.trim_matches('"').trim_matches('\'');
            return Some(value.to_string());
        }
    }
    None
}

/// Match skills against the user prompt, augmenting with cwd file-presence signals.
/// E.g. when `Cargo.toml` is in cwd, "rust"/"cargo" are treated as if they appeared
/// in the prompt — so a Rust-tagged skill activates even for a prompt like
/// "refactor this".
pub fn match_skills(skills: &[SkillMetadata], prompt: &str, cwd: &std::path::Path) -> Vec<usize> {
    let mut haystack = prompt.to_lowercase();
    for token in cwd_signals(cwd) {
        haystack.push(' ');
        haystack.push_str(token);
    }
    let mut matched = Vec::new();

    for (i, skill) in skills.iter().enumerate() {
        let desc_lower = skill.description.to_lowercase();
        let desc_words: Vec<&str> = desc_lower.split_whitespace().collect();

        let match_count = desc_words.iter()
            .filter(|w| w.len() > 3 && haystack.contains(*w))
            .count();

        if match_count >= 2 {
            matched.push(i);
        }
    }

    matched
}

/// Return ecosystem tokens implied by marker files in `cwd`.
fn cwd_signals(cwd: &std::path::Path) -> Vec<&'static str> {
    let mut tokens = Vec::new();
    let probe = |name: &str| cwd.join(name).exists();

    if probe("Cargo.toml") {
        tokens.extend(["rust", "cargo", "crate"]);
    }
    if probe("package.json") {
        tokens.extend(["javascript", "typescript", "node", "npm"]);
    }
    if probe("pyproject.toml") || probe("setup.py") || probe("requirements.txt") {
        tokens.extend(["python", "pip"]);
    }
    if probe("go.mod") {
        tokens.extend(["go", "golang"]);
    }
    if probe("pom.xml") || probe("build.gradle") || probe("build.gradle.kts") {
        tokens.extend(["java", "maven", "gradle"]);
    }
    if probe("Gemfile") {
        tokens.extend(["ruby", "rails"]);
    }
    if probe("Dockerfile") || probe("compose.yaml") || probe("docker-compose.yml") {
        tokens.extend(["docker", "container"]);
    }
    if probe("terraform.tf") || cwd.join("main.tf").exists() {
        tokens.extend(["terraform", "infrastructure"]);
    }
    if cwd.join(".github").is_dir() {
        tokens.push("github");
    }
    tokens
}

/// Build system prompt additions from activated skills
pub async fn build_skill_context(skills: &[SkillMetadata], indices: &[usize]) -> String {
    let mut context = String::new();

    for &idx in indices {
        if let Some(skill) = skills.get(idx) {
            let skill_md = skill.path.join("SKILL.md");
            if let Ok(content) = tokio::fs::read_to_string(&skill_md).await {
                // Strip frontmatter, get body
                let body = if let Some(rest) = content.strip_prefix("---") {
                    if let Some(end) = rest.find("---") {
                        rest[end + 3..].trim().to_string()
                    } else {
                        content
                    }
                } else {
                    content
                };
                context.push_str(&format!("\n\n## Skill: {}\n{}", skill.name, body));
            }
        }
    }

    context
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SkillMetadata;

    fn make_skill(name: &str, description: &str) -> SkillMetadata {
        SkillMetadata {
            name: name.to_string(),
            description: description.to_string(),
            path: std::path::PathBuf::from("/tmp/fake"),
            scripts: vec![],
        }
    }

    #[test]
    fn test_match_skills_by_keywords() {
        let skills = vec![
            make_skill("rust-reviewer", "Expert Rust code reviewer. Use when working with Rust projects."),
            make_skill("security-scanner", "Security vulnerability scanner for checking code security issues."),
        ];

        // Should match rust-reviewer (2 keywords: "rust", "code")
        let empty = std::path::Path::new("/nonexistent_dir_for_skill_test");
        let matched = match_skills(&skills, "Please review my Rust code", empty);
        assert!(matched.contains(&0));

        // Should match security-scanner
        let matched = match_skills(&skills, "Check security vulnerabilities in my code", empty);
        assert!(matched.contains(&1));

        // No match for unrelated prompt
        let matched = match_skills(&skills, "What is the weather today?", empty);
        assert!(matched.is_empty());
    }

    #[test]
    fn test_match_skills_cwd_inference() {
        let skills = vec![
            make_skill("rust-reviewer", "Expert Rust cargo crate reviewer."),
        ];
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();

        // Prompt alone says nothing about Rust — should still match because Cargo.toml
        // contributes "rust"/"cargo" to the haystack.
        let matched = match_skills(&skills, "refactor this please", dir.path());
        assert!(matched.contains(&0), "expected cwd-inferred match, got {:?}", matched);
    }

    #[test]
    fn test_match_skills_empty() {
        let empty = std::path::Path::new("/nonexistent_dir_for_skill_test");
        let matched = match_skills(&[], "anything", empty);
        assert!(matched.is_empty());
    }

    #[tokio::test]
    async fn test_load_skills_nonexistent_dir() {
        let path = std::path::Path::new("/tmp/nonexistent_skills_dir_12345");
        let skills = load_skills(path).await;
        assert!(skills.is_empty());
    }

    #[tokio::test]
    async fn test_load_skills_with_skill_md() {
        let dir = tempfile::tempdir().unwrap();
        let skill_dir = dir.path().join("my-skill");
        tokio::fs::create_dir_all(&skill_dir).await.unwrap();
        let skill_md = skill_dir.join("SKILL.md");
        tokio::fs::write(&skill_md, r#"---
name: my-skill
description: A test skill for testing purposes.
---

# My Skill
Do things."#).await.unwrap();

        let skills = load_skills(dir.path()).await;
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "my-skill");
        assert!(skills[0].description.contains("test skill"));
    }

    #[tokio::test]
    async fn test_build_skill_context_empty() {
        let ctx = build_skill_context(&[], &[]).await;
        assert!(ctx.is_empty());
    }
}
