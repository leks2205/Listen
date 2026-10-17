//! The stream proxy over loopback: a local file with Range, a remote stream needing a header,
//! unknown tokens.

use listen_dlna::{Proxy, Source};
use std::io::Write;

fn sample(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

async fn proxy() -> Proxy {
    Proxy::start("127.0.0.1:0".parse().unwrap()).await.unwrap()
}

fn temp_file(bytes: &[u8], name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "listen-dlna-test-{}-{}",
        std::process::id(),
        rand::random::<u32>()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::File::create(&path).unwrap().write_all(bytes).unwrap();
    path
}

#[tokio::test]
async fn serves_a_file_whole_and_by_range() {
    let data = sample(300_000);
    let path = temp_file(&data, "song.flac");
    let p = proxy().await;
    let url = p.register(Source::File { path }, "song.flac");
    assert!(url.ends_with(".flac"));
    let http = reqwest::Client::new();

    let whole = http.get(&url).send().await.unwrap();
    assert_eq!(whole.status(), 200);
    assert_eq!(whole.headers()["content-type"], "audio/flac");
    assert_eq!(whole.headers()["accept-ranges"], "bytes");
    assert_eq!(whole.bytes().await.unwrap().as_ref(), data.as_slice());

    let part = http.get(&url).header("Range", "bytes=1000-1999").send().await.unwrap();
    assert_eq!(part.status(), 206);
    assert_eq!(part.headers()["content-range"], "bytes 1000-1999/300000");
    assert_eq!(part.bytes().await.unwrap().as_ref(), &data[1000..2000]);

    let tail = http.get(&url).header("Range", "bytes=299000-").send().await.unwrap();
    assert_eq!(tail.status(), 206);
    assert_eq!(tail.bytes().await.unwrap().as_ref(), &data[299_000..]);

    let last = http.get(&url).header("Range", "bytes=-10").send().await.unwrap();
    assert_eq!(last.bytes().await.unwrap().as_ref(), &data[299_990..]);

    let bad = http.get(&url).header("Range", "bytes=400000-").send().await.unwrap();
    assert_eq!(bad.status(), 416);

    let head = http.head(&url).send().await.unwrap();
    assert_eq!(head.status(), 200);
    assert_eq!(head.headers()["content-length"], "300000");
    assert!(head.bytes().await.unwrap().is_empty());
}

#[tokio::test]
async fn unknown_tokens_and_methods_are_refused() {
    let p = proxy().await;
    let base = format!("http://{}", p.addr());
    let http = reqwest::Client::new();
    assert_eq!(http.get(format!("{base}/s/deadbeef.mp3")).send().await.unwrap().status(), 404);
    assert_eq!(http.get(format!("{base}/etc/passwd")).send().await.unwrap().status(), 404);
    let url = p.register(Source::File { path: "/nonexistent/file.mp3".into() }, "file.mp3");
    assert_eq!(http.get(&url).send().await.unwrap().status(), 404, "a vanished file");
    assert_eq!(http.post(&url).send().await.unwrap().status(), 405);
    p.clear();
    assert_eq!(http.get(&url).send().await.unwrap().status(), 404, "cleared routes are gone");
}

#[tokio::test]
async fn proxies_a_remote_stream_with_its_headers_and_ranges() {
    // The headers of the proxied request reach the origin, and Range is honoured.
    let data = sample(50_000);
    let origin = proxy().await;
    let path = temp_file(&data, "origin.m4a");
    let origin_url = origin.register(Source::File { path }, "origin.m4a");
    // An origin that echoes the received headers.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let echo = listener.local_addr().unwrap();
    let body = data.clone();
    tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        while let Ok((mut s, _)) = listener.accept().await {
            let body = body.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_lowercase();
                let ok = head.contains("x-stream-key: secret") && !head.contains("cookie");
                if !ok {
                    let _ = s
                        .write_all(
                            b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        )
                        .await;
                    return;
                }
                let (status, slice, extra) = match head.lines().find_map(|l| l.strip_prefix("range: bytes="))
                {
                    Some(r) => {
                        let (a, b) = r.split_once('-').unwrap();
                        let a: usize = a.parse().unwrap();
                        let b: usize = b.parse().unwrap();
                        (
                            "206 Partial Content",
                            &body[a..=b],
                            format!("Content-Range: bytes {a}-{b}/{}\r\n", body.len()),
                        )
                    }
                    None => ("200 OK", &body[..], String::new()),
                };
                let head = format!(
                    "HTTP/1.1 {status}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n",
                    slice.len()
                );
                let _ = s.write_all(head.as_bytes()).await;
                let _ = s.write_all(slice).await;
            });
        }
    });
    let _ = origin_url;
    let p = proxy().await;
    let url = p.register(
        Source::Remote {
            url: format!("http://{echo}/audio"),
            headers: vec![("X-Stream-Key".into(), "secret".into())],
        },
        "audio.m4a",
    );
    let http = reqwest::Client::new();
    let whole = http.get(&url).send().await.unwrap();
    assert_eq!(whole.status(), 200);
    assert_eq!(whole.bytes().await.unwrap().as_ref(), data.as_slice());
    let part = http.get(&url).header("Range", "bytes=10-19").send().await.unwrap();
    assert_eq!(part.status(), 206);
    assert_eq!(part.headers()["content-range"], "bytes 10-19/50000");
    assert_eq!(part.bytes().await.unwrap().as_ref(), &data[10..20]);
    // A client cookie never reaches the origin.
    let cookie = http.get(&url).header("Cookie", "SID=abc").send().await.unwrap();
    assert_eq!(cookie.status(), 200);
}
