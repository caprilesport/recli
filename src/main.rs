use clap::{Parser, Subcommand};
use clap_verbosity_flag::LevelFilter;
use tracing::info;

use std::path::PathBuf;
use tabled::builder::Builder;

use connection::SshConnection;
use job::{Job, JobStatus};

mod config;
mod connection;
mod job;
mod queuemanager;
mod remote;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
#[command(arg_required_else_help = true)]
struct Cli {
    #[command(subcommand)]
    mode: Mode,
    /// Verbosity options.
    #[command(flatten)]
    verbosity: clap_verbosity_flag::Verbosity<clap_verbosity_flag::InfoLevel>,
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

    let loglevel = match cli.verbosity.log_level_filter() {
        LevelFilter::Off => tracing_subscriber::filter::LevelFilter::OFF,
        LevelFilter::Warn => tracing_subscriber::filter::LevelFilter::WARN,
        LevelFilter::Error => tracing_subscriber::filter::LevelFilter::ERROR,
        LevelFilter::Info => tracing_subscriber::filter::LevelFilter::INFO,
        LevelFilter::Trace => tracing_subscriber::filter::LevelFilter::TRACE,
        LevelFilter::Debug => tracing_subscriber::filter::LevelFilter::DEBUG,
    };

    // Initialize the logger with the verbosity level from the CLI.
    tracing_subscriber::fmt().with_max_level(loglevel).init();

    let config = crate::config::Config::read()?;

    match cli.mode {
        Mode::Submit { inpfile, remote } => {
            let id = uuid::Uuid::new_v4();
            let remote = config.get_remote(&remote);
            let connection = SshConnection::new(&remote)?;

            let job = remote.submit(id, &inpfile, &connection)?;

            let mut jobs = Job::load_jobs()?;
            jobs.push(job);
            Job::save_jobs(&jobs)?;

            info!(
                "Job submitted successfully with id: {}. Remote id: {}",
                id,
                jobs.last().unwrap().remote_id()
            );
        }

        Mode::Fetch => {
            let mut jobs = Job::load_jobs()?;
            let mut changed_jobs = Vec::new();

            for remote in config.remotes {
                let connection = SshConnection::new(&remote)?;
                let statuses = remote.status(&connection)?;
                for job in jobs.iter_mut() {
                    let old_status = job.status().clone();
                    let status = statuses.get(job.remote_id());

                    match status {
                        Some(st) => job.set_status(st.to_owned()),
                        None => (),
                    }
                    if job.status() != &old_status {
                        changed_jobs.push(job.clone());
                    }
                }
            }

            Job::save_jobs(&jobs)?;

            if changed_jobs.is_empty() {
                info!("No job status changes.");
            } else {
                info!("Jobs with status changes:");
                for job in changed_jobs {
                    info!("  - Job {}: changed to {:?}", job.id(), job.status());
                }
            }
        }
        Mode::Status => {
            let jobs = Job::load_jobs()?;
            let table = create_status_table(jobs);
            println!("{}", table);
        }
        Mode::Sync { job_id } => {
            let mut jobs = Job::load_jobs()?;
            let mut synced_jobs_count = 0;

            // If a specific job_id is provided, sync only that job
            if let Some(id) = job_id {
                // Filter out the jobs
                let job = jobs
                    .iter_mut()
                    .filter(|j| j.id().to_string() == id)
                    .next()
                    .unwrap();
                let remote = config.get_remote(job.remote());
                let connection = SshConnection::new(&remote)?;

                remote.sync(&job, &connection)?;
            } else {
                for job in jobs.iter_mut() {
                    if job.status() == &JobStatus::Finished
                        || job.status() == &JobStatus::Error && !job.synced()
                    {
                        // If no job_id is provided, sync all unsynced and finished jobs
                        let remote = config.clone().get_remote(job.remote());
                        let connection = SshConnection::new(&remote)?;
                        remote.sync(&job, &connection)?;
                        job.set_synced_status(true);
                        synced_jobs_count += 1;
                    }
                }
                if synced_jobs_count > 0 {
                    info!("Successfully synced {} job(s).", synced_jobs_count);
                } else {
                    info!("No finished jobs to sync.");
                }
            }

            Job::save_jobs(&jobs)?;
        }
    };

    Ok(())
}

fn create_status_table(jobs: Vec<Job>) -> String {
    let mut builder = Builder::default();
    builder.push_record(["Name", "Project", "St", "Synced", "Remote", "Remote ID"]);

    jobs.iter().for_each(|j| {
        builder.push_record(vec![
            j.name(),
            j.project(),
            j.status().as_str(),
            &j.synced().to_string(),
            j.remote(),
            j.remote_id(),
        ])
    });

    let mut table = builder.build();
    table.with(tabled::settings::Style::rounded());
    table.to_string()
}
