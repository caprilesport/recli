use clap::{Parser, Subcommand};

use chrono::{DateTime, Local};
use std::path::PathBuf;
use tabled::builder::Builder;

mod config;
mod remote;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
#[command(arg_required_else_help = true)]
struct Cli {
    #[command(subcommand)]
    mode: Mode,
    /// Verbosity options.
    #[clap(flatten)]
    verbosity: clap_verbosity_flag::Verbosity,
}

#[derive(Debug, Subcommand)]
enum Mode {
    Submit {
        inpfile: PathBuf,
        #[arg(short, long)]
        remote: String,
    },
    Fetch,
    Sync {
        job_id: Option<String>,
    },
    Status,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = crate::config::Config::read_config()?;

    match cli.mode {
        Mode::Submit { inpfile, remote } => {
            let id = uuid::Uuid::new_v4();
            let remote = config.get_remote(&remote);

            remote.prepare(&inpfile)?;
            let job = remote.submit(id, &inpfile)?;

            let mut jobs = remote::Job::load_jobs()?;
            jobs.push(job);
            remote::Job::save_jobs(&jobs)?;

            println!(
                "Job submitted successfully with id: {}. Remote id: {}",
                id,
                jobs.last().unwrap().remote_id()
            );
        }
        Mode::Fetch => {
            let mut jobs = remote::Job::load_jobs()?;
            let mut changed_jobs = Vec::new();

            for remote in config.remotes {
                let statuses = remote.status()?;
                for job in jobs.iter_mut() {
                    let old_status = job.status().clone();
                    job.update_status(&statuses);
                    if job.status() != &old_status {
                        changed_jobs.push(job.clone());
                    }
                }
            }

            remote::Job::save_jobs(&jobs)?;

            if changed_jobs.is_empty() {
                println!("No job status changes.");
            } else {
                println!("Jobs with status changes:");
                for job in changed_jobs {
                    println!("  - Job {}: changed to {:?}", job.id(), job.status());
                }
            }
        }
        Mode::Status => {
            let jobs = remote::Job::load_jobs()?;

            let mut builder = Builder::default();
            builder.push_record([
                "Working Dir",
                "Status",
                "Synced",
                "Submit Time",
                "Project",
                "Remote",
                "Remote ID",
                "ID",
            ]);

            for job in jobs {
                let submit_time_str = if let Ok(timestamp) = job.submit_time().parse::<i64>() {
                    if let Some(datetime) = DateTime::from_timestamp(timestamp, 0) {
                        datetime
                            .with_timezone(&Local)
                            .format("%Y-%m-%d %H:%M:%S")
                            .to_string()
                    } else {
                        "Invalid Timestamp".to_string()
                    }
                } else {
                    "N/A".to_string()
                };

                builder.push_record(vec![
                    job.working_dir().to_string_lossy().into_owned(),
                    format!("{:?}", job.status()),
                    job.synced().to_string(),
                    submit_time_str,
                    job.project().to_owned(),
                    job.remote().to_string(),
                    job.remote_id().to_string(),
                    job.id().to_string(),
                ]);
            }

            let mut table = builder.build();
            table.with(tabled::settings::Style::rounded());
            println!("{table}");
        }
        Mode::Sync { job_id } => {
            let mut jobs = remote::Job::load_jobs()?;
            let mut synced_jobs_count = 0;

            // If a specific job_id is provided, sync only that job
            if let Some(id) = job_id {
                // Filter out the jobs
                let job = jobs
                    .iter_mut()
                    .filter(|j| j.id().to_string() == id)
                    .next()
                    .unwrap();
                let remote_config = config.clone().get_remote(job.remote());

                // job.set_sync_status(true);
                job.sync(&remote_config)?;
            } else {
                for job in jobs.iter_mut() {
                    if job.status() == &remote::JobStatus::Finished
                        || job.status() == &remote::JobStatus::Error && !job.synced()
                    {
                        // If no job_id is provided, sync all unsynced and finished jobs
                        let remote_config = config.clone().get_remote(job.remote());
                        job.sync(&remote_config)?;
                        job.set_synced_status(true);
                        synced_jobs_count += 1;
                    }
                }
                if synced_jobs_count > 0 {
                    println!("Successfully synced {} job(s).", synced_jobs_count);
                } else {
                    println!("No finished jobs to sync.");
                }
            }

            remote::Job::save_jobs(&jobs)?;
        }
    };

    Ok(())
}
