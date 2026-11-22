use rayon::prelude::*;
use std::sync::{Arc, Mutex};

use crate::Context;
use crate::connection::SshConnection;
use crate::job::Job;
use crate::jobs::Jobs;
use crate::remote::Remote;

use color_eyre::eyre::eyre;
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

/// Result of [`fetch_statuses`]: the jobs whose status changed, and the remotes
/// that could not be reached, queried or updated.
#[derive(Debug, Default)]
pub struct FetchOutcome {
    pub changed: Vec<Job>,
    pub failed: Vec<String>,
}

/// Fetches statuses from a slice of remotes in parallel and updates the shared
/// job store. A failing remote is logged and recorded in `failed`; the others
/// are still updated.
pub fn fetch_statuses(remotes: &[Remote], arcmtx: &Arc<Mutex<Jobs>>) -> FetchOutcome {
    let outcome = Mutex::new(FetchOutcome::default());

    remotes.par_iter().for_each(|remote| {
        let result = SshConnection::new(remote)
            .map_err(|e| format!("Failed to connect to {}, caused by: {e}", remote.name()))
            .and_then(|conn| {
                remote.status(&conn).map_err(|e| {
                    format!(
                        "Failed to fetch statuses at {}, caused by {e}",
                        remote.name()
                    )
                })
            })
            .and_then(|statuses| {
                arcmtx
                    .lock()
                    .unwrap()
                    .update(&statuses, remote.name())
                    .map_err(|e| {
                        format!(
                            "Failed to update statuses at {}, caused by {e}",
                            remote.name()
                        )
                    })
            });

        let mut outcome = outcome.lock().unwrap();
        match result {
            Ok(updated) => outcome.changed.extend(updated),
            Err(msg) => {
                error!("{msg}");
                outcome.failed.push(remote.name().to_owned());
            }
        }
    });

    let mut outcome = outcome.into_inner().unwrap();
    outcome.failed.sort();
    outcome
}

#[allow(clippy::needless_pass_by_value)]
pub fn execute(args: Args, ctx: Context) -> color_eyre::Result<()> {
    let jobs = Jobs::load_from_db(&ctx.db_path)?;
    let arcmtx = Arc::new(Mutex::new(jobs));

    let remotes: &[Remote] = match &args.remote {
        Some(name) => std::slice::from_ref(ctx.config.get_remote(name)?),
        None => &ctx.config.remotes,
    };

    let outcome = fetch_statuses(remotes, &arcmtx);

    if ctx.json {
        println!("{}", serde_json::to_string_pretty(&outcome.changed)?);
    }

    if !outcome.failed.is_empty() {
        return Err(eyre!("fetch failed for: {}", outcome.failed.join(", ")));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unreachable_remote(name: &str) -> Remote {
        // Port 1 on loopback refuses the connection at once.
        toml::from_str(&format!(
            r#"
            name = "{name}"
            hostname = "127.0.0.1"
            port = 1
            user = "nobody"
            work_directory = "/tmp"
            queue_manager = "Pbs"
            "#
        ))
        .unwrap()
    }

    #[test]
    fn test_fetch_reports_unreachable_remotes() {
        let db = tempfile::NamedTempFile::new().unwrap();
        let jobs = Arc::new(Mutex::new(Jobs::load_from_db(db.path()).unwrap()));
        let remotes = [unreachable_remote("b"), unreachable_remote("a")];

        let outcome = fetch_statuses(&remotes, &jobs);

        assert!(outcome.changed.is_empty());
        assert_eq!(outcome.failed, ["a", "b"]);
    }
}
