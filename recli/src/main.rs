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
    Submit(commands::submit::Args),
    /// Fetch all job changes from the remotes
    Fetch,
    /// Downloads all files for finished jobs
    Sync(commands::sync::Args),
    /// Displays the status of jobs with optional filters
    Status(commands::status::Args),
}

struct Context {
    config: crate::Config,
    json_file: std::path::PathBuf,
}

impl Context {
    fn new() -> anyhow::Result<Self> {
        Ok(Self {
            config: Config::read()?,
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

    let ctx = Context::new()?;

    match cli.mode {
        Mode::Submit(args) => commands::submit::execute(args, &ctx)?,
        Mode::Fetch => commands::fetch::execute(&ctx)?,
        Mode::Status(args) => commands::status::execute(args, &ctx)?,
        Mode::Sync(args) => commands::sync::execute(args, &ctx)?,
    };

    Ok(())
}
