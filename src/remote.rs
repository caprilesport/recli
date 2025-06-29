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

#[derive(thiserror::Error, std::fmt::Debug)]
pub enum PrepareError {
    #[error("IO, caused by {0}")]
    InputOutputError(#[from] std::io::Error),
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

impl PartialEq<&str> for Remote {
    fn eq(&self, other: &&str) -> bool {
        self.name == *other
    }
}

impl Remote {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn hostname(&self) -> &str {
        &self.hostname
    }

    pub fn port(&self) -> i16 {
        self.port
    }

    pub fn user(&self) -> &str {
        &self.user
    }

    pub fn work_dir(&self) -> PathBuf {
        self.work_directory.clone()
    }

    pub fn prepare(&self, input_file: &std::path::Path) -> Result<(), PrepareError> {
        let mut args = self.prepare_args.clone();
        args.push(input_file.to_string_lossy().into_owned());
        // TODO: when the log level is set, print the output of qprep in log
        let prep = duct::cmd("qprep", args).stdout_capture().run()?;
        Ok(())
    }
#[derive(thiserror::Error, std::fmt::Debug)]
pub enum JobError {
    #[error("Not in a recli project folder")]
    NotInAProject,
    #[error("SSH error")]
    Ssh(#[from] ssh2::Error),
    #[error("IO error")]
    Io(#[from] std::io::Error),
    #[error("Job submission failed with exit code {0}. Output:\n{1}")]
    SubmissionFailed(i32, String),
}

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
    pub fn id(&self) -> &uuid::Uuid {
        &self.id
    }

    pub fn remote_id(&self) -> &str {
        &self.remote_id
    }

    pub fn submit_time(&self) -> &str {
        &self.submit_time
    }

    pub fn working_dir(&self) -> &PathBuf {
        &self.working_dir
    }
    pub fn project(&self) -> &std::ffi::OsString {
        &self.project
    }

    pub fn remote(&self) -> &str {
        &self.remote
    }

    pub fn synced(&self) -> bool {
        self.synced
    }
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum JobStatus {
    Queued,
    Running,
    Finished,
    Error,
}
