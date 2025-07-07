use crate::remote::Remote;
use ssh2::Session;
use std::io::prelude::*;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};

use tracing::{debug, trace};

const CONNECTION_TIMEOUT: u32 = 3000;

#[derive(thiserror::Error, std::fmt::Debug)]
pub enum Error {
    #[error("Connection to remote {0} timed out.")]
    TimeoutToRemote(String),
    #[error("Failed to create remote dir:\n{0}")]
    FailedToCreateDirError(#[from] ssh2::Error),
    #[error("{0}")]
    IO(#[from] std::io::Error),
    #[error(
        "Command '{command}' failed with exit code
      {exit_code}\n---\nSTDOUT:\n{stdout}\n---\nSTDERR:\n{stderr}"
    )]
    CommandFailed {
        command: String,
        exit_code: i32,
        stdout: String,
        stderr: String,
    },
}

/// A trait that defines the actions that can be performed on a remote machine.
/// This abstraction allows for decoupling the runtime logic from the test logic
pub trait RemoteConnection {
    /// Executes a command on the remote and returns its stdout.
    fn execute(&self, command: &str) -> Result<String, Error>;

    /// Creates a directory on the remote.
    fn mkdir(&self, path: &Path) -> Result<(), Error>;

    /// Uploads a list of local files to a remote directory.
    fn upload_files(&self, local_paths: &[PathBuf], remote_dir: &Path) -> Result<(), Error>;

    /// Downloads files from a remote directory to a local one, based on a basename.
    fn download_files(
        &self,
        remote_dir: &Path,
        local_dir: &Path,
        basename: &str,
    ) -> Result<(), Error>;
}

pub struct SshConnection {
    session: Session,
}

impl SshConnection {
    /// Creates a new SSH connection based on the remote's configuration.
    pub fn new(remote: &Remote) -> Result<Self, Error> {
        debug!("Connecting to {:?}", remote.name());
        let target = format!("{}:{}", remote.hostname(), remote.port());
        let socket_adress = target.to_socket_addrs().unwrap().next().unwrap();
        trace!("Using {} as target.", target);

        let tcp = match TcpStream::connect_timeout(
            &socket_adress,
            std::time::Duration::from_millis(3000),
        ) {
            Ok(tcp) => tcp,
            Err(_err) => return Err(Error::TimeoutToRemote(remote.name().to_owned())),
        };
        trace!("TCP Stream established, creating session");
        let mut session = Session::new()?;
        session.set_timeout(CONNECTION_TIMEOUT);
        session.set_tcp_stream(tcp);
        session.handshake()?;
        session.userauth_agent(remote.user())?;
        Ok(SshConnection { session })
    }
}

impl RemoteConnection for SshConnection {
    fn execute(&self, command: &str) -> Result<String, Error> {
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
            return Err(Error::CommandFailed {
                command: command.to_string(),
                exit_code,
                stdout,
                stderr,
            });
        }

        Ok(stdout)
    }

    fn mkdir(&self, path: &Path) -> Result<(), Error> {
        debug!("Creating directory {:?} @ remote", path);
        let sftp = self.session.sftp()?;
        sftp.mkdir(path, 0o755)?;
        Ok(())
    }

    fn upload_files(&self, local_paths: &[PathBuf], remote_dir: &Path) -> Result<(), Error> {
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
    ) -> Result<(), Error> {
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
