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
    #[error("current path is not a valid project subfolder")]
    InvalidSubprojectPathError(#[from] std::path::StripPrefixError),
    #[error("failed to retrieve local path, caused by: {0}")]
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
    fn get_remote_path(&self, local: Local, cwd: PathBuf) -> Result<PathBuf, RemoteError> {
        let suffix = cwd.strip_prefix(&local.projects_folder)?;
        Ok(self.work_directory.join(suffix))
    }

    fn push(&self, sync: bool) -> duct::Expression {
        unimplemented!()
    }

    fn pull(&self, sync: bool) -> duct::Expression {
        unimplemented!()
    }

    fn diff(&self) -> duct::Expression {
        unimplemented!()
    }
}

// rsync -rtvuc $ignore_flag $last_flag "$source" "$destination"

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
        let remote = Remote {
            name: "remote_test".to_string(),
            user: "test_user".to_string(),
            work_directory: PathBuf::from("/scratch/test_user/"),
        };

        let local_project = Local {
            projects_folder: PathBuf::from("/home/test_user/projects"),
        };

        let cwd = PathBuf::from("/home/test_user/projects/project_a");

        match remote.get_remote_path(local_project, cwd) {
            Ok(path) => assert_eq!(PathBuf::from("/scratch/test_user/project_a"), path),
            Err(e) => panic!("Failed to get correct path {}", e),
        }
    }
}
