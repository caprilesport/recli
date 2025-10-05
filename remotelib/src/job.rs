use chrono::{DateTime, Utc};
use std::path::PathBuf;
use thiserror;
use uuid::Uuid;

#[derive(thiserror::Error, std::fmt::Debug)]
pub enum Error {
    #[error("Job with id {0} not found")]
    JobNotFound(Uuid),
    #[error("Not in a recli project folder")]
    NotInAProject,
    #[error("Connection error:\n{0}")]
    Ssh(#[from] crate::connection::Error),
    #[error("IO error:\n{0}")]
    Io(#[from] std::io::Error),
    #[error("Failed to serialize job database {0}")]
    JsonError(#[from] serde_json::Error),
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

/// Main abstraction for a `Job`.
///
/// The main referral unit in any job is it's UUID, to interact with jobs throught the application this is the preferred way to refer to a job.
///
/// Beyond information that is related to each job, such as in which remote it's being ran, what is it's status, it's remote ID, etc., we also hold to which project this belongs.
/// The standard that was taken for a project to be considered by recli is it should have an empty .recli file in it's root folder. This way one can easily query job's by a project.
// TODO: allow jobs to be submitted without a parent project, and use the complete path to refer to it. In the project field, we should just leave a "no-project" field.
// TODO: add an optional description :)
#[derive(std::fmt::Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct Job {
    #[serde(with = "uuid_as_string")]
    id: Uuid,
    name: String,
    remote: String,
    remote_id: String,
    basename: String,
    working_dir: PathBuf,
    project: String,
    status: JobStatus,
    submit_time: DateTime<Utc>,
    sync_time: Option<DateTime<Utc>>,
    synced: bool,
}

impl Job {
    pub fn new(
        id: Uuid,
        remote: String,
        remote_id: String,
        basename: String,
    ) -> Result<Self, Error> {
        let cwd = std::env::current_dir().unwrap();
        let project = Self::find_project(&cwd)?;
        let name = Job::get_name(&project, &basename).unwrap();

        Ok(Self {
            id,
            name,
            remote,
            remote_id,
            basename,
            working_dir: cwd,
            project,
            status: JobStatus::Queued,
            submit_time: Utc::now(),
            sync_time: None,
            synced: false,
        })
    }

    pub fn id(&self) -> &Uuid {
        &self.id
    }

    pub fn remote_id(&self) -> &str {
        &self.remote_id
    }

    pub fn submit_time(&self) -> &DateTime<Utc> {
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

    pub fn set_status(&mut self, status: JobStatus) {
        self.status = status;
    }

    pub fn basename(&self) -> &str {
        &self.basename
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn set_sync_time(&mut self, time: DateTime<Utc>) {
        self.sync_time = Some(time);
    }

    pub fn sync_time(&self) -> &Option<DateTime<Utc>> {
        &self.sync_time
    }

    /// Get a name description for a job.
    ///
    /// The current standard is to join the name of the file with the relative path to it's project absoluted path
    ///
    ///
    /// Examples:
    /// If one has a project in a `/home/user/projectA` folder, and submits a job called `init.inp` in `/home/user/projectA/job1`:
    /// let name = Jobs::get_name(&projectA).unwrap();
    /// assert_eq!(name, String::from("projectA-job1"));
    ///
    /// Errors:
    /// If the current directory doesn't have the correct permissions, this will raise a JobError::IO error.
    // TODO: this function could be better defined possibly?
    fn get_name(project: &str, basename: &str) -> std::io::Result<String> {
        let cwd = std::env::current_dir()?;

        let mut parts: Vec<String> = cwd
            .iter()
            .skip_while(|part| *part != std::ffi::OsStr::new(&project))
            .skip(1)
            .filter_map(|s| s.to_str().map(String::from))
            .collect();

        parts.push(basename.to_owned());
        Ok(parts.join("-"))
    }

    /// Continue to search parent folders until a .recli file is found.
    /// This will exit upon reaching the system root `/`.
    fn find_project(start_path: &std::path::Path) -> Result<String, Error> {
        let mut current_path = start_path;

        loop {
            if current_path.join(".recli").is_file() {
                return current_path
                    .file_name() // Returns Option<&OsStr>
                    .and_then(|name| name.to_str()) // Converts to
                    .map(|name_str| name_str.to_owned()) // Converts
                    .ok_or(Error::NotInAProject); // Converts
            }

            match current_path.parent() {
                Some(parent) => current_path = parent,
                None => return Err(Error::NotInAProject),
            }
        }
    }
}

/// Abstraction on status for all the supported QueueManagers.
///
/// Relevant documentation can be found here:
/// [Slurm](https://slurm.schedmd.com/job_state_codes.html)
/// [PBS](https://www.unisq.edu.au/-/media/usq/current-students/academic/research/conducting-research/eresearch-services/hpc/pbs-documentation_may17.ashx?la=en&hash=a8ba909a56c14aea7de6a4876ea9b30e)
/// [Pueue](https://github.com/Nukesor/pueue/blob/21c6b928d0728439cf708b4fb58acca5effc1a25/pueue_lib/src/task.rs#L11)
///
/// The `Undefined` variant is reserved for all status that are encountered and are not defined here. This may happen mainly with different PBS versions and some status for SLURM that are currently not implemented.
//TODO: complete this with all possible variants encoutered in the Queue managers we support.
// Slurm, for instance, has several status descriptions which could be usefull
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Copy, clap::ValueEnum)]
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
        let result = Job::find_project(&dir.path());
        assert!(matches!(result, Err(Error::NotInAProject)));
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
            submit_time: Utc::now(),
            sync_time: None,
            synced: false,
        };

        let serialized = serde_json::to_string(&job).unwrap();
        let deserialized: Job = serde_json::from_str(&serialized).unwrap();

        assert_eq!(job.id(), deserialized.id());
        assert_eq!(job.remote(), deserialized.remote());
        assert_eq!(job.status(), deserialized.status());
    }
}
