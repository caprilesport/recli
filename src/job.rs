use std::path::PathBuf;
use thiserror;

#[derive(thiserror::Error, std::fmt::Debug)]
pub enum JobError {
    #[error("Not in a recli project folder")]
    NotInAProject,
    #[error("SSH error:\n{0}")]
    Ssh(#[from] ssh2::Error),
    #[error("IO error:\n{0}")]
    Io(#[from] std::io::Error),
    #[error(
        "Command '{command}' failed with exit code
      {exit_code}\n---\nSTDOUT:\n{stdout}\n---\nSTDERR:\n{stderr}"
    )]
    CommandFailed {
        command: String,
        exit_code: i32,
        stdout: String,
        stderr: String,
    },
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
    name: String,
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
        let name = Job::get_name(&project, &basename).unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Ok(Self {
            id,
            name,
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

    // pub fn submit_time(&self) -> &str {
    //     &self.submit_time
    // }

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

    pub fn set_status(&mut self, status: JobStatus) {
        self.status = status;
    }

    pub fn basename(&self) -> &str {
        &self.basename
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    fn get_name(project: &str, _basename: &str) -> std::io::Result<String> {
        let cwd = std::env::current_dir()?;

        let parts: Vec<String> = cwd
            .iter()
            .skip_while(|part| *part != std::ffi::OsStr::new(&project))
            .skip(1)
            .filter_map(|s| s.to_str().map(String::from))
            .collect();

        // parts.push(basename.to_owned());
        Ok(parts.join("-"))
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
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum JobStatus {
    Queued,
    Running,
    Finished,
    Error,
    Undefined,
}

impl JobStatus {
    pub fn as_str(&self) -> &'static str {
        match &self {
            Self::Queued => "Q",
            Self::Finished => "F",
            Self::Error => "E",
            Self::Running => "R",
            Self::Undefined => "U",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;
    use uuid::Uuid;

    #[test]
    fn test_job_new_and_find_project() {
        let dir = tempdir().unwrap();
        let project_dir = dir.path().join("my_project");
        fs::create_dir(&project_dir).unwrap();
        fs::File::create(project_dir.join(".recli")).unwrap();

        let current_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&project_dir).unwrap();

        let job_id = Uuid::new_v4();
        let job = Job::new(
            job_id,
            "test_remote".to_string(),
            "12345".to_string(),
            "test_job".to_string(),
        )
        .unwrap();

        assert_eq!(job.id(), &job_id);
        assert_eq!(job.remote(), "test_remote");
        assert_eq!(job.remote_id(), "12345");
        assert_eq!(job.basename(), "test_job");
        assert_eq!(job.project(), "my_project");
        assert_eq!(job.status(), &JobStatus::Queued);

        std::env::set_current_dir(current_dir).unwrap();
    }

    #[test]
    fn test_find_project_not_found() {
        let dir = tempdir().unwrap();
        let result = Job::find_project(dir.path().to_path_buf());
        assert!(matches!(result, Err(JobError::NotInAProject)));
    }

    #[test]
    fn test_job_serialization_deserialization() {
        let job_id = Uuid::new_v4();
        let job = Job {
            id: job_id,
            name: "test_name".to_string(),
            remote: "test_remote".to_string(),
            remote_id: "12345".to_string(),
            basename: "test_job".to_string(),
            working_dir: PathBuf::from("/tmp"),
            project: "my_project".to_string(),
            status: JobStatus::Running,
            submit_time: "1234567890".to_string(),
            finish_time: None,
            synced: false,
        };

        let serialized = serde_json::to_string(&job).unwrap();
        let deserialized: Job = serde_json::from_str(&serialized).unwrap();

        assert_eq!(job.id(), deserialized.id());
        assert_eq!(job.remote(), deserialized.remote());
        assert_eq!(job.status(), deserialized.status());
    }
}
