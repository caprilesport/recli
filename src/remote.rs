use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror;

#[derive(Debug, Clone, Copy, thiserror::Error)]
pub enum QueueError {
    #[error("Failed to submit Job")]
    SubmitError,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Local {
    pub projects_folder: PathBuf,
    // submit_command: ,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Remote {
    pub name: String,
    pub user: String,
    pub work_directory: PathBuf,
    // queue_manager: impl QueueManager,
    // queue: Option<QueueManager>,
    // submit_command: Command,
}

pub trait QueueManager {
    fn status(job: Job) -> Status;
    fn submit(job: Job);
}

pub enum Status {
    Queued,
    Running,
    Finished,
}

pub struct Job {
    id: u32,
    remote_id: u32,
    location: Remote,
    status: Status,
    working_dir: std::path::Path,
}
