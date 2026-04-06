use crate::jobs::Jobs;
use std::io::{IsTerminal, Write};

use tracing::{error, info};

/// Shows the log output of a job from its remote directory.
///
/// Each queue manager has a default log location: PBS uses <name>.o<id> and
/// <name>.e<id>, Slurm uses slurm-<id>.out, and Pueue uses `pueue log`.
/// Use --pattern to override with a specific filename in the remote directory.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Job UUID prefix
    job_id: String,
    /// Show a specific file from the remote directory instead of the default log
    #[arg(long)]
    pattern: Option<String>,
}

const GREEN: &str = "\x1b[32;1m";
const RED: &str = "\x1b[31;1m";
const RESET: &str = "\x1b[0m";

fn section_color(label: &str) -> &'static str {
    match label {
        "stderr" => RED,
        _ => GREEN,
    }
}

#[allow(clippy::needless_pass_by_value)]
pub fn execute(args: Args, ctx: crate::context::Context) -> color_eyre::Result<()> {
    let jobs = Jobs::load_from_db(ctx.db_path())?;
    let job = jobs.find_by_prefix(&args.job_id)?;

    if job.synced() {
        let mut stderr = std::io::stderr().lock();
        write!(
            stderr,
            "Warning: job {} is already synced. Continue? [y/N] ",
            job.short_id()
        )?;
        stderr.flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            return Ok(());
        }
    }

    let remote = ctx.config().get_remote(job.remote())?;

    let connection = match ctx.connect(remote) {
        Ok(conn) => conn,
        Err(e) => {
            error!("Failed to connect to {}, caused by: {}", remote.name(), e);
            return Err(e)?;
        }
    };

    let is_tty = std::io::stdout().is_terminal();

    let sections: Vec<(String, String)> = if let Some(filename) = &args.pattern {
        let path = job.remote_dir().join(filename);
        let cmd = format!("cat '{}'", path.to_string_lossy().replace('\'', "'\\''"));
        match connection.execute(&cmd) {
            Ok(output) => vec![(filename.clone(), output)],
            Err(e) => return Err(e.into()),
        }
    } else {
        remote.logs(
            job.remote_dir(),
            job.remote_id(),
            job.script_file(),
            &*connection,
        )
    };

    if sections.is_empty() {
        info!("No log output found for job {}.", job.short_id());
        return Ok(());
    }

    let mut stdout = std::io::stdout().lock();
    for (label, output) in &sections {
        if is_tty {
            let color = section_color(label);
            writeln!(stdout, "{color}── {label} ──{RESET}")?;
        } else {
            writeln!(stdout, "── {label} ──")?;
        }
        writeln!(stdout, "{output}")?;
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

    fn running_job() -> Job {
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
            JobStatus::Running,
            Utc::now(),
            None,
        )
    }

    #[test]
    fn test_log_default_fetches_stdout_and_stderr() {
        let tmp_db = tempfile::NamedTempFile::new().unwrap();
        let db_path = tmp_db.path().to_path_buf();
        let job = running_job();
        Jobs::insert_job(&db_path, &job).unwrap();

        let prefix = job.uuid().to_string()[..7].to_string();
        let (factory, state) = MockFactory::new();

        execute(
            Args {
                job_id: prefix,
                pattern: None,
            },
            make_context(db_path, factory),
        )
        .unwrap();

        let commands = &state.lock().unwrap().commands;
        assert_eq!(commands.len(), 2);
        assert!(commands[0].contains("myjob.pbs.o12345"));
        assert!(commands[1].contains("myjob.pbs.e12345"));
    }

    #[test]
    fn test_log_pattern_cats_named_file() {
        let tmp_db = tempfile::NamedTempFile::new().unwrap();
        let db_path = tmp_db.path().to_path_buf();
        let job = running_job();
        Jobs::insert_job(&db_path, &job).unwrap();

        let prefix = job.uuid().to_string()[..7].to_string();
        let (factory, state) = MockFactory::new();

        execute(
            Args {
                job_id: prefix,
                pattern: Some("output.log".to_string()),
            },
            make_context(db_path, factory),
        )
        .unwrap();

        let commands = &state.lock().unwrap().commands;
        assert_eq!(commands.len(), 1);
        assert!(commands[0].contains("output.log"));
        assert!(commands[0].starts_with("cat "));
    }
}
