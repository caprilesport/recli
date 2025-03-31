use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror;

#[derive(Debug, thiserror::Error)]
pub enum QueueError {
    #[error("Failed to submit Job")]
    SubmitError,
}

// TODO: Improve these error massages, they should catch:
// - A local directory that is not a `projects` subfolder
// - Errors in the config declaration for paths in remotes
#[derive(Debug, thiserror::Error)]
pub enum RemoteError {
    #[error("Current path is not a valid project subfolder")]
    InvalidRemotePathError,
    #[error("Failed to retrieve local path, caused by: {0}")]
    InvalidLocalPathError(#[from] std::io::Error),
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

impl Remote {
    fn get_remote_path(self: Self, local: Local) -> Result<PathBuf, RemoteError> {
        let cwd = std::env::current_dir()?;

        if !cwd.starts_with(&local.projects_folder) {
            return Err(RemoteError::InvalidRemotePathError);
        }

        let suffix = cwd
            .strip_prefix(&local.projects_folder)
            .map_err(|_| RemoteError::InvalidRemotePathError)?;

        Ok(self.work_directory.join(suffix))
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_remote_path_works() {
        assert_eq!(4, 4);
    }
}
// rsync -rtvuc $ignore_flag $last_flag "$source" "$destination"
