use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror;
use ssh2::Session;
use std::collections::HashMap;
use std::io::prelude::*;
use std::net::TcpStream;
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

    pub fn submit(&self, job_id: uuid::Uuid, inp_file: &Path) -> Result<Job, JobError> {
        let target = format!("{}:{}", self.hostname(), self.port);

        let tcp = TcpStream::connect(&target)?;
        let mut sess = Session::new()?;
        sess.set_tcp_stream(tcp);
        sess.handshake()?;
        sess.userauth_agent(self.user())?;

        let remote_dir = self.work_dir().join(job_id.to_string());
        let sftp = sess.sftp()?;
        sftp.mkdir(&remote_dir, 0o755)?;

        let file_stem = inp_file.file_stem().unwrap().to_str().unwrap();
        let job_script_name = format!("{}.job", file_stem);

        let files_to_send = std::fs::read_dir(".")?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.is_file()
                    && path
                        .file_name()
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .starts_with(file_stem)
            });

        for file_path in files_to_send {
            let mut local_file = std::fs::File::open(&file_path)?;
            let remote_path = remote_dir.join(file_path.file_name().unwrap());
            let mut remote_file = sftp.create(remote_path.as_path())?;
            std::io::copy(&mut local_file, &mut remote_file)?;
        }

        let command = match self.queue_manager {
            QueueManager::Pueue => format!(
                "cd {} && . ./{}",
                remote_dir.to_str().unwrap(),
                job_script_name
            ),
            _ => format!(
                "cd {} && {} {}",
                remote_dir.to_str().unwrap(),
                self.queue_manager.submit_command(),
                job_script_name
            ),
        };

        let mut channel = sess.channel_session()?;
        channel.exec(&command)?;
        let mut output = String::new();
        channel.read_to_string(&mut output)?;
        channel.wait_close()?;
        let exit_code = channel.exit_status()?;
        if exit_code != 0 {
            return Err(JobError::SubmissionFailed(exit_code, output));
        }

        let remote_id = match self.queue_manager {
            QueueManager::Pueue => {
                let re = Regex::new(r"id (\d+)").unwrap();
                re.captures(&output)
                    .and_then(|caps| caps.get(1))
                    .map_or_else(|| "".to_string(), |m| m.as_str().to_string())
            }
            _ => output.trim().to_string(),
        };

        Job::new(
            job_id,
            self.name().to_string(),
            remote_id,
            file_stem.to_string(),
        )
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
}

impl Job {
    pub fn new(
        id: uuid::Uuid,
        remote: String,
        remote_id: String,
        basename: String,
    ) -> Result<Self, JobError> {
        let cwd = std::env::current_dir().unwrap();
        let project = Self::find_project(cwd.clone())?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Ok(Self {
            id,
            remote,
            remote_id,
            basename,
            working_dir: cwd,
            project,
            status: JobStatus::Queued,
            submit_time: now.to_string(),
            finish_time: None,
            synced: false,
        })
    }

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
    fn find_project(cwd: PathBuf) -> Result<std::ffi::OsString, JobError> {
        let project_file: PathBuf = [cwd.clone(), PathBuf::from(".recli")].iter().collect();

        if std::path::Path::exists(&project_file) {
            return Ok(cwd.file_name().unwrap().to_owned());
        } else {
            let parent_folder = cwd.parent();
            match parent_folder {
                Some(parent) => Self::find_project(parent.to_path_buf()),
                None => Err(JobError::NotInAProject),
            }
        }
    }

}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum JobStatus {
    Queued,
    Running,
    Finished,
    Error,
}
