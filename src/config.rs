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
    #[error("IO error, {0}")]
    IO(#[from] std::io::Error),
    #[error("Error in config.toml file:\n{0}")]
    MalformedTomlConfig(#[from] toml::de::Error),
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

    pub fn get_dir() -> std::io::Result<std::path::PathBuf> {
        let mut config_dir = dirs::config_dir().ok_or(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "OS Config directory not found",
        ))?;
        config_dir.push("recli");
        std::fs::create_dir_all(&config_dir)?;
        Ok(config_dir)
    }
}
