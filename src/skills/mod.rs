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
    let (name, description) = if content.starts_with("---") {
        let end = content[3..].find("---").map(|i| i + 3);
        if let Some(end_idx) = end {
            let frontmatter = &content[3..end_idx];
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
    for line in yaml.lines() {
        let line = line.trim();
        if line.starts_with(&format!("{}:", field)) {
            let value = line[field.len() + 1..].trim();
            // Remove quotes
            let value = value.trim_matches('"').trim_matches('\'');
            return Some(value.to_string());
        }
    }
    None
}

/// Match skills against user prompt and current directory
pub fn match_skills(skills: &[SkillMetadata], prompt: &str) -> Vec<usize> {
    let prompt_lower = prompt.to_lowercase();
    let mut matched = Vec::new();

    for (i, skill) in skills.iter().enumerate() {
        let desc_lower = skill.description.to_lowercase();
        let desc_words: Vec<&str> = desc_lower.split_whitespace().collect();

        // Simple keyword matching
        let match_count = desc_words.iter()
            .filter(|w| w.len() > 3 && prompt_lower.contains(*w))
            .count();

        if match_count >= 2 {
            matched.push(i);
        }
    }

    matched
}

/// Build system prompt additions from activated skills
pub async fn build_skill_context(skills: &[SkillMetadata], indices: &[usize]) -> String {
    let mut context = String::new();

    for &idx in indices {
        if let Some(skill) = skills.get(idx) {
            let skill_md = skill.path.join("SKILL.md");
            if let Ok(content) = tokio::fs::read_to_string(&skill_md).await {
                // Strip frontmatter, get body
                let body = if content.starts_with("---") {
                    if let Some(end) = content[3..].find("---") {
                        content[end + 6..].trim().to_string()
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
