use crate::Context;
use anyhow::Result;
use remotelib::connection::SshConnection;
use remotelib::jobs::Jobs;

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

pub fn execute(args: Args, ctx: &Context) -> Result<()> {
    let mut jobs = Jobs::load_jobs(&ctx.json_file)?;

    match args.remote {
        Some(remote_name) => {
            let remote = &ctx.config.get_remote(&remote_name)?;
            let connection = SshConnection::new(&remote)?;
            let statuses = remote.status(&connection)?;
            jobs.update(statuses, remote.name());
        }
        None => {
            for remote in &ctx.config.remotes {
                let connection = match SshConnection::new(&remote) {
                    Ok(sshconnection) => sshconnection,
                    Err(err) => {
                        error!("Failed to connect to {}, caused by: {}", remote.name(), err);
                        continue;
                    }
                };
                let statuses = remote.status(&connection)?;
                jobs.update(statuses, remote.name());
            }
        }
    }

    jobs.save_jobs(&ctx.json_file)?;

    Ok(())
}
