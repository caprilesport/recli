// use crate::config::Config;
use serde::{Deserialize, Serialize};

use std::path::PathBuf;

// #[derive(thiserror::Error, std::fmt::Debug)]
// pub enum Error {
//     #[error("IO error, {0}")]
//     IO(#[from] std::io::Error),
// }

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct JobManifest {
    pub remote: String,
    pub template: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub spec: Spec,
    pub exec: Exec,
    #[serde(default)]
    pub files: Files,
    pub context: toml::Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Spec {
    pub nodes: u32,
    pub name: String,
    pub queue: String,
    pub nprocs: u32,
    pub memory: String,
    pub walltime: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Exec {
    pub command: String,
    pub args: Vec<String>,
    pub setup_commands: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Default, Serialize)]
pub struct Files {
    pub include: Vec<PathBuf>,
    pub include_glob: Vec<String>,
}

// impl Files {
//     pub fn collapse(self) -> Vec<PathBuf> {
//         unimplemented!()
//     }
// }

// impl JobManifest {
//     pub fn validate(&self) -> Result<(), Error> {
//         unimplemented!()
//     }

//     pub fn merge(&self, _cfg: Config) -> Result<Self, Error> {
//         unimplemented!()
//     }
// }
