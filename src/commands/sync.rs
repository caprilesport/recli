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
    job_id: Option<u16>,
    #[arg(short, long, default_value_t = false)]
    update_status: bool,
    /// Sync all files, ignoring the ignore file
    #[arg(long, global = true, default_value_t = false)]
    sync_all_files: bool,
}

pub fn execute(args: Args, mut ctx: crate::Context) -> anyhow::Result<()> {
    if args.sync_all_files {
        ctx.config.ignore.clear();
    }

    let mut jobs = Jobs::load_jobs(&ctx.json_file)?;

    match args.job_id {
        Some(id) => {
            let (job, job_uuid, remote_name) = {
                let job = jobs
                    .iter()
                    .find(|j| j.id() == &id)
                    .ok_or_else(|| anyhow::anyhow!("Job with ID {0} not found", &id))?;
                (job, *job.uuid(), job.remote())
            };

            let remote = ctx.config.get_remote(remote_name)?;
            let connection = SshConnection::new(remote)?;

            match Jobs::sync_job(job, &connection, &ctx.config.ignore) {
                Ok(sync_time) => {
                    if args.update_status {
                        jobs.update_synced_job(&job_uuid, sync_time)?;
                    }
                }
                Err(e) => {
                    error!("Failed to sync job {}: {}", job.id(), e);
                    return Err(e.into());
                }
            }
        }
        None => {
            let arcmtx = Arc::new(Mutex::new(jobs));
            let syncable_jobs = arcmtx.lock().unwrap().syncable();

            if syncable_jobs.is_empty() {
                info!("No jobs to sync")
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
                        let job = match arcmtx.lock().unwrap().find_by_id(id).cloned() {
                            Some(j) => j,
                            None => {
                                error!("Unable to find job {}", id);
                                return;
                            }
                        };

                        match Jobs::sync_job(&job, &connection, &ctx.config.ignore) {
                            Ok(sync_time) => {
                                if let Err(e) =
                                    arcmtx.lock().unwrap().update_synced_job(id, sync_time)
                                {
                                    error!("Failed to sync job {}: {}", job.id(), e);
                                };
                            }
                            Err(e) => {
                                error!("Failed to sync job {}: {}", job.id(), e);
                            }
                        }
                    });
                });
            }
            arcmtx.lock().unwrap().save_jobs(&ctx.json_file)?;
        }
    }

    Ok(())
}
