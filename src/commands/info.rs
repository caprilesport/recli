use std::io::{IsTerminal, Write};
// use crate::job::Job;
use crate::jobs::Jobs;
use anyhow::anyhow;
use tabled::builder::Builder;

// use tracing::error;

#[derive(Debug, clap::Args)]
/// Shows detailed information about a specific job.
pub struct Args {
    /// Job UUID prefix (any unambiguous prefix length)
    job: String,
    #[arg(long, action)]
    json: bool,
}

#[allow(clippy::needless_pass_by_value)]
pub fn execute(args: Args, ctx: crate::Context) -> anyhow::Result<()> {
    let jobs = Jobs::load_from_db(&ctx.db_path)?;
    let job = jobs.find_by_prefix(&args.job)?;

    if args.json {
        let json_str = serde_json::to_string_pretty(job)?;
        println!("{json_str}");
    } else {
        let mut stdout = std::io::stdout().lock();
        let mut builder = Builder::default();

        builder.push_record(["ID", &job.short_id()]);
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

        writeln!(stdout, "{table}")?;
    }
    Ok(())
}
