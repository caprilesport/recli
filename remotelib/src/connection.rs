use crate::remote::Remote;
use ssh2::Session;
use std::io::prelude::*;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};

use tracing::{debug, trace};

const CONNECTION_TIMEOUT: u32 = 9000;

#[derive(thiserror::Error, std::fmt::Debug)]
pub enum Error {
    #[error("Connection to remote {0} timed out.")]
    TimeoutToRemote(String),
    #[error("Ssh2 failed, cause by: {0}")]
    Ssh2Error(#[from] ssh2::Error),
    #[error("{0}")]
    IO(#[from] std::io::Error),
    #[error(
        "Command '{command}' failed with exit code {exit_code}
---
STDOUT:
{stdout}
---
STDERR:
{stderr}"
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
    fn upload_files(
        &self,
        local_paths: &[PathBuf],
        remote_dir: &Path,
        ignore: &[glob::Pattern],
    ) -> Result<(), Error>;

    /// Downloads files from a remote directory to a local one, based on a basename.
    fn download_files(
        &self,
        remote_dir: &Path,
        local_dir: &Path,
        basename: &str,
        ignore: &[glob::Pattern],
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
        // TODO: adress these unwraps, as connect_timeout doesnt accept a vector we need the .next() for now, but there's certainly a better way to throw errros here
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

    /// Checks if a local file should be uploaded by comparing modification times.
    fn should_upload_file(
        local_path: &Path,
        remote_files: &std::collections::HashMap<String, u64>,
    ) -> Result<bool, std::io::Error> {
        let file_name = local_path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let local_meta = std::fs::metadata(local_path)?;
        let local_mtime = local_meta
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        if let Some(remote_mtime) = remote_files.get(&file_name) {
            if local_mtime > *remote_mtime {
                debug!("Local file {:?} is newer, uploading.", local_path);
                Ok(true)
            } else {
                debug!("Remote file {:?} is up-to-date, skipping.", file_name);
                Ok(false)
            }
        } else {
            Ok(true)
        }
    }

    /// Checks if a remote file should be downloaded by comparing modification times.
    fn should_download_file(
        remote_stat: &ssh2::FileStat,
        local_path: &Path,
    ) -> Result<bool, std::io::Error> {
        if local_path.exists() {
            let local_meta = std::fs::metadata(local_path)?;
            let local_mtime = local_meta
                .modified()?
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let remote_mtime = remote_stat.mtime.unwrap_or(0);
            if remote_mtime > local_mtime {
                debug!("Remote file {:?} is newer, downloading.", local_path);
                Ok(true)
            } else {
                debug!("Local file {:?} is up-to-date, skipping.", local_path);
                Ok(false)
            }
        } else {
            Ok(true)
        }
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
        sftp.mkdir(path, 0o755)?; // 755 chmod basically
        Ok(())
    }

    fn upload_files(
        &self,
        local_paths: &[PathBuf],
        remote_dir: &Path,
        ignore: &[glob::Pattern],
    ) -> Result<(), Error> {
        let sftp = self.session.sftp()?;
        let remote_files: std::collections::HashMap<String, u64> = sftp
            .readdir(remote_dir)?
            .into_iter()
            .filter_map(|(path, stat)| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| (name.to_string(), stat.mtime.unwrap_or(0)))
            })
            .collect();

        for file_path in local_paths {
            if ignore.iter().any(|p| p.matches_path(file_path)) {
                debug!("Ignoring {:?} due to ignore pattern", file_path);
                continue;
            }

            if Self::should_upload_file(file_path, &remote_files)? {
                debug!("Uploading {:?} to {:?}", file_path, remote_dir);
                let mut local_file = std::fs::File::open(file_path)?;
                let remote_path = remote_dir.join(file_path.file_name().unwrap());
                let mut remote_file = sftp.create(remote_path.as_path())?;
                std::io::copy(&mut local_file, &mut remote_file)?;
            }
        }
        Ok(())
    }

    fn download_files(
        &self,
        remote_dir: &Path,
        local_dir: &Path,
        basename: &str,
        ignore: &[glob::Pattern],
    ) -> Result<(), Error> {
        let sftp = self.session.sftp()?;
        for entry in sftp.readdir(remote_dir)? {
            let (remote_path, stat) = entry;
            if ignore.iter().any(|p| p.matches_path(&remote_path)) {
                debug!("Ignoring {:?} due to ignore pattern", &remote_path);
                continue;
            }
            if stat.is_file()
                && remote_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .starts_with(basename)
            {
                let local_path = local_dir.join(remote_path.file_name().unwrap());
                if Self::should_download_file(&stat, &local_path)? {
                    let mut remote_file = sftp.open(&remote_path)?;
                    let mut local_file = std::fs::File::create(&local_path)?;
                    debug!("Downloading {:?}", remote_path);
                    std::io::copy(&mut remote_file, &mut local_file)?;
                }
            }
        }
        Ok(())
    }
}
