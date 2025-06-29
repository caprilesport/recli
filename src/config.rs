use anyhow::{anyhow, Context, Error, Result};
// use clap::ValueEnum;
use crate::remote::Remote;
use serde::{Deserialize, Serialize};

use std::path::PathBuf;
use toml::{map::Map, Value};

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
