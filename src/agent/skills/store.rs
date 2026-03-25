use tokio::fs;

use super::{loader, Skill, SkillLoadError, SkillStore};

impl SkillStore {
    /// Loads all skills from the `skills` directory inside the configuration directory.
    /// An Anthropic compatible skill is either a `SKILL.md` inside a subdirectory,
    /// or a `.md` file directly in the `skills` directory.
    pub async fn load_skills() -> Result<Self, std::io::Error> {
        let mut store = Self { skills: Vec::new() };
        let skills_dir = std::path::Path::new(&*crate::globals::CONFIG_DIR).join("skills");

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
            match loader::load_skill_from_path(&path).await {
                Ok(skill) => store.skills.push(skill),
                Err(SkillLoadError::MissingSkillFile(_))
                | Err(SkillLoadError::UnsupportedPath(_)) => {}
                Err(SkillLoadError::FsError { source, .. }) => return Err(source),
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
            combined_skills.push_str(&format!(
                "<skill name=\"{}\" path=\"{}\"",
                skill.name, skill.path
            ));

            if let Some(desc) = &skill.description {
                combined_skills.push_str(&format!(" description=\"{}\"", desc));
            }
            if let Some(ver) = &skill.version {
                combined_skills.push_str(&format!(" version=\"{}\"", ver));
            }

            combined_skills.push_str(" />\n");
        }
        combined_skills.push_str("</skills>");

        combined_skills
    }

    /// Fetches a skill by its name
    pub fn get_skill(&self, name: &str) -> Option<&Skill> {
        self.skills.iter().find(|s| s.name == name)
    }
}
