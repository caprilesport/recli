use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::connection::RemoteConnection;
use crate::job::JobStatus;
use crate::queuemanager::QueueManager;

use std::collections::HashMap;

// use tracing::debug;

#[derive(thiserror::Error, std::fmt::Debug)]
pub enum Error {
    #[error("Connection error:\n{0}")]
    Connection(#[from] crate::connection::Error),
    #[error("{0}")]
    IO(#[from] std::io::Error), // #[error()]
}

/// Represents a remote computational resource for job execution.
///
/// Contains all configuration needed to connect to and use a remote machine including connection details, working directory, and queue manager settings.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Remote {
    name: String,
    hostname: String,
    port: u16,
    user: String,
    work_directory: PathBuf,
    queue_manager: QueueManager,
    identity_file: Option<PathBuf>,
    defaults: Option<RemoteDefault>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RemoteDefault {
    pub template: Option<String>,
    pub queue: Option<String>,
    pub nprocs: Option<u32>,
    pub memory: Option<String>,
    pub walltime: Option<String>,
    pub name: Option<String>,
}

impl PartialEq<&str> for Remote {
    fn eq(&self, other: &&str) -> bool {
        self.name == *other
    }
}

impl Remote {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn hostname(&self) -> &str {
        &self.hostname
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn user(&self) -> &str {
        &self.user
    }

    /// Returns the working directory on the remote machine.
    ///
    /// This is the base directory where job directories will be created.
    pub fn work_dir(&self) -> &Path {
        &self.work_directory
    }

    pub fn identity_file(&self) -> Option<PathBuf> {
        self.identity_file.clone()
    }

    // #[cfg(test)]
    // pub fn queue_manager(&self) -> &QueueManager {
    //     &self.queue_manager
    // }

    /// Submits a job to the remote queue manager.
    ///
    /// Creates a remote job directory, uploads necessary files,
    /// and submits the job to the queue manager. Returns the remote job ID.
    ///
    /// # Errors
    ///
    /// Returns an error if there are no permissions to create the remote directory, of upload the files fails, or if the submission command fails.
    pub fn submit(
        &self,
        job_name: &str,
        connection: &dyn RemoteConnection,
        remote_dir: &Path,
        files_to_send: Vec<PathBuf>,
    ) -> Result<String, Error> {
        connection.mkdir(remote_dir)?;
        connection.upload_files(&files_to_send, remote_dir)?;

        // now we need to write the rendered template to a file to submit
        let job_script_name = format!("{}.job", job_name);

        let command = self
            .queue_manager
            .submit_command(remote_dir, &job_script_name);

        let output = connection.execute(&command)?;
        let remote_id = self.queue_manager.get_id(output);

        Ok(remote_id)
    }

    /// Retrieves job statuses from the remote queue manager.
    ///
    /// Queries the queue manager for current job statuses and parses the output
    /// into a map of job IDs to their status.
    ///
    /// # Errors
    ///
    /// Returns an error if the status command fails or output cannot be parsed.
    pub fn status(
        &self,
        connection: &dyn RemoteConnection,
    ) -> Result<HashMap<String, JobStatus>, Error> {
        let command = self.queue_manager.status_command(self.user());
        let output = connection.execute(&command)?;
        Ok(self.queue_manager.status(output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::RemoteConnection;
    use crate::job::JobStatus;
    use crate::queuemanager::QueueManager;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    struct MockConnection {
        commands: RefCell<Vec<String>>,
        uploads: RefCell<Vec<(Vec<PathBuf>, PathBuf)>>,
        downloads: RefCell<Vec<(PathBuf, PathBuf)>>,
        mock_output: RefCell<HashMap<String, String>>,
    }

    impl MockConnection {
        fn new() -> Self {
            MockConnection {
                commands: RefCell::new(Vec::new()),
                uploads: RefCell::new(Vec::new()),
                downloads: RefCell::new(Vec::new()),
                mock_output: RefCell::new(HashMap::new()),
            }
        }
    }

    impl RemoteConnection for MockConnection {
        fn execute(&self, command: &str) -> Result<String, crate::connection::Error> {
            self.commands.borrow_mut().push(command.to_string());
            if let Some(output) = self.mock_output.borrow().get(command) {
                Ok(output.clone())
            } else {
                Ok("".to_string())
            }
        }

        fn mkdir(&self, _path: &Path) -> Result<(), crate::connection::Error> {
            Ok(())
        }

        fn upload_files(
            &self,
            local_paths: &[PathBuf],
            remote_dir: &Path,
        ) -> Result<(), crate::connection::Error> {
            self.uploads
                .borrow_mut()
                .push((local_paths.to_vec(), remote_dir.to_path_buf()));
            Ok(())
        }

        fn download_files(
            &self,
            remote_dir: &Path,
            local_dir: &Path,
            _ignore: &[glob::Pattern],
        ) -> Result<(), crate::connection::Error> {
            self.downloads
                .borrow_mut()
                .push((remote_dir.to_path_buf(), local_dir.to_path_buf()));
            Ok(())
        }
    }

    fn create_test_remote() -> Remote {
        Remote {
            name: "test_remote".to_string(),
            hostname: "localhost".to_string(),
            port: 22,
            user: "testuser".to_string(),
            work_directory: PathBuf::from("/remote/work"),
            queue_manager: QueueManager::Pbs,
            identity_file: None,
            defaults: None,
        }
    }

    // #[test]
    // fn test_submit() {
    //     let remote = create_test_remote();
    //     let connection = MockConnection::new();
    //     let job_id = Uuid::new_v4();
    //     let inp_file = Path::new("test.inp");

    //     // Create a dummy file to be found by the submit function
    //     let _dummy_file = std::fs::File::create(format!(
    //         "{}.job",
    //         inp_file.file_stem().unwrap().to_str().unwrap()
    //     ))
    //     .unwrap();

    //     connection.mock_output.borrow_mut().insert(
    //         format!("cd /remote/work/{} && qsub test.job ", job_id),
    //         "12345.server".to_string(),
    //     );

    //     let job = remote.submit(job_id, inp_file, &connection).unwrap();

    //     assert_eq!(job.remote_id(), "12345.server");
    //     assert_eq!(connection.commands.borrow().len(), 1);
    //     assert_eq!(connection.uploads.borrow().len(), 1);
    // }

    #[test]
    fn test_status() {
        let remote = create_test_remote();
        let connection = MockConnection::new();

        let status_output = r#"
ufsc:
                                                            Req'd  Req'd   Elap
Job ID          Username Queue    Jobname    SessID NDS TSK Memory Time  S Time
--------------- -------- -------- ---------- ------ --- --- ------ ----- - -----
12345.server    testuser  small   ts-produc* 13716*   1   8   11gb 10000 Q 2345:
"#;
        connection.mock_output.borrow_mut().insert(
            "qstat -u testuser -x".to_string(),
            status_output.to_string(),
        );

        let statuses = remote.status(&connection).unwrap();

        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses.get("12345.server"), Some(&JobStatus::Queued));
        assert_eq!(connection.commands.borrow().len(), 1);
    }

    // #[test]
    // fn test_sync() {
    //     let remote = create_test_remote();
    //     let connection = MockConnection::new();
    //     let job_id = Uuid::new_v4();
    //     let job = Job::new(
    //         job_id,
    //         "test_remote".to_string(),
    //         "12345".to_string(),
    //         "test_job".to_string(),
    //     )
    //     .unwrap();

    //     remote.sync(&job, &connection).unwrap();

    //     assert_eq!(connection.downloads.borrow().len(), 1);
    //     let (remote_dir, local_dir, basename) = connection.downloads.borrow()[0].clone();
    //     assert_eq!(remote_dir, remote.work_dir().join(job.id().to_string()));
    //     assert_eq!(local_dir, *job.working_dir());
    //     assert_eq!(basename, job.basename());
    // }
}
