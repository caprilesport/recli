use crate::config::{Column, Display};
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

    /// Filter by tag
    #[arg(long)]
    pub tag: Option<String>,

    /// Filter by queue
    #[arg(long)]
    pub queue: Option<String>,

    /// Filter by script name (substring match)
    #[arg(long)]
    pub script: Option<String>,
}

#[allow(clippy::needless_pass_by_value)]
pub fn execute(args: Args, ctx: crate::Context) -> color_eyre::Result<()> {
    let display = &ctx.config.display;
    let mut jobs = Jobs::load_from_db(&ctx.db_path)?;

    if !args.all && display.status_window_hours > 0 {
        let cutoff = Utc::now() - Duration::hours(i64::from(display.status_window_hours));
        jobs.retain(|j| {
            !j.synced() || (j.synced() && j.sync_time().is_some_and(|st| *st > cutoff))
        });
    }

    let mut query = jobs.query();

    if let Some(prefix) = &args.id {
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

    if let Some(queue) = &args.queue {
        query = query.with_queue(queue);
    }

    if let Some(script) = &args.script {
        query = query.with_script(script);
    }

    let jobs: Vec<&Job> = if let Some(tag) = &args.tag {
        query
            .iter()
            .filter(|j| j.tags().iter().any(|t| t == tag))
            .collect()
    } else {
        query.iter().collect()
    };

    if ctx.json {
        println!("{}", serde_json::to_string_pretty(&jobs)?);
        return Ok(());
    }

    let mut stdout = std::io::stdout().lock();
    let is_tty = stdout.is_terminal();
    let table = create_status_table(jobs, args.show_id, is_tty, display);
    writeln!(stdout, "{table}")?;

    Ok(())
}

fn short_path(path: &std::path::Path, n: usize) -> String {
    if n == 0 {
        return path.to_string_lossy().to_string();
    }
    let parts: Vec<_> = path.iter().collect();
    if parts.len() <= n {
        return path.to_string_lossy().to_string();
    }
    let tail: std::path::PathBuf = parts[parts.len() - n..].iter().collect();
    format!("…/{}", tail.display())
}

fn fmt_datetime(dt: &chrono::DateTime<chrono::Utc>, two_line: bool, fmt: &str) -> String {
    let local = dt.with_timezone(&chrono::Local);
    let formatted = local.format(fmt).to_string();
    if two_line && let Some(pos) = formatted.find(' ') {
        return format!("{}\n{}", &formatted[..pos], &formatted[pos + 1..]);
    }
    formatted
}

fn fmt_tags(tags: &[String], max_tags: usize) -> String {
    if max_tags == 0 || tags.len() <= max_tags {
        return tags.join(", ");
    }
    let remaining = tags.len() - max_tags;
    format!("{}, +{remaining} more", tags[..max_tags].join(", "))
}

#[allow(clippy::needless_pass_by_value)]
fn create_status_table(jobs: Vec<&Job>, with_id: bool, is_tty: bool, display: &Display) -> Table {
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

    let mut headers: Vec<&str> = display.columns.iter().map(Column::header).collect();
    if with_id {
        headers.push("Remote ID");
        headers.push("UUID");
    }
    table.set_header(headers);

    for j in &jobs {
        let mut row: Vec<Cell> = display
            .columns
            .iter()
            .map(|col| match col {
                Column::Id => Cell::new(j.short_id()),
                Column::WorkDir => {
                    let path = if is_tty {
                        short_path(j.work_dir(), display.path_components)
                    } else {
                        j.work_dir().to_string_lossy().to_string()
                    };
                    Cell::new(path)
                }
                Column::Name => Cell::new(j.filename()),
                Column::Status => super::status_cell(*j.status(), is_tty),
                Column::Remote => Cell::new(j.remote()),
                Column::Queue => Cell::new(j.queue().unwrap_or("─")),
                Column::SubmitTime => Cell::new(fmt_datetime(
                    j.submit_time(),
                    is_tty,
                    &display.datetime_format,
                )),
                Column::SyncTime => Cell::new(match j.sync_time() {
                    Some(dt) => fmt_datetime(dt, is_tty, &display.datetime_format),
                    None => "─".to_string(),
                }),
                Column::Tags => Cell::new(fmt_tags(j.tags(), display.max_tags)),
            })
            .collect();

        if with_id {
            row.push(Cell::new(j.remote_id()));
            row.push(Cell::new(j.uuid().to_string()));
        }
        table.add_row(row);
    }

    table
}
