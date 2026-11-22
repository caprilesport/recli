use crate::remote::Remote;
use ssh2::{Session, Sftp};
use std::io::prelude::*;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};

use tracing::{debug, trace};

// TODO: this looks like a config setting
const CONNECTION_TIMEOUT: u32 = 9000;

#[derive(thiserror::Error, std::fmt::Debug)]
pub enum Error {
    #[error("Invalid public key")]
    InvalidPublicKey,
    #[error("No public key found")]
    NoPublicKeyAccepted,
    #[error("Authentication failed for '{user}' @ '{remote}'")]
    AuthFailed { user: String, remote: String },
    #[error("Connection to remote {0} timed out.")]
    TimeoutToRemote(String),
    #[error("SSH handshake with '{remote}' failed: {source}")]
    Handshake {
        remote: String,
        #[source]
        source: ssh2::Error,
    },
    #[error("SFTP {operation} on '{path}': {source}")]
    SftpOperation {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: ssh2::Error,
    },
    #[error("Local file '{path}': {source}")]
    LocalFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
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

    /// Removes a directory and all its contents on the remote.
    ///
    /// # Errors
    ///
    /// Returns an error if the command fails.
    fn remove_dir(&self, path: &Path) -> Result<(), Error>;
}

/// libssh2's `LIBSSH2_ERROR_CHANNEL_FAILURE`: the server refused to open a channel.
///
/// `sshd` caps open channels per connection (`MaxSessions`, 10 by default), and
/// reports it with this code for SFTP and exec channels alike.
const CHANNEL_FAILURE: i32 = -21;

/// Buffer size for SFTP file transfers.
///
/// libssh2 pipelines read and write requests only within the buffer it is given, so
/// `io::copy`'s 8 KB buffer costs a network round trip per 8 KB. A large buffer keeps
/// many requests in flight.
const TRANSFER_BUFFER_SIZE: usize = 1 << 20;

/// Like `io::copy`, but with a `TRANSFER_BUFFER_SIZE` buffer.
fn copy_large<R: Read, W: Write>(reader: &mut R, writer: &mut W) -> std::io::Result<u64> {
    let mut buffer = vec![0; TRANSFER_BUFFER_SIZE];
    let mut total = 0;
    loop {
        let n = match reader.read(&mut buffer) {
            Ok(0) => return Ok(total),
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        writer.write_all(&buffer[..n])?;
        total += n as u64;
    }
}

fn is_channel_limit(err: &ssh2::Error) -> bool {
    err.code() == ssh2::ErrorCode::Session(CHANNEL_FAILURE)
}

#[derive(Debug)]
struct PoolState<T> {
    idle: Vec<T>,
    /// Channels handed out or idle, plus opens in progress.
    open: usize,
    /// Learned from the server the first time it refuses a channel.
    limit: Option<usize>,
}

/// A pool of channels on one SSH connection, shared by parallel operations.
///
/// Channels are reused across operations. New ones are opened on demand until the
/// server refuses one; from then on the pool stays at that size, and callers wait
/// for a channel to be released instead of failing.
#[derive(Debug)]
struct ChannelPool<T> {
    state: Mutex<PoolState<T>>,
    released: Condvar,
}

impl<T> ChannelPool<T> {
    const fn new() -> Self {
        Self {
            state: Mutex::new(PoolState {
                idle: Vec::new(),
                open: 0,
                limit: None,
            }),
            released: Condvar::new(),
        }
    }

    fn acquire(&self, open: impl Fn() -> Result<T, ssh2::Error>) -> Result<T, ssh2::Error> {
        let mut state = self.state.lock().unwrap();
        loop {
            if let Some(channel) = state.idle.pop() {
                return Ok(channel);
            }
            if state.limit.is_none_or(|limit| state.open < limit) {
                // Reserve a slot, and open without holding the lock so releases can proceed.
                state.open += 1;
                drop(state);
                let result = open();
                state = self.state.lock().unwrap();
                match result {
                    Ok(channel) => return Ok(channel),
                    Err(e) => {
                        state.open -= 1;
                        self.released.notify_one();
                        if !is_channel_limit(&e) || state.open == 0 {
                            return Err(e);
                        }
                        let limit = state.limit.map_or(state.open, |l| l.min(state.open));
                        debug!("Server refused a new channel; using at most {limit}");
                        state.limit = Some(limit);
                        continue;
                    }
                }
            }
            state = self.released.wait(state).unwrap();
        }
    }

    /// Opens channels one at a time, until `max` are open or the server refuses one,
    /// and keeps them idle in the pool. Returns how many channels the pool holds.
    ///
    /// Errors only if no channel could be opened at all.
    fn fill(
        &self,
        max: usize,
        open: impl Fn() -> Result<T, ssh2::Error>,
    ) -> Result<usize, ssh2::Error> {
        let mut state = self.state.lock().unwrap();
        while state.open < max && state.limit.is_none_or(|limit| state.open < limit) {
            state.open += 1;
            drop(state);
            let result = open();
            state = self.state.lock().unwrap();
            match result {
                Ok(channel) => state.idle.push(channel),
                Err(e) => {
                    state.open -= 1;
                    if state.open == 0 {
                        return Err(e);
                    }
                    if is_channel_limit(&e) {
                        debug!("Server refused a new channel; using at most {}", state.open);
                        state.limit = Some(state.open);
                    } else {
                        debug!("Stopped opening channels at {}: {e}", state.open);
                    }
                    break;
                }
            }
        }
        self.released.notify_all();
        Ok(state.open)
    }

    fn release(&self, channel: T) {
        self.state.lock().unwrap().idle.push(channel);
        self.released.notify_one();
    }

    /// Drops a channel that may be broken, freeing its slot.
    fn discard(&self, channel: T) {
        drop(channel);
        self.state.lock().unwrap().open -= 1;
        self.released.notify_one();
    }

    /// Runs `f` with a channel from the pool. On error the channel is discarded, since
    /// it may be in a bad state.
    fn with<R, E: From<ssh2::Error>>(
        &self,
        open: impl Fn() -> Result<T, ssh2::Error>,
        f: impl FnOnce(&T) -> Result<R, E>,
    ) -> Result<R, E> {
        let channel = self.acquire(open)?;
        match f(&channel) {
            Ok(value) => {
                self.release(channel);
                Ok(value)
            }
            Err(e) => {
                self.discard(channel);
                Err(e)
            }
        }
    }
}

pub struct SshConnection {
    session: Session,
    sftp: ChannelPool<Sftp>,
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

        let tcp =
            match TcpStream::connect_timeout(&socket_adress, std::time::Duration::from_secs(3)) {
                Ok(tcp) => tcp,
                Err(_err) => return Err(Error::TimeoutToRemote(remote.name().to_owned())),
            };
        trace!("TCP Stream established, creating session");
        let mut session = Session::new()?;
        session.set_timeout(CONNECTION_TIMEOUT);
        session.set_tcp_stream(tcp);
        session.handshake().map_err(|e| Error::Handshake {
            remote: remote.name().to_string(),
            source: e,
        })?;
        Self::authenticate(&mut session, remote.user(), remote)?;
        Ok(Self {
            session,
            sftp: ChannelPool::new(),
        })
    }

    fn with_sftp<R>(&self, f: impl FnOnce(&Sftp) -> Result<R, Error>) -> Result<R, Error> {
        self.sftp.with(|| self.session.sftp(), f)
    }

    /// Opens up to `max` SFTP channels ahead of parallel transfers, stopping at the
    /// server's limit. Returns how many are available, i.e. how many transfers can
    /// run at once on this connection.
    ///
    /// # Errors
    ///
    /// Returns an error if not even one channel can be opened.
    pub fn open_sftp_channels(&self, max: usize) -> Result<usize, Error> {
        Ok(self.sftp.fill(max, || self.session.sftp())?)
    }

    fn authenticate(
        sess: &mut ssh2::Session,
        username: &str,
        remote: &Remote,
    ) -> Result<(), Error> {
        if let Some(key) = remote.identity_file()
            && Self::try_pubkey_auth(sess, username, &key).is_ok()
        {
            return Ok(());
        }

        if Self::try_agent_auth(sess, username).is_err() {
            debug!(
                "Agent authentication failed @ {} or agent not running.",
                remote.name()
            );
            if Self::try_default_pubkey_auth(sess, username).is_err() {
                debug!(
                    "Default public key authentication failed @ {}.",
                    remote.name()
                );
                if Self::try_password_auth(sess, username).is_err() {
                    debug!("Password authentication also failed.");
                    return Err(Error::AuthFailed {
                        user: username.to_string(),
                        remote: remote.name().to_string(),
                    });
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
            if key_path.exists() && Self::try_pubkey_auth(sess, username, &key_path).is_ok() {
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
        let local_meta = std::fs::metadata(local_path).map_err(|e| Error::LocalFile {
            path: local_path.to_path_buf(),
            source: e,
        })?;
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
            let local_meta = std::fs::metadata(local_path).map_err(|e| Error::LocalFile {
                path: local_path.to_path_buf(),
                source: e,
            })?;
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
        self.with_sftp(|sftp| {
            if sftp.stat(path).is_ok() {
                debug!("Directory already exists, skipping: {:?}", path);
            } else {
                debug!("Creating directory {:?}", path);
                sftp.mkdir(path, 0o755).map_err(|e| Error::SftpOperation {
                    operation: "mkdir",
                    path: path.to_path_buf(),
                    source: e,
                })?;
            }
            Ok(())
        })
    }

    fn upload_files(
        &self,
        local_paths: &[PathBuf],
        remote_dir: &Path,
        ignore: &[glob::Pattern],
    ) -> Result<(), Error> {
        self.with_sftp(|sftp| {
            let remote_files: std::collections::HashMap<String, u64> = sftp
                .readdir(remote_dir)
                .map_err(|e| Error::SftpOperation {
                    operation: "readdir",
                    path: remote_dir.to_path_buf(),
                    source: e,
                })?
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
                    let mut local_file =
                        std::fs::File::open(file_path).map_err(|e| Error::LocalFile {
                            path: file_path.clone(),
                            source: e,
                        })?;
                    let file_name = file_path.file_name().ok_or_else(|| {
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            "Path does not have a file name",
                        )
                    })?;
                    let remote_path = remote_dir.join(file_name);
                    let mut remote_file =
                        sftp.create(remote_path.as_path())
                            .map_err(|e| Error::SftpOperation {
                                operation: "create",
                                path: remote_path.clone(),
                                source: e,
                            })?;
                    copy_large(&mut local_file, &mut remote_file)?;
                }
            }
            Ok(())
        })
    }

    fn download_files(
        &self,
        remote_dir: &Path,
        local_dir: &Path,
        ignore: &[glob::Pattern],
    ) -> Result<(), Error> {
        self.with_sftp(|sftp| {
            for entry in sftp.readdir(remote_dir).map_err(|e| Error::SftpOperation {
                operation: "readdir",
                path: remote_dir.to_path_buf(),
                source: e,
            })? {
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
                        let mut remote_file =
                            sftp.open(&remote_path).map_err(|e| Error::SftpOperation {
                                operation: "open",
                                path: remote_path.clone(),
                                source: e,
                            })?;
                        let mut local_file =
                            std::fs::File::create(&local_path).map_err(|e| Error::LocalFile {
                                path: local_path.clone(),
                                source: e,
                            })?;
                        debug!("Downloading {:?}", remote_path);
                        copy_large(&mut remote_file, &mut local_file)?;
                    }
                }
            }
            Ok(())
        })
    }

    fn remove_dir(&self, path: &Path) -> Result<(), Error> {
        debug!("Removing directory {:?} @ remote", path);
        let quoted = format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"));
        self.execute(&format!("rm -rf {quoted}"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Barrier;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Opens numbered fake channels, refusing like `sshd` once `limit` are open.
    struct FakeServer {
        limit: usize,
        open: AtomicUsize,
        max_seen: AtomicUsize,
        opened_total: AtomicUsize,
    }

    impl FakeServer {
        fn new(limit: usize) -> Self {
            Self {
                limit,
                open: AtomicUsize::new(0),
                max_seen: AtomicUsize::new(0),
                opened_total: AtomicUsize::new(0),
            }
        }

        fn open(&self) -> Result<usize, ssh2::Error> {
            let n = self.open.fetch_add(1, Ordering::SeqCst) + 1;
            if n > self.limit {
                self.open.fetch_sub(1, Ordering::SeqCst);
                return Err(ssh2::Error::new(
                    ssh2::ErrorCode::Session(CHANNEL_FAILURE),
                    "Unable to startup channel",
                ));
            }
            self.max_seen.fetch_max(n, Ordering::SeqCst);
            Ok(self.opened_total.fetch_add(1, Ordering::SeqCst))
        }
    }

    #[test]
    fn test_pool_stays_within_server_limit() {
        let server = FakeServer::new(3);
        let pool = ChannelPool::new();
        let done = AtomicUsize::new(0);
        let start = Barrier::new(16);

        std::thread::scope(|s| {
            for _ in 0..16 {
                s.spawn(|| {
                    start.wait();
                    pool.with(
                        || server.open(),
                        |_| -> Result<(), ssh2::Error> {
                            std::thread::sleep(std::time::Duration::from_millis(10));
                            done.fetch_add(1, Ordering::SeqCst);
                            Ok(())
                        },
                    )
                    .unwrap();
                });
            }
        });

        assert_eq!(done.load(Ordering::SeqCst), 16);
        assert_eq!(server.max_seen.load(Ordering::SeqCst), 3);
        assert_eq!(pool.state.lock().unwrap().limit, Some(3));
    }

    /// Returns data in small, uneven chunks, like a network stream.
    struct ChunkedReader {
        data: Vec<u8>,
        pos: usize,
    }

    impl Read for ChunkedReader {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = buf.len().min(7919).min(self.data.len() - self.pos);
            buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
            self.pos += n;
            Ok(n)
        }
    }

    #[test]
    fn test_copy_large_copies_everything() {
        let data: Vec<u8> = (0..3 * TRANSFER_BUFFER_SIZE + 123)
            .map(|i| u8::try_from(i % 251).unwrap())
            .collect();
        let mut reader = ChunkedReader {
            data: data.clone(),
            pos: 0,
        };
        let mut out = Vec::new();
        let copied = copy_large(&mut reader, &mut out).unwrap();
        assert_eq!(copied, data.len() as u64);
        assert_eq!(out, data);
    }

    #[test]
    fn test_copy_large_empty_input() {
        let mut out = Vec::new();
        assert_eq!(copy_large(&mut std::io::empty(), &mut out).unwrap(), 0);
        assert!(out.is_empty());
    }

    #[test]
    fn test_fill_stops_at_server_limit() {
        let server = FakeServer::new(3);
        let pool = ChannelPool::new();
        assert_eq!(pool.fill(16, || server.open()).unwrap(), 3);
        let state = pool.state.lock().unwrap();
        assert_eq!(state.limit, Some(3));
        assert_eq!(state.idle.len(), 3);
    }

    #[test]
    fn test_fill_opens_no_more_than_needed() {
        let server = FakeServer::new(10);
        let pool = ChannelPool::new();
        assert_eq!(pool.fill(4, || server.open()).unwrap(), 4);
        assert_eq!(server.opened_total.load(Ordering::SeqCst), 4);
        assert_eq!(pool.state.lock().unwrap().limit, None);
    }

    #[test]
    fn test_fill_refused_first_channel_is_an_error() {
        let server = FakeServer::new(0);
        let pool = ChannelPool::new();
        let err = pool.fill(16, || server.open()).unwrap_err();
        assert!(is_channel_limit(&err));
    }

    #[test]
    fn test_fill_then_workers_never_open_more() {
        let server = FakeServer::new(3);
        let pool = ChannelPool::new();
        let workers = pool.fill(16, || server.open()).unwrap();
        let done = AtomicUsize::new(0);

        std::thread::scope(|s| {
            for _ in 0..workers {
                s.spawn(|| {
                    for _ in 0..5 {
                        pool.with(
                            || server.open(),
                            |_| -> Result<(), ssh2::Error> {
                                done.fetch_add(1, Ordering::SeqCst);
                                Ok(())
                            },
                        )
                        .unwrap();
                    }
                });
            }
        });

        assert_eq!(done.load(Ordering::SeqCst), 15);
        assert_eq!(server.opened_total.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn test_pool_reuses_released_channels() {
        let server = FakeServer::new(10);
        let pool = ChannelPool::new();
        for _ in 0..5 {
            pool.with(|| server.open(), |_| Ok::<_, ssh2::Error>(()))
                .unwrap();
        }
        assert_eq!(server.opened_total.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_pool_refused_first_channel_is_an_error() {
        let server = FakeServer::new(0);
        let pool = ChannelPool::new();
        let err = pool
            .with(|| server.open(), |_| Ok::<_, ssh2::Error>(()))
            .unwrap_err();
        assert!(is_channel_limit(&err));
    }

    #[test]
    fn test_pool_other_open_errors_are_returned() {
        let pool: ChannelPool<usize> = ChannelPool::new();
        let err = pool
            .with(
                || {
                    Err(ssh2::Error::new(
                        ssh2::ErrorCode::Session(-7),
                        "Socket send",
                    ))
                },
                |_| Ok::<_, ssh2::Error>(()),
            )
            .unwrap_err();
        assert_eq!(err.code(), ssh2::ErrorCode::Session(-7));
        assert_eq!(pool.state.lock().unwrap().open, 0);
    }

    #[test]
    fn test_pool_discards_channel_after_failed_operation() {
        let server = FakeServer::new(10);
        let pool = ChannelPool::new();
        let result = pool.with(
            || server.open(),
            |_| Err::<(), _>(ssh2::Error::new(ssh2::ErrorCode::Session(-31), "SFTP")),
        );
        assert!(result.is_err());
        let state = pool.state.lock().unwrap();
        assert!(state.idle.is_empty());
        assert_eq!(state.open, 0);
    }
}
