// use crate::config::Config;
use serde::{Deserialize, Serialize};
use tracing::error;

use std::path::PathBuf;

#[derive(thiserror::Error, std::fmt::Debug)]
pub enum Error {
    #[error("IO error, {0}")]
    IO(#[from] std::io::Error),
    #[error("Pattern error, {0}")]
    Pattern(#[from] glob::PatternError),
    #[error("Other glob error {0}")]
    ReadPath(#[from] glob::GlobError),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct JobManifest {
    pub remote: String,
    pub template: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub spec: Spec,
    pub exec: Exec,
    #[serde(default)]
    pub files: Vec<String>,
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

impl JobManifest {
    /// Parses all the globs in the include field, and merges all the paths that match with the files included in self.include
    pub fn build_files(&self) -> Result<Vec<PathBuf>, Error> {
        let mut files_to_send = vec![];
        for pattern in &self.files {
            for path in glob::glob(pattern)? {
                match path {
                    Ok(p) => files_to_send.push(p),
                    Err(e) => {
                        error!("{}", e);
                        continue;
                    }
                }
            }
        }
        Ok(files_to_send)
    }
}
