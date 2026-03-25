use std::path::Path;

use tokio::fs;

use super::{Skill, SkillLoadError};

impl Skill {
    pub fn parse(content: &str, default_name: String, path: String) -> Self {
        let mut name = default_name;
        let mut description = None;
        let mut version = None;

        let mut lines = content.lines().peekable();
        let mut has_frontmatter = false;

        if let Some(&first_line) = lines.peek() {
            if first_line.trim() == "---" {
                has_frontmatter = true;
                lines.next();
            }
        }

        if has_frontmatter {
            for line in lines {
                let trimmed = line.trim();
                if trimmed == "---" {
                    break;
                }
                if let Some((key, value)) = line.split_once(':') {
                    let key = key.trim();
                    let value = value
                        .trim()
                        .trim_matches('"')
                        .trim_matches('\'')
                        .to_string();
                    match key {
                        "name" => name = value,
                        "description" => description = Some(value),
                        "version" => version = Some(value),
                        _ => {}
                    }
                }
            }
        }

        Self {
            name,
            description,
            version,
            path,
        }
    }
}

pub async fn load_skill_from_path(path: &Path) -> Result<Skill, SkillLoadError> {
    let metadata = fs::metadata(path)
        .await
        .map_err(|source| SkillLoadError::FsError {
            context: format!("Failed to inspect {}", path.display()),
            source,
        })?;

    if metadata.is_dir() {
        return load_skill_from_directory(path).await;
    }

    if metadata.is_file() && path.extension().unwrap_or_default() == "md" {
        return load_skill_from_markdown_file(path).await;
    }

    Err(SkillLoadError::UnsupportedPath(path.to_path_buf()))
}

async fn load_skill_from_directory(path: &Path) -> Result<Skill, SkillLoadError> {
    let skill_md_path = path.join("SKILL.md");
    let metadata = match fs::metadata(&skill_md_path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(SkillLoadError::MissingSkillFile(skill_md_path));
        }
        Err(source) => {
            return Err(SkillLoadError::FsError {
                context: format!("Failed to inspect {}", skill_md_path.display()),
                source,
            });
        }
    };

    if !metadata.is_file() {
        return Err(SkillLoadError::MissingSkillFile(skill_md_path));
    }

    let raw_contents =
        fs::read_to_string(&skill_md_path)
            .await
            .map_err(|source| SkillLoadError::FsError {
                context: format!("Failed to read {}", skill_md_path.display()),
                source,
            })?;
    let default_name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    Ok(Skill::parse(
        &raw_contents,
        default_name,
        path.to_string_lossy().to_string(),
    ))
}

async fn load_skill_from_markdown_file(path: &Path) -> Result<Skill, SkillLoadError> {
    let raw_contents =
        fs::read_to_string(path)
            .await
            .map_err(|source| SkillLoadError::FsError {
                context: format!("Failed to read {}", path.display()),
                source,
            })?;
    let default_name = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    Ok(Skill::parse(
        &raw_contents,
        default_name,
        path.to_string_lossy().to_string(),
    ))
}
