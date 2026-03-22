use crate::connection::SshConnection;
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
pub fn execute(args: Args, mut ctx: crate::Context) -> color_eyre::Result<()> {
    if args.all_files {
        ctx.config.ignore.clear();
    }

    if let Some(ref prefix) = args.job_id {
        // Extract remote name without keeping a borrow on jobs
        let remote_name = {
            let jobs = Jobs::load_from_db(&ctx.db_path)?;
            jobs.find_by_prefix(prefix)?.remote().to_owned()
        };
        let remote = ctx.config.get_remote(&remote_name)?;
        let connection = SshConnection::new(remote)?;

        // Update status first (best effort)
        {
            let mut jobs = Jobs::load_from_db(&ctx.db_path)?;
            match remote.status(&connection) {
                Ok(statuses) => {
                    if let Err(e) = jobs.update(&statuses, remote.name()) {
                        error!("Failed to update status: {}", e);
                    }
                }
                Err(e) => error!("Failed to fetch status for {}: {}", remote_name, e),
            }
        }

        // Reload and download
        let jobs = Jobs::load_from_db(&ctx.db_path)?;
        let job = jobs.find_by_prefix(prefix)?;
        let job_uuid = *job.uuid();

        match Jobs::sync_job(job, &connection, &ctx.config.ignore) {
            Ok(sync_time) => {
                jobs.update_sync_time(&job_uuid, sync_time)?;
                if ctx.json {
                    let jobs = Jobs::load_from_db(&ctx.db_path)?;
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
        // Step 1: fetch all statuses sequentially
        let mut jobs = Jobs::load_from_db(&ctx.db_path)?;
        for remote in &ctx.config.remotes {
            match SshConnection::new(remote) {
                Ok(connection) => match remote.status(&connection) {
                    Ok(statuses) => {
                        if let Err(e) = jobs.update(&statuses, remote.name()) {
                            error!("Failed to update statuses for {}: {}", remote.name(), e);
                        }
                    }
                    Err(e) => error!("Failed to fetch statuses from {}: {}", remote.name(), e),
                },
                Err(e) => error!("Failed to connect to {}: {}", remote.name(), e),
            }
        }

        // Step 2: download files for all now-finished jobs
        let jobs = Jobs::load_from_db(&ctx.db_path)?;
        let syncable_jobs = jobs.syncable();

        if syncable_jobs.is_empty() {
            info!("No jobs to pull");
            return Ok(());
        }

        let arcmtx = Arc::new(Mutex::new(jobs));
        let synced: Arc<Mutex<Vec<Job>>> = Arc::new(Mutex::new(Vec::new()));

        syncable_jobs.par_iter().for_each(|(remote_name, ids)| {
            let remote = match ctx.config.get_remote(remote_name) {
                Ok(r) => r,
                Err(e) => {
                    error!("Invalid remote {}: {}", remote_name, e);
                    return;
                }
            };

            let connection = match SshConnection::new(remote) {
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

                match Jobs::sync_job(&job, &connection, &ctx.config.ignore) {
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

        if ctx.json {
            let synced = Arc::try_unwrap(synced).unwrap().into_inner().unwrap();
            println!("{}", serde_json::to_string_pretty(&synced)?);
        }
    }

    Ok(())
}
