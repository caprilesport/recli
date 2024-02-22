use serde_derive::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Debug)]
pub struct Worker {
    name: String,
    queue_parser: PathBuf,
}

pub struct WorkerConfig {}
