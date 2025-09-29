# recli

A powerful command-line tool for submitting, monitoring, and managing
computational jobs on remote HPC clusters and servers.

## Overview

recli simplifies the workflow of working with remote high-performance computing
resources by providing:

- Easy job submission to multiple remote clusters

- Real-time status monitoring across different queue managers

- Intelligent file synchronization that only transfers changed files

- Unified interface for PBS, Slurm, and Pueue queue systems

## Installation

Right now the only way to install recli is through cloning the repository.
Ensure the [rust toolchain](https://rustup.rs/) is already installed before
proceeding.

```bash
# Build from source
git clone https://github.com/your-username/recli
cd recli
cargo install --path .
```

## Quick Start

- Configure your remotes by editing the config file (automatically created on
  first run):

```bash
recli status # This will create a default config if none exists
```

Submit a job to a remote cluster:

```bash
recli submit --remote my-cluster simulation.inp
```

Check the status of all jobs across configured remotes.

```bash
recli status --remote specific-cluster # Filter by remote
```

Check all of the jobs statuses:

```bash
recli status
```

Check for updates on all remotes and update the status of jobs.

```bash
recli fetch # check for all changes
recli fetch --remote <remote> # fetch a single remote
```

Synchronize files from remote clusters. Only transfers files that have changed
since last sync.

```bash
recli sync # Sync all finished jobs
recli sync --job-id <uuid> # Sync specific
recli sync --sync-all-files # Ignore ignore patterns and sync everything
```

Submit a job to a remote cluster. The tool automatically prepares input files,
uploads them, and submits to the queue manager.

```bash
recli submit --remote cluster-name input-file.inp
```

## Configuration

recli uses a toml configuration file located at `~/.config/recli/config.toml`
(Linux) or equivalent platform-specific config directory.

Example configuration:

```toml
[remotes.cluster1]
hostname = "cluster.university.edu"
port = 22
user = "yourusername"
work_directory = "/home/yourusername/jobs"
queue_manager = "Slurm"
prepare_args = ["-t", "template.q"]

[remotes.cluster2]
hostname = "hpc.company.com"
port = 22
user = "yourname"
work_directory = "/scratch/yourname/jobs"
queue_manager = "PBS"
```

Beyond this, one can create an ignore file also located at
`~/.config/recli/ignore`, in which patterns for files that should be ignored
when syncing are checked.

For example, to ignore any files that contain `smd`, add the folowing to
`~/.config/recli/ignore`:

```txt
*smd*
```

## Supported Queue Managers

- Slurm - Popular HPC workload manager
- PBS - Portable Batch System
- Pueue - Modern daemon-based process manager

## Features

- Smart File Sync: Only transfers files that have actually changed
- Multiple Remote Support: Manage jobs across different clusters simultaneously
- Cross-Platform: Works on Linux, macOS, and Windows
- Persistent Job Tracking: Maintains job database between sessions
- Flexible Ignore Patterns: Skip unnecessary files during sync operations
- Comprehensive Logging: Detailed output for debugging and monitoring

## Job Management

Jobs are automatically tracked in a local database (jobs.json). Each job is
assigned a unique UUID and associated with:

- Remote cluster
- Submission time
- Current status (Queued, Running, Finished, Error)
- Sync status
- Remote job ID

## Contributing

Contributions are welcome! Please feel free to submit pull requests or open
issues for bugs and feature requests.

## License

```
MIT License

Copyright (c) 2025 Vinícius C. Port       <caprilesport@gmail.com>

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
