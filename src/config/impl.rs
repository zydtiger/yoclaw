use std::env;
use std::fmt;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;

use crate::config::{Config, ConfigError};

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFormat(e) => write!(f, "Invalid format: {}", e),
            Self::FsError(e) => write!(f, "File system error: {}", e),
        }
    }
}

impl Config {
    /// Get the config file path from CONFIG_PATH environment variable
    fn get_config_path() -> PathBuf {
        let config_dir = env::var("CONFIG_PATH").unwrap_or_else(|_| ".".to_string()); // TODO: change default config path
        PathBuf::from(config_dir).join("config.toml")
    }

    /// Load config from $CONFIG_PATH/config.toml
    /// If the file doesn't exist, log a warning and create a default config
    pub async fn load() -> Result<Self, ConfigError> {
        let config_path = Self::get_config_path();

        if config_path.exists() {
            match Self::parse_file(&config_path).await {
                Ok(config) => Ok(config),
                Err(e) => {
                    log::error!(
                        "Failed to parse config file: {}, creating default config",
                        e
                    );
                    Err(e)
                }
            }
        } else {
            log::warn!(
                "Config file not found at {:?}, creating default config",
                config_path
            );
            Self::create_default().await
        }
    }

    /// Create a default config from template and save it to $CONFIG_PATH/config.toml
    async fn create_default() -> Result<Self, ConfigError> {
        let config_content = include_str!("template/config.toml");
        let config_path = Self::get_config_path();

        // Parse the default config from the template
        let config: Self =
            toml::from_str(config_content).expect("Failed to parse default config template");

        // Create the config directory if it doesn't exist
        if let Some(parent) = config_path.parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                log::error!("Failed to create config directory: {}", e);
                return Err(ConfigError::FsError(e));
            }
        }

        // Write the config to file
        match Self::write_to_file(&config_path, config_content).await {
            Ok(_) => {
                log::info!("Created default config at {:?}", config_path);
                Ok(config)
            }
            Err(e) => {
                log::error!("Failed to write default config to {:?}: {}", config_path, e);
                Err(e)
            }
        }
    }

    /// Parse config from a file path
    async fn parse_file(path: &PathBuf) -> Result<Self, ConfigError> {
        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(ConfigError::FsError)?;

        toml::from_str(&content).map_err(ConfigError::InvalidFormat)
    }

    /// Write config to a file
    async fn write_to_file(path: &PathBuf, content: &str) -> Result<(), ConfigError> {
        let mut file = tokio::fs::File::create(path)
            .await
            .map_err(ConfigError::FsError)?;
        file.write_all(content.as_bytes())
            .await
            .map_err(ConfigError::FsError)?;
        Ok(())
    }
}
