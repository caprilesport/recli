use crate::job::Job;
use crate::jobs::Jobs;
use chrono::{DateTime, Duration, Utc};

use tracing::{error, info};

/// Removes remote working directories for old, synced jobs.
///
/// By default shows what would be removed without actually deleting anything
/// (dry run). Use --execute to perform the deletion.
///
/// Only jobs whose sync time is older than the configured threshold qualify
/// (default: `prune_after_days` in `[settings]`, or 90 days). Use --job to
/// target a specific job regardless of age.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Target a specific job by UUID prefix, bypassing the age filter
    #[arg(long)]
    job: Option<String>,

    /// Only consider jobs from this remote
    #[arg(long, short)]
    remote: Option<String>,

    /// Minimum age in days since sync (overrides `prune_after_days` in config)
    #[arg(long)]
    days: Option<u32>,

    /// Also remove matching jobs from the local database
    #[arg(long)]
    db: bool,

    /// Actually perform the deletion (default is dry run)
    #[arg(long)]
    execute: bool,
}

#[allow(clippy::needless_pass_by_value)]
pub fn execute(args: Args, ctx: crate::context::Context) -> color_eyre::Result<()> {
    let jobs = Jobs::load_from_db(ctx.db_path())?;
    let days = args.days.unwrap_or(ctx.config().settings.prune_after_days);
    let cutoff = Utc::now() - Duration::days(i64::from(days));

    let to_prune: Vec<Job> = collect_prunable_jobs(args.job, &jobs, args.remote, cutoff)?;

    if to_prune.is_empty() {
        if ctx.json() {
            println!("[]");
        } else {
            info!("No jobs to prune");
        }
        return Ok(());
    }

    if !args.execute {
        // Dry run
        if ctx.json() {
            println!("{}", serde_json::to_string_pretty(&to_prune)?);
        } else {
            println!("Dry run — {} job(s) would be pruned:", to_prune.len());
            for job in &to_prune {
                println!(
                    "  {} ({}) @ {} — remote dir: {}",
                    job.filename(),
                    job.short_id(),
                    job.remote(),
                    job.remote_dir().display()
                );
            }
            println!("Run with --execute to perform deletion.");
        }
        return Ok(());
    }

    // Group by remote to open one connection per remote
    let mut by_remote: std::collections::HashMap<String, Vec<&crate::job::Job>> =
        std::collections::HashMap::new();
    for job in &to_prune {
        by_remote
            .entry(job.remote().to_owned())
            .or_default()
            .push(job);
    }

    for (remote_name, remote_jobs) in &by_remote {
        let remote = ctx.config().get_remote(remote_name)?;
        let connection = match ctx.connect(remote) {
            Ok(c) => c,
            Err(err) => {
                error!("Failed to connect to {}, caused by: {}", remote.name(), err);
                return Err(err)?;
            }
        };

        for job in remote_jobs {
            match connection.remove_dir(job.remote_dir()) {
                Ok(()) => {
                    info!(
                        "Removed remote dir for {} ({})",
                        job.filename(),
                        job.short_id()
                    );
                    if args.db {
                        Jobs::delete_job(ctx.db_path(), job.uuid())?;
                        info!(
                            "Removed {} ({}) from local database",
                            job.filename(),
                            job.short_id()
                        );
                    }
                }
                Err(e) => {
                    error!(
                        "Failed to remove remote dir for {} ({}): {}",
                        job.filename(),
                        job.short_id(),
                        e
                    );
                }
            }
        }
    }

    if ctx.json() {
        println!("{}", serde_json::to_string_pretty(&to_prune)?);
    }

    Ok(())
}

fn collect_prunable_jobs(
    job: Option<String>,
    jobs: &Jobs,
    remote: Option<String>,
    cutoff: DateTime<Utc>,
) -> color_eyre::Result<Vec<Job>> {
    let to_prune = if let Some(prefix) = job {
        let job = jobs.find_by_prefix(&prefix)?;
        if let Some(remote_filter) = remote
            && job.remote() != remote_filter
        {
            return Err(color_eyre::eyre::eyre!(
                "Job {} is on remote '{}', not '{}'",
                job.short_id(),
                job.remote(),
                remote_filter
            ));
        }
        vec![job.clone()]
    } else {
        let mut query = jobs.query().synced(true);
        if let Some(remote_filter) = &remote {
            query = query.with_remote(remote_filter);
        }
        query
            .iter()
            .filter(|j| j.sync_time().is_some_and(|t| *t < cutoff))
            .cloned()
            .collect()
    };

    Ok(to_prune)
}
