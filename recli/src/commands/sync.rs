use remotelib::connection::SshConnection;
use remotelib::jobs::Jobs;

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
    let mut jobs = Jobs::load_jobs(&ctx.json_file)?;

    match args.job_id {
        Some(id) => {
            if let Some(job) = jobs.find_by_id(&id) {
                let remote = ctx.config.get_remote(job.remote())?;
                let connection = SshConnection::new(remote)?;
                let job = jobs
                    .find_by_id(&id)
                    .ok_or(remotelib::job::Error::JobNotFound(id))?;
                Jobs::sync_job(job, remote, &connection, &ctx.config.ignore)?;
                if args.update_status {
                    let current_time = chrono::Utc::now();
                    jobs.update_synced_job(&id, current_time)?;
                }
            } else {
                return Err(anyhow::anyhow!(remotelib::job::Error::JobNotFound(id)));
            }
        }
        None => {
            let syncable_jobs = jobs.syncable();

            if syncable_jobs.is_empty() {
                info!("No jobs to sync")
            } else {
                for (remote_name, ids) in syncable_jobs {
                    let remote = ctx.config.get_remote(&remote_name)?;
                    let connection = match SshConnection::new(remote) {
                        Ok(sshconnection) => sshconnection,
                        Err(err) => {
                            error!("Failed to connect to {}, caused by: {}", remote.name(), err);
                            continue;
                        }
                    };
                    for id in ids {
                        let job = jobs
                            .find_by_id(&id)
                            .ok_or(remotelib::job::Error::JobNotFound(id))?;
                        let sync_time =
                            Jobs::sync_job(job, remote, &connection, &ctx.config.ignore)?;
                        jobs.update_synced_job(&id, sync_time)?;
                    }
                }
            }
        }
    }

    jobs.save_jobs(&ctx.json_file)?;
    Ok(())
}
