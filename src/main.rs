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
#[command(version, author, about)]
enum Mode {
    /// pushes a file to the remote, if --all is given, push all files in the current dir
    Push {
        remote: String,
    },
    Pull {
        remote: String,
    },
    Sub {
        remote: String,
        inpfile: String,
    },
}

fn get_config() -> Result<RecliConfig, confy::ConfyError> {
    let cfg: RecliConfig = confy::load("recli", "config")?;
    Ok(cfg)
}

fn remote_push(remote: String, cfg: RecliConfig) {
    println!("pushing all files to {}", remote)
}

fn remote_pull(remote: String, cfg: RecliConfig) {
    println!("pulling all files to {}", remote)
}

fn submit_job(remote: String, inpfile: String, cfg: RecliConfig) {
    println!("submitting job {} at {}", inpfile, remote)
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = get_config()?;

    match cli.mode {
        Mode::Push { remote } => {
            remote_push(remote, cfg);
        }
        Mode::Pull { remote } => {
            remote_pull(remote, cfg);
        }
        Mode::Sub { remote, inpfile } => {
            submit_job(remote, inpfile, cfg);
        }
    };

    Ok(())
}
