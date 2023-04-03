#![allow(unused_variables, unused_imports)]
use std::collections::HashMap;

use anyhow::{Context, Result};
use serde_derive::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct RecliConfig {
    app: AppSettings,
    remotes: HashMap<String, RemoteLocationSettings>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AppSettings {
    local_folder: String,
    // script_folder: String,
    // parser_folder: String,
}

//todo implement a queue script location for submitting and a parser for trating queue commands
#[derive(Serialize, Deserialize, Debug)]
pub struct RemoteLocationSettings {
    name: String,
    host: String,
    user: String,
    port: String,
    // queue_script: String,
    remote_folder: String,
}

impl ::std::default::Default for RecliConfig {
    fn default() -> Self {
        Self {
            app: AppSettings {
                local_folder: (String::from("~/projects/")),
                // script_folder: (String::from("~/.config/recli/scripts")),
                // parser_folder: (String::from("~/.config/recli/parsers")),
            },
            remotes: HashMap::new(),
        }
    }
}

pub fn get_config() -> Result<RecliConfig, confy::ConfyError> {
    let cfg: RecliConfig = confy::load("recli", "config")?;
    Ok(cfg)
}
