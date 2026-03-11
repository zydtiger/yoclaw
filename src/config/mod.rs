use serde::{Deserialize, Serialize};

mod r#impl;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub openai_api_base_url: String,
    pub openai_api_key: String,
    pub openai_model: String,
    pub system_prompt: String,
    pub debug_mode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelsConfig {
    pub telegram_token: String,
    #[serde(default)]
    pub allowed_users: Vec<String>,
    pub recv_confirm: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    pub openai_api_base_url: String,
    pub openai_api_key: String,
    pub openai_model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub agent: AgentConfig,
    pub embedding: EmbeddingConfig,
    pub channels: ChannelsConfig,
    #[serde(default)]
    pub environment: std::collections::HashMap<String, String>,
}

#[derive(Debug)]
pub enum ConfigError {
    InvalidFormat(toml::de::Error),
    FsError(std::io::Error),
}
