use crate::remote::Remote;
use serde::{Deserialize, Serialize};
use tracing::trace;

/// Controls which local files are uploaded alongside the job script on submit.
///
/// Configured globally under `[settings]` in `config.toml`. Can be overridden
/// per invocation with `recli submit --strategy <value>`.
///
/// For `Basename` and `Directory`, files are scanned from the **script's parent
/// directory** (not the working directory where recli is invoked). `--files`
/// paths are always resolved relative to the current working directory.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum FileStrategy {
    /// Upload only the job script itself. Use when input data is pre-staged
    /// on the remote or shared across many jobs.
    #[default]
    Script,
    /// Upload the script plus all files in its directory that share the same
    /// stem (e.g. `myjob.pbs`, `myjob.inp`, `myjob.xyz`).
    Basename,
    /// Upload all files in the script's directory, filtered by the `ignore`
    /// file (`~/.config/recli/ignore`).
    Directory,
}

/// Columns available in the `recli status` table.
/// Configured under `[display].columns` in `config.toml`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Column {
    Id,
    WorkDir,
    Name,
    Status,
    Remote,
    Queue,
    SubmitTime,
    SyncTime,
    Tags,
}

impl Column {
    pub fn header(&self) -> &'static str {
        match self {
            Self::Id => "ID",
            Self::WorkDir => "Work dir",
            Self::Name => "Name",
            Self::Status => "Status",
            Self::Remote => "Remote",
            Self::Queue => "Queue",
            Self::SubmitTime => "Submit time",
            Self::SyncTime => "Sync time",
            Self::Tags => "Tags",
        }
    }
}

fn default_path_components() -> usize {
    3
}
fn default_datetime_format() -> String {
    "%Y-%m-%d %H:%M".to_string()
}
fn default_status_window_hours() -> u32 {
    48
}
fn default_max_tags() -> usize {
    0
}

fn default_columns() -> Vec<Column> {
    vec![
        Column::Id,
        Column::WorkDir,
        Column::Name,
        Column::Status,
        Column::Remote,
        Column::Queue,
        Column::SubmitTime,
        Column::SyncTime,
        Column::Tags,
    ]
}

/// Display settings for the status table, configured under `[display]` in `config.toml`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Display {
    /// Number of trailing path components to show in Work dir (0 = full path).
    #[serde(default = "default_path_components")]
    pub path_components: usize,
    /// strftime-style format string for datetime columns.
    #[serde(default = "default_datetime_format")]
    pub datetime_format: String,
    /// Which columns to show and in what order.
    #[serde(default = "default_columns")]
    pub columns: Vec<Column>,
    /// Hide synced jobs older than this many hours (0 = show all).
    #[serde(default = "default_status_window_hours")]
    pub status_window_hours: u32,
    /// Maximum tags to display per job (0 = show all).
    #[serde(default = "default_max_tags")]
    pub max_tags: usize,
}

impl Default for Display {
    fn default() -> Self {
        Self {
            path_components: default_path_components(),
            datetime_format: default_datetime_format(),
            columns: default_columns(),
            status_window_hours: default_status_window_hours(),
            max_tags: default_max_tags(),
        }
    }
}

fn default_prune_days() -> u32 {
    90
}

/// Global recli settings, configured under `[settings]` in `config.toml`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub file_strategy: FileStrategy,
    /// Minimum age in days since sync before a job qualifies for `recli prune`.
    #[serde(default = "default_prune_days")]
    pub prune_after_days: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            file_strategy: FileStrategy::default(),
            prune_after_days: default_prune_days(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub remotes: Vec<Remote>,
    #[serde(skip)]
    pub ignore: Vec<glob::Pattern>,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub display: Display,
}

#[derive(thiserror::Error, std::fmt::Debug)]
pub enum ConfigError {
    #[error("IO error, {0}")]
    IO(#[from] std::io::Error),
    #[error("Cannot read '{path}': {source}")]
    FileRead {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Error in config.toml file: {0}")]
    MalformedTomlConfig(#[from] toml::de::Error),
    #[error("Remote {0} not found in config file")]
    RemoteNotFound(String),
    #[error("Invalid glob pattern {0}")]
    Glob(#[from] glob::PatternError),
}

impl Config {
    pub fn read() -> Result<Self, ConfigError> {
        let config_dir = Config::get_dir()?;
        let config_file = config_dir.join("config.toml");
        trace!("Attempting to read {:?}", config_file);
        let toml_string =
            std::fs::read_to_string(&config_file).map_err(|e| ConfigError::FileRead {
                path: config_file,
                source: e,
            })?;
        trace!("Parsing config file");
        let mut config: Config = toml::from_str(&toml_string)?;

        let ignore_file = config_dir.join("ignore");
        if ignore_file.exists() {
            let ignore_patterns = std::fs::read_to_string(&ignore_file)
                .map_err(|e| ConfigError::FileRead {
                    path: ignore_file.clone(),
                    source: e,
                })?
                .lines()
                .map(glob::Pattern::new)
                .collect::<Result<Vec<glob::Pattern>, glob::PatternError>>()?;
            config.ignore = ignore_patterns;
        } else {
            config.ignore = Vec::new();
        }

        Ok(config)
    }

    pub fn get_remote(&self, remote: &str) -> Result<&Remote, ConfigError> {
        self.remotes
            .iter()
            .find(|r| r.name() == remote)
            .ok_or(ConfigError::RemoteNotFound(remote.to_string()))
    }

    pub fn get_dir() -> std::io::Result<std::path::PathBuf> {
        let mut config_dir = dirs::config_dir().ok_or(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "OS Config directory not found",
        ))?;
        config_dir.push("recli");
        Ok(config_dir)
    }
}
