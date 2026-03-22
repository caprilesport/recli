use clap::builder::styling::{AnsiColor, Effects};
use clap::{CommandFactory, Parser, Subcommand};

const STYLES: clap::builder::Styles = clap::builder::Styles::styled()
    .header(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .usage(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .literal(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
    .placeholder(AnsiColor::Cyan.on_default())
    .error(AnsiColor::Red.on_default().effects(Effects::BOLD))
    .valid(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
    .invalid(AnsiColor::Yellow.on_default().effects(Effects::BOLD));
use clap_complete::{Shell, generate};
use clap_verbosity_flag::LevelFilter;

use crate::config::Config;

mod build_info {
    #![allow(clippy::format_push_string)]
    include!(env!("BOSION_PATH"));
}
use build_info::Bosion;

mod commands;
mod config;

mod connection;
mod job;
mod jobs;
mod queuemanager;
mod remote;

#[derive(Parser, Debug)]
#[command(author, version, long_version = Bosion::LONG_VERSION, styles = STYLES, about = "A remote job submission and management CLI.")]
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
    /// Output as JSON
    #[arg(long, global = true, action)]
    json: bool,
}

#[derive(Debug, Subcommand)]
enum Mode {
    Fetch(commands::fetch::Args),
    Submit(commands::submit::Args),
    Pull(commands::pull::Args),
    Status(commands::status::Args),
    Info(commands::info::Args),
    Cancel(commands::cancel::Args),
    Prune(commands::prune::Args),
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
    json: bool,
}

impl Context {
    fn new(json: bool) -> color_eyre::Result<Self> {
        let config = Config::read()?;
        Ok(Self {
            config,
            db_path: Config::get_dir()?.join("jobs.db"),
            json,
        })
    }
}

fn main() -> color_eyre::Result<()> {
    color_eyre::config::HookBuilder::default()
        .add_issue_metadata("binary", env!("CARGO_BIN_NAME"))
        .add_issue_metadata("version", Bosion::CRATE_VERSION)
        .add_issue_metadata("build-date", Bosion::BUILD_DATETIME)
        .add_issue_metadata("target", env!("TARGET"))
        .add_issue_metadata("features", Bosion::CRATE_FEATURE_STRING)
        .install()?;
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

    let ctx = Context::new(cli.json)?;

    match cli.mode {
        Some(Mode::Completions { shell: _ }) => unreachable!(),
        Some(Mode::Fetch(args)) => commands::fetch::execute(args, ctx)?,
        Some(Mode::Submit(args)) => commands::submit::execute(args, ctx)?,
        Some(Mode::Pull(args)) => commands::pull::execute(args, ctx)?,
        Some(Mode::Info(args)) => commands::info::execute(args, ctx)?,
        Some(Mode::Status(args)) => commands::status::execute(args, ctx)?,
        Some(Mode::Cancel(args)) => commands::cancel::execute(args, ctx)?,
        Some(Mode::Prune(args)) => commands::prune::execute(args, ctx)?,
        None => {
            let args = commands::status::Args::default();
            commands::status::execute(args, ctx)?;
        }
    }

    Ok(())
}
