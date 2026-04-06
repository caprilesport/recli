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

    if ctx.json_output() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::test_support::MockFactory;
    use crate::context::Context;
    use crate::job::{Job, JobStatus};
    use crate::jobs::Jobs;
    use chrono::Utc;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn make_context(db_path: PathBuf, factory: MockFactory) -> Context {
        let config: crate::config::Config = toml::from_str(
            r#"
            [[remotes]]
            name = "test"
            hostname = "localhost"
            port = 22
            user = "testuser"
            work_directory = "/remote/work"
            queue_manager = "Pbs"
            "#,
        )
        .unwrap();
        Context::with_factory(config, db_path, Box::new(factory))
    }

    fn queued_job() -> Job {
        Job::new(
            Uuid::new_v4(),
            "test".to_string(),
            "12345.server".to_string(),
            "myjob".to_string(),
            PathBuf::from("myjob.pbs"),
            vec![],
            vec![],
            None,
            PathBuf::from("/tmp"),
            PathBuf::from("/remote/work/uuid"),
            JobStatus::Queued,
            Utc::now(),
            None,
        )
    }

    #[test]
    fn test_cancel_sends_qdel_command() {
        let tmp_db = tempfile::NamedTempFile::new().unwrap();
        let db_path = tmp_db.path().to_path_buf();
        let job = queued_job();
        Jobs::insert_job(&db_path, &job).unwrap();

        let prefix = job.uuid().to_string()[..7].to_string();
        let (factory, state) = MockFactory::new();

        execute(Args { job: prefix }, make_context(db_path, factory)).unwrap();

        let commands = &state.lock().unwrap().commands;
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0], format!("qdel '{}'", job.remote_id()));
    }
}
