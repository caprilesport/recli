use anyhow::{anyhow, Context, Error, Result};
// use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use crate::remote::Remote;
use std::path::PathBuf;
// use toml::{map::Map, Value};

const CONFIG_NAME: &str = "config.toml";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    config_path: PathBuf,
    remotes: Vec<Remote>,
    // work_directory: PathBuf,
    projects_folder: PathBuf,
}

impl Config {
    pub fn parse_config(&mut self) {
        println!("aha")
    }
}
