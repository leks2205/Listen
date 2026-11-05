//! The Home shelf "New from your artists": the release radar listen-server keeps (`GET /v1/releases`,
//! rules in `listen_core::releases`), shown above the YouTube Music feed when YouTube Music is the
//! active server and "Merge with other devices" has a server address and token.

use super::Listen;
use listen_core::model::{Album, ArtistRef, ImageRef, ServerId, SourceKind};
use listen_core::releases::Release;
use listen_source::HomeShelf;
use serde::Deserialize;
use std::time::Duration;

pub const SHELF_TITLE: &str = "New from your artists";

/// Where to ask and for which server the albums are.
#[derive(Debug, Clone)]
pub struct Request {
    url: String,
    token: String,
    server: ServerId,
}

#[derive(Debug, Deserialize)]
struct Feed {
    #[serde(default)]
    releases: Vec<Release>,
}

/// The shelf of `releases` as albums of the YouTube Music server `server`; none when empty.
pub fn shelf(server: &ServerId, releases: Vec<Release>) -> Option<HomeShelf> {
    if releases.is_empty() {
        return None;
    }
    let albums = releases
        .into_iter()
        .map(|r| Album {
            id: r.id,
            server_id: server.clone(),
            source: SourceKind::YouTubeMusic,
            name: r.title,
            album_artists: vec![ArtistRef { id: Some(r.artist_id), name: r.artist }],
            image: r.image.map(ImageRef::Url),
            year: r.year,
            release_type: Some(r.kind.clone()),
            release_types: vec![r.kind],
            genres: vec![],
            song_count: None,
            duration: Duration::ZERO,
            favorite: false,
            rating: None,
            play_count: None,
            songs: vec![],
            info: Default::default(),
        })
        .collect();
    Some(HomeShelf { title: SHELF_TITLE.into(), albums, playlists: vec![], songs: vec![] })
}

/// Asks listen-server; `None` on any failure (the shelf is left out, the rest of Home loads).
pub async fn fetch(req: Request) -> Option<HomeShelf> {
    let get = async {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| e.to_string())?;
        let response = http
            .get(&req.url)
            .bearer_auth(&req.token)
            .send()
            .await
            .map_err(|e| e.without_url().to_string())?;
        let response = response.error_for_status().map_err(|e| e.without_url().to_string())?;
        response.json::<Feed>().await.map_err(|e| e.without_url().to_string())
    };
    match get.await {
        Ok(feed) => {
            tracing::info!("home: {} new releases from listen-server", feed.releases.len());
            shelf(&req.server, feed.releases)
        }
        Err(e) => {
            tracing::info!("home: release radar unavailable: {e}");
            None
        }
    }
}

impl Listen {
    /// The radar request for the active server, if the shelf applies.
    pub(super) fn releases_request(&self) -> Option<Request> {
        let s = &self.config.settings;
        let base = s.history.sync_server.trim();
        if !s.home.new_releases || !s.history.sync || base.is_empty() || self.demo {
            return None;
        }
        let server = self.config.active_server.clone()?;
        let cfg = self.config.servers.iter().find(|c| c.id == server)?;
        if cfg.kind != SourceKind::YouTubeMusic {
            return None;
        }
        let token = self.history_sync_token()?;
        Some(Request { url: format!("{}/v1/releases", base.trim_end_matches('/')), token, server })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn releases_become_album_cards_of_the_youtube_music_server() {
        let server = ServerId("ytm-1".into());
        assert!(shelf(&server, vec![]).is_none());
        let r = Release {
            id: "MPREb_x".into(),
            title: "Comeback".into(),
            kind: "single".into(),
            year: Some(2026),
            image: Some("https://lh3/x".into()),
            artist: "IVE".into(),
            artist_id: "UCive".into(),
            found: 1,
            top: true,
        };
        let s = shelf(&server, vec![r]).unwrap();
        assert_eq!(s.title, SHELF_TITLE);
        let a = &s.albums[0];
        assert_eq!((a.id.as_str(), a.name.as_str(), a.server_id.clone()), ("MPREb_x", "Comeback", server));
        assert_eq!(a.album_artists[0].id.as_deref(), Some("UCive"));
        assert_eq!(a.image, Some(ImageRef::Url("https://lh3/x".into())));
        assert_eq!(a.release_type.as_deref(), Some("single"));
    }

    #[tokio::test]
    async fn the_feed_is_read_from_the_server() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut c, _) = listener.accept().unwrap();
            let mut buf = [0u8; 2048];
            let n = c.read(&mut buf).unwrap();
            let head = String::from_utf8_lossy(&buf[..n]).to_lowercase();
            assert!(head.starts_with("get /v1/releases"), "{head}");
            assert!(head.contains("authorization: bearer tok"), "{head}");
            let body = r#"{"updated":1,"artists":2,"releases":[{"id":"MPREb_a","title":"A","kind":"album","artist":"X","artist_id":"UCx","found":1}]}"#;
            write!(
                c,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });
        let req = Request {
            url: format!("http://127.0.0.1:{port}/v1/releases"),
            token: "tok".into(),
            server: ServerId("ytm".into()),
        };
        let s = fetch(req).await.unwrap();
        assert_eq!(s.albums[0].id, "MPREb_a");
        server.join().unwrap();
        // Nothing listening: no shelf, no error.
        let req = Request {
            url: format!("http://127.0.0.1:{port}/v1/releases"),
            token: "t".into(),
            server: ServerId("y".into()),
        };
        assert!(fetch(req).await.is_none());
    }
}
