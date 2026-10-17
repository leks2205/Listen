//! Sign-in extras headlessly: LAN discovery list, option toggles, Quick
//! Connect, the edit-server dialog and the action-required page.

use iced_test::simulator::Simulator;
use listen::app::servers::{Failure, ServersMsg};
use listen::app::{INTER, Listen, LoginMsg, Message, SourceHandle};
use listen::{Args, FONTS};
use listen_core::config::ServerConfig;
use listen_core::model::{ServerId, SourceKind};
use listen_jellyfin::discovery::Found;
use listen_jellyfin::quick_connect::Initiated;
use std::sync::Arc;

/// One `Simulator` (Vulkan instance) at a time, see `compact_layout.rs`.
static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn gpu() -> std::sync::MutexGuard<'static, ()> {
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

fn settings() -> iced::Settings {
    iced::Settings {
        fonts: FONTS.iter().map(|f| std::borrow::Cow::Borrowed(*f)).collect(),
        default_font: INTER,
        ..Default::default()
    }
}

fn state() -> Listen {
    let (mut s, _) =
        Listen::boot(&Args { theme: Some("tokyoNight".into()), ephemeral: true, ..Args::default() });
    // Never the real config of the machine running the tests.
    s.config = Default::default();
    s.connecting = false;
    let _ = s.update(Message::WindowResized(iced::Size::new(1200.0, 800.0)));
    s
}

fn nd() -> ServerConfig {
    ServerConfig {
        id: ServerId("test-nd".into()),
        name: "Home Navidrome".into(),
        kind: SourceKind::Subsonic,
        url: "https://music.example.com".into(),
        username: "alex".into(),
    }
}

fn click_text(s: &mut Listen, text: &str) -> Vec<Message> {
    let messages: Vec<Message> = {
        let _gpu = gpu();
        let mut ui = Simulator::with_size(settings(), s.window_size, s.view());
        ui.click(text).unwrap_or_else(|_| panic!("no widget with “{text}”"));
        ui.into_messages().collect()
    };
    for m in messages.clone() {
        let _ = s.update(m);
    }
    messages
}

fn has_text(s: &Listen, text: &str) -> bool {
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings(), s.window_size, s.view());
    ui.find(text).is_ok()
}

fn found() -> Vec<Found> {
    vec![
        Found { name: "Living room NAS".into(), url: "http://192.168.1.20:8096".into(), id: "a".into() },
        Found { name: "Cellar".into(), url: "http://192.168.1.21:8096".into(), id: "b".into() },
    ]
}

#[test]
fn discovered_servers_are_listed_and_fill_the_sign_in_form() {
    let mut s = state();
    let _ = s.update(Message::Login(LoginMsg::Kind(SourceKind::Jellyfin)));
    // Nothing found yet (the scan may already be running on a fresh config).
    assert!(!has_text(&s, "Living room NAS"));
    let _ = s.update(Message::Servers(ServersMsg::Discovered(found())));
    assert!(has_text(&s, "Found on your network"));
    assert!(has_text(&s, "Living room NAS"));

    // The second "Use" belongs to “Cellar”; the first one fills the form.
    let messages = click_text(&mut s, "Use");
    assert!(messages.iter().any(|m| matches!(m, Message::Servers(ServersMsg::UseDiscovered(_)))));
    assert_eq!(s.login.kind, SourceKind::Jellyfin);
    assert!(s.login.url.starts_with("http://192.168.1.2"));
}

#[test]
fn a_known_server_is_not_offered_again() {
    let mut s = state();
    s.config.servers = vec![ServerConfig {
        id: ServerId("test-jf".into()),
        name: "Known".into(),
        kind: SourceKind::Jellyfin,
        url: "http://192.168.1.20:8096".into(),
        username: "me".into(),
    }];
    let _ = s.update(Message::AddServer);
    let _ = s.update(Message::Login(LoginMsg::Kind(SourceKind::Jellyfin)));
    let _ = s.update(Message::Servers(ServersMsg::Discovered(found())));
    assert!(!has_text(&s, "Living room NAS"));
    assert!(has_text(&s, "Cellar"));
}

#[test]
fn option_toggles_follow_the_server_type() {
    let mut s = state();
    assert!(has_text(&s, "Legacy authentication"));
    assert!(has_text(&s, "Ignore certificate errors"));
    assert!(!has_text(&s, "Prefer instant mix"));
    let _ = s.update(Message::Servers(ServersMsg::LegacyAuth(true)));
    let _ = s.update(Message::Servers(ServersMsg::IgnoreSsl(true)));
    assert!(s.login.options.legacy_auth && s.login.options.ignore_ssl);

    let _ = s.update(Message::Login(LoginMsg::Kind(SourceKind::Jellyfin)));
    assert!(has_text(&s, "Prefer instant mix"));
    assert!(!has_text(&s, "Legacy authentication"));
    let _ = s.update(Message::Login(LoginMsg::Kind(SourceKind::YouTubeMusic)));
    assert!(!has_text(&s, "Ignore certificate errors"));
}

#[test]
fn quick_connect_shows_the_code_and_can_be_cancelled() {
    let mut s = state();
    let _ = s.update(Message::Login(LoginMsg::Kind(SourceKind::Jellyfin)));
    // Needs the server URL first.
    let _ = s.update(Message::Servers(ServersMsg::QuickConnectStart));
    assert!(s.servers.quick_connect.is_none());
    assert!(s.login.error.is_some());

    s.login.url = "jf.example.com".into();
    let _ = s.update(Message::Servers(ServersMsg::QuickConnectStart));
    let qc = s.servers.quick_connect.clone().expect("started");
    assert_eq!(qc.url, "https://jf.example.com");
    let started = Initiated { code: "482913".into(), secret: "s3".into() };
    let _ = s.update(Message::Servers(ServersMsg::QuickConnectStarted(Ok(started))));
    assert!(has_text(&s, "482913"));

    // An answer for another request is ignored.
    let _ = s.update(Message::Servers(ServersMsg::QuickConnectPolled("old".into(), Ok(true))));
    assert!(s.servers.quick_connect.is_some());

    click_text(&mut s, "Cancel Quick Connect");
    assert!(s.servers.quick_connect.is_none());
    assert!(has_text(&s, "Sign in with Quick Connect"));
}

fn handle() -> SourceHandle {
    let source: Arc<dyn listen_source::MusicSource> = Arc::new(
        listen_subsonic::Subsonic::new(ServerId("test-nd".into()), "https://new.example.com", "alex", "x")
            .unwrap(),
    );
    SourceHandle(source)
}

#[test]
fn editing_a_server_keeps_its_id_and_replaces_the_connection() {
    let mut s = state();
    s.config.servers = vec![nd()];
    let id = ServerId("test-nd".into());
    let _ = s.update(Message::Connected(id.clone(), Ok(handle())));
    assert_eq!(s.config.active_server, Some(id.clone()));

    let _ = s.update(Message::Servers(ServersMsg::Edit(id.clone())));
    assert!(has_text(&s, "Edit server"));
    let form = s.servers.edit.clone().expect("form");
    assert_eq!((form.name.as_str(), form.username.as_str()), ("Home Navidrome", "alex"));

    let _ = s.update(Message::Servers(ServersMsg::EditName("Renamed".into())));
    let _ = s.update(Message::Servers(ServersMsg::EditUrl("https://new.example.com".into())));
    let _ = s.update(Message::Servers(ServersMsg::EditIgnoreSsl(true)));
    let mut edited = nd();
    edited.name = "Renamed".into();
    edited.url = "https://new.example.com".into();
    let _ = s.update(Message::Servers(ServersMsg::EditSaved(Ok((edited, Some("pw".into()), handle())))));

    assert_eq!(s.config.servers.len(), 1);
    assert_eq!(s.config.servers[0].id, id);
    assert_eq!(s.config.servers[0].name, "Renamed");
    assert_eq!(s.config.servers[0].url, "https://new.example.com");
    assert!(s.config.options_of(&id).ignore_ssl);
    assert!(s.servers.edit.is_none());
    assert!(s.overlay.modal.is_none(), "dialog closed");
    assert_eq!(s.config.active_server, Some(id.clone()));
    assert!(s.sources.contains_key(&id));
}

#[test]
fn a_failed_check_keeps_the_dialog_open_with_the_reason() {
    let mut s = state();
    s.config.servers = vec![nd()];
    let _ = s.update(Message::Servers(ServersMsg::Edit(ServerId("test-nd".into()))));
    let _ = s.update(Message::Servers(ServersMsg::EditSaved(Err("Wrong username or password.".into()))));
    assert!(has_text(&s, "Wrong username or password."));
    assert_eq!(s.config.servers[0].name, "Home Navidrome", "nothing saved");
}

#[test]
fn an_unreachable_server_shows_the_action_required_page() {
    let mut s = state();
    s.config.servers = vec![nd()];
    let id = ServerId("test-nd".into());
    s.config.active_server = Some(id.clone());
    let _ = s.update(Message::Connected(id.clone(), Err("timed out".into())));
    assert_eq!(
        s.action_required(),
        Some(&Failure::Unreachable { id: id.clone(), reason: "timed out".into() })
    );
    assert!(has_text(&s, "Could not reach Home Navidrome"));
    assert!(has_text(&s, "timed out"));

    // "Edit server" opens the dialog on top of the page.
    click_text(&mut s, "Edit server");
    assert!(s.servers.edit.is_some());

    // "Add a server" leaves the page for the sign-in form, and Cancel comes back.
    let _ = s.update(Message::Overlay(listen::app::overlay::OverlayMsg::CloseModal));
    click_text(&mut s, "Add a server");
    assert!(s.action_required().is_none());
    assert!(has_text(&s, "Connect to Navidrome"));
    let _ = s.update(Message::CancelAddServer);
    assert!(s.action_required().is_some());
}

#[test]
fn wrong_credentials_ask_for_the_password() {
    let mut s = state();
    s.config.servers = vec![nd()];
    let id = ServerId("test-nd".into());
    s.config.active_server = Some(id.clone());
    let _ = s.update(Message::Connected(id.clone(), Err("Wrong username or password. (HTTP 401)".into())));
    assert_eq!(s.action_required(), Some(&Failure::Credentials { id }));
    assert!(has_text(&s, "Sign in to Home Navidrome again"));
    click_text(&mut s, "Enter the password");
    assert!(s.servers.edit.is_some());
}

#[test]
fn a_working_connection_clears_the_page() {
    let mut s = state();
    s.config.servers = vec![nd()];
    let id = ServerId("test-nd".into());
    s.config.active_server = Some(id.clone());
    let _ = s.update(Message::Connected(id.clone(), Err("timed out".into())));
    assert!(s.action_required().is_some());
    let _ = s.update(Message::Connected(id, Ok(handle())));
    assert!(s.action_required().is_none());
    assert!(!s.needs_login());
}
