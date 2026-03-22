use crate::connection::SshConnection;
use crate::job::Job;
use crate::jobs::Jobs;
use std::path::PathBuf;

use tracing::{error, info};

#[derive(Debug, clap::Args)]
/// Submits a job to a specified remote machine.
///
/// Uploads the job script (and any extra files via --files) to the remote's
/// working directory and submits it to the queue manager.
pub struct Args {
    /// Job script to submit (.pbs, .slurm, etc.)
    inpfile: PathBuf,
    #[arg(short, long)]
    remote: String,
    /// Extra files to upload alongside the script
    #[arg(short, long, num_args = 1..)]
    files: Vec<PathBuf>,
}

#[allow(clippy::needless_pass_by_value)]
pub fn execute(args: Args, ctx: crate::Context) -> color_eyre::Result<()> {
    let id = uuid::Uuid::new_v4();
    let remote = ctx.config.get_remote(&args.remote)?;

    let script_path = args.inpfile.canonicalize().map_err(|e| {
        color_eyre::eyre::eyre!("Cannot access '{}': {e}", args.inpfile.display())
    })?;
    let work_dir = script_path
        .parent()
        .ok_or_else(|| color_eyre::eyre::eyre!("Could not determine parent directory of script"))?
        .to_path_buf();
    let file_stem = script_path
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(|| {
            color_eyre::eyre::eyre!(
                "Could not extract a valid UTF-8 file stem from '{}'",
                script_path.display()
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
    let remote_id = remote.submit(&script_path, &args.files, &connection, &remote_dir)?;

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
