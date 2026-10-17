//! Servers and sign-in extras (Feishin `features/servers/*`, `features/login`, `features/action-required`):
//! per-server options, Jellyfin Quick Connect (polled every 5 s) and LAN discovery (UDP port 7359),
//! editing a saved server in place (same id and keyring entry), the action-required page,
//! and the secret store (system keyring or a 0600 file).

use super::{Listen, LoginForm, Message, SourceHandle, err, unix_millis};
use crate::secrets::Secrets;
use iced::Task;
use listen_core::config::{Config, SecretStore, ServerConfig, ServerOptions};
use listen_core::model::{ServerId, SourceKind};
use listen_jellyfin::discovery::Found;
use listen_jellyfin::quick_connect::{self, Initiated, Session};
use std::time::Duration;

/// Key of the edit-server dialog.
pub const EDIT_MODAL: &str = "server-edit";

/// Why the action-required page is showing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// There is no (usable) password / token: sign in again.
    Credentials { id: ServerId },
    /// The server did not answer, or answered with an error.
    Unreachable { id: ServerId, reason: String },
}

impl Failure {
    pub fn id(&self) -> &ServerId {
        match self {
            Self::Credentials { id } | Self::Unreachable { id, .. } => id,
        }
    }
}

/// A Quick Connect request in progress on the sign-in page.
#[derive(Debug, Clone)]
pub struct QuickConnect {
    /// Id the new server gets (the access token belongs to this device id).
    pub server_id: ServerId,
    pub url: String,
    pub options: ServerOptions,
    /// Shown to the user once the server answered.
    pub code: Option<String>,
    pub secret: Option<String>,
}

/// The edit-server dialog.
#[derive(Debug, Clone)]
pub struct EditForm {
    pub id: ServerId,
    pub kind: SourceKind,
    pub name: String,
    pub url: String,
    pub username: String,
    /// Empty keeps the stored password / token.
    pub password: String,
    pub options: ServerOptions,
    /// Subsonic: the server's music folder as in song paths, and where it is
    /// mounted on this computer ("Show in file manager").
    pub music_server: String,
    pub music_local: String,
    pub busy: bool,
    pub error: Option<String>,
    /// YouTube Music: the browser profile found and its channels to choose from.
    pub ytm_profile: Option<listen_ytmusic::browser::Profile>,
    pub ytm_channels: Vec<listen_ytmusic::channels::Channel>,
    /// Every signed-in browser profile found (for "Use … instead").
    pub ytm_browsers: Vec<listen_ytmusic::browser::Profile>,
}

impl EditForm {
    pub fn new(server: &ServerConfig, options: ServerOptions) -> Self {
        Self {
            id: server.id.clone(),
            kind: server.kind,
            name: server.name.clone(),
            url: server.url.clone(),
            username: server.username.clone(),
            password: String::new(),
            options,
            music_server: String::new(),
            music_local: String::new(),
            busy: false,
            error: None,
            ytm_profile: None,
            ytm_channels: Vec::new(),
            ytm_browsers: Vec::new(),
        }
    }
}

#[derive(Debug, Default)]
pub struct ServersState {
    /// Jellyfin servers that answered the last broadcast.
    pub discovered: Vec<Found>,
    pub discovering: bool,
    pub quick_connect: Option<QuickConnect>,
    pub edit: Option<EditForm>,
    /// Whether a Secret Service answered (None = still asking).
    pub keyring_ok: Option<bool>,
    pub failure: Option<Failure>,
    /// Options of the server being signed in, kept once the sign-in works.
    pending_options: Option<(ServerId, ServerOptions)>,
}

#[derive(Debug, Clone)]
pub enum ServersMsg {
    // sign-in page
    Discover,
    Discovered(Vec<Found>),
    UseDiscovered(usize),
    LegacyAuth(bool),
    IgnoreSsl(bool),
    PreferInstantMix(bool),
    QuickConnectStart,
    QuickConnectStarted(Result<Initiated, String>),
    QuickConnectPolled(String, Result<bool, String>),
    QuickConnectAuthenticated(String, Result<Session, String>),
    QuickConnectCancel,
    OpenQuickConnectPage,
    // edit dialog
    Edit(ServerId),
    EditName(String),
    EditUrl(String),
    EditUsername(String),
    EditPassword(String),
    EditLegacyAuth(bool),
    EditIgnoreSsl(bool),
    EditPreferInstantMix(bool),
    EditMusicServer(String),
    EditMusicLocal(String),
    EditSave,
    EditSaved(Result<(ServerConfig, Option<String>, SourceHandle), String>),
    EditYtmLook,
    EditYtmFound(
        Result<(Vec<listen_ytmusic::browser::Profile>, Vec<listen_ytmusic::channels::Channel>), String>,
    ),
    /// Read the channels of another signed-in browser.
    EditYtmBrowser(usize),
    EditYtmBrowserFound(
        Result<(listen_ytmusic::browser::Profile, Vec<listen_ytmusic::channels::Channel>), String>,
    ),
    EditYtmPick(usize),
    // action-required page, settings
    Retry(ServerId),
    Reconnect(ServerId),
    SetSecretStore(SecretStore),
    KeyringProbed(bool),
}

/// `jellyfin.example.com` -> `https://jellyfin.example.com` (no trailing slash).
pub fn normalize_url(url: &str) -> String {
    let url = url.trim().trim_end_matches('/');
    if url.is_empty() || url.contains("://") { url.to_string() } else { format!("https://{url}") }
}

/// Host part of a URL, the default name of a new server.
pub fn host_of(url: &str) -> String {
    url.split("://").nth(1).unwrap_or(url).split('/').next().unwrap_or(url).to_string()
}

/// Writes an edit into the config and the new secret (if any) into the store, both under the same server id.
pub fn apply_edit(
    config: &mut Config,
    secrets: Option<&Secrets>,
    server: ServerConfig,
    secret: Option<&str>,
    options: ServerOptions,
) {
    let id = server.id.clone();
    if let Some(slot) = config.servers.iter_mut().find(|s| s.id == id) {
        *slot = server;
    }
    if options == ServerOptions::default() {
        config.server_options.remove(&id.0);
    } else {
        config.server_options.insert(id.0.clone(), options);
    }
    if let (Some(secrets), Some(secret)) = (secrets, secret) {
        secrets.set(&id, secret);
    }
}

/// Stores (or clears) a server's "local music folder" mapping.
pub fn set_music_dir(config: &mut Config, id: &ServerId, server_prefix: &str, local: &str) {
    let dir = listen_core::config::MusicDir {
        server_prefix: server_prefix.trim().to_string(),
        local: local.trim().to_string(),
    };
    if dir.is_empty() {
        config.music_dirs.remove(&id.0);
    } else {
        config.music_dirs.insert(id.0.clone(), dir);
    }
}

impl Listen {
    pub(super) fn on_servers(&mut self, msg: ServersMsg) -> Task<Message> {
        use ServersMsg as M;
        match msg {
            M::Discover => return self.start_discovery(),
            M::Discovered(found) => {
                self.servers.discovering = false;
                self.servers.discovered = found;
            }
            M::UseDiscovered(i) => {
                if let Some(found) = self.servers.discovered.get(i).cloned() {
                    self.login =
                        LoginForm { kind: SourceKind::Jellyfin, url: found.url, ..Default::default() };
                }
            }
            M::LegacyAuth(v) => self.login.options.legacy_auth = v,
            M::IgnoreSsl(v) => self.login.options.ignore_ssl = v,
            M::PreferInstantMix(v) => self.login.options.prefer_instant_mix = v,
            M::QuickConnectStart => return self.quick_connect_start(),
            M::QuickConnectStarted(Ok(started)) => {
                let Some(qc) = self.servers.quick_connect.as_mut().filter(|q| q.secret.is_none()) else {
                    return Task::none();
                };
                qc.code = Some(started.code);
                qc.secret = Some(started.secret.clone());
                return self.quick_connect_poll(started.secret);
            }
            M::QuickConnectStarted(Err(e)) => {
                self.servers.quick_connect = None;
                self.login.error = Some(e);
            }
            M::QuickConnectPolled(secret, result) => {
                let Some(qc) =
                    self.servers.quick_connect.clone().filter(|q| q.secret.as_ref() == Some(&secret))
                else {
                    return Task::none();
                };
                match result {
                    Ok(false) => return self.quick_connect_poll(secret),
                    Ok(true) => {
                        let device = format!("listen-{}", qc.server_id.0);
                        return Task::perform(
                            async move {
                                let r = quick_connect::authenticate(
                                    &qc.url,
                                    &device,
                                    &secret,
                                    jf_options(qc.options),
                                )
                                .await;
                                (secret, r.map_err(err))
                            },
                            |(secret, r)| Message::Servers(M::QuickConnectAuthenticated(secret, r)),
                        );
                    }
                    Err(e) => {
                        self.servers.quick_connect = None;
                        self.login.error = Some(e);
                    }
                }
            }
            M::QuickConnectAuthenticated(secret, result) => {
                let Some(qc) =
                    self.servers.quick_connect.clone().filter(|q| q.secret.as_ref() == Some(&secret))
                else {
                    return Task::none();
                };
                self.servers.quick_connect = None;
                match result {
                    Ok(session) => {
                        let server = ServerConfig {
                            id: qc.server_id,
                            name: host_of(&qc.url),
                            kind: SourceKind::Jellyfin,
                            url: qc.url,
                            username: session.user_name,
                        };
                        self.login.options = qc.options;
                        return self.connect_new(
                            server,
                            format!("{}{}", listen_jellyfin::TOKEN_PREFIX, session.token),
                        );
                    }
                    Err(e) => self.login.error = Some(e),
                }
            }
            M::QuickConnectCancel => self.servers.quick_connect = None,
            M::OpenQuickConnectPage => {
                if let Some(url) =
                    self.servers.quick_connect.as_ref().and_then(|q| quick_connect::approval_url(&q.url))
                {
                    super::album_page::open_url(&url);
                }
            }
            M::Edit(id) => {
                let Some(server) = self.config.servers.iter().find(|s| s.id == id) else {
                    return Task::none();
                };
                let mut form = EditForm::new(server, self.config.options_of(&id));
                if let Some(dir) = self.config.music_dirs.get(&id.0) {
                    form.music_server = dir.server_prefix.clone();
                    form.music_local = dir.local.clone();
                }
                self.servers.edit = Some(form);
                self.open_modal(EDIT_MODAL);
            }
            M::EditName(v) => self.edit_with(|f| f.name = v),
            M::EditUrl(v) => self.edit_with(|f| f.url = v),
            M::EditUsername(v) => self.edit_with(|f| f.username = v),
            M::EditPassword(v) => self.edit_with(|f| f.password = v),
            M::EditLegacyAuth(v) => self.edit_with(|f| f.options.legacy_auth = v),
            M::EditIgnoreSsl(v) => self.edit_with(|f| f.options.ignore_ssl = v),
            M::EditPreferInstantMix(v) => self.edit_with(|f| f.options.prefer_instant_mix = v),
            M::EditMusicServer(v) => self.edit_with(|f| f.music_server = v),
            M::EditMusicLocal(v) => self.edit_with(|f| f.music_local = v),
            M::EditSave => return self.edit_save(),
            M::EditSaved(Ok((server, secret, handle))) => {
                let id = server.id.clone();
                let options =
                    self.servers.edit.as_ref().map_or_else(|| self.config.options_of(&id), |f| f.options);
                apply_edit(
                    &mut self.config,
                    self.secrets.as_ref().filter(|_| !self.ephemeral),
                    server,
                    secret.as_deref(),
                    options,
                );
                if let Some(f) = &self.servers.edit {
                    set_music_dir(&mut self.config, &id, &f.music_server, &f.music_local);
                }
                self.save_config();
                self.servers.edit = None;
                self.close_modal();
                self.toast_success("Server saved.".into());
                self.sources.remove(&id);
                self.library_cache.remove(&id);
                return self.update(Message::Connected(id, Ok(handle)));
            }
            M::EditSaved(Err(e)) => {
                self.edit_with(|f| {
                    f.busy = false;
                    f.error = Some(e);
                });
            }
            M::EditYtmLook => {
                self.edit_with(|f| {
                    f.busy = true;
                    f.error = None;
                });
                return Task::perform(
                    async {
                        let found = tokio::task::spawn_blocking(listen_ytmusic::browser::signed_in)
                            .await
                            .map_err(|e| e.to_string())??;
                        let channels = listen_ytmusic::channels::list(&found[0].1).await?;
                        Ok((found.into_iter().map(|(p, _)| p).collect(), channels))
                    },
                    |r| Message::Servers(M::EditYtmFound(r)),
                );
            }
            M::EditYtmFound(Ok((browsers, channels))) => {
                self.edit_with(|f| {
                    f.busy = false;
                    f.ytm_profile = browsers.first().cloned();
                    f.ytm_browsers = browsers;
                    f.ytm_channels = channels;
                });
            }
            M::EditYtmBrowser(i) => {
                let Some(profile) = self.servers.edit.as_ref().and_then(|f| f.ytm_browsers.get(i).cloned())
                else {
                    return Task::none();
                };
                self.edit_with(|f| {
                    f.busy = true;
                    f.error = None;
                });
                return Task::perform(
                    async move {
                        let p = profile.clone();
                        let header = tokio::task::spawn_blocking(move || p.youtube_header())
                            .await
                            .map_err(|e| e.to_string())??;
                        let channels = listen_ytmusic::channels::list(&header).await?;
                        Ok((profile, channels))
                    },
                    |r| Message::Servers(M::EditYtmBrowserFound(r)),
                );
            }
            M::EditYtmBrowserFound(Ok((profile, channels))) => {
                self.edit_with(|f| {
                    f.busy = false;
                    f.ytm_profile = Some(profile);
                    f.ytm_channels = channels;
                });
            }
            M::EditYtmBrowserFound(Err(e)) => {
                self.edit_with(|f| {
                    f.busy = false;
                    f.error = Some(e);
                });
            }
            M::EditYtmFound(Err(e)) => {
                self.edit_with(|f| {
                    f.busy = false;
                    f.error = Some(e);
                });
            }
            M::EditYtmPick(i) => {
                let Some(form) = self.servers.edit.clone() else { return Task::none() };
                let (Some(mut profile), Some(channel)) =
                    (form.ytm_profile.clone(), form.ytm_channels.get(i).cloned())
                else {
                    return Task::none();
                };
                profile.page_id = channel.page_id;
                profile.channel = Some(channel.name.clone());
                let Some(mut server) = self.config.servers.iter().find(|s| s.id == form.id).cloned() else {
                    return Task::none();
                };
                server.name = format!("YouTube Music ({})", channel.name);
                apply_edit(
                    &mut self.config,
                    self.secrets.as_ref().filter(|_| !self.ephemeral),
                    server,
                    Some(&profile.to_secret()),
                    form.options,
                );
                self.save_config();
                self.servers.edit = None;
                self.close_modal();
                self.toast_success(format!("Now using the channel {}.", channel.name));
                return self.reconnect(form.id);
            }
            M::Retry(id) => {
                self.servers.failure = None;
                return self.reconnect(id);
            }
            M::Reconnect(id) => {
                self.toast_info("Reconnecting…".into());
                return self.reconnect(id);
            }
            M::SetSecretStore(store) => {
                if let Some(secrets) = &self.secrets {
                    let ids: Vec<ServerId> = self.config.servers.iter().map(|s| s.id.clone()).collect();
                    secrets.switch_store(store, &ids);
                }
                self.config.secret_store = store;
                self.save_config();
                return self.probe_keyring();
            }
            M::KeyringProbed(ok) => self.servers.keyring_ok = Some(ok),
        }
        Task::none()
    }

    fn edit_with(&mut self, f: impl FnOnce(&mut EditForm)) {
        if let Some(form) = self.servers.edit.as_mut() {
            f(form);
        }
    }

    /// Drops the connection of a server and connects it again.
    pub(super) fn reconnect(&mut self, id: ServerId) -> Task<Message> {
        self.sources.remove(&id);
        if self.config.active_server.as_ref() == Some(&id) || self.source.is_none() {
            self.config.active_server = Some(id.clone());
            self.connecting = true;
            return self.connect_server(id);
        }
        self.connect_background(id)
    }

    /// Asks the LAN for Jellyfin servers (3 s).
    pub(super) fn start_discovery(&mut self) -> Task<Message> {
        if self.demo || self.servers.discovering {
            return Task::none();
        }
        self.servers.discovering = true;
        Task::perform(listen_jellyfin::discovery::discover(), |found| {
            Message::Servers(ServersMsg::Discovered(found))
        })
    }

    /// Whether a Secret Service answers; off the UI thread.
    pub(super) fn probe_keyring(&mut self) -> Task<Message> {
        if self.demo || self.ephemeral || self.secrets.is_none() {
            return Task::none();
        }
        Task::perform(
            async { tokio::task::spawn_blocking(Secrets::keyring_available).await.unwrap_or(false) },
            |ok| Message::Servers(ServersMsg::KeyringProbed(ok)),
        )
    }

    fn quick_connect_start(&mut self) -> Task<Message> {
        if self.servers.quick_connect.is_some() {
            return Task::none();
        }
        let url = normalize_url(&self.login.url);
        if url.is_empty() {
            self.login.error = Some("Enter the server URL first.".into());
            return Task::none();
        }
        self.login.error = None;
        let qc = QuickConnect {
            server_id: ServerId(format!("jf-{:x}", unix_millis())),
            url: url.clone(),
            options: self.login.options,
            code: None,
            secret: None,
        };
        let device = format!("listen-{}", qc.server_id.0);
        let options = jf_options(qc.options);
        self.servers.quick_connect = Some(qc);
        Task::perform(
            async move { quick_connect::initiate(&url, &device, options).await.map_err(err) },
            |r| Message::Servers(ServersMsg::QuickConnectStarted(r)),
        )
    }

    /// Asks again after five seconds (Feishin's interval).
    fn quick_connect_poll(&self, secret: String) -> Task<Message> {
        let Some(qc) = self.servers.quick_connect.clone() else { return Task::none() };
        let device = format!("listen-{}", qc.server_id.0);
        Task::perform(
            async move {
                tokio::time::sleep(Duration::from_secs(quick_connect::POLL_SECS)).await;
                let r = quick_connect::is_approved(&qc.url, &device, &secret, jf_options(qc.options)).await;
                (secret, r.map_err(err))
            },
            |(secret, r)| Message::Servers(ServersMsg::QuickConnectPolled(secret, r)),
        )
    }

    /// Checks the edited sign-in (unless only the name changed), then saves it.
    fn edit_save(&mut self) -> Task<Message> {
        let Some(form) = self.servers.edit.clone().filter(|f| !f.busy) else { return Task::none() };
        let Some(old) = self.config.servers.iter().find(|s| s.id == form.id).cloned() else {
            return Task::none();
        };
        let name = form.name.trim();
        let mut server = ServerConfig {
            name: if name.is_empty() { old.name.clone() } else { name.to_string() },
            ..old.clone()
        };
        match form.kind {
            SourceKind::YouTubeMusic => {}
            SourceKind::Local => server.url = form.url.trim().to_string(),
            _ => {
                server.url = normalize_url(&form.url);
                server.username = form.username.trim().to_string();
                if server.url.is_empty() || server.username.is_empty() {
                    self.edit_with(|f| f.error = Some("Server URL and username are required.".into()));
                    return Task::none();
                }
            }
        }
        let new_secret = Some(form.password.clone()).filter(|p| !p.is_empty());
        let secret = new_secret.clone().or_else(|| self.secret_of(&old));
        let Some(secret) = secret else {
            self.edit_with(|f| f.error = Some("Enter the password.".into()));
            return Task::none();
        };
        self.edit_with(|f| {
            f.busy = true;
            f.error = None;
        });
        let options = form.options;
        Task::perform(
            async move {
                let handle = Self::connect_source(server.clone(), secret, options).await?;
                Ok((server, new_secret, handle))
            },
            |r| Message::Servers(ServersMsg::EditSaved(r)),
        )
    }

    /// Remembers the options of a server being signed in (kept when it works).
    pub(super) fn note_pending_options(&mut self, id: &ServerId, options: ServerOptions) {
        self.servers.pending_options = Some((id.clone(), options));
    }

    /// The sign-in of `server` worked: its options are saved.
    pub(super) fn commit_pending_options(&mut self, id: &ServerId) {
        if let Some((pid, options)) = self.servers.pending_options.take()
            && pid == *id
            && options != ServerOptions::default()
        {
            self.config.server_options.insert(id.0.clone(), options);
        }
    }

    /// Where the passwords are kept (for the settings row).
    pub fn secret_store_description(&self) -> Option<String> {
        self.secrets.as_ref().map(|s| s.describe(self.servers.keyring_ok))
    }

    /// The page to show instead of the sign-in form, if a saved server needs attention.
    pub fn action_required(&self) -> Option<&Failure> {
        if self.demo || self.adding_server || self.connecting || self.source.is_some() {
            return None;
        }
        self.servers.failure.as_ref().filter(|f| self.config.servers.iter().any(|s| s.id == *f.id()))
    }
}

pub fn jf_options(o: ServerOptions) -> listen_jellyfin::Options {
    listen_jellyfin::Options { ignore_ssl: o.ignore_ssl, prefer_instant_mix: o.prefer_instant_mix }
}

/// Reason texts of the action-required page: (title, message).
pub fn failure_text(failure: &Failure, name: &str) -> (String, String) {
    match failure {
        Failure::Credentials { .. } => (
            format!("Sign in to {name} again"),
            "Listen has no saved password for this server, or the server no longer accepts it.".into(),
        ),
        Failure::Unreachable { reason, .. } => (format!("Could not reach {name}"), reason.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(id: &str) -> ServerConfig {
        ServerConfig {
            id: ServerId(id.into()),
            name: "Home".into(),
            kind: SourceKind::Subsonic,
            url: "https://old.example".into(),
            username: "me".into(),
        }
    }

    #[test]
    fn normalizes_server_urls() {
        assert_eq!(normalize_url(" music.example.com/ "), "https://music.example.com");
        assert_eq!(normalize_url("http://10.0.0.2:4533/"), "http://10.0.0.2:4533");
        assert_eq!(normalize_url(""), "");
        assert_eq!(host_of("https://music.example.com/nd"), "music.example.com");
    }

    #[test]
    fn music_dir_is_stored_trimmed_and_cleared_when_empty() {
        let mut config = Config::default();
        let id = ServerId("nd-1".into());
        set_music_dir(&mut config, &id, " /music ", " /mnt/nas/Music ");
        let dir = config.music_dirs.get("nd-1").unwrap();
        assert_eq!((dir.server_prefix.as_str(), dir.local.as_str()), ("/music", "/mnt/nas/Music"));
        set_music_dir(&mut config, &id, "/music", "  ");
        assert!(config.music_dirs.is_empty());
    }

    #[test]
    fn edit_keeps_the_server_id_and_its_stored_secret_entry() {
        let dir = std::env::temp_dir().join(format!("listen-edit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let secrets = Secrets::with_store(&dir, SecretStore::File);
        let mut config = Config { servers: vec![server("nd-1"), server("nd-2")], ..Default::default() };
        let (a, b) = (ServerId("nd-1".into()), ServerId("nd-2".into()));
        secrets.set(&a, "old");
        secrets.set(&b, "other");

        let mut edited = server("nd-1");
        edited.url = "https://new.example".into();
        edited.username = "you".into();
        edited.name = "Renamed".into();
        let options = ServerOptions { legacy_auth: true, ..Default::default() };
        apply_edit(&mut config, Some(&secrets), edited, Some("new"), options);

        assert_eq!(config.servers.len(), 2);
        assert_eq!(config.servers[0].id, a);
        assert_eq!(
            (config.servers[0].name.as_str(), config.servers[0].url.as_str()),
            ("Renamed", "https://new.example")
        );
        assert_eq!(config.servers[0].username, "you");
        assert_eq!(secrets.get(&a).as_deref(), Some("new"), "same entry, new password");
        assert_eq!(secrets.get(&b).as_deref(), Some("other"), "other servers untouched");
        assert!(config.options_of(&a).legacy_auth);

        // Without a new password the stored one stays; default options are not stored.
        apply_edit(&mut config, Some(&secrets), server("nd-1"), None, ServerOptions::default());
        assert_eq!(secrets.get(&a).as_deref(), Some("new"));
        assert!(config.server_options.is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn options_roundtrip_and_old_configs_load() {
        let mut config = Config::default();
        config.server_options.insert("nd-1".into(), ServerOptions { ignore_ssl: true, ..Default::default() });
        config.secret_store = SecretStore::File;
        let text = toml::to_string_pretty(&config).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back, config);
        // A config written before these fields existed.
        let old: Config =
            toml::from_str("active_server = \"nd\"\n[[servers]]\nid = \"nd\"\nname = \"x\"\nkind = \"subsonic\"\nurl = \"u\"\nusername = \"me\"\n")
                .unwrap();
        assert_eq!(old.options_of(&ServerId("nd".into())), ServerOptions::default());
        assert_eq!(old.secret_store, SecretStore::Auto);
    }

    #[test]
    fn failure_texts_name_the_server() {
        let id = ServerId("nd".into());
        let (title, _) = failure_text(&Failure::Credentials { id: id.clone() }, "Home");
        assert!(title.contains("Home"));
        let (title, msg) = failure_text(&Failure::Unreachable { id, reason: "timed out".into() }, "Home");
        assert!(title.contains("Home") && msg == "timed out");
    }
}
