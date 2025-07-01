use crate::remote::Remote;
use dirs;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub remotes: Vec<Remote>,
    pub work_directory: std::path::PathBuf,
}

#[derive(thiserror::Error, std::fmt::Debug)]
pub enum ConfigError {
    #[error("OS Config directory not found.")]
    ConfigDirNotFound,
    #[error("Recli config directory not found.")]
    RecliConfigDirNotFound,
    #[error("IO error, {0}")]
    IO(#[from] std::io::Error),
    #[error("Error in config.toml file:\n{0}")]
    MalformedTomlFile(#[from] toml::de::Error),
}

impl Config {
    pub fn read() -> Result<Self, ConfigError> {
        let config_file = Config::get_dir()?.join("config.toml");
        let toml_string = std::fs::read_to_string(config_file)?;
        let config: Config = toml::from_str(&toml_string)?;
        Ok(config)
    }

    pub fn get_remote(self, remote: &str) -> Remote {
        self.remotes
            .into_iter()
            .filter(|r| r.name() == remote)
            .next()
            .unwrap()
    }

    fn get_dir() -> Result<std::path::PathBuf, ConfigError> {
        let mut config_dir = dirs::config_dir().ok_or(ConfigError::ConfigDirNotFound)?;
        config_dir.push("recli");
        if config_dir.exists() {
            Ok(config_dir)
        } else {
            Err(ConfigError::RecliConfigDirNotFound)
        }
    }
}
