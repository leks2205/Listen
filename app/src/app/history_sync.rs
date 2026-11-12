//! The merged listening history: this computer's plays go up to listen-server, the other devices'
//! come down into the local file. A removed play is removed everywhere; Delete all and the retention
//! limit stay on this computer. Wire format and rules: `docs/history-sync.md`.
//!
//! This module also owns the listen-server connection that every listen-server feature uses (history,
//! exclusions, flow weights, release radar, Describe it, handoff): [`ListenServer`] from
//! [`Listen::listen_server`] (address from the `history.sync_server` setting, token from the keyring)
//! and [`SYNC_TIMEOUT`]. A new listen-server feature starts from that accessor.

use super::history::HistoryMsg;
use super::{Listen, Message};
use iced::Task;
use listen_core::config::ServerConfig;
use listen_core::history::{self, Entry};
use listen_core::model::{ServerId, SourceKind};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::Duration;

/// The keyring entry of listen-server's token.
const TOKEN_SECRET: &str = "history-sync-token";
const STATE_FILE: &str = "history-sync.json";
/// Plays per push: the server takes 2 MB a request.
const BATCH: usize = 500;
const PAGE: usize = 1000;
/// How often the history merges while the app runs.
pub const EVERY: Duration = Duration::from_secs(300);

/// A play across devices: server ids differ per device, so they are no part of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Key {
    ts: i64,
    song_id: String,
}

/// What is kept between syncs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SyncState {
    /// The newest play sent. Plays from this second on go out again: two can share a second.
    pushed_ts: i64,
    /// The last log record taken in.
    cursor: u64,
    /// Removed here, not yet told to the server.
    removed: Vec<Key>,
}

#[derive(Debug, Clone, Serialize)]
struct Play {
    server_key: String,
    entry: Entry,
}

struct Job {
    base: String,
    token: String,
    device: String,
    servers: Vec<ServerConfig>,
    plays: Vec<Play>,
    removed: Vec<Key>,
    pushed_ts: i64,
    cursor: u64,
}

/// What a sync brought back.
#[derive(Debug, Clone, Default)]
pub struct Outcome {
    pushed_ts: i64,
    cursor: u64,
    sent: usize,
    removed: Vec<Key>,
    incoming: Vec<Entry>,
    gone: Vec<(i64, String)>,
}

/// The last sync, for the settings row.
#[derive(Debug, Clone)]
pub enum SyncStatus {
    Done { sent: usize, received: usize },
    Failed(String),
}

pub(super) fn source_name(kind: SourceKind) -> String {
    serde_json::to_value(kind).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

fn host(url: &str) -> String {
    let url = url.split_once("://").map_or(url, |(_, rest)| rest);
    let url = url.split_once('@').map_or(url, |(_, rest)| rest);
    url.trim_end_matches('/').to_lowercase()
}

pub(super) fn key_of(server: &ServerConfig) -> String {
    match server.kind {
        SourceKind::YouTubeMusic => source_name(server.kind),
        SourceKind::Local => format!("{}:{}", source_name(server.kind), server.id.0),
        kind => format!("{}:{}", source_name(kind), host(&server.url)),
    }
}

/// The name every device gives the play's server, no credentials in it.
fn server_key(entry: &Entry, servers: &[ServerConfig]) -> String {
    match servers.iter().find(|s| s.id == entry.server_id) {
        Some(s) if s.kind == entry.source => key_of(s),
        _ if entry.source == SourceKind::YouTubeMusic => source_name(entry.source),
        _ => format!("{}:{}", source_name(entry.source), entry.server_id.0),
    }
}

/// The same server here: by its key, else the only server of that kind (the song ids are the same
/// under another address). Local folders never match.
pub(super) fn local_server<'a>(
    key: &str,
    kind: SourceKind,
    servers: &'a [ServerConfig],
) -> Option<&'a ServerConfig> {
    if kind == SourceKind::Local {
        return None;
    }
    let same: Vec<&ServerConfig> = servers.iter().filter(|s| s.kind == kind).collect();
    same.iter().find(|s| key_of(s) == key).or(same.first().filter(|_| same.len() == 1)).copied()
}

/// An `add` record as a play of this computer; one this version cannot read is skipped.
fn local_entry(record: &Value, servers: &[ServerConfig]) -> Option<Entry> {
    let mut entry: Entry = serde_json::from_value(record.get("entry")?.clone()).ok()?;
    let key = record.get("server_key").and_then(Value::as_str).unwrap_or_default();
    if let Some(server) = local_server(key, entry.source, servers) {
        entry.server_id = server.id.clone();
    }
    Some(entry)
}

/// Address and token of listen-server; from [`Listen::listen_server`].
#[derive(Debug, Clone)]
pub struct ListenServer {
    /// `https://host[:port][/prefix]`, without a trailing `/`.
    pub base: String,
    pub token: String,
}

impl ListenServer {
    /// `path` (`v1/...`, with or without a leading `/`) on this server.
    pub fn url(&self, path: &str) -> String {
        format!("{}/{}", self.base, path.trim_start_matches('/'))
    }
}

/// The listen-server address of the "Merge with other devices" setting: trimmed, without trailing `/`.
/// Empty means no server; every "is a server set" check reads this, not the raw setting.
fn server_base(setting: &str) -> &str {
    setting.trim().trim_end_matches('/')
}

/// Whole-request limit of the history, exclusions and flow-weight exchanges.
pub(super) const SYNC_TIMEOUT: Duration = Duration::from_secs(30);

fn failed(e: reqwest::Error) -> String {
    e.without_url().to_string()
}

async fn post(http: &reqwest::Client, job: &Job, path: &str, body: &Value) -> Result<(), String> {
    let response = http
        .post(format!("{}{path}", job.base))
        .timeout(SYNC_TIMEOUT)
        .bearer_auth(&job.token)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body.to_string())
        .send()
        .await
        .map_err(failed)?;
    response.error_for_status().map(|_| ()).map_err(failed)
}

async fn run(http: reqwest::Client, job: Job) -> Result<Outcome, String> {
    let mut out = Outcome {
        pushed_ts: job.pushed_ts,
        cursor: job.cursor,
        sent: job.plays.len(),
        removed: job.removed.clone(),
        ..Default::default()
    };
    for batch in job.plays.chunks(BATCH) {
        post(&http, &job, &format!("v1/history/{}", job.device), &json!({ "entries": batch })).await?;
        out.pushed_ts = batch.iter().map(|p| p.entry.ts).fold(out.pushed_ts, i64::max);
    }
    if !job.removed.is_empty() {
        post(&http, &job, &format!("v1/history/{}/remove", job.device), &json!({ "keys": job.removed }))
            .await?;
    }
    loop {
        let url = format!("{}v1/history?after={}&limit={PAGE}", job.base, out.cursor);
        let response =
            http.get(url).timeout(SYNC_TIMEOUT).bearer_auth(&job.token).send().await.map_err(failed)?;
        let text = response.error_for_status().map_err(failed)?.text().await.map_err(failed)?;
        let page: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        let Some(items) = page.get("items").and_then(Value::as_array).filter(|i| !i.is_empty()) else {
            return Ok(out);
        };
        for record in items {
            match record.get("op").and_then(Value::as_str) {
                Some("add") => out.incoming.extend(local_entry(record, &job.servers)),
                Some("remove") => {
                    if let (Some(ts), Some(id)) = (
                        record.get("ts").and_then(Value::as_i64),
                        record.get("song_id").and_then(Value::as_str),
                    ) {
                        out.gone.push((ts, id.to_string()));
                    }
                }
                _ => {}
            }
        }
        out.cursor = items.last().and_then(|r| r.get("seq")).and_then(Value::as_u64).unwrap_or(out.cursor);
    }
}

impl Listen {
    fn history_sync_file(&self) -> Option<PathBuf> {
        self.history.path().map(|p| p.with_file_name(STATE_FILE))
    }

    fn history_sync_save(&self) {
        let Some(path) = self.history_sync_file() else { return };
        let tmp = path.with_extension("json.tmp");
        let text = serde_json::to_string(&self.history_view.sync).unwrap_or_default();
        if let Err(e) = std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, &path)) {
            tracing::warn!("saving the history sync state: {e}");
        }
    }

    pub(super) fn history_sync_boot(&mut self) -> Task<Message> {
        let text = self.history_sync_file().and_then(|p| std::fs::read_to_string(p).ok());
        self.history_view.sync = text.and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        self.history_view.sync_token_saved = self.history_sync_token().is_some();
        Task::done(Message::History(HistoryMsg::Sync))
    }

    /// listen-server as set under "Merge with other devices": the address (no trailing `/`) and the
    /// saved token, or `None` without either. Every listen-server feature starts here (history,
    /// exclusions, flow weights, release radar, Describe it, handoff) and adds its own on/off check;
    /// send its requests with the shared `self.http` and a per-request `.timeout(..)`.
    pub(super) fn listen_server(&self) -> Option<ListenServer> {
        let base = server_base(&self.config.settings.history.sync_server);
        if base.is_empty() {
            return None;
        }
        Some(ListenServer { base: base.to_string(), token: self.history_sync_token()? })
    }

    pub(super) fn history_sync_token(&self) -> Option<String> {
        let id = ServerId(TOKEN_SECRET.into());
        self.secrets.as_ref().and_then(|s| s.get(&id)).filter(|t| !t.is_empty())
    }

    pub(super) fn history_sync_save_token(&mut self) -> Task<Message> {
        let token = std::mem::take(&mut self.history_view.sync_token).trim().to_string();
        if token.is_empty() || self.ephemeral {
            return Task::none();
        }
        if let Some(s) = &self.secrets {
            s.set(&ServerId(TOKEN_SECRET.into()), &token);
        }
        self.history_view.sync_token_saved = true;
        Task::done(Message::History(HistoryMsg::Sync))
    }

    /// The user removed this play: the other devices drop it at the next sync.
    pub(super) fn history_sync_removed(&mut self, entry: &Entry) {
        self.history_view.sync.removed.push(Key { ts: entry.ts, song_id: entry.song_id.clone() });
        self.history_sync_save();
    }

    /// Skipped while the history is off, without a server or token, in a demo, or while one runs.
    pub(super) fn history_sync(&mut self) -> Task<Message> {
        let settings = &self.config.settings.history;
        if !settings.enabled
            || !settings.sync
            || server_base(&settings.sync_server).is_empty()
            || self.history_view.syncing
        {
            return Task::none();
        }
        if self.history.path().is_none() {
            return Task::none();
        }
        let Some(server) = self.listen_server() else {
            self.history_view.sync_status = Some(SyncStatus::Failed("no token saved".into()));
            return Task::none();
        };
        let state = &self.history_view.sync;
        let servers = self.config.servers.clone();
        let job = Job {
            base: format!("{}/", server.base),
            token: server.token,
            device: listen_analysis::store::machine_id(),
            plays: (self.history.entries.iter().filter(|e| e.ts >= state.pushed_ts))
                .map(|e| Play { server_key: server_key(e, &servers), entry: e.clone() })
                .collect(),
            removed: state.removed.clone(),
            pushed_ts: state.pushed_ts,
            cursor: state.cursor,
            servers,
        };
        self.history_view.syncing = true;
        Task::perform(run(self.http.clone(), job), |r| Message::History(HistoryMsg::Synced(r)))
    }

    pub(super) fn history_synced(&mut self, result: Result<Outcome, String>) {
        self.history_view.syncing = false;
        let out = match result {
            Ok(out) => out,
            Err(e) => {
                tracing::warn!("merging the listening history: {e}");
                self.history_view.sync_status = Some(SyncStatus::Failed(e));
                return;
            }
        };
        let received = match self.history.merge(out.incoming, &out.gone) {
            Ok(n) => n,
            Err(e) => {
                tracing::warn!("saving the merged history: {e}");
                self.history_view.sync_status = Some(SyncStatus::Failed(e.to_string()));
                return;
            }
        };
        let now = history::now_secs();
        let pruned = self
            .history
            .prune(self.config.settings.history.retention.max_age_days(), now)
            .unwrap_or_else(|e| {
                tracing::warn!("pruning the listening history: {e}");
                0
            });
        let state = &mut self.history_view.sync;
        state.pushed_ts = out.pushed_ts;
        state.cursor = out.cursor;
        state.removed.retain(|k| !out.removed.contains(k));
        self.history_view.sync_status = Some(SyncStatus::Done { sent: out.sent, received });
        self.history_sync_save();
        if received > 0 || pruned > 0 {
            self.refresh_history_views();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(id: &str, kind: SourceKind, url: &str) -> ServerConfig {
        ServerConfig { id: ServerId(id.into()), name: id.into(), kind, url: url.into(), username: "u".into() }
    }

    fn entry(server: &str, source: SourceKind) -> Value {
        json!({"ts": 5, "server_id": server, "source": source, "song_id": "s", "status": "played", "title": "T",
               "artists": [], "album": null, "album_id": null, "image": null, "duration_secs": 1})
    }

    /// Against a throwaway listen-server: `LISTEN_TEST_SERVER=http://… LISTEN_TEST_TOKEN=… cargo test -p listen live_round_trip -- --ignored`.
    #[tokio::test]
    #[ignore = "needs a running listen-server"]
    async fn live_round_trip() {
        let base = std::env::var("LISTEN_TEST_SERVER").unwrap();
        let token = std::env::var("LISTEN_TEST_TOKEN").unwrap();
        let ts = history::now_secs();
        let song = format!("zz-listen-test-{ts}");
        let mut value = entry("desktop-id", SourceKind::Subsonic);
        value["ts"] = json!(ts);
        value["song_id"] = json!(song);
        let play: Entry = serde_json::from_value(value).unwrap();
        let desktop = [server("desktop-id", SourceKind::Subsonic, "https://music.example.org")];
        let phone = [server("phone-id", SourceKind::Subsonic, "https://MUSIC.example.org/")];
        let job = |device: &str, servers: &[ServerConfig], plays: Vec<Play>, removed: Vec<Key>, cursor| Job {
            base: base.clone(),
            token: token.clone(),
            device: device.into(),
            servers: servers.to_vec(),
            plays,
            removed,
            pushed_ts: 0,
            cursor,
        };
        let plays = vec![Play { server_key: server_key(&play, &desktop), entry: play.clone() }];
        let sent = run(reqwest::Client::new(), job("desktop", &desktop, plays.clone(), Vec::new(), 0))
            .await
            .unwrap();
        assert_eq!(sent.pushed_ts, ts);

        let got = run(reqwest::Client::new(), job("phone", &phone, Vec::new(), Vec::new(), 0)).await.unwrap();
        let there = got.incoming.iter().find(|e| e.song_id == song).unwrap();
        assert_eq!(there.server_id.0, "phone-id");
        assert_eq!(there.ts, ts);

        // A repeated push adds nothing, and the removal goes to the other device.
        let again = run(reqwest::Client::new(), job("desktop", &desktop, plays, Vec::new(), sent.cursor))
            .await
            .unwrap();
        assert_eq!(again.cursor, sent.cursor);
        let key = Key { ts, song_id: song.clone() };
        run(reqwest::Client::new(), job("desktop", &desktop, Vec::new(), vec![key], again.cursor))
            .await
            .unwrap();
        let gone = run(reqwest::Client::new(), job("phone", &phone, Vec::new(), Vec::new(), got.cursor))
            .await
            .unwrap();
        assert!(gone.gone.contains(&(ts, song)));
    }

    #[test]
    fn an_address_of_only_slashes_is_no_listen_server() {
        assert_eq!(server_base(" / "), "");
        assert_eq!(server_base("https://sync.example.com//"), "https://sync.example.com");
        assert_eq!(server_base(" https://sync.example.com/listen "), "https://sync.example.com/listen");
    }

    /// Every listen-server feature sends its requests with the app's shared client (its User-Agent,
    /// its connect timeout) and a per-request `.timeout(..)`: a module that reaches listen-server
    /// through `listen_server()` builds no client of its own.
    #[test]
    fn listen_server_features_use_the_shared_client() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/app");
        let mut features = 0;
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            if !text.contains(".listen_server()") {
                continue;
            }
            features += 1;
            let code = text.split("#[cfg(test)]").next().unwrap_or_default();
            assert!(
                !code.contains("Client::builder"),
                "{} reaches listen-server but builds its own HTTP client: use the shared `self.http`",
                path.display()
            );
        }
        assert!(features >= 6, "found only {features} listen-server features in {}", dir.display());
    }

    #[test]
    fn server_keys_match_the_phone() {
        let servers = [
            server("a", SourceKind::Subsonic, "https://user:pw@Music.Example.org/"),
            server("y", SourceKind::YouTubeMusic, ""),
        ];
        let play = |id, kind| serde_json::from_value::<Entry>(entry(id, kind)).unwrap();
        assert_eq!(server_key(&play("a", SourceKind::Subsonic), &servers), "subsonic:music.example.org");
        assert_eq!(server_key(&play("y", SourceKind::YouTubeMusic), &servers), "youtubemusic");
        assert_eq!(server_key(&play("old", SourceKind::YouTubeMusic), &servers), "youtubemusic");
        assert_eq!(server_key(&play("gone", SourceKind::Jellyfin), &servers), "jellyfin:gone");
        assert_eq!(server_key(&play("f", SourceKind::Local), &servers), "local:f");
    }

    #[test]
    fn a_received_play_gets_this_computers_server() {
        let one = [server("a", SourceKind::Subsonic, "https://music.example.org")];
        let two = [
            server("a", SourceKind::Subsonic, "https://music.example.org"),
            server("b", SourceKind::Subsonic, "https://other.example.org"),
        ];
        let record = |key: &str, id: &str, kind| json!({"server_key": key, "entry": entry(id, kind)});
        let id = |r: &Value, s: &[ServerConfig]| local_entry(r, s).unwrap().server_id.0;
        let by_key = record("subsonic:other.example.org", "phone", SourceKind::Subsonic);
        assert_eq!(id(&by_key, &two), "b");
        // Another address of the only server of that kind.
        let elsewhere = record("subsonic:192.168.1.2:4533", "phone", SourceKind::Subsonic);
        assert_eq!(id(&elsewhere, &one), "a");
        assert_eq!(id(&elsewhere, &two), "phone");
        // A local folder never matches, nor a kind this computer has no server of.
        assert_eq!(id(&record("local:phone", "phone", SourceKind::Local), &one), "phone");
        assert_eq!(id(&record("jellyfin:x", "phone", SourceKind::Jellyfin), &one), "phone");
        assert!(local_entry(&json!({"entry": {"ts": "broken"}}), &one).is_none());
    }
}
