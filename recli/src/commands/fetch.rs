use crate::Context;
use anyhow::Result;
use remotelib::connection::SshConnection;
use remotelib::jobs::Jobs;

// #[derive(Debug, clap::Args)]
// pub struct Args;

pub fn execute(ctx: &Context) -> Result<()> {
    let mut jobs = Jobs::load_jobs(&ctx.json_file)?;

    for remote in &ctx.config.remotes {
        let connection = SshConnection::new(&remote)?;
        let statuses = remote.status(&connection)?;
        jobs.update(statuses, remote.name());
    }

    jobs.save_jobs(&ctx.json_file)?;

    Ok(())
}
