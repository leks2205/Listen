//! The demo song video (`--demo`): a generated test picture served over local HTTP with a first-byte
//! delay and a bandwidth limit, so starting and seeking pay a buffering cost like a real stream does.

use std::io::{Read, Seek, SeekFrom, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

/// Length of the generated clip in seconds (the demo songs are shorter).
const SECONDS: u32 = 300;
/// Delay before the first byte of every request.
const FIRST_BYTE: Duration = Duration::from_millis(300);
/// Bandwidth per connection: 3 Mbit/s.
const BYTES_PER_SEC: usize = 375_000;
const CHUNK: usize = 16 * 1024;

/// `LISTEN_DEMO_VIDEO_KBPS` / `LISTEN_DEMO_VIDEO_FIRST_BYTE_MS` override the stream's speed (tests).
fn env_num(name: &str) -> Option<u64> {
    std::env::var(name).ok()?.parse().ok()
}

static URL: OnceLock<Result<String, String>> = OnceLock::new();

/// The address of the demo video; generates the clip and starts the server on first use.
pub fn url() -> Result<String, String> {
    URL.get_or_init(start).clone()
}

fn start() -> Result<String, String> {
    let file = generate()?;
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("demo video server: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let file = file.clone();
            std::thread::spawn(move || {
                let _ = serve(stream, &file);
            });
        }
    });
    Ok(format!("http://127.0.0.1:{port}/demo.mkv"))
}

/// Encodes the clip once (mpv, MPEG-4 in Matroska, keyframe every 2 s) into the temp dir.
fn generate() -> Result<PathBuf, String> {
    let path = std::env::temp_dir().join(format!("listen-demo-video-{SECONDS}s.mkv"));
    if path.metadata().is_ok_and(|m| m.len() > 0) {
        return Ok(path);
    }
    let part = path.with_extension("part.mkv");
    let mut cmd = Command::new(listen_core::platform::tool("mpv"));
    listen_core::platform::no_window(&mut cmd);
    let status = cmd
        .args([
            "--no-config",
            "--load-scripts=no",
            "--terminal=no",
            "--no-audio",
            "--of=matroska",
            "--ovc=mpeg4",
        ])
        .arg("--ovcopts=b=1500k,g=60")
        .arg(format!("--o={}", part.display()))
        .arg("--")
        .arg(format!("av://lavfi:testsrc=size=640x360:rate=30:duration={SECONDS}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("could not start mpv for the demo video ({e})"))?;
    if !status.success() {
        return Err("mpv could not generate the demo video".into());
    }
    std::fs::rename(&part, &path).map_err(|e| e.to_string())?;
    Ok(path)
}

/// One HTTP/1.1 request with optional `Range`, answered slowly.
fn serve(mut stream: TcpStream, file: &Path) -> std::io::Result<()> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") && head.len() < 16 * 1024 {
        if stream.read(&mut byte)? == 0 {
            return Ok(());
        }
        head.push(byte[0]);
    }
    let head = String::from_utf8_lossy(&head).to_string();
    let mut data = std::fs::File::open(file)?;
    let total = data.metadata()?.len() as usize;
    let range = head.lines().find_map(|l| {
        let l = l.to_ascii_lowercase();
        let spec = l.strip_prefix("range: bytes=")?.trim().to_string();
        let (a, b) = spec.split_once('-')?;
        let a = a.parse::<usize>().ok()?;
        let b = b.parse::<usize>().ok().map_or(total - 1, |b| b.min(total - 1));
        Some((a, b))
    });
    std::thread::sleep(env_num("LISTEN_DEMO_VIDEO_FIRST_BYTE_MS").map_or(FIRST_BYTE, Duration::from_millis));
    let bytes_per_sec =
        env_num("LISTEN_DEMO_VIDEO_KBPS").map_or(BYTES_PER_SEC, |k| (k as usize * 125).max(CHUNK));
    let (status, start, end) = match range {
        Some((a, b)) if a < total && a <= b => ("206 Partial Content", a, b),
        _ => ("200 OK", 0, total - 1),
    };
    let mut reply = format!(
        "HTTP/1.1 {status}\r\nContent-Type: video/x-matroska\r\nAccept-Ranges: bytes\r\nContent-Length: {}\r\nConnection: close\r\n",
        end - start + 1
    );
    if status.starts_with("206") {
        reply.push_str(&format!("Content-Range: bytes {start}-{end}/{total}\r\n"));
    }
    reply.push_str("\r\n");
    stream.write_all(reply.as_bytes())?;
    if head.starts_with("HEAD") {
        return Ok(());
    }
    data.seek(SeekFrom::Start(start as u64))?;
    let mut left = end - start + 1;
    let mut buf = vec![0u8; CHUNK];
    while left > 0 {
        let n = left.min(CHUNK);
        data.read_exact(&mut buf[..n])?;
        stream.write_all(&buf[..n])?;
        left -= n;
        std::thread::sleep(Duration::from_secs_f64(n as f64 / bytes_per_sec as f64));
    }
    Ok(())
}
