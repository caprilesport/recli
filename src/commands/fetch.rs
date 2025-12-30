use rayon::prelude::*;
use std::sync::{Arc, Mutex};

use crate::Context;
use crate::connection::SshConnection;
use crate::jobs::Jobs;

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

pub fn execute(args: Args, ctx: Context) -> anyhow::Result<()> {
    let jobs = Jobs::load_jobs(&ctx.json_file)?;
    let arcmtx = Arc::new(Mutex::new(jobs));

    match args.remote {
        Some(remote_name) => {
            let remote = &ctx.config.get_remote(&remote_name)?;
            let connection = SshConnection::new(remote)?;
            let statuses = remote.status(&connection)?;
            arcmtx.lock().unwrap().update(statuses, remote.name());
        }
        None => {
            ctx.config.remotes.par_iter().for_each(|remote| {
                let shared_jobs = arcmtx.clone();
                match SshConnection::new(remote) {
                    Ok(sshconnection) => match remote.status(&sshconnection) {
                        Ok(statuses) => {
                            let mut jobs_guard = shared_jobs.lock().unwrap();
                            jobs_guard.update(statuses, remote.name());
                        }
                        Err(e) => {
                            error!(
                                "Failed to fetch statuses at {}, caused by {}",
                                remote.name(),
                                e
                            );
                        }
                    },
                    Err(err) => {
                        error!("Failed to connect to {}, caused by: {}", remote.name(), err);
                    }
                };
            });
        }
    }

    arcmtx.lock().unwrap().save_jobs(&ctx.json_file)?;

    Ok(())
}
