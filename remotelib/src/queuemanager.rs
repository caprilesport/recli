use crate::job::JobStatus;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum QueueManager {
    PBS,
    Slurm,
    Pueue,
}

impl QueueManager {
    pub fn get_id(&self, output: String) -> String {
        match self {
            Self::PBS => output.trim().to_string(),
            Self::Pueue => {
                let re = Regex::new(r"id (\d+)").unwrap();
                re.captures(&output)
                    .and_then(|caps| caps.get(1))
                    .map_or_else(|| "".to_string(), |m| m.as_str().to_string())
            }
            Self::Slurm => {
                let re = Regex::new(r"(\d+)").unwrap();
                re.captures(&output)
                    .and_then(|caps| caps.get(1))
                    .map_or_else(|| "".to_string(), |m| m.as_str().to_string())
            }
        }
    }

    pub fn status(&self, output: String) -> HashMap<String, JobStatus> {
        let mut statuses = HashMap::new();

        match self {
            Self::PBS => {
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
                    let job_id = parts[0].to_string();
                    let status = parts[4].to_string();
                    let job_status = match status.as_str() {
                        "PENDING" => JobStatus::Queued,
                        "RUNNING" => JobStatus::Running,
                        "COMPLETED" => JobStatus::Finished,
                        "CANCELLED" | "CANCELLED+" => JobStatus::Error,
                        _ => JobStatus::Undefined,
                    };
                    statuses.insert(job_id, job_status);
                }
            }
        }

        statuses
    }

    pub fn status_command(&self, user: &str) -> String {
        match self {
            Self::PBS => format!("qstat -u {} -x", user),
            Self::Pueue => "pueue status".to_string(),
            //TODO: check this starttime
            Self::Slurm => "sacct -X --starttime 1970-01-01".to_string(),
        }
    }

    pub fn submit_command(&self, remote_dir: &Path, job_name: &str) -> String {
        match self {
            Self::PBS => format!("cd {} && qsub {} ", remote_dir.to_str().unwrap(), job_name),
            Self::Pueue => format!("cd {} && . ./{} ", remote_dir.to_str().unwrap(), job_name),
            Self::Slurm => format!(
                "cd {} && sbatch {} ",
                remote_dir.to_str().unwrap(),
                job_name
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_get_id() {
        let pbs_output = "12345.server".to_string();
        assert_eq!(QueueManager::PBS.get_id(pbs_output), "12345.server");

        let pueue_output = "New task added (id 2).".to_string();
        assert_eq!(QueueManager::Pueue.get_id(pueue_output), "2");

        let slurm_output = "Submitted batch job 67890".to_string();
        assert_eq!(
            QueueManager::Slurm.get_id(slurm_output),
            "Submitted batch job 67890"
        );
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
"#;
        let statuses = QueueManager::PBS.status(pbs_output.to_string());
        let mut expected = HashMap::new();
        expected.insert("12345.server".to_string(), JobStatus::Queued);
        expected.insert("12346.server".to_string(), JobStatus::Running);
        expected.insert("12347.server".to_string(), JobStatus::Finished);

        assert_eq!(statuses, expected);
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
        let statuses = QueueManager::Pueue.status(pueue_output.to_string());
        let mut expected = HashMap::new();
        expected.insert("0".to_string(), JobStatus::Running);

        assert_eq!(statuses, expected);
    }

    #[test]
    fn test_status_command() {
        assert_eq!(
            QueueManager::PBS.status_command("testuser"),
            "qstat -u testuser -x"
        );
        assert_eq!(
            QueueManager::Pueue.status_command("testuser"),
            "pueue status"
        );
        assert_eq!(QueueManager::Slurm.status_command("testuser"), "");
    }

    #[test]
    fn test_submit_command() {
        let remote_dir = Path::new("/remote/work/job1");
        let job_name = "script.job";
        assert_eq!(
            QueueManager::PBS.submit_command(remote_dir, job_name),
            "cd /remote/work/job1 && qsub script.job "
        );
        assert_eq!(
            QueueManager::Pueue.submit_command(remote_dir, job_name),
            "cd /remote/work/job1 && . ./script.job "
        );
        assert_eq!(
            QueueManager::Slurm.submit_command(remote_dir, job_name),
            "cd /remote/work/job1 && sbatch script.job "
        );
    }
}
