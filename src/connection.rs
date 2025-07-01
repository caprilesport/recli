use crate::job::JobError;
use crate::remote::Remote;
use ssh2::Session;
use std::io::prelude::*;
use std::net::TcpStream;
use std::path::{Path, PathBuf};

use tracing::{debug, trace};

/// A trait that defines the actions that can be performed on a remote machine.
/// This abstraction allows for decoupling the runtime logic from the test logic
pub trait RemoteConnection {
    /// Executes a command on the remote and returns its stdout.
    fn execute(&self, command: &str) -> Result<String, JobError>;

    /// Creates a directory on the remote.
    fn mkdir(&self, path: &Path) -> Result<(), JobError>;

    /// Uploads a list of local files to a remote directory.
    fn upload_files(&self, local_paths: &[PathBuf], remote_dir: &Path) -> Result<(), JobError>;

    /// Downloads files from a remote directory to a local one, based on a basename.
    fn download_files(
        &self,
        remote_dir: &Path,
        local_dir: &Path,
        basename: &str,
    ) -> Result<(), JobError>;
}

pub struct SshConnection {
    session: Session,
}

impl SshConnection {
    /// Creates a new SSH connection based on the remote's configuration.
    pub fn new(remote: &Remote) -> Result<Self, JobError> {
        debug!("Connecting to {:?}", remote.name());
        let target = format!("{}:{}", remote.hostname(), remote.port());
        trace!("Using {} as target.", target);
        let tcp = TcpStream::connect(target)?;
        let mut session = Session::new()?;
        session.set_tcp_stream(tcp);
        session.handshake()?;
        session.userauth_agent(remote.user())?;
        Ok(SshConnection { session })
    }
}

impl RemoteConnection for SshConnection {
    fn execute(&self, command: &str) -> Result<String, JobError> {
        let mut channel = self.session.channel_session()?;
        channel.exec(command)?;
        trace!("Executing {} @ remote", command);

        let mut stdout = String::new();
        channel.read_to_string(&mut stdout)?;

        let mut stderr = String::new();
        channel.stderr().read_to_string(&mut stderr)?;

        channel.wait_close()?;
        let exit_code = channel.exit_status()?;

        if exit_code != 0 {
            return Err(JobError::CommandFailed {
                command: command.to_string(),
                exit_code,
                stdout,
                stderr,
            });
        }

        Ok(stdout)
    }

    fn mkdir(&self, path: &Path) -> Result<(), JobError> {
        debug!("Creating directory {:?} @ remote", path);
        let sftp = self.session.sftp()?;
        sftp.mkdir(path, 0o755)?;
        Ok(())
    }

    fn upload_files(&self, local_paths: &[PathBuf], remote_dir: &Path) -> Result<(), JobError> {
        let sftp = self.session.sftp()?;
        for file_path in local_paths {
            debug!("Uploading {:?} to {:?}", file_path, remote_dir);
            let mut local_file = std::fs::File::open(file_path)?;
            let remote_path = remote_dir.join(file_path.file_name().unwrap());
            let mut remote_file = sftp.create(remote_path.as_path())?;
            std::io::copy(&mut local_file, &mut remote_file)?;
        }
        Ok(())
    }

    fn download_files(
        &self,
        remote_dir: &Path,
        local_dir: &Path,
        basename: &str,
    ) -> Result<(), JobError> {
        let sftp = self.session.sftp()?;
        for entry in sftp.readdir(remote_dir)? {
            let (remote_path, stat) = entry;
            if stat.is_file()
                && remote_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .starts_with(basename)
            {
                let local_path = local_dir.join(remote_path.file_name().unwrap());
                let mut remote_file = sftp.open(&remote_path)?;
                let mut local_file = std::fs::File::create(&local_path)?;
                debug!("Downloading {:?}", remote_path);
                std::io::copy(&mut remote_file, &mut local_file)?;
            }
        }
        Ok(())
    }
}
