use tabled::builder::Builder;
use uuid::Uuid;

use remotelib::job::Job;
use remotelib::jobs::{Jobs, Match};

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Filter by id
    #[arg(long)]
    pub id: Option<Uuid>,

    /// Filter by name
    #[arg(long)]
    pub name: Option<String>,

    /// Filter by remote
    #[arg(long)]
    pub remote: Option<String>,

    /// Filter by remote_id
    #[arg(long)]
    pub remote_id: Option<String>,

    /// Filter by basename
    #[arg(long)]
    pub basename: Option<String>,

    /// Filter by project
    #[arg(long)]
    pub project: Option<String>,

    /// Filter by status
    #[arg(long, value_enum)]
    pub status: Option<remotelib::job::JobStatus>,

    /// Filter by synced status
    #[arg(long)]
    pub synced: Option<bool>,
}

pub fn execute(args: Args, ctx: &crate::Context) -> anyhow::Result<()> {
    // let status =
    let jobs = Jobs::load_jobs(&ctx.json_file)?;
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

    if let Some(synced) = &args.synced {
        query = query.synced(*synced);
    }

    let jobs: Vec<&Job> = query.iter().collect();

    let table = create_status_table(jobs);
    println!("{}", table);

    Ok(())
}

pub fn create_status_table(jobs: Vec<&Job>) -> String {
    let mut builder = Builder::default();
    builder.push_record([
        "Name",
        "Project",
        "St",
        "Synced",
        "Remote",
        "Remote ID",
        "ID",
    ]);

    jobs.iter().for_each(|j| {
        builder.push_record(vec![
            j.name(),
            j.project(),
            j.status().as_str(),
            &j.synced().to_string(),
            j.remote(),
            j.remote_id(),
            &j.id().to_string(),
        ])
    });

    let mut table = builder.build();
    table.with(tabled::settings::Style::rounded());
    table.to_string()
}
