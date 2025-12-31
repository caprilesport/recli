use crate::jobs::Jobs;
use chrono::{Duration, Utc};
use std::io::{IsTerminal, Write};
use tabled::builder::Builder;
use uuid::Uuid;

use crate::job::{Job, JobStatus};

/// Displays the status of jobs, with optional filters.
/// By default it doesn't show jobs that are synced
/// To show all jobs recorded, use the -a/--all flag
///
/// Shows a table of all tracked jobs. You can use the flags below to filter
/// the jobs that are displayed.
#[derive(Debug, clap::Args, Default)]
pub struct Args {
    /// Show all jobs
    #[arg(long, short, action)]
    pub all: bool,

    /// Show id for each job
    #[arg(long, action)]
    pub show_id: bool,

    /// Filter by id
    #[arg(long, short)]
    pub id: Option<u16>,

    /// Filter by uuid
    #[arg(long, short)]
    pub uuid: Option<Uuid>,

    /// Filter by name
    #[arg(long, short)]
    pub name: Option<String>,

    /// Filter by directory
    #[arg(long, short)]
    pub directory: Option<String>,

    /// Filter by remote
    #[arg(long, short)]
    pub remote: Option<String>,

    /// Filter by remote_id
    #[arg(long)]
    pub remote_id: Option<String>,

    /// Filter by status
    #[arg(long, short, value_enum)]
    pub status: Option<JobStatus>,

    /// Filter synced jobs
    #[arg(long, action, default_value_t = false)]
    pub synced: bool,

    /// Filter non synced jobs
    #[arg(long, action, default_value_t = false)]
    pub not_synced: bool,
}

pub fn execute(args: Args, ctx: crate::Context) -> anyhow::Result<()> {
    let mut jobs = Jobs::load_jobs(&ctx.json_file)?;

    if !args.all {
        let two_days_ago = Utc::now() - Duration::hours(48);
        jobs.retain(|j| {
            !j.synced() || (j.synced() && j.sync_time().is_some_and(|st| st > two_days_ago))
        });
    }

    let mut query = jobs.query();

    if let Some(id) = args.id {
        query.with_id(&id);
    }

    if let Some(uuid) = args.uuid {
        query.with_uuid(&uuid);
    }

    if let Some(name) = &args.name {
        query = query.with_name(name);
    }

    if let Some(remote) = &args.remote {
        query = query.with_remote(remote);
    }

    if let Some(remote_id) = &args.remote_id {
        query = query.with_remote_id(remote_id);
    }

    if let Some(directory) = &args.directory {
        query = query.with_dir(directory);
    }

    if let Some(status) = &args.status {
        query = query.with_status(status);
    }

    if args.synced {
        query = query.synced(true);
    } else if args.not_synced {
        query = query.synced(false);
    }

    let jobs = query.iter().collect();
    let mut stdout = std::io::stdout().lock();

    if stdout.is_terminal() {
        let mut table = create_status_table(jobs, args.show_id, true);
        table
            .with(tabled::settings::Style::rounded())
            .with(tabled::settings::Alignment::center());
        writeln!(stdout, "{}", table)?;
    } else {
        let mut table = create_status_table(jobs, args.show_id, false);
        table.with(tabled::settings::Style::empty());
        writeln!(stdout, "{}", table)?;
    }

    Ok(())
}

fn create_status_table(jobs: Vec<&Job>, with_id: bool, with_header: bool) -> tabled::Table {
    let mut builder = Builder::default();

    if with_header {
        let mut headers = vec![
            "ID",
            "Work dir",
            "Name",
            "Status",
            "Remote",
            "Submit time",
            "Sync time",
        ];
        if with_id {
            headers.push("Remote ID");
            headers.push("ID");
        }
        builder.push_record(headers);
    }

    jobs.iter().for_each(|j| {
        let j_uuid = j.uuid().to_string();
        let id = j.id().to_string();
        let work_dir = j.work_dir().to_string_lossy().to_string();
        let submit_time = j
            .submit_time()
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M")
            .to_string();
        let sync_time = match j.sync_time() {
            Some(date) => date
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string(),
            None => "None".to_string(),
        };
        let mut row = vec![
            &id,
            &work_dir,
            j.filename(),
            j.status().as_str(),
            j.remote(),
            &submit_time,
            &sync_time,
        ];
        if with_id {
            row.push(j.remote_id());
            row.push(&j_uuid);
        }
        builder.push_record(row);
    });

    builder.build()
}
