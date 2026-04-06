use crate::commands::fetch::fetch_statuses;
use crate::job::Job;
use crate::jobs::Jobs;
use rayon::prelude::*;
use std::sync::{Arc, Mutex};

use tracing::{error, info};

/// Fetches the latest job statuses then downloads output files for finished jobs.
///
/// Connects to each configured remote, updates local job statuses, and downloads
/// output files for any finished, unsynced jobs. Use `recli fetch` if you only
/// want to refresh statuses without downloading files.
///
/// Specify a single job ID to pull that job only, bypassing status and sync filters.
#[derive(clap::Args, Debug)]
pub struct Args {
    /// Job UUID prefix to pull a single job
    job_id: Option<String>,
    /// Download all files, ignoring the ignore file
    #[arg(long, default_value_t = false)]
    all_files: bool,
}

#[allow(clippy::needless_pass_by_value)]
pub fn execute(args: Args, mut ctx: crate::context::Context) -> color_eyre::Result<()> {
    if args.all_files {
        ctx.clear_ignore();
    }

    if let Some(prefix) = &args.job_id {
        // Extract remote name without keeping a borrow on jobs
        let remote_name = {
            let jobs = Jobs::load_from_db(ctx.db_path())?;
            jobs.find_by_prefix(prefix)?.remote().to_owned()
        };
        let remote = ctx.config().get_remote(&remote_name)?;
        let connection = ctx.connect(remote)?;

        // Update status first (best effort)
        {
            let mut jobs = Jobs::load_from_db(ctx.db_path())?;
            match remote.status(&*connection) {
                Ok(statuses) => {
                    if let Err(e) = jobs.update(&statuses, remote.name()) {
                        error!("Failed to update status: {}", e);
                    }
                }
                Err(e) => error!("Failed to fetch status for {}: {}", remote_name, e),
            }
        }

        // Reload and download
        let jobs = Jobs::load_from_db(ctx.db_path())?;
        let job = jobs.find_by_prefix(prefix)?;
        let job_uuid = *job.uuid();

        match Jobs::sync_job(job, &*connection, &ctx.config().ignore) {
            Ok(sync_time) => {
                jobs.update_sync_time(&job_uuid, sync_time)?;
                if ctx.json_output() {
                    let jobs = Jobs::load_from_db(ctx.db_path())?;
                    let synced = jobs.find_by_prefix(prefix)?;
                    println!("{}", serde_json::to_string_pretty(&[synced])?);
                }
            }
            Err(e) => {
                error!("Failed to pull job {}: {}", job.short_id(), e);
                return Err(e.into());
            }
        }
    } else {
        // Step 1: fetch all statuses in parallel
        let jobs = Jobs::load_from_db(ctx.db_path())?;
        let arcmtx = Arc::new(Mutex::new(jobs));
        fetch_statuses(&ctx.config().remotes, &arcmtx, &ctx);

        // Step 2: download files for all now-finished jobs
        let jobs = Jobs::load_from_db(ctx.db_path())?;
        let syncable_jobs = jobs.syncable();

        if syncable_jobs.is_empty() {
            info!("No jobs to pull");
            return Ok(());
        }

        let arcmtx = Arc::new(Mutex::new(jobs));
        let synced: Arc<Mutex<Vec<Job>>> = Arc::new(Mutex::new(Vec::new()));

        syncable_jobs.par_iter().for_each(|(remote_name, ids)| {
            let remote = match ctx.config().get_remote(remote_name) {
                Ok(r) => r,
                Err(e) => {
                    error!("Invalid remote {}: {}", remote_name, e);
                    return;
                }
            };

            let connection = match ctx.connect(remote) {
                Ok(conn) => conn,
                Err(e) => {
                    error!("Failed to connect to {}: {}", remote.name(), e);
                    return;
                }
            };

            ids.par_iter().for_each(|id| {
                let Some(job) = arcmtx.lock().unwrap().find_by_id(id).cloned() else {
                    error!("Unable to find job {id}");
                    return;
                };

                match Jobs::sync_job(&job, &*connection, &ctx.config().ignore) {
                    Ok(sync_time) => {
                        if let Err(e) = arcmtx.lock().unwrap().update_sync_time(id, sync_time) {
                            error!("Failed to update sync time for {}: {}", job.short_id(), e);
                        } else {
                            synced.lock().unwrap().push(job);
                        }
                    }
                    Err(e) => error!("Failed to pull job {}: {}", job.short_id(), e),
                }
            });
        });

        if ctx.json_output() {
            let synced = Arc::try_unwrap(synced).unwrap().into_inner().unwrap();
            println!("{}", serde_json::to_string_pretty(&synced)?);
        }
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

    fn finished_job() -> Job {
        Job::new(
            Uuid::new_v4(),
            "test".to_string(),
            "12345.server".to_string(),
            "myjob".to_string(),
            PathBuf::from("myjob.pbs"),
            vec![],
            vec![],
            None,
            PathBuf::from("/tmp"),
            PathBuf::from("/remote/work/uuid"),
            JobStatus::Finished,
            Utc::now(),
            None,
        )
    }

    #[test]
    fn test_pull_by_prefix_sets_sync_time() {
        let tmp_db = tempfile::NamedTempFile::new().unwrap();
        let db_path = tmp_db.path().to_path_buf();
        let job = finished_job();
        Jobs::insert_job(&db_path, &job).unwrap();

        let prefix = &job.uuid().to_string()[..7];
        let (factory, _state) = MockFactory::new();

        execute(
            Args {
                job_id: Some(prefix.to_string()),
                all_files: false,
            },
            make_context(db_path.clone(), factory),
        )
        .unwrap();

        let jobs = Jobs::load_from_db(&db_path).unwrap();
        assert!(jobs[0].sync_time().is_some());
    }

    #[test]
    fn test_pull_all_syncs_finished_jobs() {
        let tmp_db = tempfile::NamedTempFile::new().unwrap();
        let db_path = tmp_db.path().to_path_buf();
        let job = finished_job();
        Jobs::insert_job(&db_path, &job).unwrap();

        let (factory, _state) = MockFactory::new();

        execute(
            Args {
                job_id: None,
                all_files: false,
            },
            make_context(db_path.clone(), factory),
        )
        .unwrap();

        let jobs = Jobs::load_from_db(&db_path).unwrap();
        assert!(jobs[0].sync_time().is_some());
    }

    #[test]
    fn test_pull_all_skips_queued_jobs() {
        let tmp_db = tempfile::NamedTempFile::new().unwrap();
        let db_path = tmp_db.path().to_path_buf();
        let job = Job::new(
            Uuid::new_v4(),
            "test".to_string(),
            "12345.server".to_string(),
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
        );
        Jobs::insert_job(&db_path, &job).unwrap();

        let (factory, _state) = MockFactory::new();

        execute(
            Args {
                job_id: None,
                all_files: false,
            },
            make_context(db_path.clone(), factory),
        )
        .unwrap();

        let jobs = Jobs::load_from_db(&db_path).unwrap();
        assert!(jobs[0].sync_time().is_none());
    }
}
