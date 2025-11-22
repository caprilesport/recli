use rayon::prelude::*;
use remotelib::connection::SshConnection;
use remotelib::jobs::Jobs;
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
    job_id: Option<uuid::Uuid>,
    #[arg(short, long, default_value_t = false)]
    update_status: bool,
}

pub fn execute(args: Args, ctx: crate::Context) -> anyhow::Result<()> {
    let jobs = Jobs::load_jobs(&ctx.json_file)?;
    let arcmtx = Arc::new(Mutex::new(jobs));

    match args.job_id {
        Some(id) => {
            let (job, remote_name) = {
                let guard = arcmtx.lock().unwrap();
                let job = guard
                    .find_by_id(&id)
                    .ok_or(remotelib::job::Error::JobNotFound(id))?;
                (job.clone(), job.remote().to_string())
            };

            let remote = ctx.config.get_remote(&remote_name)?;
            let connection = SshConnection::new(remote)?;

            match Jobs::sync_job(&job, remote, &connection, &ctx.config.ignore) {
                Ok(sync_time) => {
                    if args.update_status {
                        let mut guard = arcmtx.lock().unwrap();
                        guard.update_synced_job(&id, sync_time)?;
                    }
                }
                Err(e) => {
                    error!("Failed to sync job {}: {}", id, e);
                    return Err(e.into());
                }
            }
        }
        None => {
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

                        match Jobs::sync_job(&job, remote, &connection, &ctx.config.ignore) {
                            Ok(sync_time) => {
                                if let Err(e) =
                                    arcmtx.lock().unwrap().update_synced_job(id, sync_time)
                                {
                                    error!("Failed to sync job {}: {}", id, e);
                                };
                            }
                            Err(e) => {
                                error!("Failed to sync job {}: {}", id, e);
                            }
                        }
                    });
                });
            }
        }
    }

    arcmtx.lock().unwrap().save_jobs(&ctx.json_file)?;
    Ok(())
}
