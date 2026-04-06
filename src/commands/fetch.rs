use rayon::prelude::*;
use std::sync::{Arc, Mutex};

use crate::context::Context;
use crate::job::Job;
use crate::jobs::Jobs;
use crate::remote::Remote;

use tracing::error;

#[derive(Debug, clap::Args)]
/// Fetches the latest status for all tracked jobs from the remotes.
///
/// This command connects to each configured remote, queries the queue manager
/// for the current status of your jobs, and updates the local job cache.
pub struct Args {
    #[arg(long, short)]
    remote: Option<String>,
}

/// Fetches statuses from a slice of remotes in parallel, updates the shared
/// job store, and returns the jobs whose status changed.
pub fn fetch_statuses(
    remotes: &[Remote],
    arcmtx: &Arc<Mutex<Jobs>>,
    ctx: &crate::context::Context,
) -> Vec<Job> {
    let changed: Arc<Mutex<Vec<Job>>> = Arc::new(Mutex::new(Vec::new()));

    remotes.par_iter().for_each(|remote| {
        let shared_changed = changed.clone();
        match ctx.connect(remote) {
            Ok(conn) => match remote.status(&*conn) {
                Ok(statuses) => {
                    let mut jobs_guard = arcmtx.lock().unwrap();
                    match jobs_guard.update(&statuses, remote.name()) {
                        Ok(updated) => shared_changed.lock().unwrap().extend(updated),
                        Err(e) => error!(
                            "Failed to update statuses at {}, caused by {}",
                            remote.name(),
                            e
                        ),
                    }
                }
                Err(e) => error!(
                    "Failed to fetch statuses at {}, caused by {}",
                    remote.name(),
                    e
                ),
            },
            Err(err) => {
                error!("Failed to connect to {}, caused by: {}", remote.name(), err);
            }
        }
    });

    Arc::try_unwrap(changed).unwrap().into_inner().unwrap()
}

#[allow(clippy::needless_pass_by_value)]
pub fn execute(args: Args, ctx: Context) -> color_eyre::Result<()> {
    let jobs = Jobs::load_from_db(ctx.db_path())?;
    let arcmtx = Arc::new(Mutex::new(jobs));

    let remotes: &[Remote] = match &args.remote {
        Some(name) => std::slice::from_ref(ctx.config().get_remote(name)?),
        None => &ctx.config().remotes,
    };

    let changed = fetch_statuses(remotes, &arcmtx, &ctx);

    if ctx.json_output() {
        println!("{}", serde_json::to_string_pretty(&changed)?);
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
    use std::path::PathBuf;
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

    fn queued_job(remote_id: &str) -> Job {
        Job::new(
            Uuid::new_v4(),
            "test".to_string(),
            remote_id.to_string(),
            "myjob".to_string(),
            PathBuf::from("myjob.pbs"),
            vec![],
            vec![],
            None,
            PathBuf::from("/tmp"),
            PathBuf::from("/remote/work/uuid"),
            JobStatus::Queued,
            Utc::now(),
            None,
        )
    }

    #[test]
    fn test_fetch_updates_job_status() {
        let tmp_db = tempfile::NamedTempFile::new().unwrap();
        let db_path = tmp_db.path().to_path_buf();

        let job = queued_job("12345.server");
        Jobs::insert_job(&db_path, &job).unwrap();

        // Five header lines then the data line — matches the PBS parser's skip(5)
        let pbs_output = r#"
ufsc:
                                                            Req'd  Req'd   Elap
Job ID          Username Queue    Jobname    SessID NDS TSK Memory Time  S Time
--------------- -------- -------- ---------- ------ --- --- ------ ----- - -----
12345.server    testuser  small   myjob      13716*   1   8   11gb 10000 R 2345:
"#;

        let (factory, state) = MockFactory::new();
        state
            .lock()
            .unwrap()
            .set_output("qstat -u testuser -x", pbs_output);

        execute(
            Args { remote: None },
            make_context(db_path.clone(), factory),
        )
        .unwrap();

        let jobs = Jobs::load_from_db(&db_path).unwrap();
        assert_eq!(jobs[0].status(), &JobStatus::Running);
    }

    #[test]
    fn test_fetch_ignores_unknown_remote_ids() {
        let tmp_db = tempfile::NamedTempFile::new().unwrap();
        let db_path = tmp_db.path().to_path_buf();

        let job = queued_job("99999.server");
        Jobs::insert_job(&db_path, &job).unwrap();

        // Remote reports a job we don't track — our job should stay Queued
        let pbs_output = r#"
ufsc:
                                                            Req'd  Req'd   Elap
Job ID          Username Queue    Jobname    SessID NDS TSK Memory Time  S Time
--------------- -------- -------- ---------- ------ --- --- ------ ----- - -----
12345.server    testuser  small   other      13716*   1   8   11gb 10000 R 2345:
"#;

        let (factory, state) = MockFactory::new();
        state
            .lock()
            .unwrap()
            .set_output("qstat -u testuser -x", pbs_output);

        execute(
            Args { remote: None },
            make_context(db_path.clone(), factory),
        )
        .unwrap();

        let jobs = Jobs::load_from_db(&db_path).unwrap();
        assert_eq!(jobs[0].status(), &JobStatus::Queued);
    }
}
