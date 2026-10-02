use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::connection::RemoteConnection;
use crate::job::JobStatus;
use crate::queuemanager::QueueManager;

use std::collections::HashMap;

#[derive(thiserror::Error, std::fmt::Debug)]
pub enum Error {
    #[error("{0}")]
    Connection(#[from] crate::connection::Error),
    #[error("{0}")]
    IO(#[from] std::io::Error), // #[error()]
    #[error("Invalid file stem from file: {0}")]
    InvalidFileStem(PathBuf),
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
    #[serde(default)]
    check_queue: bool,
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

    pub const fn port(&self) -> u16 {
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

    pub const fn queue_manager(&self) -> &QueueManager {
        &self.queue_manager
    }

    pub const fn check_queue(&self) -> bool {
        self.check_queue
    }

    /// Submits a job to the remote queue manager.
    ///
    /// Prepares input files, creates a remote job directory, uploads necessary files,
    /// and submits the job to the queue manager. Returns the remote job ID.
    ///
    /// # Errors
    ///
    /// Returns an error if file preparation, upload, or submission fails.
    pub fn submit(
        &self,
        script: &Path,
        extra_files: &[PathBuf],
        connection: &dyn RemoteConnection,
        remote_dir: &Path,
    ) -> Result<String, Error> {
        connection.mkdir(remote_dir)?;

        let script_name = script
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .ok_or_else(|| Error::InvalidFileStem(script.to_path_buf()))?;

        let mut files_to_send = vec![script.to_path_buf()];
        files_to_send.extend_from_slice(extra_files);
        connection.upload_files(&files_to_send, remote_dir, &[])?;

        let command = self.queue_manager.submit_command(remote_dir, script_name);
        let output = connection.execute(&command)?;
        let remote_id = self.queue_manager.get_id(&output);

        Ok(remote_id)
    }

    /// Fetches log sections for a job, returning (label, output) pairs.
    ///
    /// Sections that fail (e.g. file not found) are silently skipped.
    pub fn logs(
        &self,
        remote_dir: &Path,
        remote_id: &str,
        script_file: &Path,
        connection: &dyn RemoteConnection,
    ) -> Vec<(String, String)> {
        self.queue_manager
            .log_commands(remote_dir, remote_id, script_file)
            .into_iter()
            .filter_map(|(label, cmd)| connection.execute(&cmd).ok().map(|output| (label, output)))
            .collect()
    }

    /// Cancels a job on the remote queue manager.
    ///
    /// # Errors
    ///
    /// Returns an error if the cancel command fails (e.g. job already finished).
    pub fn cancel(&self, remote_id: &str, connection: &dyn RemoteConnection) -> Result<(), Error> {
        let command = self.queue_manager.cancel_command(remote_id);
        connection.execute(&command)?;
        Ok(())
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
        Ok(self.queue_manager.status(&output))
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
            _ignore: &[glob::Pattern],
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

        fn remove_dir(&self, _path: &Path) -> Result<(), crate::connection::Error> {
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
            check_queue: false,
            queue_manager: QueueManager::Pbs,
            identity_file: None,
        }
    }

    #[test]
    fn test_submit() {
        let remote = create_test_remote();
        let connection = MockConnection::new();
        let script = Path::new("/home/user/jobs/myjob.pbs");
        let remote_dir = Path::new("/remote/work/uuid-1");

        connection.mock_output.borrow_mut().insert(
            "cd '/remote/work/uuid-1' && qsub 'myjob.pbs'".to_string(),
            "12345.server\n".to_string(),
        );

        let remote_id = remote.submit(script, &[], &connection, remote_dir).unwrap();

        assert_eq!(remote_id, "12345.server");
        // mkdir, upload, execute — in that order
        assert_eq!(connection.commands.borrow().len(), 1);
        assert_eq!(connection.uploads.borrow().len(), 1);
        let (uploaded_files, uploaded_to) = connection.uploads.borrow()[0].clone();
        assert_eq!(uploaded_files, vec![script.to_path_buf()]);
        assert_eq!(uploaded_to, remote_dir);
    }

    #[test]
    fn test_submit_with_extra_files() {
        let remote = create_test_remote();
        let connection = MockConnection::new();
        let script = Path::new("/home/user/jobs/myjob.pbs");
        let extra = vec![
            PathBuf::from("/home/user/jobs/myjob.inp"),
            PathBuf::from("/home/user/jobs/myjob.xyz"),
        ];
        let remote_dir = Path::new("/remote/work/uuid-2");

        connection.mock_output.borrow_mut().insert(
            "cd '/remote/work/uuid-2' && qsub 'myjob.pbs'".to_string(),
            "99.server\n".to_string(),
        );

        let remote_id = remote
            .submit(script, &extra, &connection, remote_dir)
            .unwrap();

        assert_eq!(remote_id, "99.server");
        let (uploaded_files, _) = connection.uploads.borrow()[0].clone();
        assert_eq!(uploaded_files.len(), 3); // script + 2 extra
        assert_eq!(uploaded_files[0], script);
        assert_eq!(uploaded_files[1], extra[0]);
        assert_eq!(uploaded_files[2], extra[1]);
    }

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
}
