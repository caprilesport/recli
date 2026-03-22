use crate::connection::SshConnection;
use crate::job::Job;
use crate::jobs::Jobs;
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

#[allow(clippy::needless_pass_by_value)]
pub fn execute(args: Args, ctx: crate::Context) -> anyhow::Result<()> {
    let id = uuid::Uuid::new_v4();
    let remote = ctx.config.get_remote(&args.remote)?;
    let file_stem = args
        .inpfile
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Could not extract a valid UTF-8 file stem from the file {}",
                args.inpfile.display()
            )
        })?;

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
        id,
        remote.name().to_owned(),
        remote_id,
        file_stem.to_owned(),
        remote_dir,
        work_dir,
    );

    info!(
        "Job {} ({}) with remote id: {} submitted successfully",
        job.filename(),
        job.short_id(),
        job.remote_id()
    );

    Jobs::insert_job(&ctx.db_path, &job)?;
    Ok(())
}
