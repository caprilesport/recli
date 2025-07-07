use dirs;
use remotelib::remote::Remote;
use serde::{Deserialize, Serialize};
use tracing::trace;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub remotes: Vec<Remote>,
    #[serde(skip)]
    pub ignore: Vec<glob::Pattern>,
}

#[derive(thiserror::Error, std::fmt::Debug)]
pub enum ConfigError {
    #[error("IO error, {0}")]
    IO(#[from] std::io::Error),
    #[error("Error in config.toml file: {0}")]
    MalformedTomlConfig(#[from] toml::de::Error),
    #[error("Remote {0} not found in config file")]
    RemoteNotFound(String),
    #[error("Invalid glob pattern {0}")]
    Glob(#[from] glob::PatternError),
}

impl Config {
    pub fn read() -> Result<Self, ConfigError> {
        let config_dir = Config::get_dir()?;
        let config_file = config_dir.join("config.toml");
        trace!("Attempting to read {:?}", config_file);
        let toml_string = std::fs::read_to_string(config_file)?;
        trace!("Parsing config file");
        let mut config: Config = toml::from_str(&toml_string)?;

        let ignore_file = config_dir.join("ignore");
        if ignore_file.exists() {
            let ignore_patterns = std::fs::read_to_string(ignore_file)?
                .lines()
                .map(glob::Pattern::new)
                .collect::<Result<Vec<glob::Pattern>, glob::PatternError>>()?;
            config.ignore = ignore_patterns;
        } else {
            config.ignore = Vec::new();
        }

        Ok(config)
    }

    pub fn get_remote(&self, remote: &str) -> Result<&Remote, ConfigError> {
        self.remotes
            .iter()
            .filter(|r| r.name() == remote)
            .next()
            .ok_or(ConfigError::RemoteNotFound(remote.to_string()))
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
