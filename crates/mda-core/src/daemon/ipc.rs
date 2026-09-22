//! Control channel between `mda` commands and the daemon: a local socket per root and a
//! newline-delimited JSON protocol.
//!
//! One [`Request`] per line, one [`Response`] per line. [`Request::Watch`] keeps the
//! connection open and streams [`Response::Event`] lines until the client hangs up.
//!
//! The socket is a Unix domain socket at `<root>/.markdownattractor/daemon.sock` when that
//! path fits the platform limit (104 bytes on macOS), a socket under the temp directory named
//! from a hash of the root otherwise, and a named pipe with the same hashed name on Windows.
//! Both sides compute the name from the root, so nothing has to be looked up. Liveness is
//! "does the socket answer [`Request::Ping`]", never a signal.

use std::path::{Path, PathBuf};
use std::time::Duration;

use interprocess::local_socket::tokio::prelude::*;
use interprocess::local_socket::tokio::{RecvHalf, SendHalf};
use interprocess::local_socket::{GenericFilePath, GenericNamespaced, ListenerOptions, Name};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

use super::{DaemonEvent, LiveStatus};
use crate::config::{state_dir, write_private};
use crate::pipeline::IndexReport;
use crate::{Error, Result};

/// File under the state directory holding the daemon's pid.
pub const PID_FILE: &str = "daemon.pid";
/// File under the state directory describing the running daemon.
pub const INFO_FILE: &str = "daemon.json";
/// Socket file name under the state directory (Unix, when the path is short enough).
pub const SOCKET_FILE: &str = "daemon.sock";
/// Lock file under the state directory: held (OS file lock) for the daemon's whole life.
pub const LOCK_FILE: &str = "daemon.lock";
/// Longest request line a client may send.
pub const MAX_REQUEST_BYTES: u64 = 64 * 1024;
/// Longest `sun_path` we rely on; macOS allows 104 bytes including the terminator.
const MAX_SOCKET_PATH: usize = 100;
/// How long a client waits for the daemon to answer one request.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// What a command asks the daemon to do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// Liveness check.
    Ping,
    /// Live counters.
    Status,
    /// Stop gracefully: in-flight calls finish and are recorded, the rest stays pending.
    Stop,
    /// Keep watching and indexing, stop calling the model.
    Pause,
    /// Undo [`Request::Pause`].
    Resume,
    /// Index one path now (relative to the root, or absolute inside it), or the whole root.
    Index {
        /// Path to index; `None` means the whole root.
        path: Option<String>,
    },
    /// Reconcile the whole root with the walker.
    Rescan,
    /// Stream events until the connection closes.
    Watch,
}

/// What the daemon answers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Response {
    /// Done.
    Ok,
    /// Answer to [`Request::Status`].
    Status(Box<LiveStatus>),
    /// Answer to [`Request::Index`] and [`Request::Rescan`].
    Indexed(Box<IndexReport>),
    /// The request failed.
    Error {
        /// Why.
        message: String,
    },
    /// One streamed event (after [`Request::Watch`]).
    Event(DaemonEvent),
}

/// Facts about a running daemon, written to [`INFO_FILE`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonInfo {
    /// Process id.
    pub pid: u32,
    /// `mda` version.
    pub version: String,
    /// When it started.
    pub started_at: Timestamp,
    /// Watched root, canonical.
    pub root: String,
    /// Where the socket is (path or pipe name), for humans.
    pub socket: String,
}

impl DaemonInfo {
    /// Read the info file of `root`, if there is one.
    pub fn read(root: &Path) -> Result<Option<Self>> {
        let path = state_dir(root)?.join(INFO_FILE);
        match std::fs::read_to_string(&path) {
            Ok(text) => Ok(Some(serde_json::from_str(&text)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Error::io(path, e)),
        }
    }

    /// Write the info file (and the pid file) for `root`. Never follows a symlink.
    pub fn write(&self, root: &Path) -> Result<()> {
        let dir = state_dir(root)?;
        std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        write_private(&dir.join(INFO_FILE), &serde_json::to_vec_pretty(self)?)?;
        write_private(&dir.join(PID_FILE), self.pid.to_string().as_bytes())
    }

    /// Remove the info and pid files. Missing files are fine.
    pub fn remove(root: &Path) {
        for p in [info_path(root), pid_path(root)] {
            if let Err(e) = std::fs::remove_file(&p)
                && e.kind() != std::io::ErrorKind::NotFound
            {
                tracing::warn!(path = %p.display(), error = %e, "could not remove");
            }
        }
    }
}

/// `<root>/.markdownattractor/daemon.pid`.
pub fn pid_path(root: &Path) -> PathBuf {
    root.join(crate::config::STATE_DIR).join(PID_FILE)
}

/// `<root>/.markdownattractor/daemon.json`.
pub fn info_path(root: &Path) -> PathBuf {
    root.join(crate::config::STATE_DIR).join(INFO_FILE)
}

/// The per-root lock a daemon holds for its whole life. The OS releases it when the process
/// dies, however it dies, so a stale lock is impossible and two daemons cannot share a root:
/// the second `try_lock` fails while the first process lives.
#[derive(Debug)]
pub struct Lock {
    _file: std::fs::File,
}

impl Lock {
    /// Take the lock for `root`, or fail with [`Error::Daemon`] if another process holds it.
    pub fn acquire(root: &Path) -> Result<Self> {
        let dir = state_dir(root)?;
        std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        let path = dir.join(LOCK_FILE);
        if let Ok(meta) = std::fs::symlink_metadata(&path)
            && meta.file_type().is_symlink()
        {
            return Err(Error::Daemon(format!("{} is a symlink; refusing", path.display())));
        }
        let mut opts = std::fs::OpenOptions::new();
        opts.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let file = opts.open(&path).map_err(|e| Error::io(&path, e))?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(std::fs::TryLockError::WouldBlock) => {
                Err(Error::Daemon(format!("another daemon holds {}", path.display())))
            }
            Err(std::fs::TryLockError::Error(e)) => Err(Error::io(&path, e)),
        }
    }
}

/// Where the daemon of `root` listens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocketLocation {
    /// A Unix domain socket at this path.
    Path(PathBuf),
    /// A named local socket (Windows named pipe).
    Namespaced(String),
}

impl std::fmt::Display for SocketLocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Path(p) => write!(f, "{}", p.display()),
            Self::Namespaced(n) => f.write_str(n),
        }
    }
}

/// Sixteen hex characters that identify a root without exposing its path.
fn root_tag(root: &Path) -> String {
    blake3::hash(root.to_string_lossy().as_bytes()).to_hex()[..16].to_owned()
}

/// The socket location for `root`, deterministic on both sides.
pub fn socket_location(root: &Path) -> SocketLocation {
    if cfg!(windows) {
        return SocketLocation::Namespaced(format!("mda-{}", root_tag(root)));
    }
    let in_root = root.join(crate::config::STATE_DIR).join(SOCKET_FILE);
    if in_root.as_os_str().len() <= MAX_SOCKET_PATH {
        SocketLocation::Path(in_root)
    } else {
        SocketLocation::Path(std::env::temp_dir().join(format!("mda-{}.sock", root_tag(root))))
    }
}

fn socket_name(loc: &SocketLocation) -> Result<Name<'static>> {
    let name = match loc {
        SocketLocation::Path(p) => p.clone().to_fs_name::<GenericFilePath>(),
        SocketLocation::Namespaced(n) => n.clone().to_ns_name::<GenericNamespaced>(),
    };
    name.map_err(|e| Error::Daemon(format!("bad socket name {loc}: {e}")))
}

/// The daemon's listening end.
pub struct Server {
    listener: LocalSocketListener,
    location: SocketLocation,
}

impl std::fmt::Debug for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server").field("location", &self.location).finish_non_exhaustive()
    }
}

impl Server {
    /// Bind the socket for `root`. The caller holds the root's [`Lock`], which is what rules
    /// out a second daemon; a socket file left by a crashed daemon is removed first. On Unix
    /// the socket is made private to the user.
    pub fn bind(root: &Path) -> Result<Self> {
        let location = socket_location(root);
        if let SocketLocation::Path(p) = &location {
            state_dir(root)?;
            match std::fs::symlink_metadata(p) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    return Err(Error::Daemon(format!("{} is a symlink; refusing", p.display())));
                }
                Ok(_) => {
                    std::fs::remove_file(p).map_err(|e| Error::io(p, e))?;
                    tracing::info!(path = %p.display(), "removed stale socket");
                }
                Err(_) => {}
            }
            if let Some(dir) = p.parent() {
                std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
            }
        }
        let listener = ListenerOptions::new()
            .name(socket_name(&location)?)
            .create_tokio()
            .map_err(|e| Error::Daemon(format!("cannot listen on {location}: {e}")))?;
        #[cfg(unix)]
        if let SocketLocation::Path(p) = &location {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| Error::io(p, e))?;
        }
        tracing::info!(%location, "listening");
        Ok(Self { listener, location })
    }

    /// Where this server listens.
    pub fn location(&self) -> &SocketLocation {
        &self.location
    }

    /// Wait for the next connection.
    pub async fn accept(&self) -> Result<Connection> {
        let stream = self
            .listener
            .accept()
            .await
            .map_err(|e| Error::Daemon(format!("accept failed: {e}")))?;
        Ok(Connection { line: Framed::new(stream) })
    }
}

/// One accepted connection, seen from the daemon.
pub struct Connection {
    line: Framed,
}

impl std::fmt::Debug for Connection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Connection")
    }
}

impl Connection {
    /// Read the next request line. `None` when the client closed the connection.
    pub async fn read(&mut self) -> Result<Option<Request>> {
        self.line.read().await
    }

    /// Write one response line.
    pub async fn write(&mut self, response: &Response) -> Result<()> {
        self.line.write(response).await
    }
}

/// A command's end of the socket.
pub struct Client {
    line: Framed,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Client")
    }
}

impl Client {
    /// Connect to the daemon of `root`. Fails fast when nothing listens.
    pub async fn connect(root: &Path) -> Result<Self> {
        let location = socket_location(root);
        let name = socket_name(&location)?;
        let stream = tokio::time::timeout(Duration::from_secs(2), LocalSocketStream::connect(name))
            .await
            .map_err(|_| Error::Daemon(format!("timed out connecting to {location}")))?
            .map_err(|e| Error::Daemon(format!("no daemon on {location}: {e}")))?;
        Ok(Self { line: Framed::new(stream) })
    }

    /// Send one request and read one response, within [`REQUEST_TIMEOUT`].
    pub async fn request(&mut self, request: &Request) -> Result<Response> {
        tokio::time::timeout(REQUEST_TIMEOUT, async {
            self.line.write(request).await?;
            self.next_response().await?.ok_or_else(|| {
                Error::Daemon("daemon closed the connection without answering".to_owned())
            })
        })
        .await
        .map_err(|_| Error::Daemon("daemon did not answer in time".to_owned()))?
    }

    /// Read the next response line (used after [`Request::Watch`]). `None` when the daemon
    /// closed the connection.
    pub async fn next_response(&mut self) -> Result<Option<Response>> {
        self.line.read().await
    }
}

/// Line framing over a split stream. The reader is persistent on purpose: a `BufReader`
/// created per call would read ahead and drop whatever it had buffered beyond the first line.
struct Framed {
    reader: BufReader<RecvHalf>,
    writer: SendHalf,
}

impl Framed {
    fn new(stream: LocalSocketStream) -> Self {
        let (recv, send) = stream.split();
        Self { reader: BufReader::new(recv), writer: send }
    }

    async fn read<T: serde::de::DeserializeOwned>(&mut self) -> Result<Option<T>> {
        let mut line = String::new();
        // A line longer than the cap ends without a newline: reject it instead of buffering.
        let n = (&mut self.reader)
            .take(MAX_REQUEST_BYTES)
            .read_line(&mut line)
            .await
            .map_err(|e| Error::Daemon(format!("read failed: {e}")))?;
        if n == 0 {
            return Ok(None);
        }
        if !line.ends_with('\n') {
            return Err(Error::Daemon(format!("line exceeds {MAX_REQUEST_BYTES} bytes")));
        }
        Ok(Some(serde_json::from_str(line.trim())?))
    }

    async fn write<T: Serialize>(&mut self, value: &T) -> Result<()> {
        let mut line = serde_json::to_vec(value)?;
        line.push(b'\n');
        self.writer
            .write_all(&line)
            .await
            .map_err(|e| Error::Daemon(format!("write failed: {e}")))?;
        self.writer.flush().await.map_err(|e| Error::Daemon(format!("flush failed: {e}")))
    }
}

/// `true` when a daemon answers on the socket of `root`.
pub async fn is_running(root: &Path) -> bool {
    match Client::connect(root).await {
        Ok(mut c) => matches!(c.request(&Request::Ping).await, Ok(Response::Ok)),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_location_is_deterministic_and_short() {
        let dir = tempfile::tempdir().unwrap();
        let a = socket_location(dir.path());
        assert_eq!(a, socket_location(dir.path()));
        if cfg!(windows) {
            assert!(matches!(a, SocketLocation::Namespaced(ref n) if n.starts_with("mda-")));
        } else {
            let SocketLocation::Path(p) = &a else { unreachable!("unix uses a path") };
            assert!(p.as_os_str().len() <= MAX_SOCKET_PATH);
        }
        let long = Path::new("/").join("x".repeat(150));
        let b = socket_location(&long);
        if let SocketLocation::Path(p) = &b {
            assert!(p.as_os_str().len() <= MAX_SOCKET_PATH, "{}", p.display());
            assert!(p.starts_with(std::env::temp_dir()));
        }
        assert_ne!(b, socket_location(Path::new("/other")));
    }

    #[test]
    fn requests_and_responses_round_trip_as_json() {
        let r = Request::Index { path: Some("a.md".into()) };
        let s = serde_json::to_string(&r).unwrap();
        assert_eq!(s, r#"{"op":"index","path":"a.md"}"#);
        assert_eq!(serde_json::from_str::<Request>(&s).unwrap(), r);
        let e = Response::Error { message: "x".into() };
        let s = serde_json::to_string(&e).unwrap();
        assert_eq!(s, r#"{"kind":"error","message":"x"}"#);
        assert_eq!(serde_json::from_str::<Response>(&s).unwrap(), e);
    }

    #[test]
    fn info_file_round_trips_and_removes() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(DaemonInfo::read(dir.path()).unwrap(), None);
        let info = DaemonInfo {
            pid: 42,
            version: "0.1.0".into(),
            started_at: Timestamp::from_second(1_700_000_000).unwrap(),
            root: dir.path().display().to_string(),
            socket: "s".into(),
        };
        info.write(dir.path()).unwrap();
        assert_eq!(DaemonInfo::read(dir.path()).unwrap(), Some(info));
        assert_eq!(std::fs::read_to_string(pid_path(dir.path())).unwrap(), "42");
        DaemonInfo::remove(dir.path());
        assert_eq!(DaemonInfo::read(dir.path()).unwrap(), None);
        assert!(!pid_path(dir.path()).exists());
        DaemonInfo::remove(dir.path());
    }

    #[test]
    fn lock_is_exclusive_per_root_and_released_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let first = Lock::acquire(dir.path()).unwrap();
        let err = Lock::acquire(dir.path()).unwrap_err();
        assert!(err.to_string().contains("another daemon"), "{err}");
        drop(first);
        assert!(Lock::acquire(dir.path()).is_ok());
    }

    #[tokio::test]
    async fn oversized_request_lines_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let server = Server::bind(&root).unwrap();
        let task = tokio::spawn(async move {
            let mut conn = server.accept().await.unwrap();
            conn.read().await
        });
        let mut c = Client::connect(&root).await.unwrap();
        let huge = "x".repeat(usize::try_from(MAX_REQUEST_BYTES).unwrap() + 10);
        let _ = c.line.writer.write_all(huge.as_bytes()).await;
        let err = task.await.unwrap().unwrap_err();
        assert!(err.to_string().contains("exceeds"), "{err}");
    }

    #[tokio::test]
    async fn server_and_client_exchange_lines_over_a_real_socket() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        assert!(!is_running(&root).await);
        let server = Server::bind(&root).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let SocketLocation::Path(p) = socket_location(&root) else { unreachable!() };
            assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let root2 = root.clone();
        let task = tokio::spawn(async move {
            let mut seen = Vec::new();
            'conns: loop {
                let mut conn = server.accept().await.unwrap();
                while let Some(req) = conn.read().await.unwrap() {
                    seen.push(req.clone());
                    let resp = match req {
                        Request::Ping => Response::Ok,
                        Request::Watch => {
                            conn.write(&Response::Ok).await.unwrap();
                            conn.write(&Response::Event(DaemonEvent::Paused)).await.unwrap();
                            break 'conns;
                        }
                        _ => Response::Error { message: "nope".into() },
                    };
                    conn.write(&resp).await.unwrap();
                }
            }
            // Binding again while the first listener lives replaces its socket file, which
            // is why daemons hold a `Lock` first; here only the lock semantics matter.
            let _ = &root2;
            seen
        });
        assert!(is_running(&root).await);
        let mut c = Client::connect(&root).await.unwrap();
        assert_eq!(
            c.request(&Request::Stop).await.unwrap(),
            Response::Error { message: "nope".into() }
        );
        assert_eq!(c.request(&Request::Watch).await.unwrap(), Response::Ok);
        assert_eq!(c.next_response().await.unwrap(), Some(Response::Event(DaemonEvent::Paused)));
        let seen = task.await.unwrap();
        assert_eq!(seen, vec![Request::Ping, Request::Stop, Request::Watch]);
        assert_eq!(c.next_response().await.unwrap(), None, "server hung up");
        // The listener is gone with the task; nothing answers any more.
        assert!(!is_running(&root).await);
    }
}
