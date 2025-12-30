use std::io::{IsTerminal, Write};
// use crate::remote_system::job::Job;
use crate::remote_system::jobs::Jobs;
use anyhow::anyhow;
use tabled::builder::Builder;

// use tracing::error;

#[derive(Debug, clap::Args)]
/// Submits a job to a specified remote machine.
///
/// This command prepares the necessary job files, uploads them to the remote's
/// working directory, and submits the job to the queue manager.
pub struct Args {
    job: u16,
    #[arg(long, action)]
    json: bool,
}

pub fn execute(args: Args, ctx: crate::Context) -> anyhow::Result<()> {
    let jobs = Jobs::load_jobs(&ctx.json_file)?;
    let job = jobs
        .iter()
        .find(|j| j.id() == &args.job)
        .ok_or_else(|| anyhow!("Job with ID {0} not found", args.job))?;

    if args.json {
        let json_str = serde_json::to_string_pretty(job)?;
        println!("{}", json_str);
    } else {
        let mut stdout = std::io::stdout().lock();
        let mut builder = Builder::default();

        builder.push_record(["ID", &job.id().to_string()]);
        builder.push_record(["UUID", &job.uuid().to_string()]);
        builder.push_record(["remote", job.remote()]);
        builder.push_record(["remote_id", job.remote_id()]);
        builder.push_record(["filename", job.filename()]);
        builder.push_record([
            "work_dir",
            job.work_dir().to_str().ok_or_else(|| {
                anyhow!("Job work directory path contains invalid UTF-8 characters.")
            })?,
        ]);
        builder.push_record([
            "remote_dir",
            job.remote_dir().to_str().ok_or_else(|| {
                anyhow!("Job remote directory path contains invalid UTF-8 characters.")
            })?,
        ]);
        builder.push_record(["status", job.status().as_str()]);

        let mut table = builder.build();

        if stdout.is_terminal() {
            table
                .with(tabled::settings::Style::rounded())
                .with(tabled::settings::Alignment::center());
        } else {
            table.with(tabled::settings::Style::empty());
        }

        writeln!(stdout, "{}", table)?;
    }
    Ok(())
}
