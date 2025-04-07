use anyhow::{anyhow, Context, Error, Result};
// use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use crate::remote::{Local, Remote};
use std::path::PathBuf;
use toml::{map::Map, Value};

const CONFIG_NAME: &str = "/home/vport/projects/recli/config.toml";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub remotes: Vec<Remote>,
    pub local: Local,
}

impl Config {
    pub fn read_config() -> anyhow::Result<Self> {
        let toml_string = std::fs::read_to_string(&CONFIG_NAME)?;
        let config: Config = toml::from_str(&toml_string)?;
        Ok(config)
    }
}
