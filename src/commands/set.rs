use crate::job::JobStatus;
use crate::jobs::Jobs;
use std::path::PathBuf;

/// Update fields on an existing job record.
///
/// At least one field flag must be provided. Use --clear-tags to remove all
/// tags from a job.
#[derive(Debug, clap::Args)]
#[group(required = true, multiple = true)]
pub struct Args {
    /// Job UUID prefix to update
    #[arg(required = true)]
    job_id: String,

    /// Set the local working directory
    #[arg(long)]
    work_dir: Option<PathBuf>,

    /// Set the remote working directory
    #[arg(long)]
    remote_dir: Option<PathBuf>,

    /// Set the remote name
    #[arg(long)]
    remote: Option<String>,

    /// Set the remote job ID
    #[arg(long)]
    remote_id: Option<String>,

    /// Set the script file path
    #[arg(long)]
    script_file: Option<String>,

    /// Set the job status
    #[arg(long, value_enum)]
    status: Option<JobStatus>,

    /// Replace all tags
    #[arg(long, num_args = 1..)]
    tags: Vec<String>,

    /// Clear all tags
    #[arg(long, action, conflicts_with = "tags")]
    clear_tags: bool,
}

#[allow(clippy::needless_pass_by_value)]
pub fn execute(args: Args, ctx: crate::context::Context) -> color_eyre::Result<()> {
    let jobs = Jobs::load_from_db(ctx.db_path())?;
    let job = jobs.find_by_prefix(&args.job_id)?;
    let uuid = *job.uuid();

    if let Some(path) = &args.work_dir {
        let path = path
            .canonicalize()
            .map_err(|e| color_eyre::eyre::eyre!("Cannot access '{}': {e}", path.display()))?;
        jobs.update_work_dir(&uuid, &path)?;
    }
    if let Some(path) = &args.remote_dir {
        jobs.update_remote_dir(&uuid, path)?;
    }
    if let Some(remote) = &args.remote {
        jobs.update_remote(&uuid, remote)?;
    }
    if let Some(remote_id) = &args.remote_id {
        jobs.update_remote_id(&uuid, remote_id)?;
    }
    if let Some(script_file) = &args.script_file {
        jobs.update_script_file(&uuid, script_file)?;
    }
    if let Some(status) = args.status {
        jobs.update_job_status(&uuid, status)?;
    }
    if !args.tags.is_empty() {
        jobs.update_tags(&uuid, &args.tags)?;
    } else if args.clear_tags {
        jobs.update_tags(&uuid, &[])?;
    }

    if ctx.json() {
        let jobs = Jobs::load_from_db(ctx.db_path())?;
        let updated = jobs.find_by_prefix(&args.job_id)?;
        println!("{}", serde_json::to_string_pretty(&updated)?);
    }

    Ok(())
}
