use chrono::{Duration, Utc};
use tabled::builder::Builder;
use uuid::Uuid;

use remotelib::job::Job;
use remotelib::jobs::{Jobs, Match};

#[derive(Debug, clap::Args)]
/// Displays the status of jobs, with optional filters.
/// By default it doesn't show jobs that are synced
/// To show all jobs recorded, use the -a/--all flag
///
/// Shows a table of all tracked jobs. You can use the flags below to filter
/// the jobs that are displayed.
pub struct Args {
    /// Show all jobs
    #[arg(long, short, action)]
    pub all: bool,

    /// Show id for each job
    #[arg(long, action)]
    pub show_id: bool,

    /// Filter by id
    #[arg(long, short)]
    pub id: Option<Uuid>,

    /// Filter by name
    #[arg(long, short)]
    pub name: Option<String>,

    /// Filter by remote
    #[arg(long, short)]
    pub remote: Option<String>,

    /// Filter by remote_id
    #[arg(long)]
    pub remote_id: Option<String>,

    /// Filter by basename
    #[arg(long, short)]
    pub basename: Option<String>,

    /// Filter by project
    #[arg(long, short)]
    pub project: Option<String>,

    /// Filter by status
    #[arg(long, short, value_enum)]
    pub status: Option<remotelib::job::JobStatus>,

    /// Filter by synced status
    #[arg(long, action, default_value_t = false)]
    pub synced: bool,
}

pub fn execute(args: Args, ctx: &crate::Context) -> anyhow::Result<()> {
    let mut jobs = Jobs::load_jobs(&ctx.json_file)?;

    if !args.all {
        let twenty_four_hours_ago = Utc::now() - Duration::hours(24);
        jobs.retain(|j| {
            !j.synced()
                || (j.synced() && j.sync_time().map_or(false, |st| st > twenty_four_hours_ago))
        });
    }

    let mut query = jobs.query();

    if let Some(id) = args.id {
        query.with_id(&id);
    }

    if let Some(name) = &args.name {
        query = query.with_name(Match::Contains(&name));
    }

    if let Some(remote) = &args.remote {
        query = query.with_remote(Match::Contains(&remote));
    }

    if let Some(remote_id) = &args.remote_id {
        query = query.with_remote_id(&remote_id);
    }

    if let Some(basename) = &args.basename {
        query = query.with_basename(Match::Contains(&basename));
    }

    if let Some(project) = &args.project {
        query = query.with_project(Match::Contains(&project));
    }

    if let Some(status) = &args.status {
        query = query.with_status(&status);
    }

    if args.synced {
        query = query.synced(args.synced);
    }

    let jobs = query.iter().collect();

    let table = create_status_table(jobs, args.show_id);
    println!("{}", table);

    Ok(())
}

pub fn create_status_table(jobs: Vec<&Job>, with_id: bool) -> String {
    let mut builder = Builder::default();
    let mut headers = vec![
        "Name",
        "Project",
        "St",
        "Synced",
        "Remote",
        "Submit time",
        "Sync time",
    ];
    if with_id {
        headers.push("Remote ID");
        headers.push("ID");
    }
    builder.push_record(headers);

    jobs.iter().for_each(|j| {
        let synced = if j.synced() { "Yes" } else { "No" };
        let id = j.id().to_string();
        let submit_time = j
            .submit_time()
            .with_timezone(&chrono::Local)
            .format("%m-%d %H:%M")
            .to_string();
        let sync_time = match j.sync_time() {
            Some(date) => date
                .with_timezone(&chrono::Local)
                .format("%m-%d %H:%M")
                .to_string(),
            None => "None".to_string(),
        };
        let mut row = vec![
            j.name(),
            j.project(),
            j.status().as_str(),
            synced,
            j.remote(),
            &submit_time,
            &sync_time,
        ];
        if with_id {
            row.push(j.remote_id());
            row.push(&id);
        }
        builder.push_record(row);
    });

    let mut table = builder.build();
    table.with(tabled::settings::Style::rounded());
    table.to_string()
}
