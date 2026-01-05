use crate::connection::SshConnection;
use crate::job::Job;
use crate::jobs::Jobs;
use crate::manifest::JobManifest;
use std::path::PathBuf;

use tracing::{error, info};

#[derive(Debug, clap::Args)]
/// Submits a job to a specified remote machine.
pub struct Args {
    input_file: PathBuf,
}

// TODO: add debug information
pub fn execute(args: Args, ctx: crate::Context) -> anyhow::Result<()> {
    let manifest = JobManifest::from_file(&args.input_file)?;
    let files_to_send = manifest.build()?;
    let job_file = format!("{}.job", &manifest.spec.name);

    let current_dir = std::env::current_dir()?;
    let manifest_path = current_dir.clone().join(&args.input_file);

    let remote = ctx.config.get_remote(&manifest.remote)?;
    let connection = match SshConnection::new(remote) {
        Ok(sshconnection) => sshconnection,
        Err(err) => {
            error!("Failed to connect to {}, caused by: {}", remote.name(), err);
            return Err(err)?;
        }
    };
    let id = uuid::Uuid::new_v4();
    let remote_dir = remote.work_dir().join(id.to_string());
    let remote_id = remote.submit(&job_file, &connection, &remote_dir, files_to_send)?;

    let mut jobs = Jobs::load_jobs(&ctx.json_file)?;
    let internal_id = (jobs.iter().count() + 1) as u16;

    let job = Job::new(
        internal_id,
        id,
        remote_id,
        remote_dir,
        current_dir,
        manifest,
        manifest_path,
    )?;

    info!(
        "Job {} with id: {} and remote id: {} submitted successfully ",
        job.name(),
        job.id(),
        job.remote_id()
    );

    jobs.add(job);
    jobs.save_jobs(&ctx.json_file)?;
    Ok(())
}
