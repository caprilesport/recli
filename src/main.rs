#![allow(dead_code, unused_variables, unused_imports)]
use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};

mod config;
mod server;
mod worker;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
#[command(arg_required_else_help = true)]
struct Cli {
    #[command(subcommand)]
    mode: Option<Mode>,
    /// Verbosity options.
    #[clap(flatten)]
    verbosity: clap_verbosity_flag::Verbosity,
}

#[derive(Debug, Subcommand)]
enum Mode {
    /// Submits a job in the specified remote
    Job {
        #[command(subcommand)]
        subcommand: JobSubcommand,
    },
    Server {
        #[command(subcommand)]
        subcommand: ServerSubcommand,
    },
    Worker {
        #[command(subcommand)]
        subcommand: WorkerSubcommand,
    },
    Submit,
}

#[derive(Debug, Subcommand)]
enum WorkerSubcommand {
    Start { worker: String },
    List,
}

#[derive(Debug, Subcommand)]
enum ServerSubcommand {
    Start,
    Stop,
}

#[derive(Debug, Subcommand)]
enum JobSubcommand {
    List,
    Retrieve,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // match cli.mode {
    //     Mode::Sub { remote, inpfile } => {
    //         submit_job(remote, inpfile);
    //     }
    // };

    Ok(())
}
