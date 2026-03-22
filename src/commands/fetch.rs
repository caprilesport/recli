use rayon::prelude::*;
use std::sync::{Arc, Mutex};

use crate::Context;
use crate::connection::SshConnection;
use crate::job::Job;
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

#[allow(clippy::needless_pass_by_value)]
pub fn execute(args: Args, ctx: Context) -> color_eyre::Result<()> {
    let jobs = Jobs::load_from_db(&ctx.db_path)?;
    let arcmtx = Arc::new(Mutex::new(jobs));
    let changed: Arc<Mutex<Vec<Job>>> = Arc::new(Mutex::new(Vec::new()));

    match args.remote {
        Some(remote_name) => {
            let remote = &ctx.config.get_remote(&remote_name)?;
            let connection = SshConnection::new(remote)?;
            let statuses = remote.status(&connection)?;
            let updated = arcmtx.lock().unwrap().update(&statuses, remote.name())?;
            changed.lock().unwrap().extend(updated);
        }
        None => {
            ctx.config.remotes.par_iter().for_each(|remote| {
                let shared_jobs = arcmtx.clone();
                let shared_changed = changed.clone();
                match SshConnection::new(remote) {
                    Ok(sshconnection) => match remote.status(&sshconnection) {
                        Ok(statuses) => {
                            let mut jobs_guard = shared_jobs.lock().unwrap();
                            match jobs_guard.update(&statuses, remote.name()) {
                                Ok(updated) => shared_changed.lock().unwrap().extend(updated),
                                Err(e) => error!(
                                    "Failed to update statuses at {}, caused by {}",
                                    remote.name(),
                                    e
                                ),
                            }
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
                }
            });
        }
    }

    if ctx.json {
        let changed = Arc::try_unwrap(changed).unwrap().into_inner().unwrap();
        println!("{}", serde_json::to_string_pretty(&changed)?);
    }

    Ok(())
}
