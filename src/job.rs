use std::{collections::HashMap, path::PathBuf};
use tabled::builder::Builder;
use thiserror;
use uuid::Uuid;

use std::ops::{Deref, DerefMut};

use tracing::info;

#[derive(thiserror::Error, std::fmt::Debug)]
pub enum Error {
    #[error("Not in a recli project folder")]
    NotInAProject,
    #[error("Connection error:\n{0}")]
    Ssh(#[from] crate::connection::Error),
    #[error("IO error:\n{0}")]
    Io(#[from] std::io::Error),
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
    id: Uuid,
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
        id: Uuid,
        remote: String,
        remote_id: String,
        basename: String,
    ) -> Result<Self, Error> {
        let cwd = std::env::current_dir().unwrap();
        let project = Self::find_project(&cwd)?;
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

    pub fn id(&self) -> &Uuid {
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Copy)]
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Jobs {
    jobs: Vec<Job>,
}

impl Deref for Jobs {
    type Target = Vec<Job>;

    fn deref(&self) -> &Self::Target {
        &self.jobs
    }
}

impl DerefMut for Jobs {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.jobs
    }
}

impl Jobs {
    pub fn save_jobs(&self) -> Result<(), std::io::Error> {
        tracing::debug!("Saving jobs to CONFIG_DIR/jobs.json");
        let config_dir = crate::config::Config::get_dir()?;
        let jobs_file = config_dir.join("jobs.json");
        let file = std::fs::File::create(jobs_file)?;
        serde_json::to_writer_pretty(file, &self.jobs)?;
        Ok(())
    }

    pub fn load_jobs() -> Result<Self, std::io::Error> {
        tracing::debug!("Loading jobs from CONFIG_DIR/jobs.json");
        let config_dir = crate::config::Config::get_dir()?;
        let jobs_file = config_dir.join("jobs.json");
        if !jobs_file.exists() {
            return Ok(Jobs { jobs: Vec::new() });
        }

        let file = std::fs::File::open(&jobs_file)?;
        if file.metadata()?.len() == 0 {
            return Ok(Jobs { jobs: Vec::new() });
        }

        let file = std::fs::File::open(jobs_file)?;
        let reader = std::io::BufReader::new(file);
        let jobs = serde_json::from_reader(reader)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        Ok(jobs)
    }

    pub fn update(&mut self, statuses: HashMap<String, JobStatus>) {
        let mut changed_jobs = Vec::new();

        for job in self.jobs.iter_mut() {
            let old_status = job.status().clone();
            let status = statuses.get(job.remote_id());

            match status {
                Some(st) => job.status = *st,
                None => (),
            }
            if job.status() != &old_status {
                changed_jobs.push(job.clone());
            }
        }

        if changed_jobs.is_empty() {
            info!("No job status changes.");
        } else {
            info!("Jobs with status changes:");
            for job in changed_jobs {
                info!("  - Job {}: changed to {:?}", job.id(), job.status());
            }
        }
    }

    pub fn query(&self) -> JobQuery {
        JobQuery::new(&self.jobs)
    }

    pub fn filter(&mut self, query: JobQuery) {
        let ids: std::collections::HashSet<Uuid> = query.iter().map(|job| *job.id()).collect();
        self.jobs.retain(|job| ids.contains(job.id()));
    }

    pub fn create_status_table(&self) -> String {
        let mut builder = Builder::default();
        builder.push_record(["Name", "Project", "St", "Synced", "Remote", "Remote ID"]);

        self.jobs.iter().for_each(|j| {
            builder.push_record(vec![
                j.name(),
                j.project(),
                j.status().as_str(),
                &j.synced().to_string(),
                j.remote(),
                j.remote_id(),
            ])
        });

        let mut table = builder.build();
        table.with(tabled::settings::Style::rounded());
        table.to_string()
    }

    pub fn add(&mut self, job: Job) {
        self.jobs.push(job);
    }
}

pub enum Match<'a> {
    Exact(&'a str),
    Contains(&'a str),
}

pub struct JobQuery<'a> {
    jobs: &'a [Job],
    id: Option<&'a Uuid>,
    synced: Option<bool>,
    status: Option<&'a JobStatus>,
    remote_id: Option<&'a str>,
    remote: Option<Match<'a>>,
    name: Option<Match<'a>>,
    basename: Option<Match<'a>>,
    project: Option<Match<'a>>,
    // working_dir: Option<&'a PathBuf>,
}

impl<'a> JobQuery<'a> {
    fn new(jobs: &'a [Job]) -> Self {
        Self {
            jobs,
            id: None,
            synced: None,
            status: None,
            remote: None,
            remote_id: None,
            name: None,
            basename: None,
            project: None,
        }
    }

    pub fn with_id(mut self, id: &'a Uuid) -> Self {
        self.id = Some(id);
        self
    }

    pub fn synced(mut self, synced: bool) -> Self {
        self.synced = Some(synced);
        self
    }

    pub fn with_status(mut self, status: &'a JobStatus) -> Self {
        self.status = Some(status);
        self
    }

    pub fn with_remote_id(mut self, remote_id: &'a str) -> Self {
        self.remote_id = Some(remote_id);
        self
    }

    pub fn with_remote(mut self, remote_matcher: Match<'a>) -> Self {
        self.remote = Some(remote_matcher);
        self
    }

    pub fn with_name(mut self, name_matcher: Match<'a>) -> Self {
        self.name = Some(name_matcher);
        self
    }

    pub fn with_basename(mut self, basename_matcher: Match<'a>) -> Self {
        self.basename = Some(basename_matcher);
        self
    }

    pub fn with_project(mut self, project_matcher: Match<'a>) -> Self {
        self.project = Some(project_matcher);
        self
    }

    pub fn iter(self) -> impl Iterator<Item = &'a Job> {
        let Self {
            jobs,
            id,
            synced,
            status,
            remote_id,
            remote,
            name,
            basename,
            project,
        } = self;

        let check = |value: &str, matcher: &Option<Match<'a>>| {
            matcher.as_ref().map_or(true, |m| match m {
                Match::Exact(val) => value == *val,
                Match::Contains(val) => value.contains(val),
            })
        };

        jobs.iter().filter(move |job| {
            let id_match = id.map_or(true, |id| id == job.id());
            let synced_match = synced.map_or(true, |synced| synced == job.synced());
            let status_match = status.map_or(true, |st| st == job.status());
            let remote_id_match = remote_id.map_or(true, |id| id == job.remote_id());
            let remote_match = check(job.remote(), &remote);
            let name_match = check(job.name(), &name);
            let basename_match = check(job.basename(), &basename);
            let project_match = check(job.project(), &project);

            id_match
                && synced_match
                && status_match
                && remote_id_match
                && remote_match
                && name_match
                && basename_match
                && project_match
        })
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
