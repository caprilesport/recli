#![allow(unused_variables, unused_imports)]
use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use recli::get_config;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    mode: Mode,
    /// Verbosity options.
    #[clap(flatten)]
    verbosity: clap_verbosity_flag::Verbosity,
}

#[derive(Debug, Subcommand)]
enum Mode {
    /// generates a .gedent.toml file with configurations to be used
    /// in the current project, options are a file to be used as default, if none
    /// is provided, use the default in ~/.config/gedent
    Push { remote: String },
    ///Generate a new input based on a template and a xyz file
    Pull { remote: String },
    /// Submits a job in the specified remote
    Sub { remote: String, inpfile: String },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = get_config()?;

    match cli.mode {
        Mode::Push { remote } => {
            remote_push(remote);
        }
        Mode::Pull { remote } => {
            remote_pull(remote);
        }
        Mode::Sub { remote, inpfile } => {
            submit_job(remote, inpfile);
        }
    };

    Ok(())
}

fn remote_push(remote: String) {
    println!("{}", remote)
}

fn remote_pull(remote: String) {
    println!("generating input")
}

fn submit_job(remote: String, inpfile: String) {
    println!("generating new template")
}
