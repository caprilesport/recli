use clap::{Parser, Subcommand};
use clap_verbosity_flag::LevelFilter;

use crate::config::Config;

mod commands;
mod config;

#[derive(Parser, Debug)]
#[command(author, version, about = "A remote job submission and management CLI.")]
#[command(
    long_about = "recli is a tool designed to simplify the process of submitting, 
    monitoring, and retrieving files from jobs running on remote high-performance 
    computing (HPC) clusters or servers."
)]
#[command(propagate_version = true)]
#[command(arg_required_else_help = true)]
struct Cli {
    #[command(subcommand)]
    mode: Mode,
    /// Verbosity options.
    #[command(flatten)]
    verbosity: clap_verbosity_flag::Verbosity<clap_verbosity_flag::InfoLevel>,
    /// Sync all files, ignoring the ignore file
    #[arg(long, global = true, default_value_t = false)]
    sync_all_files: bool,
}

#[derive(Debug, Subcommand)]
enum Mode {
    Fetch(commands::fetch::Args),
    Submit(commands::submit::Args),
    Sync(commands::sync::Args),
    Status(commands::status::Args),
}

/// Running context of the application
///
/// Holds the config for all the remotes registered, as well as the a the database for all the submitted jobs
struct Context {
    config: crate::Config,
    json_file: std::path::PathBuf,
}

impl Context {
    fn new(cli: &Cli) -> anyhow::Result<Self> {
        let mut config = Config::read()?;
        if cli.sync_all_files {
            config.ignore.clear();
        }
        Ok(Self {
            config,
            json_file: Config::get_dir()?.join("jobs.json"),
        })
    }
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

    let ctx = Context::new(&cli)?;

    match cli.mode {
        Mode::Fetch(args) => commands::fetch::execute(args, &ctx)?,
        Mode::Submit(args) => commands::submit::execute(args, &ctx)?,
        Mode::Status(args) => commands::status::execute(args, &ctx)?,
        Mode::Sync(args) => commands::sync::execute(args, &ctx)?,
    };

    Ok(())
}
