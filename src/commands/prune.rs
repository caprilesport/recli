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
        if ctx.json_output() {
            println!("[]");
        } else {
            info!("No jobs to prune");
        }
        return Ok(());
    }

    if !args.execute {
        // Dry run
        if ctx.json_output() {
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

    if ctx.json_output() {
        println!("{}", serde_json::to_string_pretty(&to_prune)?);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::test_support::MockFactory;
    use crate::context::Context;
    use crate::job::{Job, JobStatus};
    use crate::jobs::Jobs;
    use chrono::Utc;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn make_context(db_path: PathBuf, factory: MockFactory) -> Context {
        let config: crate::config::Config = toml::from_str(
            r#"
            [[remotes]]
            name = "babel"
            hostname = "localhost"
            port = 22
            user = "testuser"
            work_directory = "/remote/work"
            queue_manager = "Pbs"
            "#,
        )
        .unwrap();
        Context::with_factory(config, db_path, Box::new(factory))
    }

    fn synced_job(remote: &str, synced_days_ago: i64) -> Job {
        let sync_time = Utc::now() - Duration::days(synced_days_ago);
        Job::new(
            Uuid::new_v4(),
            remote.to_string(),
            "1".to_string(),
            "myjob".to_string(),
            PathBuf::from("myjob.pbs"),
            vec![],
            vec![],
            None,
            PathBuf::from("/tmp"),
            PathBuf::from("/remote/work/uuid"),
            JobStatus::Finished,
            Utc::now() - Duration::days(synced_days_ago + 1),
            Some(sync_time),
        )
    }

    fn temp_db() -> (tempfile::NamedTempFile, PathBuf) {
        let f = tempfile::NamedTempFile::new().unwrap();
        let p = f.path().to_path_buf();
        (f, p)
    }

    #[test]
    fn test_collect_by_prefix_finds_job() {
        let (_f, path) = temp_db();
        let job = synced_job("babel", 100);
        Jobs::insert_job(&path, &job).unwrap();
        let jobs = Jobs::load_from_db(&path).unwrap();

        let prefix = &job.uuid().to_string()[..7];
        let result = collect_prunable_jobs(
            Some(prefix.to_string()),
            &jobs,
            None,
            Utc::now() - Duration::days(90),
        )
        .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].uuid(), job.uuid());
    }

    #[test]
    fn test_collect_respects_cutoff() {
        let (_f, path) = temp_db();
        let old_job = synced_job("babel", 100);
        let new_job = synced_job("babel", 10);
        Jobs::insert_job(&path, &old_job).unwrap();
        Jobs::insert_job(&path, &new_job).unwrap();
        let jobs = Jobs::load_from_db(&path).unwrap();

        let result =
            collect_prunable_jobs(None, &jobs, None, Utc::now() - Duration::days(90)).unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].uuid(), old_job.uuid());
    }

    #[test]
    fn test_collect_filters_by_remote() {
        let (_f, path) = temp_db();
        Jobs::insert_job(&path, &synced_job("babel", 100)).unwrap();
        Jobs::insert_job(&path, &synced_job("newton", 100)).unwrap();
        let jobs = Jobs::load_from_db(&path).unwrap();

        let result = collect_prunable_jobs(
            None,
            &jobs,
            Some("babel".to_string()),
            Utc::now() - Duration::days(90),
        )
        .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].remote(), "babel");
    }

    #[test]
    fn test_collect_prefix_wrong_remote_errors() {
        let (_f, path) = temp_db();
        let job = synced_job("babel", 100);
        Jobs::insert_job(&path, &job).unwrap();
        let jobs = Jobs::load_from_db(&path).unwrap();

        let prefix = job.uuid().to_string()[..7].to_string();
        let result = collect_prunable_jobs(
            Some(prefix),
            &jobs,
            Some("newton".to_string()),
            Utc::now() - Duration::days(90),
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_execute_removes_remote_dir() {
        let (_f, db_path) = temp_db();
        let job = synced_job("babel", 100);
        Jobs::insert_job(&db_path, &job).unwrap();

        let prefix = job.uuid().to_string()[..7].to_string();
        let (factory, state) = MockFactory::new();

        execute(
            Args {
                job: Some(prefix),
                remote: None,
                days: None,
                db: false,
                execute: true,
            },
            make_context(db_path.clone(), factory),
        )
        .unwrap();

        let removals = &state.lock().unwrap().removals;
        assert_eq!(removals.len(), 1);
        assert_eq!(removals[0], *job.remote_dir());
    }

    #[test]
    fn test_execute_with_db_removes_job_from_db() {
        let (_f, db_path) = temp_db();
        let job = synced_job("babel", 100);
        Jobs::insert_job(&db_path, &job).unwrap();

        let prefix = job.uuid().to_string()[..7].to_string();
        let (factory, _state) = MockFactory::new();

        execute(
            Args {
                job: Some(prefix),
                remote: None,
                days: None,
                db: true,
                execute: true,
            },
            make_context(db_path.clone(), factory),
        )
        .unwrap();

        let jobs = Jobs::load_from_db(&db_path).unwrap();
        assert!(jobs.is_empty());
    }

    #[test]
    fn test_dry_run_does_not_remove_dir() {
        let (_f, db_path) = temp_db();
        let job = synced_job("babel", 100);
        Jobs::insert_job(&db_path, &job).unwrap();

        let prefix = job.uuid().to_string()[..7].to_string();
        let (factory, state) = MockFactory::new();

        execute(
            Args {
                job: Some(prefix),
                remote: None,
                days: None,
                db: false,
                execute: false,
            },
            make_context(db_path.clone(), factory),
        )
        .unwrap();

        assert!(state.lock().unwrap().removals.is_empty());
        let jobs = Jobs::load_from_db(&db_path).unwrap();
        assert_eq!(jobs.len(), 1);
    }
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
