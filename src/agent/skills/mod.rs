pub mod store;

#[derive(Debug, Clone)]
pub struct SkillMetadata {
    pub name: String,
    pub description: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Skill {
    pub metadata: SkillMetadata,
    pub contents: String,
    pub base_dir: String,
}

#[derive(Debug, Default)]
pub struct SkillStore {
    pub skills: Vec<Skill>,
}
