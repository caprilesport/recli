use crate::config::FileStrategy;
use crate::connection::SshConnection;
use crate::job::Job;
use crate::jobs::Jobs;
use std::path::PathBuf;

use tracing::{error, info};

#[derive(Debug, clap::Args)]
/// Submits one or more jobs to a specified remote machine.
///
/// Uploads each job script (and any extra files) to its own directory on the
/// remote and submits it to the queue manager. Multiple scripts can be given
/// at once — the shell expands globs before recli sees them.
///
/// Which files are uploaded alongside each script is controlled by --strategy
/// (or `file_strategy` in `[settings]`). `--files` always adds extra files
/// on top for every script, resolved relative to the current working directory.
pub struct Args {
    /// Job script(s) to submit (.pbs, .slurm, etc.)
    #[arg(num_args = 1..)]
    jobfiles: Vec<PathBuf>,
    #[arg(short, long)]
    remote: String,
    /// Extra files to upload alongside every script, resolved relative to the
    /// current working directory
    #[arg(short, long, num_args = 1..)]
    files: Vec<PathBuf>,
    /// Which files to upload from each script's directory (overrides
    /// `file_strategy` in config)
    #[arg(long, value_enum)]
    strategy: Option<FileStrategy>,
}

#[allow(clippy::needless_pass_by_value)]
pub fn execute(args: Args, ctx: crate::Context) -> color_eyre::Result<()> {
    let remote = ctx.config.get_remote(&args.remote)?;
    let strategy = args
        .strategy
        .unwrap_or_else(|| ctx.config.settings.file_strategy.clone());

    // Canonicalize --files once; they are shared across all scripts in the batch.
    let shared_files: Vec<PathBuf> = args
        .files
        .iter()
        .map(|f| {
            f.canonicalize()
                .map_err(|e| color_eyre::eyre::eyre!("Cannot access '{}': {e}", f.display()))
        })
        .collect::<color_eyre::Result<_>>()?;

    let connection = match SshConnection::new(remote) {
        Ok(c) => c,
        Err(err) => {
            error!("Failed to connect to {}, caused by: {}", remote.name(), err);
            return Err(err)?;
        }
    };

    for inpfile in &args.jobfiles {
        let id = uuid::Uuid::new_v4();

        let script_path = inpfile
            .canonicalize()
            .map_err(|e| color_eyre::eyre::eyre!("Cannot access '{}': {e}", inpfile.display()))?;
        let work_dir = script_path
            .parent()
            .ok_or_else(|| {
                color_eyre::eyre::eyre!("Could not determine parent directory of script")
            })?
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

        let mut companion_files: Vec<PathBuf> = match &strategy {
            FileStrategy::Script => vec![],
            FileStrategy::Basename => work_dir
                .read_dir()?
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.is_file() && p != &script_path)
                .filter(|p| {
                    p.file_name()
                        .and_then(std::ffi::OsStr::to_str)
                        .is_some_and(|n| n.starts_with(file_stem))
                })
                .collect(),
            FileStrategy::Directory => work_dir
                .read_dir()?
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.is_file() && p != &script_path)
                .filter(|p| {
                    let name = p.file_name().and_then(std::ffi::OsStr::to_str);
                    !ctx.config
                        .ignore
                        .iter()
                        .any(|pat| name.is_some_and(|n| pat.matches(n)))
                })
                .collect(),
        };
        companion_files.extend_from_slice(&shared_files);

        let remote_dir = remote.work_dir().join(id.to_string());
        let remote_id = remote.submit(&script_path, &companion_files, &connection, &remote_dir)?;

        let job = Job::new(
            id,
            remote.name().to_owned(),
            remote_id,
            file_stem.to_owned(),
            remote_dir,
            work_dir,
        );

        Jobs::insert_job(&ctx.db_path, &job)?;

        if ctx.json {
            println!("{}", serde_json::to_string_pretty(&job)?);
        } else {
            info!(
                "Job {} ({}) with remote id: {} submitted successfully",
                job.filename(),
                job.short_id(),
                job.remote_id()
            );
        }
    }

    Ok(())
}
