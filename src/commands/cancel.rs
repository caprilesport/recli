use crate::jobs::Jobs;

use tracing::{error, info};

/// Cancels a running or queued job on its remote.
///
/// Sends a cancel request to the queue manager for the specified job.
/// The job status in the local database is not updated immediately;
/// run `recli fetch` afterwards to reflect the new state.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Job UUID prefix
    job: String,
}

#[allow(clippy::needless_pass_by_value)]
pub fn execute(args: Args, ctx: crate::context::Context) -> color_eyre::Result<()> {
    let jobs = Jobs::load_from_db(ctx.db_path())?;
    let job = jobs.find_by_prefix(&args.job)?;
    let remote = ctx.config().get_remote(job.remote())?;
    let connection = match ctx.connect(remote) {
        Ok(c) => c,
        Err(err) => {
            error!("Failed to connect to {}, caused by: {}", remote.name(), err);
            return Err(err)?;
        }
    };

    remote.cancel(job.remote_id(), &*connection)?;

    if ctx.json() {
        println!("{}", serde_json::to_string_pretty(job)?);
    } else {
        info!(
            "Cancelled job {} ({}) with remote id: {}",
            job.filename(),
            job.short_id(),
            job.remote_id()
        );
    }

    Ok(())
}
