use crate::job::{Error, Job, JobStatus};
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use tracing::info;
use uuid::Uuid;

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
    pub fn save_jobs(&self, json_file_path: &std::path::Path) -> Result<(), Error> {
        tracing::debug!("Saving jobs to {:?}", json_file_path);
        let file = std::fs::File::create(json_file_path)?;
        serde_json::to_writer_pretty(file, &self.jobs)?;
        Ok(())
    }

    pub fn load_jobs(json_file_path: &std::path::Path) -> Result<Self, Error> {
        tracing::debug!("Loading jobs from {:?}", json_file_path);
        if !json_file_path.exists() {
            return Ok(Jobs { jobs: Vec::new() });
        }

        let file = std::fs::File::open(&json_file_path)?;
        if file.metadata()?.len() == 0 {
            return Ok(Jobs { jobs: Vec::new() });
        }

        let file = std::fs::File::open(json_file_path)?;
        let reader = std::io::BufReader::new(file);
        let jobs: Vec<Job> = serde_json::from_reader(reader)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        Ok(Jobs { jobs })
    }

    pub fn update(&mut self, statuses: HashMap<String, JobStatus>, remotename: &str) {
        let mut changed_jobs = Vec::new();

        for job in self.jobs.iter_mut() {
            let old_status = *job.status();
            let status = statuses.get(job.remote_id());

            match status {
                Some(st) => {
                    job.set_status(*st);
                }
                None => (),
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
                info!("  - Job {}: changed to {:?}", job.id(), job.status());
            }
        }
    }

    pub fn sync_job(
        &mut self,
        id: &Uuid,
        remote: &crate::remote::Remote,
        connection: &dyn crate::connection::RemoteConnection,
        update_status: bool,
    ) -> Result<(), Error> {
        let job = self
            .iter_mut()
            .find(|j| j.id() == id)
            .ok_or_else(|| Error::JobNotFound(id.to_owned()))?;
        let remote_dir = remote.work_dir().join(&id.to_string());

        connection.download_files(&remote_dir, &job.working_dir(), job.basename())?;

        if update_status {
            job.set_synced_status(true);
        }

        Ok(())
    }

    pub fn find_by_id(&self, id: &Uuid) -> Option<&Job> {
        self.jobs.iter().find(|j| j.id() == id)
    }

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

        jobs_by_remote
    }

    pub fn query(&self) -> JobQuery {
        JobQuery::new(&self.jobs)
    }

    pub fn add(&mut self, job: Job) {
        self.jobs.push(job);
    }
}

#[derive(Copy, Clone)]
pub enum Match<'a> {
    Exact(&'a str),
    Contains(&'a str),
}

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
