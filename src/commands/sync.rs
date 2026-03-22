use crate::connection::SshConnection;
use crate::jobs::Jobs;
use rayon::prelude::*;
use std::sync::{Arc, Mutex};

use tracing::{error, info};

/// Downloads output files for finished jobs.
///
/// By default, this command finds all jobs that have a 'Finished' status but
/// have not yet been synced, and downloads their output files.
///
/// You can also specify a single job ID to sync only that job, regardless of
/// its current status.
#[derive(clap::Args, Debug)]
pub struct Args {
    /// Job UUID prefix to sync a single job
    job_id: Option<String>,
    #[arg(short, long, default_value_t = false)]
    update_status: bool,
    /// Sync all files, ignoring the ignore file
    #[arg(long, global = true, default_value_t = false)]
    sync_all_files: bool,
}

#[allow(clippy::needless_pass_by_value)]
pub fn execute(args: Args, mut ctx: crate::Context) -> color_eyre::Result<()> {
    if args.sync_all_files {
        ctx.config.ignore.clear();
    }

    let jobs = Jobs::load_from_db(&ctx.db_path)?;

    if let Some(ref prefix) = args.job_id {
        let (job, job_uuid, remote_name) = {
            let job = jobs.find_by_prefix(prefix)?;
            (job, *job.uuid(), job.remote())
        };

        let remote = ctx.config.get_remote(remote_name)?;
        let connection = SshConnection::new(remote)?;

        match Jobs::sync_job(job, &connection, &ctx.config.ignore) {
            Ok(sync_time) => {
                if args.update_status {
                    jobs.update_sync_time(&job_uuid, sync_time)?;
                }
            }
            Err(e) => {
                error!("Failed to sync job {}: {}", job.short_id(), e);
                return Err(e.into());
            }
        }
    } else {
        let arcmtx = Arc::new(Mutex::new(jobs));
        let syncable_jobs = arcmtx.lock().unwrap().syncable();

        if syncable_jobs.is_empty() {
            info!("No jobs to sync");
        } else {
            // let mut errors = vec![];

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
                        error!("Failed to connect to {}, caused by: {}", remote.name(), e);
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
                                error!("Failed to sync job {}: {}", job.short_id(), e);
                            }
                        }
                        Err(e) => {
                            error!("Failed to sync job {}: {}", job.short_id(), e);
                        }
                    }
                });
            });
        }
    }

    Ok(())
}
