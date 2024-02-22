use crate::worker::Worker;
use serde_derive::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct RecliConfig {
    workers: Vec<Worker>,
}
