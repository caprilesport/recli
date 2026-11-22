# Repository Guidelines

`recli` is a single-binary Rust CLI (edition 2024, Rust 1.85+) for submitting, tracking, and retrieving jobs on remote PBS, Slurm, or Pueue machines over SSH. User-facing configuration and command docs are in `README.md`.

## Build, Test, and Development Commands

```bash
cargo build
cargo run -- status                  # any subcommand; no subcommand == status
cargo test                           # full suite
cargo test test_slurm_status_parsing # single test by name substring
cargo test queuemanager::            # all tests in one module
cargo fmt --check
cargo clippy --all-targets -- -D clippy::pedantic
```

CI (`.github/workflows/ci.yml`) and the pre-commit hook (`.githooks/pre-commit`) run exactly the fmt check, pedantic clippy as errors, and tests. Code must pass `clippy::pedantic`; prefer fixing lints over `#[allow]`. Enable the hook with `git config core.hooksPath .githooks`.

`build.rs` uses `bosion` to generate build info (commit hash, dates) that `main.rs` includes for `--version` and color-eyre issue metadata.

## Architecture

`main.rs` parses the clap `Cli`, builds a `Context` (config, `jobs.db` path, `--json` flag), and dispatches to `commands::<name>::execute(args, ctx)`. Each module in `src/commands/` owns its `clap::Args` struct and `execute`. `completions` runs before the config is read, so it works without a config file.

Layers, from bottom up:

- **`connection.rs`**: the `RemoteConnection` trait (`execute`, `mkdir`, `upload_files`, `download_files`, `remove_dir`) and its SSH implementation (`ssh2`; auth order: identity file, agent, default keys, password prompt). Uploads and downloads skip files by mtime comparison and ignore globs.
- **`queuemanager.rs`**: the `QueueManager` enum (`Pbs`/`Slurm`/`Pueue`). It only builds shell command strings and parses their output (job ID from submit output, status map, `#PBS`/`#SBATCH` directives). No I/O.
- **`remote.rs`**: `Remote`, deserialized from `[[remotes]]` in the config, combines a `QueueManager` with a `&dyn RemoteConnection` to submit, query status, read logs, and cancel.
- **`job.rs` / `jobs.rs`**: the `Job` record and `JobStatus`. `Jobs` wraps `Vec<Job>` loaded from SQLite (`rusqlite`, bundled); every mutation is persisted immediately through per-field `update_*` methods keyed by UUID. `Jobs::update` applies a remote status map to unsynced jobs of one remote, `syncable()` returns unsynced jobs in terminal states grouped by remote, `query()` returns a `JobQuery` filter builder, and `find_by_prefix` resolves user-supplied UUID prefixes.
- **`config.rs`**: reads `~/.config/recli/config.toml` (`[[remotes]]`, `[settings]`, `[display]`). `jobs.db` and the `ignore` file live in the same directory.

Behavior that spans modules:

- A job is synced when it has a `sync_time`. Synced jobs are skipped by status updates and by `pull`. A status change into a state other than Queued/Running clears the sync time (`Job::set_status`), because a download made before completion isn't a final sync.
- `fetch` queries remotes in parallel with `rayon`, sharing `Jobs` behind `Arc<Mutex<_>>`. `pull` reuses `fetch::fetch_statuses`, then downloads the `syncable()` jobs.
- Each submitted script gets a UUID and its own directory under the remote's `work_directory`. Companion files are chosen by `FileStrategy` (`script` / `basename` / `directory`) in `commands/submit.rs`, plus any explicit `--files`.
- `--json` is a global flag; commands that support it print serde JSON instead of a `comfy-table` table.

## Coding Style

Keep code `cargo fmt` clean. Use `snake_case` for functions, modules, and variables and `PascalCase` for types. Follow the existing style: small modules, explicit error enums via `thiserror` surfaced through `color_eyre::Result` in each command's `execute`, and minimal inline comments unless a block is non-obvious.

## Testing

Unit tests live next to the code they cover under `#[cfg(test)]`; there is no `tests/` directory. Name tests `test_<behavior>`. Mock `RemoteConnection` instead of using real SSH access (see `MockConnection` in `remote.rs`). Add regression tests for parser edge cases, DB persistence, and remote-command failures when behavior changes. Parser fixtures should be real queue-manager output, not hand-written approximations.

## Commits and Pull Requests

History uses lightweight conventional commits such as `fix(job): ...`, `tests: ...`, and `chore: ...`, with an imperative summary. Keep commits scoped. PRs should explain the user-visible change, note config or protocol impacts, and mention test coverage. For parser or queue-manager changes, include the failure mode or design rationale, not just the diff.

## Security and Configuration

Don't commit personal cluster settings or SSH material. Local runtime data lives under `~/.config/recli/` (`config.toml`, `ignore`, `jobs.db`). When changing remote command construction, keep shell-escaping (`shell_quote` in `queuemanager.rs`) and treat queue-manager output as untrusted input.
