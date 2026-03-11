use std::path::Path;
use tokio::fs;

use super::{Skill, SkillMetadata, SkillStore};

impl Skill {
    pub fn parse(content: &str, default_name: String) -> Self {
        let mut name = default_name;
        let mut description = None;
        let mut version = None;

        let mut lines = content.lines().peekable();
        let mut has_frontmatter = false;

        if let Some(&first_line) = lines.peek() {
            if first_line.trim() == "---" {
                has_frontmatter = true;
                lines.next(); // consume "---"
            }
        }

        if has_frontmatter {
            while let Some(line) = lines.next() {
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

        let contents = lines.collect::<Vec<&str>>().join("\n");

        Self {
            metadata: SkillMetadata {
                name,
                description,
                version,
            },
            contents,
        }
    }
}

impl SkillStore {
    /// Loads all skills from the `skills` directory inside the configuration directory.
    /// An Anthropic compatible skill is either a `SKILL.md` inside a subdirectory,
    /// or a `.md` file directly in the `skills` directory.
    pub async fn load_skills() -> Result<Self, std::io::Error> {
        let mut store = Self { skills: Vec::new() };
        let skills_dir = Path::new(&*crate::globals::CONFIG_DIR).join("skills");

        if !fs::try_exists(&skills_dir).await.unwrap_or(false) {
            return Ok(store);
        }

        let metadata = match fs::metadata(&skills_dir).await {
            Ok(m) => m,
            Err(_) => return Ok(store),
        };

        if !metadata.is_dir() {
            return Ok(store);
        }

        let mut entries = fs::read_dir(&skills_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let file_type = entry.file_type().await?;

            if file_type.is_dir() {
                // Check for SKILL.md in the subdirectory
                let skill_md_path = path.join("SKILL.md");
                if fs::try_exists(&skill_md_path).await.unwrap_or(false) {
                    if let Ok(metadata) = fs::metadata(&skill_md_path).await {
                        if metadata.is_file() {
                            let raw_contents = fs::read_to_string(&skill_md_path).await?;
                            let default_name = path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string();
                            store.skills.push(Skill::parse(&raw_contents, default_name));
                        }
                    }
                }
            } else if file_type.is_file() && path.extension().unwrap_or_default() == "md" {
                // Check for direct .md files
                let raw_contents = fs::read_to_string(&path).await?;
                let default_name = path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                store.skills.push(Skill::parse(&raw_contents, default_name));
            }
        }

        Ok(store)
    }

    /// Returns the combined context format to inject into the agent.
    pub fn get_context(&self) -> String {
        if self.skills.is_empty() {
            return String::new();
        }

        let mut combined_skills = String::from("<skills>\n");
        for skill in &self.skills {
            combined_skills.push_str(&format!("<skill name=\"{}\"", skill.metadata.name));

            if let Some(desc) = &skill.metadata.description {
                combined_skills.push_str(&format!(" description=\"{}\"", desc));
            }
            if let Some(ver) = &skill.metadata.version {
                combined_skills.push_str(&format!(" version=\"{}\"", ver));
            }

            combined_skills.push_str(" />\n");
        }
        combined_skills.push_str("</skills>");

        combined_skills
    }
}
