use crate::commands::fetch::fetch_statuses;
use crate::connection::SshConnection;
use crate::job::Job;
use crate::jobs::Jobs;
use crate::remote::Remote;
use color_eyre::eyre::eyre;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use tracing::{debug, error, info};

/// Fetches the latest job statuses then downloads output files for finished jobs.
///
/// Connects to each configured remote, updates local job statuses, and downloads
/// output files for any finished, unsynced jobs. Use `recli fetch` if you only
/// want to refresh statuses without downloading files.
///
/// Specify a single job ID to pull that job only, bypassing status and sync filters.
#[derive(clap::Args, Debug)]
pub struct Args {
    /// Job UUID prefix to pull a single job
    job_id: Option<String>,
    /// Download all files, ignoring the ignore file
    #[arg(long, default_value_t = false)]
    all_files: bool,
}

#[allow(clippy::needless_pass_by_value)]
pub fn execute(args: Args, mut ctx: crate::Context) -> color_eyre::Result<()> {
    if args.all_files {
        ctx.config.ignore.clear();
    }

    if let Some(prefix) = &args.job_id {
        // Extract remote name without keeping a borrow on jobs
        let remote_name = {
            let jobs = Jobs::load_from_db(&ctx.db_path)?;
            jobs.find_by_prefix(prefix)?.remote().to_owned()
        };
        let remote = ctx.config.get_remote(&remote_name)?;
        let connection = SshConnection::new(remote)?;

        // Update status first (best effort)
        {
            let mut jobs = Jobs::load_from_db(&ctx.db_path)?;
            match remote.status(&connection) {
                Ok(statuses) => {
                    if let Err(e) = jobs.update(&statuses, remote.name()) {
                        error!("Failed to update status: {}", e);
                    }
                }
                Err(e) => error!("Failed to fetch status for {}: {}", remote_name, e),
            }
        }

        // Reload and download
        let jobs = Jobs::load_from_db(&ctx.db_path)?;
        let job = jobs.find_by_prefix(prefix)?;
        let job_uuid = *job.uuid();

        match Jobs::sync_job(job, &connection, &ctx.config.ignore) {
            Ok(sync_time) => {
                jobs.update_sync_time(&job_uuid, sync_time)?;
                if ctx.json {
                    let jobs = Jobs::load_from_db(&ctx.db_path)?;
                    let synced = jobs.find_by_prefix(prefix)?;
                    println!("{}", serde_json::to_string_pretty(&[synced])?);
                }
            }
            Err(e) => {
                error!("Failed to pull job {}: {}", job.short_id(), e);
                return Err(e.into());
            }
        }
    } else {
        // Step 1: fetch all statuses in parallel
        let jobs = Jobs::load_from_db(&ctx.db_path)?;
        let arcmtx = Arc::new(Mutex::new(jobs));
        // A remote that fails here is reported at the end; pull still downloads
        // what it can.
        let fetched = fetch_statuses(&ctx.config.remotes, &arcmtx);
        let mut failures = Vec::new();
        if !fetched.failed.is_empty() {
            failures.push(format!("fetch failed for: {}", fetched.failed.join(", ")));
        }

        // Step 2: download files for all now-finished jobs
        let jobs = Jobs::load_from_db(&ctx.db_path)?;
        let syncable_jobs = jobs.syncable();

        if syncable_jobs.is_empty() {
            info!("No jobs to pull");
        }

        let jobs = Mutex::new(jobs);
        let synced = Mutex::new(Vec::new());
        let failures = Mutex::new(failures);

        // Each remote downloads on its own threads: a worker waiting for one remote's
        // channel pool must not hold up the others.
        std::thread::scope(|s| {
            for (remote_name, ids) in &syncable_jobs {
                match ctx.config.get_remote(remote_name) {
                    Ok(remote) => {
                        let (jobs, synced, failures) = (&jobs, &synced, &failures);
                        let ignore = &ctx.config.ignore;
                        s.spawn(move || {
                            let failed = sync_remote(remote, ids, jobs, synced, ignore);
                            failures.lock().unwrap().extend(failed);
                        });
                    }
                    Err(e) => {
                        error!("Invalid remote {}: {}", remote_name, e);
                        failures
                            .lock()
                            .unwrap()
                            .push(format!("unknown remote {remote_name}"));
                    }
                }
            }
        });

        if ctx.json {
            let synced = synced.into_inner().unwrap();
            println!("{}", serde_json::to_string_pretty(&synced)?);
        }

        let mut failures = failures.into_inner().unwrap();
        if !failures.is_empty() {
            failures.sort();
            return Err(eyre!("pull incomplete: {}", failures.join("; ")));
        }
    }

    Ok(())
}

/// Downloads the given jobs from one remote over a single connection, running as
/// many downloads at once as the server allows channels (at most one per job).
/// Returns a short description of each failure; errors are also logged.
fn sync_remote(
    remote: &Remote,
    ids: &[Uuid],
    jobs: &Mutex<Jobs>,
    synced: &Mutex<Vec<Job>>,
    ignore: &[glob::Pattern],
) -> Vec<String> {
    let unreachable = || {
        vec![format!(
            "no downloads from {} ({} jobs)",
            remote.name(),
            ids.len()
        )]
    };

    let connection = match SshConnection::new(remote) {
        Ok(conn) => conn,
        Err(e) => {
            error!("Failed to connect to {}: {}", remote.name(), e);
            return unreachable();
        }
    };

    let workers = match connection.open_sftp_channels(ids.len()) {
        Ok(n) => n,
        Err(e) => {
            error!("Failed to open SFTP channels on {}: {}", remote.name(), e);
            return unreachable();
        }
    };
    debug!(
        "Downloading {} jobs from {} over {workers} channels",
        ids.len(),
        remote.name()
    );

    let queue = Mutex::new(ids.iter());
    let failed = Mutex::new(Vec::new());

    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                loop {
                    let Some(id) = queue.lock().unwrap().next() else {
                        break;
                    };
                    let Some(job) = jobs.lock().unwrap().find_by_id(id).cloned() else {
                        error!("Unable to find job {id}");
                        failed.lock().unwrap().push(id.to_string());
                        continue;
                    };

                    match Jobs::sync_job(&job, &connection, ignore) {
                        Ok(sync_time) => {
                            if let Err(e) = jobs.lock().unwrap().update_sync_time(id, sync_time) {
                                error!("Failed to update sync time for {}: {}", job.short_id(), e);
                                failed.lock().unwrap().push(job.short_id());
                            } else {
                                synced.lock().unwrap().push(job);
                            }
                        }
                        Err(e) => {
                            error!("Failed to pull job {}: {}", job.short_id(), e);
                            failed.lock().unwrap().push(job.short_id());
                        }
                    }
                }
            });
        }
    });

    let mut failed = failed.into_inner().unwrap();
    if failed.is_empty() {
        return failed;
    }
    failed.sort();
    vec![format!("download failed for: {}", failed.join(", "))]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_remote_reports_unreachable_remote() {
        let db = tempfile::NamedTempFile::new().unwrap();
        let jobs = Mutex::new(Jobs::load_from_db(db.path()).unwrap());
        let synced = Mutex::new(Vec::new());
        let ids = [Uuid::new_v4(), Uuid::new_v4()];

        let failures = sync_remote(&Remote::unreachable("dead"), &ids, &jobs, &synced, &[]);

        assert_eq!(failures, ["no downloads from dead (2 jobs)"]);
        assert!(synced.into_inner().unwrap().is_empty());
    }
}
