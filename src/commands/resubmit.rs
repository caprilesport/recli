use crate::connection::SshConnection;
use crate::job::JobStatus;
use crate::jobs::Jobs;
use chrono::Utc;
use std::path::PathBuf;
use tracing::info;

/// Re-submits an existing job to a queue, optionally to a different remote.
///
/// Files are uploaded before submitting (smart transfer: skips files whose
/// modification time has not changed). Use this after fixing an input file that
/// caused an error, or to move a job to a different remote.
///
/// Tags are replaced when --tags is provided; omit --tags to keep existing tags.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Job UUID prefix to resubmit
    job_id: String,
    /// Submit to a different remote (re-uploads all files)
    #[arg(long)]
    remote: Option<String>,
    /// Replace the job's tags
    #[arg(long, num_args = 1..)]
    tags: Vec<String>,
}

#[allow(clippy::needless_pass_by_value)]
pub fn execute(args: Args, ctx: crate::context::Context) -> color_eyre::Result<()> {
    let jobs = Jobs::load_from_db(ctx.db_path())?;
    let job = jobs.find_by_prefix(&args.job_id)?;

    let target_remote_name = args.remote.as_deref().unwrap_or(job.remote());
    let cross_remote = target_remote_name != job.remote();

    let remote = ctx.config().get_remote(target_remote_name)?;
    let connection = SshConnection::new(remote)?;

    // For cross-remote, derive a new remote_dir under the new remote's work_dir.
    // For same remote, reuse the existing remote_dir.
    let remote_dir: PathBuf = if cross_remote {
        remote.work_dir().join(job.uuid().to_string())
    } else {
        job.remote_dir().clone()
    };

    let script_path = PathBuf::from(job.script_file());
    // files_sent[0] is the script; the rest are extra files.
    let extra_files: Vec<PathBuf> = job
        .files_sent()
        .iter()
        .filter(|p| p.as_path() != script_path)
        .cloned()
        .collect();

    // Re-parse directives — the script may have been edited before resubmitting.
    let content = std::fs::read_to_string(&script_path).unwrap_or_default();
    let directives = remote.queue_manager().parse_directives(&content);
    let file_stem = script_path
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or(job.filename())
        .to_owned();
    let job_name = directives.name.unwrap_or(file_stem);

    let remote_id = remote.submit(&script_path, &extra_files, &connection, &remote_dir)?;

    // Collect what was sent (script + extra, same as submit does)
    let mut files_sent = vec![script_path];
    files_sent.extend(extra_files);

    let uuid = *job.uuid();

    // Always update: new remote_id, reset status and timestamps, refresh parsed fields
    jobs.update_remote_id(&uuid, &remote_id)?;
    jobs.update_submit_time(&uuid, Utc::now())?;
    jobs.update_job_status(&uuid, JobStatus::Queued)?;
    jobs.clear_sync_time(&uuid)?;
    jobs.update_files_sent(&uuid, &files_sent)?;
    jobs.update_name(&uuid, &job_name)?;
    jobs.update_queue(&uuid, directives.queue.as_deref())?;

    if cross_remote {
        jobs.update_remote(&uuid, target_remote_name)?;
        jobs.update_remote_dir(&uuid, &remote_dir)?;
    }

    if !args.tags.is_empty() {
        jobs.update_tags(&uuid, &args.tags)?;
    }

    if ctx.json() {
        let jobs = Jobs::load_from_db(ctx.db_path())?;
        let updated = jobs.find_by_prefix(&args.job_id)?;
        println!("{}", serde_json::to_string_pretty(&updated)?);
    } else {
        info!(
            "Job {} resubmitted to {} with remote id: {}",
            job.short_id(),
            target_remote_name,
            remote_id
        );
    }

    Ok(())
}
