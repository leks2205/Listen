//! One Listen per user: a Unix socket in `$XDG_RUNTIME_DIR` (Windows: a loopback
//! TCP port, announced in a lock file the running instance keeps open). A second
//! launch connects, hands over what it was asked to do (show the window, open a
//! route) and exits; the running instance brings its window forward.

use crate::app::Message;
use iced::Subscription;
use iced::futures::{SinkExt, Stream, channel::mpsc};
use std::io::Write;
#[cfg(windows)]
use std::net::TcpListener as Listener;
#[cfg(unix)]
use std::os::unix::net::{UnixListener as Listener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

/// What a second launch asks the running one to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Forward {
    /// Bring the window forward.
    Show,
    /// Bring the window forward and open this page (`--route`).
    Route(String),
}

impl Forward {
    fn encode(&self) -> String {
        match self {
            Forward::Show => "show\n".into(),
            Forward::Route(r) => format!("route {}\n", r.replace(['\n', '\r'], " ")),
        }
    }

    fn decode(line: &str) -> Option<Self> {
        let line = line.trim();
        if line == "show" {
            return Some(Forward::Show);
        }
        let route = line.strip_prefix("route ")?.trim();
        (!route.is_empty()).then(|| Forward::Route(route.to_string()))
    }
}

/// Outcome of [`acquire`].
#[derive(Debug)]
pub enum Instance {
    /// We are the first: keep the lock for the process's life.
    Primary(Lock),
    /// Another instance is running and was told what to do.
    Secondary,
}

/// The listening socket; the file is removed when it drops.
#[derive(Debug)]
pub struct Lock {
    listener: Listener,
    #[cfg(unix)]
    path: PathBuf,
    /// Windows: the lock file, held open (share-read only) for the process's life.
    #[cfg(windows)]
    _file: std::fs::File,
}

#[cfg(unix)]
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// The socket (Windows: lock file) path inside `runtime_dir`.
pub fn socket_path(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join(format!(
        "{}.{}",
        listen_core::config::APP_ID,
        if cfg!(windows) { "lock" } else { "sock" }
    ))
}

/// `$XDG_RUNTIME_DIR`, else a private per-user directory in the temp dir.
pub fn runtime_dir() -> PathBuf {
    #[cfg(unix)]
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).filter(|d| d.is_dir()) {
        return dir;
    }
    let dir = std::env::temp_dir().join(format!("listen-{}", users_id()));
    let _ = std::fs::create_dir_all(&dir);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    dir
}

fn users_id() -> String {
    std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_else(|_| "user".into())
}

/// Takes the lock in `dir`, or tells the instance holding it what to do.
#[cfg(windows)]
pub fn acquire(dir: &Path, forward: &Forward) -> std::io::Result<Instance> {
    use std::io::{Seek, SeekFrom};
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_READ: u32 = 1;
    const ERROR_SHARING_VIOLATION: i32 = 32;
    let path = socket_path(dir);
    for _ in 0..40 {
        // Opening for write while sharing only reads: a second instance cannot,
        // and the OS drops our hold if we crash (no stale lock).
        match std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(FILE_SHARE_READ)
            .open(&path)
        {
            Ok(mut file) => {
                let listener = Listener::bind(("127.0.0.1", 0))?;
                listener.set_nonblocking(true)?;
                let port = listener.local_addr()?.port();
                file.set_len(0)?;
                file.seek(SeekFrom::Start(0))?;
                file.write_all(port.to_string().as_bytes())?;
                file.flush()?;
                return Ok(Instance::Primary(Lock { listener, _file: file }));
            }
            Err(e) if e.raw_os_error() == Some(ERROR_SHARING_VIOLATION) => {
                let port = std::fs::read_to_string(&path).ok().and_then(|t| t.trim().parse::<u16>().ok());
                // No port yet: the holder is still starting; try again shortly.
                if let Some(port) = port
                    && send_tcp(port, forward).is_ok()
                {
                    return Ok(Instance::Secondary);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other("could not take the single-instance lock"))
}

#[cfg(windows)]
fn send_tcp(port: u16, forward: &Forward) -> std::io::Result<()> {
    let mut stream = std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_secs(2),
    )?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    stream.write_all(forward.encode().as_bytes())
}

/// Takes the lock in `dir`, or tells the instance holding it what to do.
#[cfg(unix)]
pub fn acquire(dir: &Path, forward: &Forward) -> std::io::Result<Instance> {
    let path = socket_path(dir);
    for _ in 0..4 {
        match Listener::bind(&path) {
            Ok(listener) => {
                listener.set_nonblocking(true)?;
                return Ok(Instance::Primary(Lock { listener, path }));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {}
            Err(e) => return Err(e),
        }
        match send(&path, forward) {
            Ok(()) => return Ok(Instance::Secondary),
            // Nobody listens: a crashed instance left the file behind.
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
                ) =>
            {
                let _ = std::fs::remove_file(&path);
            }
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other("could not take the single-instance lock"))
}

#[cfg(unix)]
fn send(path: &Path, forward: &Forward) -> std::io::Result<()> {
    let mut stream = UnixStream::connect(path)?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    stream.write_all(forward.encode().as_bytes())
}

/// What the running instance should do for this command line: `--route X` is
/// forwarded, anything else just shows the window.
pub fn forward_for(route: Option<&str>) -> Forward {
    match route {
        Some(r) if !r.is_empty() => Forward::Route(r.to_string()),
        _ => Forward::Show,
    }
}

/// Modes that never take the lock: demos, screenshots, tests and checks.
pub fn exempt(args: &crate::Args) -> bool {
    args.demo || args.ephemeral || args.screenshot.is_some() || args.login.is_some()
}

static LISTENER: Mutex<Option<Lock>> = Mutex::new(None);

/// Hands the lock to the app (its subscription listens on it).
pub fn install(lock: Lock) {
    *LISTENER.lock().unwrap_or_else(|e| e.into_inner()) = Some(lock);
}

impl Lock {
    /// Reads the pending forwards (tests).
    #[cfg(test)]
    fn accept_all(&self, mut on: impl FnMut(Forward)) {
        use std::io::Read;
        loop {
            match self.listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = stream.set_nonblocking(false);
                    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
                    let mut buf = String::new();
                    let _ = (&mut stream).take(512).read_to_string(&mut buf);
                    if let Some(f) = buf.lines().next().and_then(Forward::decode) {
                        on(f);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return,
                Err(_) => return,
            }
        }
    }
}

pub fn subscription() -> Subscription<Message> {
    Subscription::run(worker)
}

fn worker() -> impl Stream<Item = Message> {
    iced::stream::channel(8, async move |mut out: mpsc::Sender<Message>| {
        let lock = LISTENER.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(lock) = lock
            && let Ok(clone) = lock.listener.try_clone()
            && let Ok(listener) = {
                #[cfg(unix)]
                {
                    tokio::net::UnixListener::from_std(clone)
                }
                #[cfg(windows)]
                {
                    tokio::net::TcpListener::from_std(clone)
                }
            }
        {
            while let Ok((mut stream, _)) = listener.accept().await {
                use tokio::io::AsyncReadExt;
                let mut buf = vec![0u8; 512];
                let read = tokio::time::timeout(Duration::from_millis(500), stream.read(&mut buf)).await;
                let Ok(Ok(n)) = read else { continue };
                let text = String::from_utf8_lossy(&buf[..n]).into_owned();
                if let Some(f) = text.lines().next().and_then(Forward::decode) {
                    let _ = out.send(Message::SingleInstance(f)).await;
                }
            }
            drop(lock);
        }
        std::future::pending::<()>().await;
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("listen-si-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn drain(lock: &Lock) -> Vec<Forward> {
        let mut got = Vec::new();
        // The connection may still be in flight.
        for _ in 0..20 {
            lock.accept_all(|f| got.push(f));
            if !got.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        got
    }

    #[test]
    fn second_lock_forwards_and_does_not_take_over() {
        let dir = temp_dir("two");
        let Instance::Primary(first) = acquire(&dir, &Forward::Show).unwrap() else {
            panic!("first is primary")
        };
        let second = acquire(&dir, &Forward::Route("settings".into())).unwrap();
        assert!(matches!(second, Instance::Secondary));
        assert_eq!(drain(&first), vec![Forward::Route("settings".into())]);
        let third = acquire(&dir, &Forward::Show).unwrap();
        assert!(matches!(third, Instance::Secondary));
        assert_eq!(drain(&first), vec![Forward::Show]);
        drop(first);
        // Released: the next launch is primary again.
        assert!(matches!(acquire(&dir, &Forward::Show).unwrap(), Instance::Primary(_)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn stale_socket_file_is_taken_over() {
        let dir = temp_dir("stale");
        // A crashed instance: the socket file exists, nobody listens.
        let listener = Listener::bind(socket_path(&dir)).unwrap();
        drop(listener);
        assert!(socket_path(&dir).exists());
        assert!(matches!(acquire(&dir, &Forward::Show).unwrap(), Instance::Primary(_)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn forward_round_trip() {
        for f in [Forward::Show, Forward::Route("albums".into())] {
            assert_eq!(Forward::decode(&f.encode()), Some(f));
        }
        assert_eq!(Forward::decode("route "), None);
        assert_eq!(Forward::decode("nonsense"), None);
        assert_eq!(forward_for(None), Forward::Show);
        assert_eq!(forward_for(Some("tracks")), Forward::Route("tracks".into()));
    }

    #[test]
    fn modes_without_a_lock() {
        let demo = crate::Args { demo: true, ..Default::default() };
        assert!(exempt(&demo));
        assert!(!exempt(&crate::Args::default()));
    }
}
