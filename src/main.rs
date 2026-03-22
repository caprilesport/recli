use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{Shell, generate};
use clap_verbosity_flag::LevelFilter;

use crate::config::Config;

mod commands;
mod config;

mod connection;
mod job;
mod jobs;
mod queuemanager;
mod remote;

#[derive(Parser, Debug)]
#[command(author, version, about = "A remote job submission and management CLI.")]
#[command(
    long_about = "recli is a tool designed to simplify the process of submitting, 
    monitoring, and retrieving files from jobs running on remote high-performance 
    computing (HPC) clusters or servers."
)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    mode: Option<Mode>,
    /// Verbosity options.
    #[command(flatten)]
    verbosity: clap_verbosity_flag::Verbosity<clap_verbosity_flag::InfoLevel>,
}

#[derive(Debug, Subcommand)]
enum Mode {
    Fetch(commands::fetch::Args),
    Submit(commands::submit::Args),
    Sync(commands::sync::Args),
    Status(commands::status::Args),
    Info(commands::info::Args),
    /// Generate shell completion scripts
    Completions {
        /// The shell to generate the script for
        shell: Shell,
    },
}

/// Running context of the application
///
/// Holds the config for all the remotes registered, as well as the database for all the submitted jobs
struct Context {
    config: crate::Config,
    db_path: std::path::PathBuf,
}

impl Context {
    fn new() -> anyhow::Result<Self> {
        let config = Config::read()?;
        Ok(Self {
            config,
            db_path: Config::get_dir()?.join("jobs.db"),
        })
    }
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
    tracing_subscriber::fmt().with_max_level(loglevel).init();

    // We match the completions early, as if there is no config file yet we can generate
    // the completions with no problem.
    if let Some(Mode::Completions { shell }) = cli.mode {
        let mut cmd = Cli::command();
        let bin_name = cmd.get_name().to_string();
        generate(shell, &mut cmd, bin_name, &mut std::io::stdout());
        return Ok(());
    }

    let ctx = Context::new()?;

    match cli.mode {
        Some(Mode::Completions { shell: _ }) => unreachable!(),
        Some(Mode::Fetch(args)) => commands::fetch::execute(args, ctx)?,
        Some(Mode::Submit(args)) => commands::submit::execute(args, ctx)?,
        Some(Mode::Sync(args)) => commands::sync::execute(args, ctx)?,
        Some(Mode::Info(args)) => commands::info::execute(args, ctx)?,
        Some(Mode::Status(args)) => commands::status::execute(args, ctx)?,
        None => {
            let args = commands::status::Args::default();
            commands::status::execute(args, ctx)?;
        }
    }

    Ok(())
}
