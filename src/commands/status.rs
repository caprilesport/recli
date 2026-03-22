use crate::jobs::Jobs;
use chrono::{Duration, Utc};
use comfy_table::presets::UTF8_HORIZONTAL_ONLY;
use comfy_table::{Cell, ContentArrangement, Table};
use std::io::{IsTerminal, Write};
use uuid::Uuid;

use crate::job::{Job, JobStatus};

/// Displays the status of jobs, with optional filters.
/// By default it doesn't show jobs that are synced
/// To show all jobs recorded, use the -a/--all flag
///
/// Shows a table of all tracked jobs. You can use the flags below to filter
/// the jobs that are displayed.
#[derive(Debug, clap::Args, Default)]
#[allow(clippy::struct_excessive_bools)]
pub struct Args {
    /// Show all jobs
    #[arg(long, short, action)]
    pub all: bool,

    /// Show id for each job
    #[arg(long, action)]
    pub show_id: bool,

    /// Filter by UUID prefix
    #[arg(long, short)]
    pub id: Option<String>,

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

    /// Filter by `remote_id`
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

#[allow(clippy::needless_pass_by_value)]
pub fn execute(args: Args, ctx: crate::Context) -> anyhow::Result<()> {
    let mut jobs = Jobs::load_from_db(&ctx.db_path)?;

    if !args.all {
        let two_days_ago = Utc::now() - Duration::hours(48);
        jobs.retain(|j| {
            !j.synced() || (j.synced() && j.sync_time().is_some_and(|st| *st > two_days_ago))
        });
    }

    let mut query = jobs.query();

    if let Some(ref prefix) = args.id {
        query = query.with_prefix(prefix);
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

    let jobs: Vec<&Job> = query.iter().collect();
    let mut stdout = std::io::stdout().lock();
    let is_tty = stdout.is_terminal();
    let table = create_status_table(jobs, args.show_id, is_tty);
    writeln!(stdout, "{table}")?;

    Ok(())
}

#[allow(clippy::needless_pass_by_value)]
fn create_status_table(jobs: Vec<&Job>, with_id: bool, is_tty: bool) -> Table {
    let mut table = Table::new();

    if is_tty {
        table
            .set_content_arrangement(ContentArrangement::Dynamic)
            .load_preset(UTF8_HORIZONTAL_ONLY);
    } else {
        table
            .set_content_arrangement(ContentArrangement::Disabled)
            .load_preset(comfy_table::presets::NOTHING);
    }

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
        headers.push("UUID");
    }
    table.set_header(headers);

    for j in &jobs {
        let id = j.short_id();
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
            None => "─".to_string(),
        };

        let mut row: Vec<Cell> = vec![
            Cell::new(&id),
            Cell::new(&work_dir),
            Cell::new(j.filename()),
            super::status_cell(*j.status(), is_tty),
            Cell::new(j.remote()),
            Cell::new(&submit_time),
            Cell::new(&sync_time),
        ];
        if with_id {
            row.push(Cell::new(j.remote_id()));
            row.push(Cell::new(j.uuid().to_string()));
        }
        table.add_row(row);
    }

    table
}
