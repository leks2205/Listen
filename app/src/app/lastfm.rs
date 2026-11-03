//! Last.fm scrobbling straight from Listen (`listen_lastfm`): every source scrobbles, YouTube Music
//! included, under the most common spelling of each name. The API key is a setting; its shared secret and
//! the session key live in the keyring.
//!
//! Sign-in is Last.fm's desktop flow: a token, the browser page where the user allows access, then the
//! session is fetched every few seconds until it is allowed (or 5 minutes pass).

use super::{Listen, Message};
use iced::Task;
use listen_core::model::{ServerId, Song};
use listen_lastfm::{Client, Creds, Scrobbler, Session};
use std::sync::Arc;
use std::time::Duration;

const SECRET_ENTRY: &str = "lastfm-secret";
/// The API key may also sit with the passwords (set up from another app's account, e.g. the music server's).
const KEY_ENTRY: &str = "lastfm-api-key";
const SESSION_ENTRY: &str = "lastfm-session";
/// How long the sign-in waits for the user to allow access.
const SIGN_IN_TRIES: u32 = 100;
const SIGN_IN_EVERY: Duration = Duration::from_secs(3);

#[derive(Debug, Clone)]
pub enum LastFmMsg {
    ApiKey(String),
    Secret(String),
    Connect,
    Token(Result<String, String>),
    Poll(String, u32),
    Session(String, u32, Result<Session, (String, bool)>),
    Disconnect,
    /// A scrobble / now playing / flush came back: (sent, error text, signed out).
    Sent(Result<usize, (String, bool)>),
}

#[derive(Default)]
pub struct LastFmState {
    pub scrobbler: Option<Arc<Scrobbler>>,
    /// Text being typed into the key / secret fields.
    pub key_input: Option<String>,
    pub secret_input: String,
    /// A sign-in is waiting for the browser.
    pub signing_in: bool,
    pub status: Option<String>,
}

impl std::fmt::Debug for LastFmState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LastFmState")
    }
}

impl Listen {
    fn lastfm_secret(&self) -> Option<String> {
        self.secrets.as_ref()?.get(&ServerId(SECRET_ENTRY.into())).filter(|s| !s.is_empty())
    }

    /// The API account is known (Connect needs nothing typed in).
    pub fn lastfm_account_known(&self) -> bool {
        self.lastfm_creds().is_some()
    }

    fn lastfm_creds(&self) -> Option<Creds> {
        let mut key = self.config.settings.integrations.lastfm.api_key.trim().to_string();
        if key.is_empty() {
            key = self
                .secrets
                .as_ref()?
                .get(&ServerId(KEY_ENTRY.into()))
                .unwrap_or_default()
                .trim()
                .to_string();
        }
        let secret = if self.lastfm.secret_input.trim().is_empty() {
            self.lastfm_secret()?
        } else {
            self.lastfm.secret_input.trim().to_string()
        };
        (!key.is_empty()).then_some(Creds { api_key: key, secret })
    }

    fn lastfm_dir(&self) -> Option<std::path::PathBuf> {
        self.paths.as_ref().map(|p| p.data_dir.join("lastfm"))
    }

    /// Start-up: signed in before → scrobble again, and send what waited.
    pub(super) fn lastfm_boot(&mut self) -> Task<Message> {
        if self.demo || self.ephemeral {
            return Task::none();
        }
        let user = self.config.settings.integrations.lastfm.user.clone();
        let session = self.secrets.as_ref().and_then(|s| s.get(&ServerId(SESSION_ENTRY.into())));
        let (Some(creds), Some(key), Some(dir)) = (self.lastfm_creds(), session, self.lastfm_dir()) else {
            return Task::none();
        };
        let s = Arc::new(Scrobbler::new(creds, Session { name: user, key }, &dir));
        self.lastfm.scrobbler = Some(s.clone());
        Task::perform(async move { s.flush().await.map_err(|e| (e.to_string(), e.signed_out())) }, |r| {
            Message::LastFm(LastFmMsg::Sent(r))
        })
    }

    /// Now playing / a finished play of `song` (the song as queued, not an own-server copy: one play, one
    /// scrobble). `started` = Unix seconds the play began.
    pub(super) fn lastfm_report(&self, song: &Song, submission: Option<i64>) -> Task<Message> {
        let Some(s) = self.lastfm.scrobbler.clone() else { return Task::none() };
        if !self.config.settings.integrations.lastfm.scrobble {
            return Task::none();
        }
        let song = song.clone();
        Task::perform(
            async move {
                let r = match submission {
                    Some(ts) => s.scrobble(&song, ts).await,
                    None => s.now_playing(&song).await.map(|()| 0),
                };
                r.map_err(|e| (e.to_string(), e.signed_out()))
            },
            |r| Message::LastFm(LastFmMsg::Sent(r)),
        )
    }

    pub(super) fn on_lastfm(&mut self, msg: LastFmMsg) -> Task<Message> {
        match msg {
            LastFmMsg::ApiKey(text) => self.lastfm.key_input = Some(text),
            LastFmMsg::Secret(text) => self.lastfm.secret_input = text,
            LastFmMsg::Connect => {
                if let Some(key) = self.lastfm.key_input.take() {
                    self.config.settings.integrations.lastfm.api_key = key.trim().to_string();
                    self.save_config();
                }
                let Some(creds) = self.lastfm_creds() else {
                    self.lastfm.status = Some(
                        "Enter the API key and the shared secret of your Last.fm API account first.".into(),
                    );
                    return Task::none();
                };
                self.lastfm.signing_in = true;
                self.lastfm.status = Some("Asking Last.fm…".into());
                let client = Client::new(creds);
                return Task::perform(
                    async move { client.get_token().await.map_err(|e| e.to_string()) },
                    |r| Message::LastFm(LastFmMsg::Token(r)),
                );
            }
            LastFmMsg::Token(Ok(token)) => {
                let Some(creds) = self.lastfm_creds() else { return Task::none() };
                super::album_page::open_url(&Client::new(creds).auth_url(&token));
                self.lastfm.status =
                    Some("Allow access in the browser page that opened; Listen waits for it.".into());
                return self.lastfm_poll(token, 0);
            }
            LastFmMsg::Token(Err(e)) => {
                self.lastfm.signing_in = false;
                self.lastfm.status = Some(format!("Last.fm didn't answer: {e}"));
            }
            LastFmMsg::Poll(token, n) => {
                if !self.lastfm.signing_in {
                    return Task::none();
                }
                let Some(creds) = self.lastfm_creds() else { return Task::none() };
                let client = Client::new(creds);
                return Task::perform(
                    async move {
                        tokio::time::sleep(SIGN_IN_EVERY).await;
                        let r =
                            client.get_session(&token).await.map_err(|e| (e.to_string(), e.not_authorized()));
                        (token, r)
                    },
                    move |(token, r)| Message::LastFm(LastFmMsg::Session(token, n, r)),
                );
            }
            LastFmMsg::Session(_, _, Ok(session)) => {
                self.lastfm.signing_in = false;
                let (Some(creds), Some(dir)) = (self.lastfm_creds(), self.lastfm_dir()) else {
                    return Task::none();
                };
                if let (Some(secrets), false) = (&self.secrets, self.ephemeral) {
                    secrets.set(&ServerId(SECRET_ENTRY.into()), &creds.secret);
                    secrets.set(&ServerId(SESSION_ENTRY.into()), &session.key);
                }
                self.lastfm.secret_input.clear();
                self.config.settings.integrations.lastfm.user = session.name.clone();
                self.save_config();
                self.lastfm.status = Some(format!("Connected as {}.", session.name));
                tracing::info!("Last.fm: connected as {}", session.name);
                self.lastfm.scrobbler = Some(Arc::new(Scrobbler::new(creds, session, &dir)));
                // The playing song shows as "now playing" at once.
                if let Some(song) = self.queue.current().map(|i| i.song.clone()) {
                    return self.lastfm_report(&song, None);
                }
            }
            LastFmMsg::Session(token, n, Err((e, waiting))) => {
                // Not allowed yet: ask again in a few seconds.
                if waiting && n + 1 < SIGN_IN_TRIES && self.lastfm.signing_in {
                    return self.lastfm_poll(token, n + 1);
                }
                self.lastfm.signing_in = false;
                self.lastfm.status = Some(if waiting {
                    "Sign-in timed out. Press Connect again.".into()
                } else {
                    format!("Sign-in failed: {e}")
                });
            }
            LastFmMsg::Disconnect => {
                self.lastfm.scrobbler = None;
                self.lastfm.signing_in = false;
                if let Some(secrets) = &self.secrets {
                    secrets.delete(&ServerId(SESSION_ENTRY.into()));
                }
                self.config.settings.integrations.lastfm.user.clear();
                self.save_config();
                self.lastfm.status = Some("Disconnected from Last.fm.".into());
            }
            LastFmMsg::Sent(Ok(n)) => {
                if n > 0 {
                    tracing::info!("Last.fm: {n} scrobbles sent");
                }
            }
            LastFmMsg::Sent(Err((e, signed_out))) => {
                tracing::warn!("Last.fm: {e}");
                if signed_out {
                    self.lastfm.scrobbler = None;
                    if let Some(secrets) = &self.secrets {
                        secrets.delete(&ServerId(SESSION_ENTRY.into()));
                    }
                    self.toast_error(
                        "Last.fm signed Listen out. Connect again in Settings → General → Scrobble.".into(),
                    );
                }
            }
        }
        Task::none()
    }

    fn lastfm_poll(&self, token: String, n: u32) -> Task<Message> {
        Task::done(Message::LastFm(LastFmMsg::Poll(token, n)))
    }
}
