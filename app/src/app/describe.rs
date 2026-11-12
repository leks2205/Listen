//! "Describe it" on the search page: the query goes to listen-server's natural-language search
//! (`GET /v1/search?q=`, ranking in `listen_core::describe`) instead of the music server's title search.
//! Offered when "Merge with other devices" has a listen-server address and token. Turning the mode on
//! asks the server to load its text encoder (`POST /v1/search/warm`) so the first search is quick.

use super::{Listen, Message};
use listen_core::config::ServerConfig;
use listen_core::describe::{SearchResponse, hit_songs};
use listen_core::history::Entry;
use listen_core::model::SearchResults;
use std::time::Duration;

/// Results asked for.
pub const LIMIT: usize = 100;

/// Where to ask.
#[derive(Debug, Clone)]
pub struct Request {
    base: String,
    token: String,
    /// The app's shared client.
    http: reqwest::Client,
}

/// The ranked songs for `query`, as songs of the configured servers (`servers`, `history` for covers).
pub async fn search(
    req: Request,
    query: String,
    servers: Vec<ServerConfig>,
    history: Vec<Entry>,
) -> Result<SearchResults, String> {
    let url = format!("{}/v1/search", req.base);
    let limit = LIMIT.to_string();
    let response = req
        .http
        .get(&url)
        .timeout(Duration::from_secs(20))
        .query(&[("q", query.as_str()), ("limit", limit.as_str())])
        .bearer_auth(&req.token)
        .send()
        .await
        .map_err(|e| e.without_url().to_string())?;
    let status = response.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err("listen-server is too old for Describe it (needs 0.8)".into());
    }
    let response = response.error_for_status().map_err(|e| e.without_url().to_string())?;
    let answer: SearchResponse = response.json().await.map_err(|e| e.without_url().to_string())?;
    tracing::info!(
        "describe: {} hits of {} indexed in {} ms",
        answer.hits.len(),
        answer.indexed,
        answer.took_ms
    );
    if answer.indexed == 0 {
        return Err("No songs are indexed for Describe it yet (listen-embed).".into());
    }
    Ok(SearchResults { songs: hit_songs(&answer.hits, &servers, &history), ..Default::default() })
}

/// Loads the server's text encoder ahead of the first search; failures only log.
pub async fn warm(req: Request) {
    let url = format!("{}/v1/search/warm", req.base);
    let sent = async {
        req.http
            .post(&url)
            .timeout(Duration::from_secs(30))
            .bearer_auth(&req.token)
            .send()
            .await
            .map_err(|e| e.without_url().to_string())
    };
    match sent.await {
        Ok(r) => tracing::info!("describe: warm-up {}", r.status()),
        Err(e) => tracing::info!("describe: warm-up failed: {e}"),
    }
}

impl Listen {
    /// The listen-server to ask, when Describe it can be offered.
    pub fn describe_request(&self) -> Option<Request> {
        if self.demo || !self.config.settings.history.sync {
            return None;
        }
        let server = self.listen_server()?;
        Some(Request { base: server.base, token: server.token, http: self.http.clone() })
    }

    pub fn describe_available(&self) -> bool {
        self.describe_request().is_some()
    }

    /// Turns Describe it on or off; on: warms the server up and searches what is in the box.
    pub(super) fn set_describe(&mut self, on: bool) -> iced::Task<Message> {
        let on = on && self.describe_available();
        if self.search.describe == on {
            return iced::Task::none();
        }
        self.search.describe = on;
        self.search.next_generation();
        self.search.loading = false;
        self.search.error = None;
        self.search.results = Default::default();
        self.search.tab = super::list::SearchTab::Songs;
        if !on {
            return if self.search.query.trim().is_empty() { iced::Task::none() } else { self.run_search() };
        }
        let warm =
            self.describe_request().map_or(iced::Task::none(), |r| iced::Task::future(warm(r)).discard());
        let run = if self.search.query.trim().is_empty() { iced::Task::none() } else { self.run_search() };
        iced::Task::batch([warm, run])
    }

    /// The Describe it search for the box's text (from `run_search`).
    pub(super) fn describe_task(&self, query: String, generation: u64) -> iced::Task<Message> {
        let Some(req) = self.describe_request() else { return iced::Task::none() };
        let servers = self.config.servers.clone();
        let history = self.history.entries.clone();
        iced::Task::perform(search(req, query, servers, history), move |r| {
            Message::SearchLoaded(generation, r)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use listen_core::model::{ServerId, SourceKind};

    #[tokio::test]
    async fn hits_come_back_as_songs_of_the_configured_servers() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            for body in [
                r#"{"hits":[{"song_id":"ytm:vid1","title":"Rain","artist":"IU","duration_ms":200000,"score":2.5},{"song_id":"nd-1","title":"X","artist":"Y","server_hint":"elsewhere","score":1.0}],"indexed":295,"took_ms":60}"#,
                r#"{"hits":[],"indexed":0,"took_ms":1}"#,
            ] {
                let (mut c, _) = listener.accept().unwrap();
                let mut buf = [0u8; 4096];
                let n = c.read(&mut buf).unwrap();
                let head = String::from_utf8_lossy(&buf[..n]).to_lowercase();
                assert!(head.starts_with("get /v1/search?q=slow+90s+ballad&limit=100"), "{head}");
                assert!(head.contains("authorization: bearer tok"), "{head}");
                write!(
                    c,
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });
        let servers = vec![ServerConfig {
            id: ServerId("ytm-1".into()),
            name: "YouTube Music".into(),
            kind: SourceKind::YouTubeMusic,
            url: String::new(),
            username: String::new(),
        }];
        let req = Request {
            base: format!("http://127.0.0.1:{port}"),
            token: "tok".into(),
            http: reqwest::Client::new(),
        };
        let r = search(req.clone(), "slow 90s ballad".into(), servers.clone(), vec![]).await.unwrap();
        // The Navidrome hit of another server is left out.
        assert_eq!(r.songs.len(), 1);
        assert_eq!((r.songs[0].id.as_str(), r.songs[0].title.as_str()), ("vid1", "Rain"));
        // An empty index is an error the page shows.
        let e = search(req, "slow 90s ballad".into(), servers, vec![]).await.unwrap_err();
        assert!(e.contains("listen-embed"), "{e}");
        server.join().unwrap();
    }

    #[test]
    fn describe_needs_a_listen_server() {
        let (mut s, _) = Listen::boot(&crate::Args { demo: true, ephemeral: true, ..crate::Args::default() });
        assert!(!s.describe_available());
        let _ = s.set_describe(true);
        assert!(!s.search.describe);
    }
}
