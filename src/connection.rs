use crate::remote::Remote;
use ssh2::Session;
use std::io::prelude::*;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};

use tracing::{debug, trace};

const CONNECTION_TIMEOUT: u32 = 9000;

#[derive(thiserror::Error, std::fmt::Debug)]
pub enum Error {
    #[error("Invalid public key")]
    InvalidPublicKey,
    #[error("No public key found")]
    NoPublicKeyAccepted,
    #[error("Incorrect password, try again")]
    IncorrectPassword,
    #[error("Connection to remote {0} timed out.")]
    TimeoutToRemote(String),
    #[error("Ssh2 failed, caused by: {0}")]
    Ssh2(#[from] ssh2::Error),
    #[error("{0}")]
    IO(#[from] std::io::Error),
    #[error("{0}")]
    InvalidPath(PathBuf),
    #[error("Could not resolve hostname {0}")]
    HostNameResolution(String),
    #[error("{0}")]
    SystemTimeError(#[from] std::time::SystemTimeError),
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
    ///
    /// # Errors
    ///
    /// Returns an `Error::CommandError` if the command execution fails or returns a non-zero exit code.
    fn execute(&self, command: &str) -> Result<String, Error>;

    /// Creates a directory on the remote.
    ///
    /// # Errors
    ///
    /// Returns an `Error::CommandError` if the directory creation fails.
    fn mkdir(&self, path: &Path) -> Result<(), Error>;

    /// Uploads local files to a remote directory.
    ///
    /// Only uploads files that are newer than their remote counterparts or don't
    /// exist remotely. Files matching ignore patterns are skipped.
    ///
    /// # Errors
    ///
    /// Returns an error if file reading or uploading fails.
    fn upload_files(
        &self,
        local_paths: &[PathBuf],
        remote_dir: &Path,
        ignore: &[glob::Pattern],
    ) -> Result<(), Error>;

    /// Downloads files from a remote directory to a local one.
    ///
    /// Only downloads files that are newer than
    /// their local counterparts. Files matching ignore patterns are skipped.
    ///
    /// # Errors
    ///
    /// Returns an error if file reading or downloading fails.
    fn download_files(
        &self,
        remote_dir: &Path,
        local_dir: &Path,
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
        let socket_adress = target
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| Error::HostNameResolution(remote.hostname().to_string()))?;
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
        SshConnection::authenticate(&mut session, remote.user(), remote)?;
        Ok(SshConnection { session })
    }

    fn authenticate(
        sess: &mut ssh2::Session,
        username: &str,
        remote: &Remote,
    ) -> Result<(), Error> {
        if let Some(key) = remote.identity_file()
            && SshConnection::try_pubkey_auth(sess, username, &key).is_ok()
        {
            return Ok(());
        }

        if SshConnection::try_agent_auth(sess, username).is_err() {
            debug!(
                "Agent authentication failed @ {} or agent not running.",
                remote.name()
            );
            if SshConnection::try_default_pubkey_auth(sess, username).is_err() {
                debug!(
                    "Default public key authentication failed @ {}.",
                    remote.name()
                );
                if SshConnection::try_password_auth(sess, username).is_err() {
                    debug!("Password authentication also failed.");
                    return Err(Error::IncorrectPassword);
                }
            }
        }

        if sess.authenticated() {
            debug!("Authentication successful.");
        } else {
            debug!("Authentication failed.");
        }

        Ok(())
    }

    fn try_agent_auth(sess: &mut ssh2::Session, username: &str) -> Result<(), Error> {
        debug!("Attempting agent authentication...");
        sess.userauth_agent(username)?;
        Ok(())
    }

    fn try_pubkey_auth(
        sess: &mut ssh2::Session,
        username: &str,
        key_file: &Path,
    ) -> Result<(), Error> {
        if sess
            .userauth_pubkey_file(username, None, key_file, None)
            .is_ok()
        {
            Ok(())
        } else {
            Err(Error::InvalidPublicKey)
        }
    }

    fn try_default_pubkey_auth(sess: &mut ssh2::Session, username: &str) -> Result<(), Error> {
        debug!("Attempting default public key authentication...");
        let home =
            dirs::home_dir().ok_or_else(|| std::io::Error::other("Home directory not found"))?;

        let key_files = ["id_ed25519", "id_rsa"];

        for key_file in &key_files {
            let key_path = Path::new(&home).join(".ssh").join(key_file);
            debug!("Trying default key: {:?}", key_path);
            if key_path.exists()
                && SshConnection::try_pubkey_auth(sess, username, &key_path).is_ok()
            {
                return Ok(());
            }
        }
        Err(Error::NoPublicKeyAccepted)
    }

    fn try_password_auth(sess: &mut ssh2::Session, username: &str) -> Result<(), Error> {
        debug!("Falling back to password authentication.");
        print!("Password for {username}: ");
        std::io::stdout().flush()?;

        let password = rpassword::read_password()?;

        sess.userauth_password(username, &password)?;

        Ok(())
    }

    /// Checks if a local file should be uploaded by comparing modification times.
    fn should_upload_file(
        local_path: &Path,
        remote_files: &std::collections::HashMap<String, u64>,
    ) -> Result<bool, Error> {
        let file_name = local_path
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .ok_or_else(|| Error::InvalidPath(local_path.to_path_buf()))?
            .to_string();
        let local_meta = std::fs::metadata(local_path)?;
        let local_mtime = local_meta
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)?
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
    ) -> Result<bool, Error> {
        if local_path.exists() {
            let local_meta = std::fs::metadata(local_path)?;
            let local_mtime = local_meta
                .modified()?
                .duration_since(std::time::UNIX_EPOCH)?
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
                let file_name = file_path.file_name().ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Path does not have a file name",
                    )
                })?;
                let remote_path = remote_dir.join(file_name);
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
        ignore: &[glob::Pattern],
    ) -> Result<(), Error> {
        let sftp = self.session.sftp()?;
        for entry in sftp.readdir(remote_dir)? {
            let (remote_path, stat) = entry;
            if ignore.iter().any(|p| p.matches_path(&remote_path)) {
                debug!("Ignoring {:?} due to ignore pattern", &remote_path);
                continue;
            }
            if stat.is_file() {
                let file_name = remote_path
                    .file_name()
                    .ok_or_else(|| Error::InvalidPath(remote_path.clone()))?;
                let local_path = local_dir.join(file_name);
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
