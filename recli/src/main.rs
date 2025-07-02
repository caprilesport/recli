use clap::{Parser, Subcommand};
use clap_verbosity_flag::LevelFilter;

use crate::config::Config;

mod commands;
mod config;

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
    Sync {
        job_id: Option<uuid::Uuid>,
        #[arg(short, long)]
        update_status: bool,
    },
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

    let config = Config::read()?;
    let json_file = Config::get_dir()?.join("jobs.json");

    match cli.mode {
        Mode::Submit { inpfile, remote } => {
            let id = uuid::Uuid::new_v4();
            let remote = config.get_remote(&remote)?;
            let file_stem = inpfile.file_stem().unwrap().to_str().unwrap();

            let connection = SshConnection::new(&remote)?;
            let remote_id = remote.submit(id, &inpfile, &connection)?;

            let job = Job::new(
                id,
                remote.name().to_owned(),
                remote_id,
                file_stem.to_owned(),
            )?;
            let mut jobs = Jobs::load_jobs(&json_file)?;

            info!(
                "Job submitted successfully with id: {}. Remote id: {}",
                id,
                &job.remote_id()
            );

            jobs.add(job);
            jobs.save_jobs(&json_file)?;
        }

        Mode::Fetch => {
            let mut jobs = Jobs::load_jobs(&json_file)?;

            for remote in config.remotes {
                let connection = SshConnection::new(&remote)?;
                let statuses = remote.status(&connection)?;
                jobs.update(statuses, remote.name());
            }

            jobs.save_jobs(&json_file)?;
        }
        Mode::Status => {
            let jobs = Jobs::load_jobs(&json_file)?;
            let table = jobs.create_status_table();
            println!("{}", table);
        }
        Mode::Sync {
            job_id,
            update_status,
        } => {
            let mut jobs = Jobs::load_jobs(&json_file)?;

            match job_id {
                Some(id) => {
                    if let Some(job) = jobs.find_by_id(&id) {
                        let remote = config.get_remote(job.remote())?;
                        let connection = SshConnection::new(&remote)?;
                        jobs.sync_job(&id, &remote, &connection, update_status)?;
                    } else {
                        return Err(anyhow::anyhow!(remotelib::job::Error::JobNotFound(id)));
                    }
                }
                None => {
                    let syncable_jobs = jobs.syncable();

                    if syncable_jobs.is_empty() {
                        info!("No jobs to sync")
                    } else {
                        for (remote_name, ids) in syncable_jobs {
                            let remote = config.get_remote(&remote_name)?;
                            let connection = SshConnection::new(&remote)?;
                            for id in ids {
                                jobs.sync_job(&id, &remote, &connection, update_status)?;
                            }
                        }
                    }
                }
            }
        }
    };

    Ok(())
}
