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
            if let Some(job) = arcmtx.lock().unwrap().find_by_id(&id) {
                let remote = ctx.config.get_remote(job.remote())?;
                let connection = SshConnection::new(remote)?;
                let job = arcmtx
                    .lock()
                    .unwrap()
                    .find_by_id(&id)
                    .cloned()
                    .ok_or(remotelib::job::Error::JobNotFound(id))?;
                Jobs::sync_job(&job, remote, &connection, &ctx.config.ignore)?;
                if args.update_status {
                    let current_time = chrono::Utc::now();
                    arcmtx
                        .lock()
                        .unwrap()
                        .update_synced_job(&id, current_time)?;
                }
            } else {
                return Err(anyhow::anyhow!(remotelib::job::Error::JobNotFound(id)));
            }
        }
        None => {
            let syncable_jobs = arcmtx.lock().unwrap().syncable();

            if syncable_jobs.is_empty() {
                info!("No jobs to sync")
            } else {
                syncable_jobs.par_iter().for_each(|(remote_name, ids)| {
                    let remote = ctx.config.get_remote(remote_name).unwrap();
                    match SshConnection::new(remote) {
                        Ok(sshconnection) => {
                            ids.par_iter().for_each(|id| {
                                let job = arcmtx.lock().unwrap().find_by_id(id).cloned().unwrap();
                                let sync_time = Jobs::sync_job(
                                    &job,
                                    remote,
                                    &sshconnection,
                                    &ctx.config.ignore,
                                )
                                .unwrap();
                                arcmtx
                                    .lock()
                                    .unwrap()
                                    .update_synced_job(id, sync_time)
                                    .unwrap();
                            });
                        }
                        Err(err) => {
                            error!("Failed to connect to {}, caused by: {}", remote.name(), err);
                        }
                    };
                });
            }
        }
    }

    arcmtx.lock().unwrap().save_jobs(&ctx.json_file)?;
    Ok(())
}
