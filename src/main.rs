#![allow(dead_code, unused_variables, unused_imports)]
use anyhow::{anyhow, Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use std::borrow::Borrow;
use std::process::Command;
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
        job_id: Option<i64>,
    },
    Status,
    Init,
}
fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = crate::config::Config::read_config()?;
    match cli.mode {
        Mode::Submit { inpfile, remote } => {
            let id = uuid::Uuid::new_v4();
            let remote = config.get_remote(&remote);
        }
        Mode::Fetch => {
        }
        Mode::Status => {
        }
        Mode::Sync { job_id } => {
        }
        Mode::Init => println!("Create a .recli file"),
    };
    Ok(())
}
