use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
enum QueueManager {
    PBS,
    Slurm,
    Pueue,
}

impl QueueManager {
    fn submit_command(&self) -> &str {
        match &self {
            Self::PBS => "qsub",
            Self::Slurm => "sbatch",
            Self::Pueue => "job",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Remote {
    name: String,
    hostname: String,
    port: i16,
    user: String,
    work_directory: PathBuf,
    prepare_args: Vec<String>,
    queue_manager: QueueManager,
}

#[derive(std::fmt::Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct Job {
    id: uuid::Uuid,
    remote: String,
    remote_id: String,
    basename: String,
    working_dir: PathBuf,
    project: std::ffi::OsString,
    pub status: JobStatus,
    submit_time: String,
    finish_time: Option<String>,
    pub synced: bool,
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum JobStatus {
    Queued,
    Running,
    Finished,
    Error,
}
