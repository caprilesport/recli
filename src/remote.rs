use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror;

#[derive(Debug, Clone, Copy, thiserror::Error)]
pub enum QueueError {
    #[error("Failed to submit Job")]
    SubmitError,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Remote {
    alias: String,
    ip: std::net::Ipv4Addr,
    port: Option<u32>,
    user: String,
    // queue: Option<QueueManager>,
    work_dir: PathBuf,
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
    local_id: u32,
    remote_id: u32,
    location: Remote,
    status: Status,
    working_dir: std::path::Path,
}
