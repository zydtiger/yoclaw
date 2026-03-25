pub mod install;
pub mod loader;
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

#[derive(Debug)]
pub enum SkillLoadError {
    MissingSkillFile(std::path::PathBuf),
    UnsupportedPath(std::path::PathBuf),
    FsError {
        context: String,
        source: std::io::Error,
    },
}

impl std::fmt::Display for SkillLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingSkillFile(path) => {
                write!(f, "missing SKILL.md at {}", path.display())
            }
            Self::UnsupportedPath(path) => {
                write!(f, "unsupported skill path {}", path.display())
            }
            Self::FsError { context, source } => write!(f, "{context}: {source}"),
        }
    }
}

impl std::error::Error for SkillLoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::FsError { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum SkillInstallError {
    InvalidDirectory(std::path::PathBuf),
    InvalidZipSource(String),
    MissingSkillFile(std::path::PathBuf),
    InvalidArchiveLayout(String),
    DuplicateSkillName(String),
    DuplicateDestination(std::path::PathBuf),
    InvalidSkillName(String),
    Io {
        context: String,
        source: std::io::Error,
    },
    Http {
        context: String,
        source: reqwest::Error,
    },
    Zip {
        context: String,
        source: zip::result::ZipError,
    },
    TaskJoin(tokio::task::JoinError),
}

impl std::fmt::Display for SkillInstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidDirectory(path) => {
                write!(f, "skill source must be a directory: {}", path.display())
            }
            Self::InvalidZipSource(source) => {
                write!(
                    f,
                    "zip source must be a local path or URL ending with .zip: {source}"
                )
            }
            Self::MissingSkillFile(path) => {
                write!(f, "missing SKILL.md at {}", path.display())
            }
            Self::InvalidArchiveLayout(message) => write!(f, "{message}"),
            Self::DuplicateSkillName(name) => {
                write!(f, "a skill named '{name}' is already installed")
            }
            Self::DuplicateDestination(path) => {
                write!(f, "skill destination already exists: {}", path.display())
            }
            Self::InvalidSkillName(name) => {
                write!(
                    f,
                    "skill name cannot be converted into a destination path: {name}"
                )
            }
            Self::Io { context, source } => write!(f, "{context}: {source}"),
            Self::Http { context, source } => write!(f, "{context}: {source}"),
            Self::Zip { context, source } => write!(f, "{context}: {source}"),
            Self::TaskJoin(source) => write!(f, "blocking installer task failed: {source}"),
        }
    }
}

impl std::error::Error for SkillInstallError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Http { source, .. } => Some(source),
            Self::Zip { source, .. } => Some(source),
            Self::TaskJoin(source) => Some(source),
            _ => None,
        }
    }
}
