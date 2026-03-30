use crate::connection::{RemoteConnection, SshConnection};
use tracing::warn;

/// Shows the live queue state on one or more remotes.
///
/// If no remotes are given, checks all remotes that have `check_queue = true`
/// in the config. Use `recli queue <remote>...` to inspect specific remotes
/// regardless of that setting.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Remotes to query (defaults to all with `check_queue` = true)
    remotes: Vec<String>,
}

#[allow(clippy::needless_pass_by_value)]
pub fn execute(args: Args, ctx: crate::context::Context) -> color_eyre::Result<()> {
    let targets: Vec<_> = if args.remotes.is_empty() {
        ctx.config()
            .remotes
            .iter()
            .filter(|r| r.check_queue())
            .collect()
    } else {
        args.remotes
            .iter()
            .map(|name| ctx.config().get_remote(name))
            .collect::<Result<_, _>>()?
    };

    if targets.is_empty() {
        println!(
            "No remotes to check. Set check_queue = true in config.toml or pass remote names explicitly."
        );
        return Ok(());
    }

    let mut results = Vec::new();

    for remote in &targets {
        let connection = match SshConnection::new(remote) {
            Ok(c) => c,
            Err(e) => {
                warn!("Failed to connect to {}: {e}", remote.name());
                results.push((remote.name().to_owned(), Err(e.to_string())));
                continue;
            }
        };
        let cmd = remote.queue_manager().queue_command();
        match connection.execute(cmd) {
            Ok(output) => results.push((remote.name().to_owned(), Ok(output))),
            Err(e) => results.push((remote.name().to_owned(), Err(e.to_string()))),
        }
    }

    if ctx.json() {
        let json: Vec<_> = results
            .iter()
            .map(|(name, res)| match res {
                Ok(output) => serde_json::json!({ "remote": name, "output": output }),
                Err(e) => serde_json::json!({ "remote": name, "error": e }),
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json)?);
        return Ok(());
    }

    for (name, res) in &results {
        println!("=== {name} ===");
        match res {
            Ok(output) => print!("{output}"),
            Err(e) => println!("error: {e}"),
        }
        println!();
    }

    Ok(())
}
