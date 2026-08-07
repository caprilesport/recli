# recli

`recli` is a command-line tool for submitting, monitoring, and retrieving
computational jobs on remote machines. It provides one local workflow for PBS,
Slurm, and [Pueue](https://github.com/Nukesor/pueue), keeps a persistent record
of submitted jobs, and transfers job files over SSH.

> [!NOTE]
> `recli` is currently pre-release software. Test it with your environment
> before relying on it for important workloads.

## Features

- Submit one or more job scripts to PBS, Slurm, or Pueue.
- Track jobs from multiple remotes in a local SQLite database.
- Refresh job status and download completed job output.
- Select synced files by differente strategies.
- Inspect queues and logs, cancel jobs, resubmit jobs, and remove old remote data.

## Requirements

- [Rust 1.85 or newer](https://www.rust-lang.org/tools/install) for installation
  from source.
- SSH access to each configured remote.
- PBS, Slurm, or Pueue installed on the remote and available in its command
  search path.

Authentication is attempted with an explicitly configured identity file, the
SSH agent, the default `~/.ssh/id_ed25519` and `~/.ssh/id_rsa` keys, and finally
a password prompt.

For ease-of-use, it's recommended to have [automatic authentic](https://askubuntu.com/questions/46930/how-can-i-set-up-password-less-ssh-login)
enabled for a smoother experience. 2FA workflows are not yet smoothly supported,
but a daemon rewrite of the connection is planned.

## Installation

Install the latest development version from GitHub:

```bash
cargo install --git https://github.com/caprilesport/recli
```

Or build from a local clone:

```bash
git clone https://github.com/caprilesport/recli.git
cd recli
cargo install --path .
```

## Configuration

Before running `recli`, create its configuration directory and
`config.toml`. On Linux these are normally located at
`~/.config/recli/config.toml`; the base configuration directory follows the
XDG conventions for each operating system.

```bash
mkdir -p ~/.config/recli
```

Remotes are configured as an array of TOML tables.

```toml
[[remotes]]
name = "cluster"
hostname = "cluster.example.edu"
port = 22
user = "alice"
work_directory = "/scratch/alice/recli" # where files are sent to and ran
queue_manager = "Slurm"
identity_file = "/home/alice/.ssh/id_ed25519"
check_queue = true

[[remotes]]
name = "local-pueue"
hostname = "localhost"
port = 22
user = "alice"
work_directory = "/home/alice/recli-jobs"
queue_manager = "Pueue"

[settings]
# One of: "script", "basename", or "directory".
file_strategy = "script"
# Minimum age since synchronization before a job is eligible for pruning.
prune_after_days = 90

[display]
# Use 0 to show full paths.
path_components = 3
datetime_format = "%Y-%m-%d %H:%M"
# Use 0 to show all tags.
max_tags = 0
# Use 0 to retain all synchronized jobs in the default status view.
status_window_hours = 48
columns = [
  "id",
  "work_dir",
  "name",
  "status",
  "remote",
  "queue",
  "submit_time",
  "sync_time",
  "tags",
]
```

`identity_file` and `check_queue` are optional. `check_queue` defaults to
`false`; it controls which remotes are queried by `recli queue` when no remote
names are supplied. Valid queue-manager values are `Pbs`, `Slurm`, and `Pueue`.

### Ignore patterns

Optionally create `~/.config/recli/ignore` with one glob pattern per line. These
patterns apply when uploading with the `directory` strategy and when downloading
job output:

```text
*.tmp
*.checkpoint
large-output-*
```

Pass `--all-files` to `recli pull` to ignore this file for that download.

## Quick start

Submit a Slurm or PBS script:

```bash
recli submit --remote cluster simulation.slurm
```

Submit a script together with explicit input files and tags:

```bash
recli submit --remote cluster simulation.slurm \
  --files model.inp parameters.toml \
  --tags production benchmark
```

Refresh the locally stored status of tracked jobs:

```bash
recli fetch
recli fetch --remote cluster
```

Display the local job database. Running `recli` without a subcommand is
equivalent to `recli status`:

```bash
recli status
recli status --remote cluster --status running
recli status --all
```

Download output from finished, unsynchronized jobs:

```bash
recli pull
```

Commands accept an unambiguous prefix of the local job UUID where a job ID is
required:

```bash
recli info 7a91c2e
recli log 7a91c2e
recli cancel 7a91c2e
recli resubmit 7a91c2e
recli pull 7a91c2e
```

Use `recli <command> --help` for all options.

## Commands

| Command | Description |
|---|---|
| `status` | Show and filter jobs stored in the local database. |
| `submit` | Upload files, submit job scripts, and record the jobs locally. |
| `resubmit` | Resubmit a tracked job, optionally to another remote. |
| `fetch` | Query queue managers and update locally stored statuses. |
| `pull` | Refresh statuses and download finished, unsynchronized jobs. |
| `queue` | Show the live queue output from selected remotes. |
| `info` | Show the complete local record for one job. |
| `log` | Read a job's default or explicitly selected remote log file. |
| `cancel` | Request cancellation of a queued or running job. |
| `prune` | Remove old remote job directories; dry-run unless `--execute` is used. |
| `set` | Correct editable fields in a local job record. |
| `completions` | Generate a shell completion script. |

`status`, `info`, and other commands that do not explicitly contact a remote
show locally cached information. Run `recli fetch` to refresh it. Cancellation
also does not immediately change the cached status.

## File selection and synchronization

Every submitted script receives a UUID and its own directory beneath the
remote's configured `work_directory`. The upload strategy determines which
files accompany it:

- `script`: upload only the job script.
- `basename`: also upload files in the script's directory whose names begin
  with the same filestem.
- `directory`: also upload all regular files in the script's directory, subject
  to the ignore file.

Files supplied with `--files` are added regardless of the selected strategy.
Uploads and downloads compare modification times and skip files that do not
appear newer.

## Local data

Tracked jobs are stored in `jobs.db` alongside `config.toml` in the `recli`
configuration directory. Each record includes its UUID, scheduler ID, remote,
local and remote directories, submitted files, tags, queue, status, and submit
and synchronization times.

## Current limitations

- Each command invocation establishes a new SSH connection; persistent SSH
  sessions are not implemented yet.
- Queue status is parsed from the queue managers' human-readable command output,
  which may differ between versions or cluster configurations.
- File synchronization is based on modification times rather than content
  hashes.

## Contributing

Bug reports and pull requests are welcome. Before submitting a change, run:

```bash
cargo fmt -- --check
cargo clippy --all-targets --all-features
cargo test
```

## License

Licensed under the [MIT License](LICENSE).
