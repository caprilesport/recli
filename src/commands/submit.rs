use crate::remote_system::connection::SshConnection;
use crate::remote_system::job::Job;
use crate::remote_system::jobs::Jobs;
use std::path::PathBuf;

use tracing::{error, info};

#[derive(Debug, clap::Args)]
/// Submits a job to a specified remote machine.
///
/// This command prepares the necessary job files, uploads them to the remote's
/// working directory, and submis the job to the queue manager.
pub struct Args {
    inpfile: PathBuf,
    #[arg(short, long)]
    remote: String,
}

pub fn execute(args: Args, ctx: crate::Context) -> anyhow::Result<()> {
    let id = uuid::Uuid::new_v4();
    let remote = ctx.config.get_remote(&args.remote)?;
    let file_stem = args
        .inpfile
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Could not extract a valid UTF-8 file stem from the file {:?}",
                args.inpfile
            )
        })?;

    let mut jobs = Jobs::load_jobs(&ctx.json_file)?;
    let internal_id = (jobs.iter().count() + 1) as u16;

    let connection = match SshConnection::new(remote) {
        Ok(sshconnection) => sshconnection,
        Err(err) => {
            error!("Failed to connect to {}, caused by: {}", remote.name(), err);
            return Err(err)?;
        }
    };
    let remote_dir = remote.work_dir().join(id.to_string());
    let remote_id = remote.submit(&args.inpfile, &connection, &remote_dir, &ctx.config.ignore)?;
    let work_dir = std::env::current_dir()?;

    let job = Job::new(
        internal_id,
        id,
        remote.name().to_owned(),
        remote_id,
        file_stem.to_owned(),
        remote_dir,
        work_dir,
    )?;

    info!(
        "Job submitted successfully with id: {}. Remote id: {}",
        id,
        &job.remote_id()
    );

    jobs.add(job);
    jobs.save_jobs(&ctx.json_file)?;
    Ok(())
}
