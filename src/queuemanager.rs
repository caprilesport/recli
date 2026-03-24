use crate::job::JobStatus;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

static RE_PUEUE_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"id (\d+)").unwrap());
static RE_SLURM_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d+)").unwrap());

/// Supported queue managers
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum QueueManager {
    Pbs,
    Slurm,
    Pueue,
}

#[derive(Debug, Default, PartialEq)]
pub struct ScriptDirectives {
    pub name: Option<String>,
    pub queue: Option<String>,
}

impl QueueManager {
    /// Extracts the job ID from queue manager output.
    ///
    /// Parses the output returned by job submission commands to extract
    /// the unique job identifier for each queue system.
    ///
    /// # Examples
    ///
    /// ```
    /// let manager = QueueManager::Slurm;
    /// let output = "Submitted batch job 12345".to_string();
    /// let job_id = manager.get_id(output);
    /// assert_eq!(job_id, "12345");
    /// ```
    pub fn get_id(&self, output: &str) -> String {
        match self {
            Self::Pbs => output.trim().to_string(),
            Self::Pueue => RE_PUEUE_ID
                .captures(output)
                .and_then(|caps| caps.get(1))
                .map_or_else(String::new, |m| m.as_str().to_string()),
            Self::Slurm => RE_SLURM_ID
                .captures(output)
                .and_then(|caps| caps.get(1))
                .map_or_else(String::new, |m| m.as_str().to_string()),
        }
    }

    /// Parses status command output into job status mappings.
    ///
    /// Converts the raw output from queue manager status commands into
    /// a map of job IDs to their current status. Each queue manager has
    /// its own output format that this method knows how to parse.
    ///
    /// # Examples
    ///
    /// ```
    /// let manager = QueueManager::Pbs;
    /// let output = "12345.username queue jobname user time status".to_string();
    /// let statuses = manager.status(output);
    /// ```
    pub fn status(&self, output: &str) -> HashMap<String, JobStatus> {
        let mut statuses = HashMap::new();

        match self {
            Self::Pbs => {
                for line in output.lines().skip(5) {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 10 {
                        let job_id = parts[0].to_string();
                        let status = parts[9].to_string();
                        let job_status = match status.as_str() {
                            "Q" => JobStatus::Queued,
                            "R" => JobStatus::Running,
                            "F" => JobStatus::Finished,
                            "E" => JobStatus::Error,
                            _ => JobStatus::Undefined,
                        };
                        statuses.insert(job_id, job_status);
                    }
                }
            }
            Self::Pueue => {
                for line in output.lines().skip(4) {
                    // Skip header and separator lines
                    if line.starts_with("───") || line.starts_with("═") || line.is_empty() {
                        continue;
                    }
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 2 {
                        let job_id = parts[0].to_string();
                        let status = parts[1].to_string();
                        let job_status = match status.as_str() {
                            "Q" | "Queued" => JobStatus::Queued,
                            "R" | "Running" => JobStatus::Running,
                            "F" | "Success" | "Killed" => JobStatus::Finished,
                            "E" | "Failed" => JobStatus::Error,
                            _ => JobStatus::Undefined,
                        };
                        statuses.insert(job_id, job_status);
                    }
                }
            }
            Self::Slurm => {
                for line in output.lines().skip(2) {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() < 5 {
                        continue;
                    }
                    let job_id = parts[0].to_string();
                    let status = parts[4].to_string();
                    let job_status = match status.as_str() {
                        "PENDING" => JobStatus::Queued,
                        "RUNNING" => JobStatus::Running,
                        "COMPLETED" => JobStatus::Finished,
                        "FAILED" | "CANCELLED" | "CANCELLED+" | "NODE_FAIL" | "TIMEOUT"
                        | "OUT_OF_MEMORY" => JobStatus::Error,
                        _ => JobStatus::Undefined,
                    };
                    statuses.insert(job_id, job_status);
                }
            }
        }

        statuses
    }

    /// Returns the command to check job statuses for a user.
    ///
    /// Each queue manager has different commands and flags for retrieving
    /// job status information.
    ///
    /// # Arguments
    ///
    /// * `user` - The username to filter jobs by (not all queue managers support this)
    pub fn status_command(&self, user: &str) -> String {
        match self {
            Self::Pbs => format!("qstat -u {user} -x"),
            Self::Pueue => "pueue status".to_string(),
            //TODO: check this starttime
            Self::Slurm => "sacct -X --starttime 1970-01-01".to_string(),
        }
    }

    /// Returns a list of (label, command) pairs that together produce the job log output.
    ///
    /// Each pair represents a named section (e.g. "stdout", "stderr") and the shell
    /// command to retrieve it. Sections that produce no output or error are silently skipped.
    pub fn log_commands(
        &self,
        remote_dir: &Path,
        remote_id: &str,
        script_file: &Path,
    ) -> Vec<(String, String)> {
        match self {
            Self::Pbs => {
                // PBS IDs are like "12345.server"; filenames use only the numeric part.
                let short_id = remote_id.split('.').next().unwrap_or(remote_id);
                let script_name = script_file.file_name().unwrap_or_default().display();
                let stdout = shell_quote(&format!(
                    "{}/{}.o{short_id}",
                    remote_dir.display(),
                    script_name
                ));
                let stderr = shell_quote(&format!(
                    "{}/{}.e{short_id}",
                    remote_dir.display(),
                    script_name
                ));
                vec![
                    ("stdout".to_string(), format!("cat {stdout}")),
                    ("stderr".to_string(), format!("cat {stderr}")),
                ]
            }
            Self::Slurm => {
                let stdout =
                    shell_quote(&format!("{}/slurm.{remote_id}.out", remote_dir.display()));
                let stderr =
                    shell_quote(&format!("{}/slurm.{remote_id}.err", remote_dir.display()));
                vec![
                    ("stdout".to_string(), format!("cat {stdout}")),
                    ("stderr".to_string(), format!("cat {stderr}")),
                ]
            }
            Self::Pueue => {
                vec![(
                    "log".to_string(),
                    format!("pueue log {}", shell_quote(remote_id)),
                )]
            }
        }
    }

    /// Returns the command to cancel a job in the queue.
    pub fn cancel_command(&self, remote_id: &str) -> String {
        let remote_id = shell_quote(remote_id);
        match self {
            Self::Pbs => format!("qdel {remote_id}"),
            Self::Slurm => format!("scancel {remote_id}"),
            Self::Pueue => format!("pueue kill {remote_id}"),
        }
    }

    /// Returns the command to submit a job to the queue.
    ///
    /// Constructs the appropriate submission command for the queue manager,
    /// including changing to the correct directory and specifying the job script.
    /// # Arguments
    ///
    /// * `remote_dir` - The working directory where the job should be executed
    /// * `job_name` - The name of the job script file to submit
    pub fn submit_command(&self, remote_dir: &Path, job_name: &str) -> String {
        let remote_dir = shell_quote(&remote_dir.to_string_lossy());
        let job_name = shell_quote(job_name);
        match self {
            Self::Pbs => format!("cd {remote_dir} && qsub {job_name}"),
            Self::Pueue => format!("pueue add --working-directory {remote_dir} -- ./{job_name}"),
            Self::Slurm => format!("cd {remote_dir} && sbatch {job_name}"),
        }
    }

    /// Parses job name and queue from a script's header directives.
    ///
    /// Returns `ScriptDirectives::default()` (all `None`) if the queue manager
    /// does not use script headers (Pueue) or if the relevant directives are absent.
    pub fn parse_directives(&self, content: &str) -> ScriptDirectives {
        match self {
            Self::Pbs => {
                let mut name = None;
                let mut queue = None;
                for line in content.lines() {
                    let line = line.trim();
                    if !line.starts_with("#PBS") {
                        continue;
                    }
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    match parts.as_slice() {
                        [_, "-q", q, ..] => queue = Some((*q).to_string()),
                        [_, "-N", n, ..] => name = Some((*n).to_string()),
                        _ => {}
                    }
                }
                ScriptDirectives { name, queue }
            }
            Self::Slurm => {
                let mut name = None;
                let mut queue = None;
                for line in content.lines() {
                    let line = line.trim();
                    if !line.starts_with("#SBATCH") {
                        continue;
                    }
                    let rest = line["#SBATCH".len()..].trim();
                    if let Some(val) = rest
                        .strip_prefix("-J ")
                        .or_else(|| rest.strip_prefix("--job-name="))
                    {
                        name = Some(val.trim().to_string());
                    } else if let Some(val) = rest
                        .strip_prefix("-p ")
                        .or_else(|| rest.strip_prefix("--partition="))
                    {
                        queue = Some(val.trim().to_string());
                    }
                }
                ScriptDirectives { name, queue }
            }
            Self::Pueue => ScriptDirectives::default(),
        }
    }
}

/// Wraps a string in single quotes for safe shell interpolation.
/// Internal single quotes are escaped using the `'\''` idiom.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_get_id() {
        let pbs_output = "12345.server".to_string();
        assert_eq!(QueueManager::Pbs.get_id(&pbs_output), "12345.server");

        let pueue_output = "New task added (id 2).".to_string();
        assert_eq!(QueueManager::Pueue.get_id(&pueue_output), "2");

        let slurm_output = "Submitted batch job 67890".to_string();
        assert_eq!(QueueManager::Slurm.get_id(&slurm_output), "67890");
    }

    #[test]
    fn test_pbs_status_parsing() {
        let pbs_output = r#"
ufsc:
                                                            Req'd  Req'd   Elap
Job ID          Username Queue    Jobname    SessID NDS TSK Memory Time  S Time
--------------- -------- -------- ---------- ------ --- --- ------ ----- - -----
12345.server    testuser  small   ts-produc* 13716*   1   8   11gb 10000 Q 2345:
12346.server    testuser  big     solvation* 33526*   1  16   28gb 10000 R 104:4
12347.server    testuser  big     init.job   24242*   1  16   30gb 10000 F 497:3
12348.server    testuser  big     error.job  11111*   1  16   30gb 10000 E 001:0
12349.server    testuser  big     weird.job  22222*   1  16   30gb 10000 X 000:0
"#;
        let statuses = QueueManager::Pbs.status(pbs_output);
        let mut expected = HashMap::new();
        expected.insert("12345.server".to_string(), JobStatus::Queued);
        expected.insert("12346.server".to_string(), JobStatus::Running);
        expected.insert("12347.server".to_string(), JobStatus::Finished);
        expected.insert("12348.server".to_string(), JobStatus::Error);
        expected.insert("12349.server".to_string(), JobStatus::Undefined);

        assert_eq!(statuses, expected);
    }

    #[test]
    fn test_pbs_status_parsing_empty() {
        assert!(QueueManager::Pbs.status("").is_empty());
    }

    #[test]
    fn test_pueue_status_parsing() {
        let pueue_output = r#"
Group "default" (1 parallel): running
─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
 Id   Status    Command                                          Path                                                                     Start      End
═════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════
 0    Running   /home/vport/projects/scripts/job -v 5 init.inp   /home/vport/projects/calculations/9d90ca4e-72bb-4974-8c23-6a182791e216   12:47:13
─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
"#;
        let statuses = QueueManager::Pueue.status(pueue_output);
        let mut expected = HashMap::new();
        expected.insert("0".to_string(), JobStatus::Running);

        assert_eq!(statuses, expected);
    }

    #[test]
    fn test_pueue_status_parsing_multiple() {
        // Covers all status variants including commands with spaces in the path.
        let pueue_output = r#"
Group "default" (4 parallel): running
─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
 Id   Status    Command                                          Path                                                                     Start      End
═════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════
 0    Running   /home/user/scripts/job -v 5 init.inp             /remote/work/uuid-a                                                      12:47:13
 1    Queued    /home/user/scripts/job -v 5 other.inp            /remote/work/uuid-b
 2    Success   /home/user/scripts/job -v 5 done.inp             /remote/work/uuid-c                                                      11:00:00   11:05:00
 3    Failed    /home/user/scripts/job -v 5 fail.inp             /remote/work/uuid-d                                                      10:00:00   10:01:00
 4    Killed    /home/user/scripts/job -v 5 kill.inp             /remote/work/uuid-e                                                      09:00:00   09:00:30
─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
"#;
        let statuses = QueueManager::Pueue.status(pueue_output);
        let mut expected = HashMap::new();
        expected.insert("0".to_string(), JobStatus::Running);
        expected.insert("1".to_string(), JobStatus::Queued);
        expected.insert("2".to_string(), JobStatus::Finished);
        expected.insert("3".to_string(), JobStatus::Error);
        expected.insert("4".to_string(), JobStatus::Finished);

        assert_eq!(statuses, expected);
    }

    #[test]
    fn test_pueue_status_parsing_empty() {
        assert!(QueueManager::Pueue.status("").is_empty());
    }

    #[test]
    fn test_slurm_status_parsing() {
        // NOTE: the current parser assumes State is at whitespace-split column 4
        // (format: JobID JobName Partition Account State).
        let slurm_output = r#"       JobID    JobName  Partition    Account      State
------------ ---------- ---------- ---------- ----------
       12345      myjob    compute     grpname  COMPLETED
       12346  otherjob     compute     grpname    RUNNING
       12347  failedjob    compute     grpname     FAILED
       12348  queuedjob    compute     grpname    PENDING
       12349  canceljob    compute     grpname  CANCELLED
       12350  nodefail     compute     grpname  NODE_FAIL
       12351  unknownjob   compute     grpname   WHATEVER
"#;
        let statuses = QueueManager::Slurm.status(slurm_output);
        let mut expected = HashMap::new();
        expected.insert("12345".to_string(), JobStatus::Finished);
        expected.insert("12346".to_string(), JobStatus::Running);
        expected.insert("12347".to_string(), JobStatus::Error);
        expected.insert("12348".to_string(), JobStatus::Queued);
        expected.insert("12349".to_string(), JobStatus::Error);
        expected.insert("12350".to_string(), JobStatus::Error);
        expected.insert("12351".to_string(), JobStatus::Undefined);

        assert_eq!(statuses, expected);
    }

    #[test]
    fn test_slurm_status_parsing_empty() {
        assert!(QueueManager::Slurm.status("").is_empty());
    }

    #[test]
    fn test_submit_command_special_chars() {
        // Paths with spaces and single quotes must be safely shell-quoted.
        let dir_with_spaces = Path::new("/remote/my jobs/uuid-1");
        let script_with_quote = "it's a job.pbs";
        assert_eq!(
            QueueManager::Pbs.submit_command(dir_with_spaces, script_with_quote),
            "cd '/remote/my jobs/uuid-1' && qsub 'it'\\''s a job.pbs'"
        );
        assert_eq!(
            QueueManager::Slurm.submit_command(dir_with_spaces, script_with_quote),
            "cd '/remote/my jobs/uuid-1' && sbatch 'it'\\''s a job.pbs'"
        );
        assert_eq!(
            QueueManager::Pueue.submit_command(dir_with_spaces, script_with_quote),
            "pueue add --working-directory '/remote/my jobs/uuid-1' -- ./'it'\\''s a job.pbs'"
        );
    }

    #[test]
    fn test_parse_pbs_directives() {
        let script = "#!/bin/bash\n#PBS -l nodes=1:ppn=8\n#PBS -l walltime=720:00:00\n#PBS -l mem=11gb\n#PBS -V\n#PBS -q small\n#PBS -N myjobname\n";
        let d = QueueManager::Pbs.parse_directives(script);
        assert_eq!(d.queue, Some("small".to_string()));
        assert_eq!(d.name, Some("myjobname".to_string()));
    }

    #[test]
    fn test_parse_pbs_no_directives() {
        let script = "#!/bin/bash\n#PBS -l nodes=1:ppn=8\n#PBS -V\n";
        let d = QueueManager::Pbs.parse_directives(script);
        assert_eq!(d.queue, None);
        assert_eq!(d.name, None);
    }

    #[test]
    fn test_parse_pbs_empty_queue_value() {
        // "#PBS -q" with no value should not produce Some("") — stays None
        let script = "#!/bin/bash\n#PBS -q\n#PBS -N myjob\n";
        let d = QueueManager::Pbs.parse_directives(script);
        assert_eq!(d.queue, None);
        assert_eq!(d.name, Some("myjob".to_string()));
    }

    #[test]
    fn test_parse_slurm_directives() {
        let script = "#!/bin/bash\n#SBATCH -J slurm_job\n#SBATCH -p compute\n#SBATCH --ntasks=4\n";
        let d = QueueManager::Slurm.parse_directives(script);
        assert_eq!(d.name, Some("slurm_job".to_string()));
        assert_eq!(d.queue, Some("compute".to_string()));
    }

    #[test]
    fn test_parse_slurm_long_flags() {
        let script = "#!/bin/bash\n#SBATCH --job-name=long_name\n#SBATCH --partition=gpu\n";
        let d = QueueManager::Slurm.parse_directives(script);
        assert_eq!(d.name, Some("long_name".to_string()));
        assert_eq!(d.queue, Some("gpu".to_string()));
    }

    #[test]
    fn test_parse_pueue_directives_always_empty() {
        let script = "#!/bin/bash\n#PBS -q small\n#SBATCH -J name\n";
        let d = QueueManager::Pueue.parse_directives(script);
        assert_eq!(d, ScriptDirectives::default());
    }

    #[test]
    fn test_status_command() {
        assert_eq!(
            QueueManager::Pbs.status_command("testuser"),
            "qstat -u testuser -x"
        );
        assert_eq!(
            QueueManager::Pueue.status_command("testuser"),
            "pueue status"
        );
        assert_eq!(
            QueueManager::Slurm.status_command("testuser"),
            "sacct -X --starttime 1970-01-01"
        );
    }

    #[test]
    fn test_submit_command() {
        let remote_dir = Path::new("/remote/work/job1");
        let job_name = "script.job";
        assert_eq!(
            QueueManager::Pbs.submit_command(remote_dir, job_name),
            "cd '/remote/work/job1' && qsub 'script.job'"
        );
        assert_eq!(
            QueueManager::Pueue.submit_command(remote_dir, job_name),
            "pueue add --working-directory '/remote/work/job1' -- ./'script.job'"
        );
        assert_eq!(
            QueueManager::Slurm.submit_command(remote_dir, job_name),
            "cd '/remote/work/job1' && sbatch 'script.job'"
        );
    }
}
