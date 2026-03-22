use crate::job::{Error, Job, JobStatus};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use tracing::info;
use uuid::Uuid;

const DB_VERSION: &str = "1";

/// A collection of `Job`s that provides centralized management and operations.
///
/// This is the primary interface for interacting with jobs. The `Jobs` container provides
/// methods for persistence, querying, synchronization, and batch operations on job
/// collections.
#[derive(Debug, Clone)]
pub struct Jobs {
    jobs: Vec<Job>,
    db_path: PathBuf,
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

fn open_db(path: &Path) -> Result<Connection, Error> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA busy_timeout=5000;
         CREATE TABLE IF NOT EXISTS jobs (
             uuid        TEXT PRIMARY KEY,
             remote      TEXT NOT NULL,
             remote_id   TEXT,
             name        TEXT,
             script_file TEXT,
             work_dir    TEXT,
             remote_dir  TEXT,
             status      TEXT,
             tags        TEXT,
             queue       TEXT,
             submit_time TEXT,
             sync_time   TEXT
         );
         CREATE TABLE IF NOT EXISTS meta (
             key   TEXT PRIMARY KEY,
             value TEXT
         );",
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO meta (key, value) VALUES ('version', ?1)",
        params![DB_VERSION],
    )?;
    Ok(conn)
}

fn parse_status(s: &str) -> JobStatus {
    match s {
        "Q" => JobStatus::Queued,
        "R" => JobStatus::Running,
        "F" => JobStatus::Finished,
        "E" => JobStatus::Error,
        _ => JobStatus::Undefined,
    }
}

impl Jobs {
    /// Loads the job collection from a `SQLite` database at the specified path.
    ///
    /// If the database does not exist, it will be created with the appropriate schema.
    ///
    /// # Errors
    /// - If the database cannot be opened or created
    /// - If rows cannot be read or parsed
    pub fn load_from_db(path: &Path) -> Result<Self, Error> {
        let conn = open_db(path)?;
        let mut stmt = conn.prepare(
            "SELECT uuid, remote, remote_id, name, work_dir, remote_dir, status, submit_time, sync_time \
             FROM jobs ORDER BY submit_time",
        )?;

        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,         // uuid
                    row.get::<_, String>(1)?,         // remote
                    row.get::<_, Option<String>>(2)?, // remote_id
                    row.get::<_, Option<String>>(3)?, // name
                    row.get::<_, Option<String>>(4)?, // work_dir
                    row.get::<_, Option<String>>(5)?, // remote_dir
                    row.get::<_, Option<String>>(6)?, // status
                    row.get::<_, String>(7)?,         // submit_time
                    row.get::<_, Option<String>>(8)?, // sync_time
                ))
            })?
            .collect::<Result<Vec<_>, rusqlite::Error>>()?;

        let jobs = rows
            .into_iter()
            .map(
                |(
                    uuid_str,
                    remote,
                    remote_id,
                    name,
                    work_dir,
                    remote_dir,
                    status_str,
                    submit_time_str,
                    sync_time_str,
                )| {
                    let uuid = uuid_str.parse::<Uuid>().map_err(|_| {
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!("Invalid UUID: {uuid_str}"),
                        )
                    })?;
                    let submit_time = submit_time_str.parse::<DateTime<Utc>>().map_err(|_| {
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!("Invalid datetime: {submit_time_str}"),
                        )
                    })?;
                    let sync_time = sync_time_str
                        .map(|s| {
                            s.parse::<DateTime<Utc>>().map_err(|_| {
                                std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    format!("Invalid datetime: {s}"),
                                )
                            })
                        })
                        .transpose()?;
                    Ok(Job::from_parts(
                        uuid,
                        remote,
                        remote_id.unwrap_or_default(),
                        name.unwrap_or_default(),
                        PathBuf::from(work_dir.unwrap_or_default()),
                        PathBuf::from(remote_dir.unwrap_or_default()),
                        parse_status(status_str.as_deref().unwrap_or("")),
                        submit_time,
                        sync_time,
                    ))
                },
            )
            .collect::<Result<Vec<_>, std::io::Error>>()?;

        Ok(Self {
            jobs,
            db_path: path.to_path_buf(),
        })
    }

    /// Inserts a new job into the database at the given path.
    ///
    /// # Errors
    /// - If the database cannot be opened
    /// - If the INSERT statement fails
    pub fn insert_job(path: &Path, job: &Job) -> Result<(), Error> {
        let conn = open_db(path)?;
        conn.execute(
            "INSERT INTO jobs \
             (uuid, remote, remote_id, name, work_dir, remote_dir, status, submit_time, sync_time) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                job.uuid().to_string(),
                job.remote(),
                job.remote_id(),
                job.filename(),
                job.work_dir().to_str().unwrap_or(""),
                job.remote_dir().to_str().unwrap_or(""),
                job.status().as_str(),
                job.submit_time().to_rfc3339(),
                job.sync_time().map(DateTime::to_rfc3339),
            ],
        )?;
        Ok(())
    }

    fn update_job_status(&self, uuid: &Uuid, status: JobStatus) -> Result<(), Error> {
        let conn = open_db(&self.db_path)?;
        conn.execute(
            "UPDATE jobs SET status = ?1 WHERE uuid = ?2",
            params![status.as_str(), uuid.to_string()],
        )?;
        Ok(())
    }

    /// Updates the sync time for a job in the database.
    ///
    /// # Errors
    /// - If the database cannot be opened
    /// - If the UPDATE statement fails
    pub fn update_sync_time(&self, uuid: &Uuid, sync_time: DateTime<Utc>) -> Result<(), Error> {
        let conn = open_db(&self.db_path)?;
        conn.execute(
            "UPDATE jobs SET sync_time = ?1 WHERE uuid = ?2",
            params![sync_time.to_rfc3339(), uuid.to_string()],
        )?;
        Ok(())
    }

    /// Updates job statuses based on a provided status map and logs changes.
    ///
    /// Compares current job statuses with provided statuses and updates jobs where the
    /// remote status differs. Each change is persisted to the database immediately.
    ///
    /// # Errors
    /// - If any database UPDATE fails
    pub fn update(
        &mut self,
        statuses: &HashMap<String, JobStatus>,
        remotename: &str,
    ) -> Result<(), Error> {
        let mut changed_jobs = Vec::new();

        for job in self
            .jobs
            .iter_mut()
            .filter(|j| !j.synced() && j.remote() == remotename)
        {
            let old_status = *job.status();
            if let Some(st) = statuses.get(job.remote_id()) {
                job.set_status(*st);
            }
            if job.status() != &old_status {
                changed_jobs.push(job.clone());
            }
        }

        if changed_jobs.is_empty() {
            info!("No job status changes @ {}:", remotename);
        } else {
            info!("Jobs with status changes @ {}:", remotename);
            for job in &changed_jobs {
                info!(
                    "  - Job {} ({}) @ {}: changed to {:?}",
                    job.filename(),
                    job.short_id(),
                    job.remote(),
                    job.status()
                );
                self.update_job_status(job.uuid(), *job.status())?;
            }
        }

        Ok(())
    }

    /// Synchronizes a specific job with its remote counterpart.
    ///
    /// Downloads files from the remote working directory to the local working directory.
    ///
    /// # Errors
    /// - If the file download fails
    pub fn sync_job(
        job: &Job,
        connection: &dyn crate::connection::RemoteConnection,
        ignore: &[glob::Pattern],
    ) -> Result<DateTime<Utc>, Error> {
        info!(
            "Syncing job {} ({}) @ {}",
            job.filename(),
            job.short_id(),
            job.remote()
        );
        connection.download_files(job.remote_dir(), job.work_dir(), ignore)?;
        Ok(chrono::Utc::now())
    }

    pub fn find_by_id(&self, id: &Uuid) -> Option<&Job> {
        self.jobs.iter().find(|j| j.uuid() == id)
    }

    /// Finds a job by a UUID prefix. Accepts any prefix length.
    ///
    /// # Errors
    /// - [`Error::JobNotFound`] if no jobs match the prefix
    /// - [`Error::AmbiguousPrefix`] if multiple jobs match
    pub fn find_by_prefix(&self, prefix: &str) -> Result<&Job, Error> {
        let matches: Vec<&Job> = self
            .jobs
            .iter()
            .filter(|j| j.uuid().to_string().starts_with(prefix))
            .collect();
        match matches.len() {
            0 => Err(Error::JobNotFound(prefix.to_string())),
            1 => Ok(matches[0]),
            _ => Err(Error::AmbiguousPrefix {
                prefix: prefix.to_string(),
                matches: matches
                    .iter()
                    .map(|j| j.short_id())
                    .collect::<Vec<_>>()
                    .join(", "),
            }),
        }
    }

    /// Returns a map of syncable jobs grouped by remote system.
    ///
    /// Identifies jobs that are in terminal states (Finished, Error, or Undefined) and
    /// haven't been synced yet. The result is organized by remote system for batch
    /// processing.
    pub fn syncable(&self) -> HashMap<String, Vec<Uuid>> {
        let mut jobs_by_remote: HashMap<String, Vec<Uuid>> = HashMap::new();

        for status in [JobStatus::Finished, JobStatus::Error, JobStatus::Undefined] {
            self.query()
                .with_status(&status)
                .synced(false)
                .iter()
                .for_each(|job| {
                    jobs_by_remote
                        .entry(job.remote().to_string())
                        .or_default()
                        .push(*job.uuid());
                });
        }

        jobs_by_remote
    }

    /// Creates a new query builder for filtering and searching jobs.
    ///
    /// Returns a `JobQuery` instance that can be used to filter jobs.
    pub fn query(&self) -> JobQuery<'_> {
        JobQuery::new(&self.jobs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::Job;

    fn make_job(remote: &str, remote_id: &str, status: JobStatus) -> Job {
        Job::from_parts(
            Uuid::new_v4(),
            remote.to_string(),
            remote_id.to_string(),
            "myjob".to_string(),
            PathBuf::from("/home/user/jobs"),
            PathBuf::from("/remote/work/uuid"),
            status,
            Utc::now(),
            None,
        )
    }

    fn temp_db() -> (tempfile::NamedTempFile, PathBuf) {
        let f = tempfile::NamedTempFile::new().unwrap();
        let path = f.path().to_path_buf();
        (f, path)
    }

    #[test]
    fn test_insert_and_load() {
        let (_tmp, path) = temp_db();
        let job = make_job("babel", "12345.server", JobStatus::Running);

        Jobs::insert_job(&path, &job).unwrap();
        let jobs = Jobs::load_from_db(&path).unwrap();

        assert_eq!(jobs.len(), 1);
        let loaded = &jobs[0];
        assert_eq!(loaded.uuid(), job.uuid());
        assert_eq!(loaded.remote(), job.remote());
        assert_eq!(loaded.remote_id(), job.remote_id());
        assert_eq!(loaded.filename(), job.filename());
        assert_eq!(loaded.status(), job.status());
        assert_eq!(loaded.work_dir(), job.work_dir());
        assert_eq!(loaded.remote_dir(), job.remote_dir());
        assert!(loaded.sync_time().is_none());
    }

    #[test]
    fn test_update_status() {
        let (_tmp, path) = temp_db();
        let job = make_job("babel", "12345.server", JobStatus::Queued);
        Jobs::insert_job(&path, &job).unwrap();

        let mut jobs = Jobs::load_from_db(&path).unwrap();
        let mut statuses = HashMap::new();
        statuses.insert("12345.server".to_string(), JobStatus::Running);
        jobs.update(&statuses, "babel").unwrap();

        assert_eq!(jobs[0].status(), &JobStatus::Running);

        let reloaded = Jobs::load_from_db(&path).unwrap();
        assert_eq!(reloaded[0].status(), &JobStatus::Running);
    }

    #[test]
    fn test_update_ignores_other_remotes() {
        let (_tmp, path) = temp_db();
        let job = make_job("cluster-b", "99", JobStatus::Queued);
        Jobs::insert_job(&path, &job).unwrap();

        let mut jobs = Jobs::load_from_db(&path).unwrap();
        let mut statuses = HashMap::new();
        statuses.insert("99".to_string(), JobStatus::Running);
        jobs.update(&statuses, "cluster-a").unwrap();

        assert_eq!(jobs[0].status(), &JobStatus::Queued);
    }

    #[test]
    fn test_update_sync_time() {
        let (_tmp, path) = temp_db();
        let job = make_job("babel", "12345.server", JobStatus::Finished);
        Jobs::insert_job(&path, &job).unwrap();

        let jobs = Jobs::load_from_db(&path).unwrap();
        jobs.update_sync_time(job.uuid(), Utc::now()).unwrap();

        let reloaded = Jobs::load_from_db(&path).unwrap();
        assert!(reloaded[0].synced());
    }

    #[test]
    fn test_find_by_prefix() {
        let (_tmp, path) = temp_db();
        let job = make_job("babel", "1", JobStatus::Queued);
        Jobs::insert_job(&path, &job).unwrap();

        let jobs = Jobs::load_from_db(&path).unwrap();
        let prefix = &job.uuid().to_string()[..7];
        let found = jobs.find_by_prefix(prefix).unwrap();
        assert_eq!(found.uuid(), job.uuid());
    }

    #[test]
    fn test_find_by_prefix_not_found() {
        let (_tmp, path) = temp_db();
        let jobs = Jobs::load_from_db(&path).unwrap();
        let err = jobs.find_by_prefix("nonexistent").unwrap_err();
        assert!(matches!(err, crate::job::Error::JobNotFound(_)));
    }

    #[test]
    fn test_find_by_prefix_ambiguous() {
        let (_tmp, path) = temp_db();
        // Two UUIDs that share the same first 7 chars.
        let uuid1 = "aaaaaaaa-0000-0000-0000-000000000001"
            .parse::<Uuid>()
            .unwrap();
        let uuid2 = "aaaaaaaa-0000-0000-0000-000000000002"
            .parse::<Uuid>()
            .unwrap();
        let job1 = Job::from_parts(
            uuid1,
            "babel".to_string(),
            "1".to_string(),
            "job1".to_string(),
            PathBuf::from("/tmp"),
            PathBuf::from("/remote"),
            JobStatus::Queued,
            Utc::now(),
            None,
        );
        let job2 = Job::from_parts(
            uuid2,
            "babel".to_string(),
            "2".to_string(),
            "job2".to_string(),
            PathBuf::from("/tmp"),
            PathBuf::from("/remote"),
            JobStatus::Queued,
            Utc::now(),
            None,
        );
        Jobs::insert_job(&path, &job1).unwrap();
        Jobs::insert_job(&path, &job2).unwrap();

        let jobs = Jobs::load_from_db(&path).unwrap();
        let err = jobs.find_by_prefix("aaaaaaa").unwrap_err();
        assert!(matches!(err, crate::job::Error::AmbiguousPrefix { .. }));
    }

    #[test]
    fn test_query_by_status() {
        let (_tmp, path) = temp_db();
        Jobs::insert_job(&path, &make_job("babel", "1", JobStatus::Running)).unwrap();
        Jobs::insert_job(&path, &make_job("babel", "2", JobStatus::Finished)).unwrap();
        Jobs::insert_job(&path, &make_job("babel", "3", JobStatus::Queued)).unwrap();

        let jobs = Jobs::load_from_db(&path).unwrap();
        let running: Vec<_> = jobs
            .query()
            .with_status(&JobStatus::Running)
            .iter()
            .collect();
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].remote_id(), "1");
    }

    #[test]
    fn test_query_by_remote() {
        let (_tmp, path) = temp_db();
        Jobs::insert_job(&path, &make_job("babel", "1", JobStatus::Running)).unwrap();
        Jobs::insert_job(&path, &make_job("cluster-b", "2", JobStatus::Running)).unwrap();

        let jobs = Jobs::load_from_db(&path).unwrap();
        let babel: Vec<_> = jobs.query().with_remote("babel").iter().collect();
        assert_eq!(babel.len(), 1);
        assert_eq!(babel[0].remote_id(), "1");
    }

    #[test]
    fn test_query_synced_filter() {
        let (_tmp, path) = temp_db();
        Jobs::insert_job(&path, &make_job("babel", "1", JobStatus::Finished)).unwrap();
        let synced_job = Job::from_parts(
            Uuid::new_v4(),
            "babel".to_string(),
            "2".to_string(),
            "myjob".to_string(),
            PathBuf::from("/tmp"),
            PathBuf::from("/remote"),
            JobStatus::Finished,
            Utc::now(),
            Some(Utc::now()),
        );
        Jobs::insert_job(&path, &synced_job).unwrap();

        let jobs = Jobs::load_from_db(&path).unwrap();
        assert_eq!(jobs.query().synced(false).iter().count(), 1);
        assert_eq!(jobs.query().synced(true).iter().count(), 1);
    }

    #[test]
    fn test_syncable() {
        let (_tmp, path) = temp_db();
        Jobs::insert_job(&path, &make_job("babel", "1", JobStatus::Queued)).unwrap();
        Jobs::insert_job(&path, &make_job("babel", "2", JobStatus::Running)).unwrap();
        Jobs::insert_job(&path, &make_job("babel", "3", JobStatus::Finished)).unwrap();
        Jobs::insert_job(&path, &make_job("babel", "4", JobStatus::Error)).unwrap();
        let already_synced = Job::from_parts(
            Uuid::new_v4(),
            "babel".to_string(),
            "5".to_string(),
            "myjob".to_string(),
            PathBuf::from("/tmp"),
            PathBuf::from("/remote"),
            JobStatus::Finished,
            Utc::now(),
            Some(Utc::now()),
        );
        Jobs::insert_job(&path, &already_synced).unwrap();

        let jobs = Jobs::load_from_db(&path).unwrap();
        let syncable = jobs.syncable();
        assert_eq!(syncable.get("babel").unwrap().len(), 2); // Finished + Error, not yet synced
    }
}

/// A query builder for filtering and searching jobs.
///
/// `JobQuery` provides a builder interface for constructing complex queries against a job
/// collection. Each method adds a filter condition, and filters are combined with AND logic.
///
/// # Examples
/// ```
/// // Find all unsynced finished jobs from a specific remote submitted after a certain time
/// let results: Vec<&Job> = jobs.query()
///     .with_status(&JobStatus::Finished)
///     .synced(false)
///     .with_remote("cluster-a")
///     .iter()
///     .collect();
/// ```
#[derive(Copy, Clone)]
pub struct JobQuery<'a> {
    jobs: &'a [Job],
    prefix: Option<&'a str>,
    uuid: Option<&'a Uuid>,
    synced: Option<bool>,
    status: Option<&'a JobStatus>,
    remote_id: Option<&'a str>,
    remote: Option<&'a str>,
    filename: Option<&'a str>,
    directory: Option<&'a str>,
    submit_time_after: Option<DateTime<Utc>>,
    sync_time_after: Option<DateTime<Utc>>,
}

impl<'a> JobQuery<'a> {
    fn new(jobs: &'a [Job]) -> Self {
        Self {
            jobs,
            prefix: None,
            uuid: None,
            synced: None,
            status: None,
            remote: None,
            remote_id: None,
            filename: None,
            directory: None,
            submit_time_after: None,
            sync_time_after: None,
        }
    }

    pub fn with_prefix(mut self, prefix: &'a str) -> Self {
        self.prefix = Some(prefix);
        self
    }

    pub fn with_uuid(mut self, id: &'a Uuid) -> Self {
        self.uuid = Some(id);
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

    pub fn with_remote(mut self, remote: &'a str) -> Self {
        self.remote = Some(remote);
        self
    }

    pub fn with_name(mut self, name: &'a str) -> Self {
        self.filename = Some(name);
        self
    }

    pub fn with_dir(mut self, dir: &'a str) -> Self {
        self.directory = Some(dir);
        self
    }

    /// Executes the query and returns an iterator over matching jobs.
    ///
    /// Applies all configured filters and returns an iterator that yields references to
    /// jobs that match all conditions.
    ///
    /// # Examples
    /// ```
    /// for job in jobs.query().with_status(&JobStatus::Running).iter() {
    ///     println!("Running job: {}", job.filename());
    /// }
    /// ```
    pub fn iter(self) -> impl Iterator<Item = &'a Job> {
        let Self {
            jobs,
            prefix,
            uuid,
            synced,
            status,
            remote_id,
            remote,
            filename,
            directory,
            submit_time_after,
            sync_time_after,
        } = self;

        jobs.iter().filter(move |job| {
            let prefix_match = prefix.is_none_or(|p| job.uuid().to_string().starts_with(p));
            let uuid_match = uuid.is_none_or(|id| id == job.uuid());
            let synced_match = synced.is_none_or(|synced| synced == job.synced());
            let status_match = status.is_none_or(|st| st == job.status());
            let remote_id_match = remote_id.is_none_or(|id| id == job.remote_id());
            let remote_match = remote.is_none_or(|r| job.remote() == r);
            let name_match = filename.is_none_or(|f| job.filename().contains(f));
            let dir_match =
                directory.is_none_or(|d| job.work_dir().to_str().is_some_and(|s| s.contains(d)));
            let submit_time_match = submit_time_after.is_none_or(|t| *job.submit_time() > t);
            let sync_time_match =
                sync_time_after.is_none_or(|t| job.sync_time().is_some_and(|st| *st > t));

            prefix_match
                && uuid_match
                && synced_match
                && status_match
                && remote_id_match
                && remote_match
                && name_match
                && dir_match
                && submit_time_match
                && sync_time_match
        })
    }
}
