use crate::connection::SshConnection;
use crate::job::Job;
use crate::jobs::Jobs;
use crate::manifest::JobManifest;
use std::path::PathBuf;
use tera::{Context, Tera};

use tracing::{debug, error, info};

#[derive(Debug, clap::Args)]
/// Submits a job to a specified remote machine.
pub struct Args {
    input_file: PathBuf,
}

pub fn execute(args: Args, ctx: crate::Context) -> anyhow::Result<()> {
    debug!("Parsing input file {:?}", args.input_file);
    let manifest: JobManifest = toml::from_str(&std::fs::read_to_string(&args.input_file)?)?;
    let current_dir = std::env::current_dir()?;
    let manifest_path = current_dir.clone().join(args.input_file);

    // this only resolves files in the cwd, for nested submission (e.g. recli submit some-dir/submitfile.toml it should probably look for files inside some-dir? otherwise we should check if the file is in the same directory and return an error. dont know how to handle this yet..)
    let files_to_send: Vec<PathBuf> = manifest.build_files()?;

    let config_dir = crate::Config::get_dir()?;
    let mut templates_glob = config_dir.to_str().unwrap().to_owned();
    templates_glob.push_str("/templates/*");
    let tera = Tera::new(&templates_glob)?;

    dbg!(&files_to_send);

    let mut context = Context::new();
    context.try_insert("spec", &manifest.spec)?;
    context.try_insert("exec", &manifest.exec)?;
    context.try_insert("files", &files_to_send)?;
    context.try_insert("context", &manifest.context)?;

    let rendered = tera.render(&manifest.template, &context)?;

    // create the rendered template
    std::fs::write(
        manifest_path
            .clone()
            .parent()
            // this unwrap is probably ok, since the manifest path is a file path and the parent is the directory it is in
            .unwrap()
            .join(format!("{}.job", &manifest.spec.name)),
        rendered,
    )?;

    let id = uuid::Uuid::new_v4();
    let remote = ctx.config.get_remote(&manifest.remote)?;

    let connection = match SshConnection::new(remote) {
        Ok(sshconnection) => sshconnection,
        Err(err) => {
            error!("Failed to connect to {}, caused by: {}", remote.name(), err);
            return Err(err)?;
        }
    };

    let remote_dir = remote.work_dir().join(id.to_string());

    let remote_id = remote.submit(&manifest.spec.name, &connection, &remote_dir, files_to_send)?;

    let mut jobs = Jobs::load_jobs(&ctx.json_file)?;
    let internal_id = (jobs.iter().count() + 1) as u16;

    let job = Job::new(
        internal_id,
        id,
        remote.name().to_owned(),
        remote_id,
        manifest.spec.name,
        remote_dir,
        current_dir,
        manifest_path,
        manifest.tags,
        manifest.spec.queue,
    )?;

    info!(
        "Job {} with id: {} and remote id: {} submitted successfully ",
        job.name(),
        job.id(),
        job.remote_id()
    );

    jobs.add(job);
    jobs.save_jobs(&ctx.json_file)?;
    Ok(())
}
