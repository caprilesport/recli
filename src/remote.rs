use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror;

use regex::Regex;
use std::collections::HashMap;

use ssh2::Session;
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
    IO(#[from] std::io::Error),
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
        let target = format!("{}:{}", self.hostname(), self.port());

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

    pub fn status(&self) -> Result<HashMap<String, String>, JobError> {
        let command = match self.queue_manager {
            QueueManager::PBS => format!("qstat -u {} -x", self.user()),
            QueueManager::Pueue => "pueue status".to_string(),
            _ => return Ok(HashMap::new()),
        };

        let tcp = TcpStream::connect(format!("{}:{}", self.hostname(), self.port))?;
        let mut sess = Session::new()?;
        sess.set_tcp_stream(tcp);
        sess.handshake()?;
        sess.userauth_agent(self.user())?;

        let mut channel = sess.channel_session()?;
        channel.exec(&command)?;
        let mut output = String::new();
        channel.read_to_string(&mut output)?;
        channel.wait_close()?;

        let mut statuses = HashMap::new();

        match self.queue_manager {
            QueueManager::PBS => {
                for line in output.lines().skip(5) {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 10 {
                        let job_id = parts[0].to_string();
                        let status = parts[9].to_string();
                        statuses.insert(job_id, status);
                    }
                }
            }
            QueueManager::Pueue => {
                for line in output.lines().skip(3) {
                    // Skip header and separator lines
                    if line.starts_with("───") || line.starts_with("═") || line.is_empty() {
                        continue;
                    }
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 2 {
                        let job_id = parts[0].to_string();
                        let status = parts[1].to_string();
                        statuses.insert(job_id, status);
                    }
                }
            }
            _ => (),
        }

        Ok(statuses)
    }
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

// Helper module for UUID serialization
mod uuid_as_string {
    use serde::{self, Deserialize, Deserializer, Serializer};
    use uuid::Uuid;

    pub fn serialize<S>(uuid: &Uuid, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&uuid.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Uuid, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Uuid::parse_str(&s).map_err(serde::de::Error::custom)
    }
}

#[derive(std::fmt::Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct Job {
    #[serde(with = "uuid_as_string")]
    id: uuid::Uuid,
    remote: String,
    remote_id: String,
    basename: String,
    working_dir: PathBuf,
    project: String,
    status: JobStatus,
    submit_time: String,
    finish_time: Option<String>,
    synced: bool,
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

    pub fn project(&self) -> &str {
        &self.project
    }

    pub fn remote(&self) -> &str {
        &self.remote
    }

    pub fn synced(&self) -> bool {
        self.synced
    }

    pub fn set_synced_status(&mut self, sync: bool) {
        self.synced = sync;
    }

    pub fn status(&self) -> &JobStatus {
        &self.status
    }

    pub fn update_status(&mut self, statuses: &HashMap<String, String>) {
        if let Some(new_status) = statuses.get(self.remote_id()) {
            let new_status = match new_status.as_str() {
                "Q" | "Queued" => JobStatus::Queued,
                "R" | "Running" => JobStatus::Running,
                "F" | "Success" | "Killed" => JobStatus::Finished,
                "E" | "Failed" => JobStatus::Error,
                _ => return,
            };
            if self.status != new_status {
                self.status = new_status;
            }
        }
    }

    fn find_project(cwd: PathBuf) -> Result<String, JobError> {
        let project_file: PathBuf = [cwd.clone(), PathBuf::from(".recli")].iter().collect();

        if std::path::Path::exists(&project_file) {
            return Ok(cwd.file_name().unwrap().to_str().unwrap().to_owned());
        } else {
            let parent_folder = cwd.parent();
            match parent_folder {
                Some(parent) => Self::find_project(parent.to_path_buf()),
                None => Err(JobError::NotInAProject),
            }
        }
    }

    pub fn save_jobs(jobs: &[Job]) -> Result<(), std::io::Error> {
        let config_dir = dirs::home_dir()
            .expect("Could not find home directory")
            .join(".config")
            .join("recli");
        std::fs::create_dir_all(&config_dir)?;
        let jobs_file = config_dir.join("jobs.json");
        let file = std::fs::File::create(jobs_file)?;
        serde_json::to_writer_pretty(file, jobs)?;
        Ok(())
    }

    pub fn load_jobs() -> Result<Vec<Job>, std::io::Error> {
        let config_dir = dirs::home_dir()
            .expect("Could not find home directory")
            .join(".config")
            .join("recli");
        let jobs_file = config_dir.join("jobs.json");
        if !jobs_file.exists() {
            return Ok(Vec::new());
        }

        let file = std::fs::File::open(&jobs_file)?;
        if file.metadata()?.len() == 0 {
            return Ok(Vec::new());
        }

        let file = std::fs::File::open(jobs_file)?;
        let reader = std::io::BufReader::new(file);
        let jobs = serde_json::from_reader(reader)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        Ok(jobs)
    }

    pub fn sync(&self, remote_config: &Remote) -> Result<(), JobError> {
        println!("Syncing job {} from remote {}", self.id, self.remote);

        let target = format!("{}:{}", remote_config.hostname(), remote_config.port);
        let tcp = TcpStream::connect(&target)?;
        let mut sess = Session::new()?;
        sess.set_tcp_stream(tcp);
        sess.handshake()?;
        sess.userauth_agent(remote_config.user())?;

        let sftp = sess.sftp()?;
        let remote_job_dir = remote_config.work_dir().join(self.id.to_string());

        for entry in sftp.readdir(&remote_job_dir)? {
            let (remote_path, stat) = entry;
            if stat.is_file() {
                if let Some(file_name) = remote_path.file_name() {
                    if file_name.to_string_lossy().starts_with(&self.basename) {
                        let local_path = self.working_dir.join(file_name);
                        let mut remote_file = sftp.open(&remote_path)?;
                        let mut local_file = std::fs::File::create(&local_path)?;
                        std::io::copy(&mut remote_file, &mut local_file)?;
                        println!("Downloaded: {:?} to {:?}", remote_path, local_path);
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum JobStatus {
    Queued,
    Running,
    Finished,
    Error,
}
