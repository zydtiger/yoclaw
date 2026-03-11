pub mod store;

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: Option<String>,
    pub version: Option<String>,
    pub path: String,
}

#[derive(Debug, Default)]
pub struct SkillStore {
    pub skills: Vec<Skill>,
}
