use crate::remote::Remote;
use dirs;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub remotes: Vec<Remote>,
    pub work_directory: std::path::PathBuf,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queuemanager::QueueManager;
    use std::path::PathBuf;

    #[test]
    fn test_parse_config_and_get_remote() {
        let toml_str = r#"
            work_directory = "/home/user/projects"

            [[remotes]]
            name = "test_remote"
            hostname = "localhost"
            port = 22
            user = "testuser"
            work_directory = "/remote/work"
            prepare_args = ["arg1", "arg2"]
            queue_manager = "PBS"
            
            [[remotes]]
            name = "another_remote"
            hostname = "remote.host"
            port = 2222
            user = "anotheruser"
            work_directory = "/other/work"
            prepare_args = []
            queue_manager = "Slurm"
        "#;

        let config: Config = toml::from_str(toml_str).unwrap();

        assert_eq!(config.work_directory, PathBuf::from("/home/user/projects"));
        assert_eq!(config.remotes.len(), 2);

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
