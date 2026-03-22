use rayon::prelude::*;
use std::sync::{Arc, Mutex};

use crate::Context;
use crate::connection::SshConnection;
use crate::job::Job;
use crate::jobs::Jobs;
use crate::remote::Remote;

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

/// Fetches statuses from a slice of remotes in parallel, updates the shared
/// job store, and returns the jobs whose status changed.
pub(crate) fn fetch_statuses(remotes: &[Remote], arcmtx: &Arc<Mutex<Jobs>>) -> Vec<Job> {
    let changed: Arc<Mutex<Vec<Job>>> = Arc::new(Mutex::new(Vec::new()));

    remotes.par_iter().for_each(|remote| {
        let shared_changed = changed.clone();
        match SshConnection::new(remote) {
            Ok(conn) => match remote.status(&conn) {
                Ok(statuses) => {
                    let mut jobs_guard = arcmtx.lock().unwrap();
                    match jobs_guard.update(&statuses, remote.name()) {
                        Ok(updated) => shared_changed.lock().unwrap().extend(updated),
                        Err(e) => error!(
                            "Failed to update statuses at {}, caused by {}",
                            remote.name(),
                            e
                        ),
                    }
                }
                Err(e) => error!(
                    "Failed to fetch statuses at {}, caused by {}",
                    remote.name(),
                    e
                ),
            },
            Err(err) => {
                error!("Failed to connect to {}, caused by: {}", remote.name(), err);
            }
        }
    });

    Arc::try_unwrap(changed).unwrap().into_inner().unwrap()
}

#[allow(clippy::needless_pass_by_value)]
pub fn execute(args: Args, ctx: Context) -> color_eyre::Result<()> {
    let jobs = Jobs::load_from_db(&ctx.db_path)?;
    let arcmtx = Arc::new(Mutex::new(jobs));

    let remotes: &[Remote] = match &args.remote {
        Some(name) => std::slice::from_ref(ctx.config.get_remote(name)?),
        None => &ctx.config.remotes,
    };

    let changed = fetch_statuses(remotes, &arcmtx);

    if ctx.json {
        println!("{}", serde_json::to_string_pretty(&changed)?);
    }

    Ok(())
}
