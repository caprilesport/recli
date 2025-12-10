use crate::job::{Error, Job, JobStatus};
use atomicwrites::{AllowOverwrite, AtomicFile};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use tracing::info;
use uuid::Uuid;

/// A collection of `Job`s that provides centralized management and operations.
///
/// This is the primary interface for interacting with jobs. The `Jobs` container provides methods for persistence, querying, synchronization, and batch operations on job collections.
#[derive(Debug, Clone)]
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
    /// Saves the job collection to a JSON file at the specified path.
    ///
    /// This method serializes the entire job collection to JSON format and writes it to the given file path. If the file already exists, it will be overwritten.
    ///
    /// # Errors
    /// - If file serialization fails will throw a serde_json::Error
    /// - File creation may fail in some platforms if the directory does not exist
    ///
    /// # Examples
    /// ```
    /// jobs.save_jobs(Path::new("jobs.json"))?;
    /// ```
    pub fn save_jobs(&self, json_file_path: &std::path::Path) -> Result<(), Error> {
        tracing::debug!("Saving jobs to {:?}", json_file_path);

        let atomic_file = AtomicFile::new(json_file_path, AllowOverwrite);
        atomic_file.write(|temp_file| serde_json::to_writer_pretty(temp_file, &self.jobs))?;
        Ok(())
    }

    /// Loads a job collection from a JSON file at the specified path.
    ///
    /// If the file doesn't exist or is empty, returns an empty job collection.
    ///
    /// # Errors
    /// - If file deserialization fails will throw a serde_json::Error
    ///
    /// The above is mainly due to a malformed JSON file, which may happen if the user manually edits the file for some reason.
    ///
    /// # Examples
    /// ```
    /// let jobs = Jobs::load_jobs(Path::new("jobs.json"))?;
    /// ```
    pub fn load_jobs(json_file_path: &std::path::Path) -> Result<Self, Error> {
        tracing::debug!("Loading jobs from {:?}", json_file_path);
        if !json_file_path.exists() {
            return Ok(Jobs { jobs: Vec::new() });
        }

        let file = std::fs::File::open(json_file_path)?;
        if file.metadata()?.len() == 0 {
            return Ok(Jobs { jobs: Vec::new() });
        }

        let file = std::fs::File::open(json_file_path)?;
        let reader = std::io::BufReader::new(file);
        let jobs: Vec<Job> = serde_json::from_reader(reader)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        Ok(Jobs { jobs })
    }

    /// Updates job statuses based on a provided status map and logs changes.
    ///
    /// Compares current job statuses with provided statuses and updates jobs where the remote status differs. Logs all status changes for monitoring purposes.
    ///
    /// # Examples
    /// ```
    /// let mut status_map = HashMap::new();
    /// status_map.insert("job123".to_string(), JobStatus::Finished);
    /// jobs.update(status_map, "remote-cluster");
    /// ```
    pub fn update(&mut self, statuses: HashMap<String, JobStatus>, remotename: &str) {
        let mut changed_jobs = Vec::new();

        for job in self.jobs.iter_mut() {
            let old_status = *job.status();
            let status = statuses.get(job.remote_id());

            if let Some(st) = status {
                job.set_status(*st);
            }

            if job.status() != &old_status {
                changed_jobs.push(job.clone());
            }
        }

        if changed_jobs.is_empty() {
            info!("No job status changes @ {}:", &remotename);
        } else {
            info!("Jobs with status changes @ {}:", &remotename);
            for job in changed_jobs {
                info!(
                    "  - Job {} @ {}: changed to {:?}",
                    job.name(),
                    job.remote(),
                    job.status()
                );
            }
        }
    }

    /// Synchronizes a specific job with its remote counterpart.
    ///
    /// Downloads files from the remote working directory to the local working
    /// directory and updates synchronization metadata. Optionally updates the
    /// job's synced status.
    pub fn sync_job(
        job: &Job,
        remote: &crate::remote::Remote,
        connection: &dyn crate::connection::RemoteConnection,
        ignore: &[glob::Pattern],
    ) -> Result<chrono::DateTime<Utc>, Error> {
        let remote_dir = remote.work_dir().join(job.id().to_string());

        info!("Syncing job {} @ {}", job.name(), job.remote());

        connection.download_files(&remote_dir, job.working_dir(), job.basename(), ignore)?;
        let sync_time = chrono::Utc::now();

        Ok(sync_time)
    }

    pub fn update_synced_job(
        &mut self,
        id: &Uuid,
        sync_time: chrono::DateTime<Utc>,
    ) -> Result<(), Error> {
        let job = self
            .iter_mut()
            .find(|j| j.id() == id)
            .ok_or_else(|| Error::JobNotFound(id.to_owned()))?;

        job.set_synced_status(true);
        job.set_sync_time(sync_time);

        Ok(())
    }

    pub fn find_by_id(&self, id: &Uuid) -> Option<&Job> {
        self.jobs.iter().find(|j| j.id() == id)
    }

    /// Returns a map of syncable jobs grouped by remote system.
    ///
    /// Identifies jobs that are in terminal states (Finished or Error) and
    /// haven't been synced yet. The result is organized by remote system
    /// for batch processing.
    ///
    /// # Examples
    /// ```
    /// let syncable = jobs.syncable();
    /// for (remote, job_ids) in syncable {
    ///     println!("Remote {} has {} jobs to sync", remote, job_ids.len());
    /// }
    /// ```
    pub fn syncable(&self) -> HashMap<String, Vec<Uuid>> {
        let mut jobs_by_remote: HashMap<String, Vec<Uuid>> = HashMap::new();

        self.query()
            .with_status(&JobStatus::Finished)
            .synced(false)
            .iter()
            .for_each(|job| {
                jobs_by_remote
                    .entry(job.remote().to_string())
                    .or_default()
                    .push(*job.id())
            });

        self.query()
            .with_status(&JobStatus::Error)
            .synced(false)
            .iter()
            .for_each(|job| {
                jobs_by_remote
                    .entry(job.remote().to_string())
                    .or_default()
                    .push(*job.id())
            });

        self.query()
            .with_status(&JobStatus::Undefined)
            .synced(false)
            .iter()
            .for_each(|job| {
                jobs_by_remote
                    .entry(job.remote().to_string())
                    .or_default()
                    .push(*job.id())
            });

        jobs_by_remote
    }

    /// Creates a new query builder for filtering and searching jobs.
    ///
    /// Returns a `JobQuery` instance
    pub fn query(&self) -> JobQuery {
        JobQuery::new(&self.jobs)
    }

    // TODO: use this to create a new job instead of delegating it to a Job::new? This way the `Jobs` struct becomes de standard for interacting with everything job related...!
    pub fn add(&mut self, job: Job) {
        self.jobs.push(job);
    }
}

#[derive(Copy, Clone)]
pub enum Match<'a> {
    Exact(&'a str),
    Contains(&'a str),
}

/// A query builder for filtering and searching jobs.
///
/// `JobQuery` provides a builder interface for constructing complex queries against a job collection. Each method adds a filter condition, and filters are combined with AND logic.
///
/// # Examples
/// ```
/// // Find all unsynced finished jobs from a specific remote submitted after a certain time
/// let results: Vec<&Job> = jobs.query()
///     .with_status(&JobStatus::Finished)
///     .synced(false)
///     .with_remote(Match::Exact("cluster-a"))
///     .with_submit_time_after(cutoff_time)
///     .iter()
///     .collect();
/// ```
#[derive(Copy, Clone)]
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
    submit_time_after: Option<DateTime<Utc>>,
    sync_time_after: Option<DateTime<Utc>>,
}

impl<'a> JobQuery<'a> {
    /// Creates a new query builder for the given job slice.
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
            submit_time_after: None,
            sync_time_after: None,
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

    pub fn with_submit_time_after(mut self, time: DateTime<Utc>) -> Self {
        self.submit_time_after = Some(time);
        self
    }

    pub fn with_sync_time_after(mut self, time: DateTime<Utc>) -> Self {
        self.sync_time_after = Some(time);
        self
    }

    /// Executes the query and returns an iterator over matching jobs.
    ///
    /// Applies all configured filters and returns an iterator that yields
    /// references to jobs that match all conditions.
    ///
    /// # Examples
    /// ```
    /// for job in jobs.query().with_status(&JobStatus::Running).iter() {
    ///     println!("Running job: {}", job.name());
    /// }
    /// ```
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
            submit_time_after,
            sync_time_after,
        } = self;

        let check = |value: &str, matcher: &Option<Match<'a>>| {
            matcher.as_ref().is_none_or(|m| match m {
                Match::Exact(val) => value == *val,
                Match::Contains(val) => value.contains(val),
            })
        };

        jobs.iter().filter(move |job| {
            let id_match = id.is_none_or(|id| id == job.id());
            let synced_match = synced.is_none_or(|synced| synced == job.synced());
            let status_match = status.is_none_or(|st| st == job.status());
            let remote_id_match = remote_id.is_none_or(|id| id == job.remote_id());
            let remote_match = check(job.remote(), &remote);
            let name_match = check(job.name(), &name);
            let basename_match = check(job.basename(), &basename);
            let project_match = check(job.project(), &project);
            let submit_time_match = submit_time_after.is_none_or(|t| *job.submit_time() > t);
            let sync_time_match =
                sync_time_after.is_none_or(|t| job.sync_time().is_some_and(|st| st > t));

            id_match
                && synced_match
                && status_match
                && remote_id_match
                && remote_match
                && name_match
                && basename_match
                && project_match
                && submit_time_match
                && sync_time_match
        })
    }
}
