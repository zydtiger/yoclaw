use serde::{Deserialize, Serialize};

mod r#impl;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentConfig {
    pub openai_api_base_url: String,
    pub openai_api_key: String,
    pub openai_model: String,
    pub system_prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub agent: AgentConfig,
}

#[derive(Debug)]
pub enum ConfigError {
    InvalidFormat(toml::de::Error),
    FsError(std::io::Error),
}
