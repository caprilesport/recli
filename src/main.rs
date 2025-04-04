#![allow(dead_code, unused_variables, unused_imports)]
use anyhow::{anyhow, Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use openssh::{KnownHosts, Session};
use remote::Remote;
use serde::{Deserialize, Serialize};
use std::borrow::Borrow;
use std::process::Command;

use std::path::PathBuf;

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
    // Submit { inpfile: PathBuf, remote: String },
    /// Pulls the files in the respective remote
    Pull {
        remote: String,
        #[arg(short, long)]
        sync: bool,
    },
    Push {
        remote: String,
        #[arg(short, long)]
        sync: bool,
    },
    // Diff,
    // Sync,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = crate::config::Config::read_config()?;

    match cli.mode {
        // Mode::Sync => println!("Syncing").await?,
        // Mode::Check => println!("Checking"),
        // Mode::Submit { remote, inpfile } => test_ssh().await?,
        Mode::Pull { remote, sync } => {
            let cwd = std::env::current_dir()?;

            let remote = match find_remote(remote, config.remotes) {
                Some(remote) => remote,
                None => panic!("Unable to find remote in the config"),
            };

            let push_cmd = remote.push(cwd, config.local, sync).await?;
            push_cmd.run()?;
        }
        Mode::Push { remote, sync } => {
            let cwd = std::env::current_dir()?;

            let remote = match find_remote(remote, config.remotes) {
                Some(remote) => remote,
                None => panic!("Unable to find remote in the config"),
            };

            let pull_cmd = remote.pull(cwd, config.local, sync).await?;
            pull_cmd.run()?;
        }
    };

    Ok(())
}

fn find_remote(name: String, remotes: Vec<Remote>) -> Option<Remote> {
    return remotes.into_iter().filter(|r| r.name() == name).next();
}

// async fn test_ssh(remote: remote::Remote, config: config::Config) -> Result<()> {
// }
