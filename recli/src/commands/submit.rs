use remotelib::connection::SshConnection;
use remotelib::job::Job;
use remotelib::jobs::Jobs;
use std::path::PathBuf;

use tracing::info;

#[derive(Debug, clap::Args)]
/// Submits a job to a specified remote machine.
///
/// This command prepares the necessary job files, uploads them to the remote's
/// working directory, and submits the job to the queue manager.
pub struct Args {
    inpfile: PathBuf,
    #[arg(short, long)]
    remote: String,
}

pub fn execute(args: Args, ctx: &crate::Context) -> anyhow::Result<()> {
    let id = uuid::Uuid::new_v4();
    let remote = ctx.config.get_remote(&args.remote)?;
    let file_stem = args.inpfile.file_stem().unwrap().to_str().unwrap();

    let connection = SshConnection::new(&remote)?;
    let remote_id = remote.submit(id, &args.inpfile, &connection)?;

    let job = Job::new(
        id,
        remote.name().to_owned(),
        remote_id,
        file_stem.to_owned(),
    )?;
    let mut jobs = Jobs::load_jobs(&ctx.json_file)?;

    info!(
        "Job submitted successfully with id: {}. Remote id: {}",
        id,
        &job.remote_id()
    );

    jobs.add(job);
    jobs.save_jobs(&ctx.json_file)?;
    Ok(())
}
