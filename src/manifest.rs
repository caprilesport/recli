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
    pub spec: Spec,
    pub exec: Exec,
    pub files: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub context: Option<toml::Value>,
    #[serde(default)]
    dir: PathBuf,
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
    pub fn read(file: &std::path::Path) -> Result<Self, Error> {
        debug!("Parsing file {:?}", file);
        // right now the manifest should be self contained.
        // next step is implementing some merging behaviour for the default defined in the config file, cli flags and the toml manifest.
        let manifest_str = std::fs::read_to_string(file)?;
        let mut manifest: JobManifest = toml::from_str(&manifest_str)?;
        let manifest_path = file
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .to_path_buf();
        manifest.dir = manifest_path;
        Ok(manifest)
    }

    /// Parses all the globs in the files input and returns a vector containing all matched files.
    fn build_files(&self) -> Result<Vec<PathBuf>, Error> {
        debug!("Building files list to send");
        let mut files_to_send = vec![];
        for pattern in &self.files {
            let full_pattern = self.dir.join(pattern);
            let full_pattern_str = full_pattern.to_string_lossy();
            for path in glob::glob(&full_pattern_str)? {
                match path {
                    Ok(p) => files_to_send.push(p),
                    Err(e) => {
                        error!("{}", e);
                        continue;
                    }
                }
            }
        }
        debug!("List of files that will be sent: {:?}", &files_to_send);
        Ok(files_to_send)
    }

    fn load_templates() -> Result<Tera, Error> {
        let config_dir = crate::config::Config::get_dir()?;
        let mut templates_glob = config_dir.to_string_lossy().into_owned();
        templates_glob.push_str("/templates/*");
        debug!("loading templates: {:?}", templates_glob);
        let tera = Tera::new(&templates_glob)?;
        let template_list: Vec<&str> = tera.get_template_names().collect();
        debug!("Following templates were parsed: {:?}", &template_list);
        Ok(tera)
    }

    /// render a template to a (self.spec.name).job file
    /// this functions is also building all the files that should be uploaded and returning them
    pub fn build(&self) -> Result<Vec<PathBuf>, Error> {
        let mut files_to_send = self.build_files()?;
        let tera_instance = Self::load_templates()?;

        let mut context = Context::new();
        context.try_insert("spec", &self.spec)?;
        context.try_insert("exec", &self.exec)?;
        context.try_insert("files", &files_to_send)?;
        if let Some(reclicontext) = &self.context {
            context.try_insert("context", reclicontext)?;
        }

        debug!("Rendering template: {:?}", &self.template);
        let rendered = tera_instance.render(&self.template, &context)?;

        let job_filename = self.dir.join(format!("{}.job", &self.spec.name));

        // create the rendered template
        std::fs::write(&job_filename, rendered)?;
        debug!("Writing rendered template to {:?}", &job_filename);

        files_to_send.push(job_filename);

        Ok(files_to_send)
    }
}
