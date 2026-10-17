//! `startScan` / `getScanStatus` against a fake Navidrome: admins get a running scan, others error 50
//! ("not authorized"), which Listen treats as "the file watcher will pick it up".

use listen_core::model::ServerId;
use listen_source::{MusicSource, ScanStatus, SourceError};
use listen_subsonic::Subsonic;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Serves one canned JSON body per request; returns the base URL and the
/// request lines it saw.
async fn fake_server(admin: bool) -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            let mut buf = vec![0u8; 8192];
            let n = sock.read(&mut buf).await.unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let line = req.lines().next().unwrap_or("").to_string();
            log.lock().unwrap().push(line.clone());
            let body = if !admin && line.contains("/rest/startScan") {
                r#"{"subsonic-response":{"status":"failed","version":"1.16.1","error":{"code":50,"message":"User not authorized"}}}"#
            } else if line.contains("/rest/startScan") {
                r#"{"subsonic-response":{"status":"ok","version":"1.16.1","scanStatus":{"scanning":true,"count":0}}}"#
            } else {
                r#"{"subsonic-response":{"status":"ok","version":"1.16.1","scanStatus":{"scanning":false,"count":734,"folderCount":40}}}"#
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes()).await;
        }
    });
    (base, seen)
}

#[tokio::test]
async fn admin_starts_a_quick_scan() {
    let (base, seen) = fake_server(true).await;
    let nd = Subsonic::new(ServerId("t".into()), &base, "admin", "pw").unwrap();
    assert_eq!(nd.start_scan().await.unwrap(), ScanStatus { scanning: true, count: 0 });
    assert_eq!(nd.scan_status().await.unwrap(), ScanStatus { scanning: false, count: 734 });
    let seen = seen.lock().unwrap();
    assert!(seen[0].starts_with("GET /rest/startScan?") && seen[0].contains("fullScan=false"), "{seen:?}");
    assert!(seen[1].starts_with("GET /rest/getScanStatus?"));
}

#[tokio::test]
async fn non_admin_gets_not_authorized() {
    let (base, _) = fake_server(false).await;
    let nd = Subsonic::new(ServerId("t".into()), &base, "listen", "pw").unwrap();
    match nd.start_scan().await {
        Err(SourceError::Auth(m)) => assert!(m.contains("not authorized")),
        other => panic!("expected Auth error, got {other:?}"),
    }
}

/// Live: `LISTEN_ND_URL/USER/PASS` set → start a scan on the real server.
/// A non-admin account must answer "not authorized", an admin one a status.
#[tokio::test]
async fn live_start_scan() {
    let (Ok(url), Ok(user), Ok(pass)) =
        (std::env::var("LISTEN_ND_URL"), std::env::var("LISTEN_ND_USER"), std::env::var("LISTEN_ND_PASS"))
    else {
        eprintln!("LISTEN_ND_* not set; skipping");
        return;
    };
    let nd = Subsonic::new(ServerId("live".into()), &url, &user, &pass).unwrap();
    match nd.start_scan().await {
        Ok(s) => eprintln!("scan started: {s:?}"),
        Err(SourceError::Auth(m)) => eprintln!("not allowed (expected for non-admins): {m}"),
        Err(e) => panic!("startScan: {e}"),
    }
}
