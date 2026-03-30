use crate::{
    config::Config,
    connection::{ConnectionFactory, RemoteConnection, SshFactory},
};
use std::path::{Path, PathBuf};

/// Running context of the application
///
/// Holds the config for all the remotes registered, as well as the database for all the submitted jobs
pub struct Context {
    config: Config,
    db_path: PathBuf,
    json_output: bool,
    connection_factory: Box<dyn ConnectionFactory>,
}

impl Context {
    pub fn new(json_output: bool) -> color_eyre::Result<Self> {
        let config = Config::read()?;
        Ok(Self {
            config,
            db_path: Config::get_dir()?.join("jobs.db"),
            json_output,
            connection_factory: Box::new(SshFactory),
        })
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn connect(
        &self,
        remote: &crate::remote::Remote,
    ) -> Result<Box<dyn RemoteConnection>, crate::connection::Error> {
        self.connection_factory.connect(remote)
    }

    pub fn clear_ignore(&mut self) {
        self.config.ignore.clear();
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn json(&self) -> bool {
        self.json
    pub fn json_output(&self) -> bool {
        self.json_output
    }
}
