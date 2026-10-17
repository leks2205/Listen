//! Internet radio CRUD and `jukeboxControl` against a fake server: exact requests and parsed answers.

use listen_core::model::ServerId;
use listen_source::{Capabilities, InternetRadio, JukeboxCmd, MusicSource};
use listen_subsonic::Subsonic;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const STATIONS: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","internetRadioStations":{"internetRadioStation":[
  {"id":"r1","name":"Jazz FM","streamUrl":"http://radio.example/jazz","homePageUrl":"http://radio.example"},
  {"id":"r2","name":"Rock","streamUrl":"http://radio.example/rock"}]}}}"#;
const JUKEBOX: &str = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","jukeboxStatus":{"currentIndex":1,"playing":true,"gain":0.8,"position":42}}}"#;
const NO_JUKEBOX: &str = r#"{"subsonic-response":{"status":"failed","version":"1.16.1","error":{"code":0,"message":"Jukebox is disabled"}}}"#;

async fn fake_server(jukebox: &'static str) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            let mut buf = vec![0u8; 8192];
            let n = sock.read(&mut buf).await.unwrap_or(0);
            let line = String::from_utf8_lossy(&buf[..n]).lines().next().unwrap_or("").to_string();
            log.lock().unwrap().push(line.clone());
            let body =
                if line.contains("/rest/getInternetRadioStations") || line.contains("/rest/createInternet") {
                    STATIONS
                } else if line.contains("/rest/jukeboxControl") {
                    jukebox
                } else {
                    r#"{"subsonic-response":{"status":"ok","version":"1.16.1"}}"#
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

fn values(line: &str, key: &str) -> Vec<String> {
    let query = line.split('?').nth(1).and_then(|q| q.split(' ').next()).unwrap_or("");
    url::form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .filter(|(k, _)| k == key)
        .map(|(_, v)| v)
        .collect()
}

fn client(base: &str) -> Subsonic {
    Subsonic::new(ServerId("t".into()), base, "alice", "pw").unwrap()
}

#[tokio::test]
async fn lists_stations() {
    let (base, seen) = fake_server(JUKEBOX).await;
    let nd = client(&base);
    assert!(nd.capabilities().contains(Capabilities::INTERNET_RADIO));
    let list = nd.internet_radios().await.unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].name, "Jazz FM");
    assert_eq!(list[0].home_page_url.as_deref(), Some("http://radio.example"));
    assert!(seen.lock().unwrap()[0].starts_with("GET /rest/getInternetRadioStations?"));
}

#[tokio::test]
async fn create_update_delete_send_the_documented_parameters() {
    let (base, seen) = fake_server(JUKEBOX).await;
    let nd = client(&base);
    let new = InternetRadio {
        id: String::new(),
        name: "Jazz FM".into(),
        stream_url: "http://radio.example/jazz".into(),
        home_page_url: Some("http://radio.example".into()),
        image_id: None,
    };
    let made = nd.create_internet_radio(&new).await.unwrap();
    assert_eq!(made.id, "r1", "the new station is found again by its stream address");
    nd.update_internet_radio(&InternetRadio { name: "Jazz".into(), ..made.clone() }).await.unwrap();
    nd.delete_internet_radio("r1").await.unwrap();
    let lines = seen.lock().unwrap().clone();
    assert!(lines[0].starts_with("GET /rest/createInternetRadioStation?"), "{}", lines[0]);
    assert_eq!(values(&lines[0], "streamUrl"), ["http://radio.example/jazz"]);
    assert_eq!(values(&lines[0], "name"), ["Jazz FM"]);
    assert_eq!(values(&lines[0], "homepageUrl"), ["http://radio.example"]);
    let update = lines.iter().find(|l| l.contains("/rest/updateInternetRadioStation")).unwrap();
    assert_eq!(values(update, "id"), ["r1"]);
    assert_eq!(values(update, "name"), ["Jazz"]);
    let delete = lines.iter().find(|l| l.contains("/rest/deleteInternetRadioStation")).unwrap();
    assert_eq!(values(delete, "id"), ["r1"]);
}

#[tokio::test]
async fn jukebox_requests_and_status() {
    let (base, seen) = fake_server(JUKEBOX).await;
    let nd = client(&base);
    let st = nd.jukebox(&JukeboxCmd::Set(vec!["s1".into(), "s2".into()])).await.unwrap();
    assert!(st.playing && st.current_index == 1 && st.position_secs == 42);
    nd.jukebox(&JukeboxCmd::Skip { index: 1, offset_secs: 5 }).await.unwrap();
    nd.jukebox(&JukeboxCmd::SetGain(0.5)).await.unwrap();
    let lines = seen.lock().unwrap().clone();
    assert_eq!(values(&lines[0], "action"), ["set"]);
    assert_eq!(values(&lines[0], "id"), ["s1", "s2"]);
    assert_eq!(values(&lines[1], "action"), ["skip"]);
    assert_eq!(values(&lines[1], "index"), ["1"]);
    assert_eq!(values(&lines[2], "gain"), ["0.50"]);
}

#[tokio::test]
async fn an_unsupported_jukebox_is_an_error() {
    let (base, _) = fake_server(NO_JUKEBOX).await;
    assert!(client(&base).jukebox(&JukeboxCmd::Status).await.is_err());
}
