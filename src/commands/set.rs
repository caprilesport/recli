use crate::job::JobStatus;
use crate::jobs::Jobs;
use std::path::PathBuf;

/// Update fields on an existing job record.
///
/// At least one field flag must be provided. Use --clear-tags to remove all
/// tags from a job.
#[derive(Debug, clap::Args)]
#[group(required = true, multiple = true)]
pub struct Args {
    /// Job UUID prefix to update
    #[arg(required = true)]
    job_id: String,

    /// Set the local working directory
    #[arg(long)]
    work_dir: Option<PathBuf>,

    /// Set the remote working directory
    #[arg(long)]
    remote_dir: Option<PathBuf>,

    /// Set the remote name
    #[arg(long)]
    remote: Option<String>,

    /// Set the remote job ID
    #[arg(long)]
    remote_id: Option<String>,

    /// Set the script file path
    #[arg(long)]
    script_file: Option<String>,

    /// Set the job status
    #[arg(long, value_enum)]
    status: Option<JobStatus>,

    /// Replace all tags
    #[arg(long, num_args = 1..)]
    tags: Vec<String>,

    /// Clear all tags
    #[arg(long, action, conflicts_with = "tags")]
    clear_tags: bool,
}

#[allow(clippy::needless_pass_by_value)]
pub fn execute(args: Args, ctx: crate::context::Context) -> color_eyre::Result<()> {
    let jobs = Jobs::load_from_db(ctx.db_path())?;
    let job = jobs.find_by_prefix(&args.job_id)?;
    let uuid = *job.uuid();

    if let Some(path) = &args.work_dir {
        let path = path
            .canonicalize()
            .map_err(|e| color_eyre::eyre::eyre!("Cannot access '{}': {e}", path.display()))?;
        jobs.update_work_dir(&uuid, &path)?;
    }
    if let Some(path) = &args.remote_dir {
        jobs.update_remote_dir(&uuid, path)?;
    }
    if let Some(remote) = &args.remote {
        jobs.update_remote(&uuid, remote)?;
    }
    if let Some(remote_id) = &args.remote_id {
        jobs.update_remote_id(&uuid, remote_id)?;
    }
    if let Some(script_file) = &args.script_file {
        jobs.update_script_file(&uuid, script_file)?;
    }
    if let Some(status) = args.status {
        jobs.update_job_status(&uuid, status)?;
    }
    if !args.tags.is_empty() {
        jobs.update_tags(&uuid, &args.tags)?;
    } else if args.clear_tags {
        jobs.update_tags(&uuid, &[])?;
    }

    if ctx.json_output() {
        let jobs = Jobs::load_from_db(ctx.db_path())?;
        let updated = jobs.find_by_prefix(&args.job_id)?;
        println!("{}", serde_json::to_string_pretty(&updated)?);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::test_support::MockFactory;
    use crate::context::Context;
    use crate::job::{Job, JobStatus};
    use crate::jobs::Jobs;
    use chrono::Utc;
    use uuid::Uuid;

    fn make_context(db_path: PathBuf, factory: MockFactory) -> Context {
        let config: crate::config::Config = toml::from_str(
            r#"
            [[remotes]]
            name = "test"
            hostname = "localhost"
            port = 22
            user = "testuser"
            work_directory = "/remote/work"
            queue_manager = "Pbs"
            "#,
        )
        .unwrap();
        Context::with_factory(config, db_path, Box::new(factory))
    }

    fn sample_job() -> Job {
        Job::new(
            Uuid::new_v4(),
            "test".to_string(),
            "12345.server".to_string(),
            "myjob".to_string(),
            PathBuf::from("myjob.pbs"),
            vec![],
            vec!["old-tag".to_string()],
            None,
            PathBuf::from("/tmp"),
            PathBuf::from("/remote/work/uuid"),
            JobStatus::Finished,
            Utc::now(),
            None,
        )
    }

    fn temp_db() -> (tempfile::NamedTempFile, PathBuf) {
        let f = tempfile::NamedTempFile::new().unwrap();
        let p = f.path().to_path_buf();
        (f, p)
    }

    #[test]
    fn test_set_status() {
        let (_f, db_path) = temp_db();
        let job = sample_job();
        Jobs::insert_job(&db_path, &job).unwrap();
        let prefix = job.uuid().to_string()[..7].to_string();
        let (factory, _) = MockFactory::new();

        execute(
            Args {
                job_id: prefix.clone(),
                work_dir: None,
                remote_dir: None,
                remote: None,
                remote_id: None,
                script_file: None,
                status: Some(JobStatus::Queued),
                tags: vec![],
                clear_tags: false,
            },
            make_context(db_path.clone(), factory),
        )
        .unwrap();

        let jobs = Jobs::load_from_db(&db_path).unwrap();
        let updated = jobs.find_by_prefix(&prefix).unwrap();
        assert_eq!(updated.status(), &JobStatus::Queued);
    }

    #[test]
    fn test_set_tags() {
        let (_f, db_path) = temp_db();
        let job = sample_job();
        Jobs::insert_job(&db_path, &job).unwrap();
        let prefix = job.uuid().to_string()[..7].to_string();
        let (factory, _) = MockFactory::new();

        execute(
            Args {
                job_id: prefix.clone(),
                work_dir: None,
                remote_dir: None,
                remote: None,
                remote_id: None,
                script_file: None,
                status: None,
                tags: vec!["new-tag".to_string()],
                clear_tags: false,
            },
            make_context(db_path.clone(), factory),
        )
        .unwrap();

        let jobs = Jobs::load_from_db(&db_path).unwrap();
        let updated = jobs.find_by_prefix(&prefix).unwrap();
        assert_eq!(updated.tags(), &["new-tag"]);
    }

    #[test]
    fn test_clear_tags() {
        let (_f, db_path) = temp_db();
        let job = sample_job();
        Jobs::insert_job(&db_path, &job).unwrap();
        let prefix = job.uuid().to_string()[..7].to_string();
        let (factory, _) = MockFactory::new();

        execute(
            Args {
                job_id: prefix.clone(),
                work_dir: None,
                remote_dir: None,
                remote: None,
                remote_id: None,
                script_file: None,
                status: None,
                tags: vec![],
                clear_tags: true,
            },
            make_context(db_path.clone(), factory),
        )
        .unwrap();

        let jobs = Jobs::load_from_db(&db_path).unwrap();
        let updated = jobs.find_by_prefix(&prefix).unwrap();
        assert!(updated.tags().is_empty());
    }

    #[test]
    fn test_set_remote_id() {
        let (_f, db_path) = temp_db();
        let job = sample_job();
        Jobs::insert_job(&db_path, &job).unwrap();
        let prefix = job.uuid().to_string()[..7].to_string();
        let (factory, _) = MockFactory::new();

        execute(
            Args {
                job_id: prefix.clone(),
                work_dir: None,
                remote_dir: None,
                remote: None,
                remote_id: Some("99999.server".to_string()),
                script_file: None,
                status: None,
                tags: vec![],
                clear_tags: false,
            },
            make_context(db_path.clone(), factory),
        )
        .unwrap();

        let jobs = Jobs::load_from_db(&db_path).unwrap();
        let updated = jobs.find_by_prefix(&prefix).unwrap();
        assert_eq!(updated.remote_id(), "99999.server");
    }
}
