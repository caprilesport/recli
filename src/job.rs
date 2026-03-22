use chrono::{DateTime, Utc};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(thiserror::Error, std::fmt::Debug)]
pub enum Error {
    #[error("No job found matching prefix '{0}'")]
    JobNotFound(String),
    #[error("Ambiguous prefix '{prefix}': matches {matches}")]
    AmbiguousPrefix { prefix: String, matches: String },
    #[error("Connection error:\n{0}")]
    Ssh(#[from] crate::connection::Error),
    #[error("Database error:\n{0}")]
    Db(#[from] rusqlite::Error),
    #[error("IO error:\n{0}")]
    Io(#[from] std::io::Error),
}

/// Main abstraction for a `Job`.
///
/// The main referral unit in any job is it's UUID, to interact with jobs throught the application this is the preferred way to refer to a job.
#[derive(std::fmt::Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct Job {
    uuid: Uuid,
    remote: String,
    remote_id: String,
    filename: String,
    work_dir: PathBuf,
    remote_dir: PathBuf,
    status: JobStatus,
    submit_time: DateTime<Utc>,
    sync_time: Option<DateTime<Utc>>,
}

impl Job {
    pub fn new(
        uuid: Uuid,
        remote: String,
        remote_id: String,
        filename: String,
        remote_dir: PathBuf,
        work_dir: PathBuf,
    ) -> Self {
        Self {
            uuid,
            remote,
            remote_id,
            filename,
            work_dir,
            remote_dir,
            status: JobStatus::Queued,
            submit_time: Utc::now(),
            sync_time: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_parts(
        uuid: Uuid,
        remote: String,
        remote_id: String,
        filename: String,
        work_dir: PathBuf,
        remote_dir: PathBuf,
        status: JobStatus,
        submit_time: DateTime<Utc>,
        sync_time: Option<DateTime<Utc>>,
    ) -> Self {
        Self {
            uuid,
            remote,
            remote_id,
            filename,
            work_dir,
            remote_dir,
            status,
            submit_time,
            sync_time,
        }
    }

    pub fn uuid(&self) -> &Uuid {
        &self.uuid
    }

    pub fn short_id(&self) -> String {
        self.uuid.to_string()[..7].to_string()
    }

    pub fn remote_id(&self) -> &str {
        &self.remote_id
    }

    pub fn submit_time(&self) -> &DateTime<Utc> {
        &self.submit_time
    }

    pub fn work_dir(&self) -> &PathBuf {
        &self.work_dir
    }

    pub fn remote_dir(&self) -> &PathBuf {
        &self.remote_dir
    }

    pub fn remote(&self) -> &str {
        &self.remote
    }

    pub fn synced(&self) -> bool {
        self.sync_time.is_some()
    }

    pub fn status(&self) -> &JobStatus {
        &self.status
    }

    pub fn filename(&self) -> &str {
        &self.filename
    }

    pub fn sync_time(&self) -> Option<&DateTime<Utc>> {
        self.sync_time.as_ref()
    }

    pub fn set_status(&mut self, status: JobStatus) {
        self.status = status;
    }
}

/// Abstraction on status for all the supported `QueueManagers`.
///
/// Relevant documentation can be found here:
/// [Slurm](https://slurm.schedmd.com/job_state_codes.html)
/// [PBS](https://www.unisq.edu.au/-/media/usq/current-students/academic/research/conducting-research/eresearch-services/hpc/pbs-documentation_may17.ashx?la=en&hash=a8ba909a56c14aea7de6a4876ea9b30e)
/// [Pueue](https://github.com/Nukesor/pueue/blob/21c6b928d0728439cf708b4fb58acca5effc1a25/pueue_lib/src/task.rs#L11)
///
/// The `Undefined` variant is reserved for all status that are encountered and are not defined here. This may happen mainly with different PBS versions and some status for SLURM that are currently not implemented.
// TODO: complete this with all possible variants encoutered in the Queue managers we support.
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
    pub fn as_str(self) -> &'static str {
        match self {
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
    use uuid::Uuid;

    fn make_job(status: JobStatus, sync_time: Option<DateTime<Utc>>) -> Job {
        Job {
            uuid: Uuid::new_v4(),
            filename: "myjob".to_string(),
            remote: "babel".to_string(),
            remote_id: "12345.server".to_string(),
            remote_dir: PathBuf::from("/scratch/work/uuid"),
            work_dir: PathBuf::from("/home/user/jobs"),
            status,
            submit_time: Utc::now(),
            sync_time,
        }
    }

    #[test]
    fn test_job_serialization_deserialization() {
        let job = make_job(JobStatus::Running, None);

        let serialized = serde_json::to_string(&job).unwrap();
        let deserialized: Job = serde_json::from_str(&serialized).unwrap();

        assert_eq!(job.short_id(), deserialized.short_id());
        assert_eq!(job.remote(), deserialized.remote());
        assert_eq!(job.status(), deserialized.status());
    }

    #[test]
    fn test_short_id_is_7_chars() {
        let job = make_job(JobStatus::Queued, None);
        assert_eq!(job.short_id().len(), 7);
    }

    #[test]
    fn test_short_id_is_uuid_prefix() {
        let job = make_job(JobStatus::Queued, None);
        assert!(job.uuid().to_string().starts_with(&job.short_id()));
    }

    #[test]
    fn test_synced_without_sync_time() {
        let job = make_job(JobStatus::Finished, None);
        assert!(!job.synced());
        assert!(job.sync_time().is_none());
    }

    #[test]
    fn test_synced_with_sync_time() {
        let job = make_job(JobStatus::Finished, Some(Utc::now()));
        assert!(job.synced());
        assert!(job.sync_time().is_some());
    }

    #[test]
    fn test_from_parts_roundtrip() {
        let uuid = Uuid::new_v4();
        let submit_time = Utc::now();
        let sync_time = Some(Utc::now());

        let job = Job::from_parts(
            uuid,
            "babel".to_string(),
            "99.server".to_string(),
            "myjob".to_string(),
            PathBuf::from("/home/user/jobs"),
            PathBuf::from("/scratch/work/uuid"),
            JobStatus::Finished,
            submit_time,
            sync_time,
        );

        assert_eq!(job.uuid(), &uuid);
        assert_eq!(job.remote(), "babel");
        assert_eq!(job.remote_id(), "99.server");
        assert_eq!(job.filename(), "myjob");
        assert_eq!(job.status(), &JobStatus::Finished);
        assert!(job.synced());
    }
}
