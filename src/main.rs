#![allow(dead_code, unused_variables, unused_imports)]
use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use openssh::{KnownHosts, Session};

use std::path::PathBuf;

mod config;

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
    Submit { inpfile: PathBuf, remote: String },
    // Check,
    // Sync,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.mode {
        Mode::Submit { remote, inpfile } => test_ssh().await?,
        // Mode::Sync => println!("Syncing").await?,
        // Mode::Check => println!("Checking"),
    };

    Ok(())
}

async fn test_ssh() -> Result<()> {
    let session = Session::connect("vport@jupiter", KnownHosts::Strict).await?;

    let ls = session.command("ls").output().await?;
    println!("{}", String::from_utf8(ls.stdout)?);

    let whoami = session.command("pwd").output().await?;

    println!("{}", String::from_utf8(whoami.stdout)?);

    session.close().await?;

    Ok(())
}
