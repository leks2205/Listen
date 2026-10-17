//! A tiny HTTP server on one LAN address that serves audio to renderers: a local file or a
//! remote stream that needs headers (googlevideo), with Range support. Each stream lives under
//! an unguessable token; only the stream's own headers are passed on, never cookies.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// What a token serves.
#[derive(Debug, Clone)]
pub enum Source {
    File { path: PathBuf },
    Remote { url: String, headers: Vec<(String, String)> },
}

#[derive(Debug, Clone)]
struct Entry {
    source: Source,
    mime: String,
}

/// The running server. Dropping it stops it.
#[derive(Debug)]
pub struct Proxy {
    addr: SocketAddr,
    routes: Arc<Mutex<HashMap<String, Entry>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Audio MIME type by file extension.
pub fn mime_for(name: &str) -> &'static str {
    match name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).as_deref() {
        Some("mp3") => "audio/mpeg",
        Some("flac") => "audio/flac",
        Some("m4a" | "mp4" | "aac") => "audio/mp4",
        Some("ogg" | "oga") => "audio/ogg",
        Some("opus") => "audio/opus",
        Some("wav") => "audio/wav",
        Some("webm") => "audio/webm",
        _ => "audio/mpeg",
    }
}

/// `Range: bytes=...` for a resource of `len` bytes: `Some(Ok((first, last)))` inclusive,
/// `None` when not a single range, `Some(Err(()))` when not satisfiable.
pub fn parse_range(header: &str, len: u64) -> Option<Result<(u64, u64), ()>> {
    let spec = header.trim().strip_prefix("bytes=")?;
    if spec.contains(',') {
        return None; // multi-range: serve the whole thing
    }
    let (a, b) = spec.split_once('-')?;
    let range = match (a.trim(), b.trim()) {
        ("", "") => return None,
        ("", n) => {
            let n: u64 = n.parse().ok()?;
            if n == 0 || len == 0 {
                return Some(Err(()));
            }
            (len.saturating_sub(n), len - 1)
        }
        (a, "") => (a.parse().ok()?, len.checked_sub(1)?),
        (a, b) => (a.parse().ok()?, b.parse::<u64>().ok()?.min(len.saturating_sub(1))),
    };
    if range.0 >= len || range.0 > range.1 { Some(Err(())) } else { Some(Ok(range)) }
}

impl Proxy {
    /// Listens on `bind` (use the LAN address that reaches the renderer; port 0 = any).
    pub async fn start(bind: SocketAddr) -> std::io::Result<Proxy> {
        let listener = TcpListener::bind(bind).await?;
        let addr = listener.local_addr()?;
        let routes: Arc<Mutex<HashMap<String, Entry>>> = Arc::default();
        let shared = routes.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else { continue };
                let routes = shared.clone();
                tokio::spawn(async move {
                    if let Err(e) = serve(stream, routes).await {
                        tracing::debug!("DLNA proxy: {e}");
                    }
                });
            }
        });
        Ok(Proxy { addr, routes, task })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Serves `source` and returns its URL; `name` only gives the URL a file extension (some
    /// renderers sniff by it).
    pub fn register(&self, source: Source, name: &str) -> String {
        let token: String = rand::random::<[u8; 16]>().iter().map(|b| format!("{b:02x}")).collect();
        let ext = name.rsplit_once('.').map(|(_, e)| e).filter(|e| e.len() <= 5 && !e.contains('/'));
        let mime = mime_for(name).to_string();
        let path = match ext {
            Some(e) => format!("{token}.{e}"),
            None => token.clone(),
        };
        let mut routes = self.routes.lock().unwrap_or_else(|e| e.into_inner());
        // Only the current and the previous song stay reachable.
        if routes.len() >= 4 {
            routes.clear();
        }
        routes.insert(token, Entry { source, mime });
        let host = match self.addr.ip() {
            IpAddr::V6(a) => format!("[{a}]"),
            a => a.to_string(),
        };
        format!("http://{host}:{}/s/{path}", self.addr.port())
    }

    pub fn clear(&self) {
        self.routes.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
}

struct Request {
    method: String,
    path: String,
    range: Option<String>,
}

async fn read_request(stream: &mut TcpStream) -> std::io::Result<Request> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if buf.len() > 16 * 1024 {
            return Err(std::io::Error::other("request head too large"));
        }
    }
    let head = String::from_utf8_lossy(&buf);
    let mut lines = head.lines();
    let mut first = lines.next().unwrap_or_default().split_whitespace();
    let (method, path) =
        (first.next().unwrap_or_default().to_string(), first.next().unwrap_or_default().to_string());
    let range = lines
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case("range"))
        .map(|(_, v)| v.trim().to_string());
    Ok(Request { method, path, range })
}

async fn respond(stream: &mut TcpStream, status: &str, headers: &[(&str, String)]) -> std::io::Result<()> {
    let mut head = format!("HTTP/1.1 {status}\r\nConnection: close\r\n");
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await
}

fn dlna_headers(mime: &str, len: Option<u64>, extra: &mut Vec<(&'static str, String)>) {
    extra.push(("Content-Type", mime.to_string()));
    extra.push(("Accept-Ranges", "bytes".to_string()));
    extra.push(("transferMode.dlna.org", "Streaming".to_string()));
    extra.push((
        "contentFeatures.dlna.org",
        "DLNA.ORG_OP=01;DLNA.ORG_CI=0;DLNA.ORG_FLAGS=01700000000000000000000000000000".to_string(),
    ));
    if let Some(len) = len {
        extra.push(("Content-Length", len.to_string()));
    }
}

async fn serve(mut stream: TcpStream, routes: Arc<Mutex<HashMap<String, Entry>>>) -> std::io::Result<()> {
    let req = read_request(&mut stream).await?;
    let head_only = req.method == "HEAD";
    if req.method != "GET" && !head_only {
        return respond(&mut stream, "405 Method Not Allowed", &[("Content-Length", "0".into())]).await;
    }
    let token = req
        .path
        .strip_prefix("/s/")
        .map(|p| p.split(['?', '#']).next().unwrap_or(p))
        .map(|p| p.split('.').next().unwrap_or(p).to_string());
    let entry = token.and_then(|t| routes.lock().unwrap_or_else(|e| e.into_inner()).get(&t).cloned());
    let Some(entry) = entry else {
        return respond(&mut stream, "404 Not Found", &[("Content-Length", "0".into())]).await;
    };
    match &entry.source {
        Source::File { path } => {
            serve_file(&mut stream, path, &entry.mime, req.range.as_deref(), head_only).await
        }
        Source::Remote { url, headers } => {
            serve_remote(&mut stream, url, headers, &entry.mime, req.range.as_deref(), head_only).await
        }
    }
}

async fn serve_file(
    stream: &mut TcpStream,
    path: &std::path::Path,
    mime: &str,
    range: Option<&str>,
    head_only: bool,
) -> std::io::Result<()> {
    let mut file = match tokio::fs::File::open(path).await {
        Ok(f) => f,
        Err(_) => return respond(stream, "404 Not Found", &[("Content-Length", "0".into())]).await,
    };
    let len = file.metadata().await?.len();
    let parsed = range.and_then(|r| parse_range(r, len));
    let (status, first, count, mut headers) = match parsed {
        Some(Err(())) => {
            return respond(
                stream,
                "416 Range Not Satisfiable",
                &[("Content-Range", format!("bytes */{len}")), ("Content-Length", "0".into())],
            )
            .await;
        }
        Some(Ok((a, b))) => {
            ("206 Partial Content", a, b - a + 1, vec![("Content-Range", format!("bytes {a}-{b}/{len}"))])
        }
        None => ("200 OK", 0, len, Vec::new()),
    };
    dlna_headers(mime, Some(count), &mut headers);
    respond(stream, status, &headers).await?;
    if head_only {
        return Ok(());
    }
    file.seek(std::io::SeekFrom::Start(first)).await?;
    let mut left = count;
    let mut buf = vec![0u8; 64 * 1024];
    while left > 0 {
        let want = buf.len().min(left as usize);
        let n = file.read(&mut buf[..want]).await?;
        if n == 0 {
            break;
        }
        stream.write_all(&buf[..n]).await?;
        left -= n as u64;
    }
    Ok(())
}

async fn serve_remote(
    stream: &mut TcpStream,
    url: &str,
    headers: &[(String, String)],
    mime: &str,
    range: Option<&str>,
    head_only: bool,
) -> std::io::Result<()> {
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .read_timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(std::io::Error::other)?;
    // HEAD is answered from a GET whose body is dropped: servers differ on HEAD.
    let mut request = client.get(url);
    for (k, v) in headers {
        request = request.header(k, v);
    }
    if let Some(r) = range {
        request = request.header("Range", r);
    }
    let mut upstream = match request.send().await {
        Ok(r) => r,
        Err(_) => return respond(stream, "502 Bad Gateway", &[("Content-Length", "0".into())]).await,
    };
    let code = upstream.status();
    let status = match code.as_u16() {
        200 => "200 OK",
        206 => "206 Partial Content",
        416 => "416 Range Not Satisfiable",
        _ => return respond(stream, "502 Bad Gateway", &[("Content-Length", "0".into())]).await,
    };
    let mut out = Vec::new();
    if let Some(v) = upstream.headers().get("content-range").and_then(|v| v.to_str().ok()) {
        out.push(("Content-Range", v.to_string()));
    }
    let len =
        upstream.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok());
    dlna_headers(mime, len, &mut out);
    respond(stream, status, &out).await?;
    if head_only {
        return Ok(());
    }
    while let Some(chunk) = upstream.chunk().await.map_err(std::io::Error::other)? {
        stream.write_all(&chunk).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        assert_eq!(parse_range("bytes=0-99", 1000), Some(Ok((0, 99))));
        assert_eq!(parse_range("bytes=900-", 1000), Some(Ok((900, 999))));
        assert_eq!(parse_range("bytes=-100", 1000), Some(Ok((900, 999))));
        assert_eq!(parse_range("bytes=500-5000", 1000), Some(Ok((500, 999))));
        assert_eq!(parse_range("bytes=1000-", 1000), Some(Err(())));
        assert_eq!(parse_range("bytes=5-2", 1000), Some(Err(())));
        assert_eq!(parse_range("bytes=0-1,5-9", 1000), None);
        assert_eq!(parse_range("items=0-1", 1000), None);
    }

    #[test]
    fn mimes() {
        assert_eq!(mime_for("a.flac"), "audio/flac");
        assert_eq!(mime_for("a.M4A"), "audio/mp4");
        assert_eq!(mime_for("noext"), "audio/mpeg");
    }
}
