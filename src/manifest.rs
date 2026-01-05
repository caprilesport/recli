// use crate::config::Config;
use serde::{Deserialize, Serialize};
use tracing::{debug, error};

use tera::{Context, Tera};

use std::path::PathBuf;

#[derive(thiserror::Error, std::fmt::Debug)]
pub enum Error {
    #[error("IO error, {0}")]
    IO(#[from] std::io::Error),
    #[error("Pattern error, {0}")]
    Pattern(#[from] glob::PatternError),
    #[error("No permissions to read a file, check your permissions {0}")]
    ReadPath(#[from] glob::GlobError),
    #[error("Failed to parse manifest toml {0}")]
    Toml(#[from] toml::de::Error),
    #[error("Tera error {0}")]
    Tera(#[from] tera::Error),
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
    /// Create a JobManifest by parsing a toml file
    pub fn from_file(file: &std::path::Path) -> Result<Self, Error> {
        debug!("Parsing file {:?}", file);
        // right now the manifest should be self contained.
        // next step is implementing some merging behaviour for the default defined in the config file, cli flags and the toml manifest.
        let manifest_str = std::fs::read_to_string(file)?;
        let manifest = toml::from_str(&manifest_str)?;
        Ok(manifest)
    }

    // TODO: add debug information
    /// Parses all the globs in the files input and returns a vector containing all matched files.
    fn build_files(&self) -> Result<Vec<PathBuf>, Error> {
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

    // TODO: add debug information
    fn load_templates() -> Result<Tera, Error> {
        let config_dir = crate::config::Config::get_dir()?;
        let mut templates_glob = config_dir.to_string_lossy().into_owned();
        templates_glob.push_str("/templates/*");
        let tera = Tera::new(&templates_glob)?;
        Ok(tera)
    }

    // TODO: add debug information
    /// render a template to a (self.spec.name).job file
    /// this functions is also building all the files that should be uploaded and returning them
    pub fn build(&self) -> Result<Vec<PathBuf>, Error> {
        let mut files_to_send = self.build_files()?;
        let tera_instance = Self::load_templates()?;

        let mut context = Context::new();
        context.try_insert("spec", &self.spec)?;
        context.try_insert("exec", &self.exec)?;
        context.try_insert("files", &files_to_send)?;
        context.try_insert("context", &self.context)?;

        let rendered = tera_instance.render(&self.template, &context)?;

        let job_name = format!("{}.job", &self.spec.name);

        // create the rendered template
        std::fs::write(job_name, rendered)?;
        let job_file = format!("{}.job", &self.spec.name);

        files_to_send.push(job_file.clone().into());

        Ok(files_to_send)
    }
}
