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
        let connection = match ctx.connect(remote) {
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

    if ctx.json_output() {
        let json_output: Vec<_> = results
            .iter()
            .map(|(name, res)| match res {
                Ok(output) => serde_json::json!({ "remote": name, "output": output }),
                Err(e) => serde_json::json!({ "remote": name, "error": e }),
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json_output)?);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::test_support::MockFactory;
    use crate::context::Context;
    use std::path::PathBuf;

    fn make_context_with_check_queue(
        db_path: PathBuf,
        factory: MockFactory,
        check_queue: bool,
    ) -> Context {
        let check_queue_str = if check_queue { "true" } else { "false" };
        let toml = format!(
            r#"
            [[remotes]]
            name = "test"
            hostname = "localhost"
            port = 22
            user = "testuser"
            work_directory = "/remote/work"
            queue_manager = "Pbs"
            check_queue = {check_queue_str}
            "#
        );
        let config: crate::config::Config = toml::from_str(&toml).unwrap();
        Context::with_factory(config, db_path, Box::new(factory))
    }

    #[test]
    fn test_queue_explicit_remote_runs_qstat() {
        let tmp_db = tempfile::NamedTempFile::new().unwrap();
        let (factory, state) = MockFactory::new();
        state.lock().unwrap().set_output("qstat", "job output");

        execute(
            Args {
                remotes: vec!["test".to_string()],
            },
            make_context_with_check_queue(tmp_db.path().to_path_buf(), factory, false),
        )
        .unwrap();

        let commands = &state.lock().unwrap().commands;
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0], "qstat");
    }

    #[test]
    fn test_queue_uses_check_queue_filter() {
        let tmp_db = tempfile::NamedTempFile::new().unwrap();
        let (factory, state) = MockFactory::new();

        execute(
            Args { remotes: vec![] },
            make_context_with_check_queue(tmp_db.path().to_path_buf(), factory, true),
        )
        .unwrap();

        let commands = &state.lock().unwrap().commands;
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0], "qstat");
    }

    #[test]
    fn test_queue_no_check_queue_remotes_prints_message() {
        let tmp_db = tempfile::NamedTempFile::new().unwrap();
        let (factory, state) = MockFactory::new();

        execute(
            Args { remotes: vec![] },
            make_context_with_check_queue(tmp_db.path().to_path_buf(), factory, false),
        )
        .unwrap();

        // No connection made when no remotes qualify
        assert!(state.lock().unwrap().commands.is_empty());
    }
}
