#![allow(unused_variables, unused_imports)]
use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug)]
struct RecliConfig {
    app: AppSettings,
    remotes: HashMap<String, RemoteLocationSettings>,
}

#[derive(Serialize, Deserialize, Debug)]
struct AppSettings {
    local_folder: String,
    script_folder: String,
    parser_folder: String,
}

//todo implement a queue script location for submitting and a parser for trating queue commands
#[derive(Serialize, Deserialize, Debug)]
struct RemoteLocationSettings {
    host: String,
    user: String,
    port: String,
    queue_script: String,
    parser: String,
    remote_folder: String,
}

impl ::std::default::Default for RecliConfig {
    fn default() -> Self {
        let a = String::from("a");
        Self {
            app: AppSettings {
                local_folder: (String::from("~/projects/")),
                script_folder: (String::from("~/.config/recli/scripts")),
                parser_folder: (String::from("~/.config/recli/parsers")),
            },
            remotes: HashMap::from([(
                String::from("example"),
                RemoteLocationSettings {
                    host: String::from("host"),
                    user: String::from("exampleuser"),
                    port: String::from("22"),
                    queue_script: String::from("example_qprep.py"),
                    parser: String::from("PBS.py"),
                    remote_folder: String::from("/mnt/data/exampleuser/projects"),
                },
            )]),
        }
    }
}

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
