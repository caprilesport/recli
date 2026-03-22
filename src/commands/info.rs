use crate::jobs::Jobs;
use anyhow::anyhow;
use comfy_table::presets::NOTHING;
use comfy_table::{Attribute, Cell, CellAlignment, ContentArrangement, Table};
use std::io::{IsTerminal, Write};

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
        return Ok(());
    }

    let mut stdout = std::io::stdout().lock();
    let is_tty = stdout.is_terminal();

    if is_tty {
        // Header: "Job <short_id>:" | colored status
        let mut header = Table::new();
        header
            .load_preset("││─ └──┘     ─ ┌┐  ")
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_header(vec![
                Cell::new(format!("Job {}:", job.short_id())).add_attribute(Attribute::Bold),
                super::status_cell(*job.status(), true),
            ]);
        writeln!(stdout, "{header}")?;

        // Details: right-aligned bold keys, plain values
        let work_dir = job
            .work_dir()
            .to_str()
            .ok_or_else(|| anyhow!("Job work directory path contains invalid UTF-8 characters."))?;
        let remote_dir = job.remote_dir().to_str().ok_or_else(|| {
            anyhow!("Job remote directory path contains invalid UTF-8 characters.")
        })?;
        let submit_time = job
            .submit_time()
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M")
            .to_string();
        let sync_time = match job.sync_time() {
            Some(t) => t
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string(),
            None => "─".to_string(),
        };

        let mut detail = Table::new();
        detail
            .load_preset(NOTHING)
            .set_content_arrangement(ContentArrangement::Dynamic);

        let bold = |s: &str| Cell::new(s).add_attribute(Attribute::Bold);
        detail.add_row(vec![bold("Filename:"), Cell::new(job.filename())]);
        detail.add_row(vec![bold("Remote:"), Cell::new(job.remote())]);
        detail.add_row(vec![bold("Remote ID:"), Cell::new(job.remote_id())]);
        detail.add_row(vec![bold("Work dir:"), Cell::new(work_dir)]);
        detail.add_row(vec![bold("Remote dir:"), Cell::new(remote_dir)]);
        detail.add_row(vec![bold("Submitted:"), Cell::new(&submit_time)]);
        detail.add_row(vec![bold("Synced:"), Cell::new(&sync_time)]);
        detail.add_row(vec![bold("UUID:"), Cell::new(job.uuid().to_string())]);

        let first_col = detail
            .column_mut(0)
            .expect("table always has at least one column");
        first_col.set_cell_alignment(CellAlignment::Right);
        first_col.set_padding((0, 1));

        writeln!(stdout, "{detail}")?;
    } else {
        let mut table = Table::new();
        table.load_preset(NOTHING);
        table.add_row(["id", &job.short_id()]);
        table.add_row(["uuid", &job.uuid().to_string()]);
        table.add_row(["remote", job.remote()]);
        table.add_row(["remote_id", job.remote_id()]);
        table.add_row(["filename", job.filename()]);
        table.add_row([
            "work_dir",
            job.work_dir().to_str().ok_or_else(|| {
                anyhow!("Job work directory path contains invalid UTF-8 characters.")
            })?,
        ]);
        table.add_row([
            "remote_dir",
            job.remote_dir().to_str().ok_or_else(|| {
                anyhow!("Job remote directory path contains invalid UTF-8 characters.")
            })?,
        ]);
        table.add_row(["status", job.status().as_str()]);
        writeln!(stdout, "{table}")?;
    }

    Ok(())
}
