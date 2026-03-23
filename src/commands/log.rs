use crate::connection::{RemoteConnection, SshConnection};
use crate::jobs::Jobs;
use std::io::{IsTerminal, Write};

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
pub fn execute(args: Args, ctx: crate::Context) -> color_eyre::Result<()> {
    let jobs = Jobs::load_from_db(&ctx.db_path)?;
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

    let remote = ctx.config.get_remote(job.remote())?;
    let connection = SshConnection::new(remote)?;

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
            job.filename(),
            &connection,
        )
    };

    if sections.is_empty() {
        eprintln!("No log output found for job {}.", job.short_id());
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
