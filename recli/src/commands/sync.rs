use remotelib::connection::SshConnection;
use remotelib::jobs::Jobs;

use tracing::info;

#[derive(clap::Args, Debug)]
pub struct Args {
    job_id: Option<uuid::Uuid>,
    #[arg(short, long)]
    update_status: bool,
}

pub fn execute(args: Args, ctx: &crate::Context) -> anyhow::Result<()> {
    let mut jobs = Jobs::load_jobs(&ctx.json_file)?;

    match args.job_id {
        Some(id) => {
            if let Some(job) = jobs.find_by_id(&id) {
                let remote = ctx.config.get_remote(job.remote())?;
                let connection = SshConnection::new(&remote)?;
                jobs.sync_job(&id, &remote, &connection, args.update_status)?;
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
                    let connection = SshConnection::new(&remote)?;
                    for id in ids {
                        jobs.sync_job(&id, &remote, &connection, args.update_status)?;
                    }
                }
            }
        }
    }
    Ok(())
}
