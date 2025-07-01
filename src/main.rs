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
    /// Submits a file at the specified remote
    Submit {
        inpfile: PathBuf,
        #[arg(short, long)]
        remote: String,
    },
    /// Fetch all job changes from the remotes
    Fetch,
    /// Downloads all files for finished jobs
    Sync { job_id: Option<uuid::Uuid> },
    /// Displays the status of jobs
    Status,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Initialize the logger with the verbosity level from the CLI.
    let loglevel = match cli.verbosity.log_level_filter() {
        LevelFilter::Off => tracing_subscriber::filter::LevelFilter::OFF,
        LevelFilter::Warn => tracing_subscriber::filter::LevelFilter::WARN,
        LevelFilter::Error => tracing_subscriber::filter::LevelFilter::ERROR,
        LevelFilter::Info => tracing_subscriber::filter::LevelFilter::INFO,
        LevelFilter::Trace => tracing_subscriber::filter::LevelFilter::TRACE,
        LevelFilter::Debug => tracing_subscriber::filter::LevelFilter::DEBUG,
    };
    tracing_subscriber::fmt().with_max_level(loglevel).init();

    let config = crate::config::Config::read()?;

    match cli.mode {
        Mode::Submit { inpfile, remote } => {
            let id = uuid::Uuid::new_v4();
            let remote = config.get_remote(&remote)?;
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
            let mut jobs = Jobs::load_jobs()?;

            for remote in config.remotes {
                let connection = SshConnection::new(&remote)?;
                let statuses = remote.status(&connection)?;
                jobs.update(statuses);
            }

            jobs.save_jobs()?;
        }
        Mode::Status => {
            let jobs = Jobs::load_jobs()?;
            let table = jobs.create_status_table();
            println!("{}", table);
        }
        Mode::Sync { job_id } => {
            unimplemented!()
        }
    };

    Ok(())
}
