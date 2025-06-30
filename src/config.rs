use crate::remote::Remote;
use serde::{Deserialize, Serialize};

const CONFIG_NAME: &str = "/home/vport/.config/recli/config.toml";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub remotes: Vec<Remote>,
    pub work_directory: std::path::PathBuf,
}

impl Config {
    pub fn read_config() -> anyhow::Result<Self> {
        let toml_string = std::fs::read_to_string(&CONFIG_NAME)?;
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

        let remote = config.get_remote("test_remote");
        assert_eq!(remote.name(), "test_remote");
        assert_eq!(remote.hostname(), "localhost");
        assert_eq!(remote.port(), 22);
        assert_eq!(remote.user(), "testuser");
        assert_eq!(remote.work_dir(), PathBuf::from("/remote/work"));
        assert_eq!(remote.queue_manager(), &QueueManager::PBS);
    }
}
