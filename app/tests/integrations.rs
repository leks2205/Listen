//! Scrobble modes, private mode, Discord and notification settings, and the
//! remote control server wired to a real app.

use futures_util::{SinkExt, StreamExt};
use iced_test::simulator::Simulator;
use listen::app::integrations::{Field, IntegrationSetting as I, IntegrationsMsg};
use listen::app::{INTER, Listen, Message, Setting};
use listen::playback::PlayerMsg;
use listen::{Args, FONTS, demo};
use listen_core::settings::{DiscordDisplay, DiscordLink, ScrobbleMode, SettingsTab};
use listen_player::PlayerEvent;
use serde_json::{Value, json};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message as Ws;

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

fn player() -> Listen {
    let (mut s, _) = Listen::boot(&Args { ephemeral: true, ..Args::default() });
    s.queue.set(demo::album_songs(0), 0);
    s.config.settings.history.enabled = true;
    s.config.settings.integrations = Default::default();
    s
}

fn pos(s: &mut Listen, secs: f64) {
    let ev = PlayerEvent::Position(Duration::from_secs_f64(secs));
    let _ = s.update(Message::Deck(0, PlayerMsg::Event(ev)));
}

fn set(s: &mut Listen, i: I) {
    let _ = s.update(Message::Setting(Setting::Integrations(i)));
}

/// Plays `total` seconds one tick at a time; the second at which the play counted (the history is the proof).
fn scrobbled_after(s: &mut Listen, total: u32) -> Option<u32> {
    for t in 0..=total {
        pos(s, f64::from(t));
        if !s.history.entries.is_empty() {
            return Some(t);
        }
    }
    None
}

#[test]
fn scrobble_modes_decide_when_a_play_counts() {
    let song_len = demo::album_songs(0)[0].duration.as_secs() as u32;
    assert!(song_len >= 60, "the test needs a song of about a minute or more, got {song_len}");
    let at_percent = |pct: u8| (f64::from(song_len) * f64::from(pct) / 100.0).ceil() as u32;
    // (mode, percent, seconds, expected second of the scrobble)
    for (mode, pct, secs, want) in [
        (ScrobbleMode::Percentage, 25u8, 240u32, Some(at_percent(25))),
        (ScrobbleMode::Percentage, 90, 1, Some(at_percent(90))),
        (ScrobbleMode::Percentage, 50, 5, Some(at_percent(50))),
        (ScrobbleMode::Seconds, 90, 30, Some(30)),
        (ScrobbleMode::Seconds, 25, 0, Some(0)),
        (ScrobbleMode::Both, 90, 20, Some(20)),
        (ScrobbleMode::Both, 25, 1200, Some(at_percent(25))),
        (ScrobbleMode::Seconds, 25, 1200, None),
    ] {
        let mut s = player();
        set(&mut s, I::ScrobbleMode(mode));
        set(&mut s, I::ScrobblePercent(pct));
        set(&mut s, I::ScrobbleSecs(secs));
        assert_eq!(scrobbled_after(&mut s, song_len), want, "{mode:?} {pct} % / {secs} s");
    }
}

#[test]
fn scrobble_settings_are_clamped_like_feishin() {
    let mut s = player();
    set(&mut s, I::ScrobblePercent(5));
    assert_eq!(s.config.settings.playback.scrobble_at_percent, 25);
    set(&mut s, I::ScrobblePercent(200));
    assert_eq!(s.config.settings.playback.scrobble_at_percent, 90);
    set(&mut s, I::ScrobbleSecs(5000));
    assert_eq!(s.config.settings.playback.scrobble_at_secs, 1200);
}

#[test]
fn private_mode_stops_the_history_and_marks_the_title() {
    let mut s = player();
    assert!(!s.title().contains("Private mode"));
    let _ = s.update(Message::Integrations(IntegrationsMsg::TogglePrivate));
    assert!(s.private_mode());
    assert!(s.title().ends_with("(Private mode)"), "{}", s.title());
    assert_eq!(s.overlay.toasts.len(), 1, "a toast says so");
    for t in 0..300 {
        pos(&mut s, f64::from(t));
    }
    assert!(s.history.entries.is_empty(), "nothing is recorded while private");
    let _ = s.update(Message::Integrations(IntegrationsMsg::TogglePrivate));
    assert!(!s.private_mode());
    assert!(!s.title().contains("Private mode"));
    // The older "Don't record this session" switch is the same flag.
    let rt = tokio::runtime::Runtime::new().unwrap();
    for m in run(&rt, s.update(Message::Setting(Setting::HistoryPrivate(true)))) {
        let _ = s.update(m);
    }
    assert!(s.private_mode());
}

#[test]
fn settings_are_stored_and_survive_a_round_trip() {
    let mut s = player();
    set(&mut s, I::DiscordListening(true));
    set(&mut s, I::DiscordPaused(false));
    set(&mut s, I::DiscordStateIcon(false));
    set(&mut s, I::DiscordDisplay(DiscordDisplay::Song));
    set(&mut s, I::DiscordLink(DiscordLink::MusicBrainzLastFm));
    set(&mut s, I::DiscordServerImage(true));
    set(&mut s, I::Notify(true));
    set(&mut s, I::RemoteEnabled(false));
    let ig = s.config.settings.integrations.clone();
    assert!(ig.discord.show_as_listening && !ig.discord.show_paused && !ig.discord.state_icon);
    assert_eq!((ig.discord.display, ig.discord.link), (DiscordDisplay::Song, DiscordLink::MusicBrainzLastFm));
    assert!(ig.discord.server_image && ig.notify_song_change);
    let back: listen_core::settings::Settings =
        toml::from_str(&toml::to_string(&s.config.settings).unwrap()).unwrap();
    assert_eq!(back.integrations, ig);
}

#[test]
fn remote_text_fields_validate_before_saving() {
    let mut s = player();
    let input = |s: &mut Listen, f: Field, v: &str| {
        let _ = s.update(Message::Integrations(IntegrationsMsg::Input(f, v.into())));
        let _ = s.update(Message::Integrations(IntegrationsMsg::Save(f)));
    };
    input(&mut s, Field::RemotePort, "4400");
    assert_eq!(s.config.settings.integrations.remote.port, 4400);
    for bad in ["0", "70000", "abc", ""] {
        input(&mut s, Field::RemotePort, bad);
        assert_eq!(s.config.settings.integrations.remote.port, 4400, "{bad:?} is refused");
    }
    input(&mut s, Field::RemoteBind, "127.0.0.1");
    assert_eq!(s.config.settings.integrations.remote.bind, "127.0.0.1");
    input(&mut s, Field::RemoteBind, "localhost");
    assert_eq!(s.config.settings.integrations.remote.bind, "127.0.0.1", "only IP addresses");
    input(&mut s, Field::RemoteUser, "  alex ");
    assert_eq!(s.config.settings.integrations.remote.username, "alex");
    input(&mut s, Field::RemotePassword, "hunter2");
    assert!(s.integ.remote_password_set());
    let saved = toml::to_string(&s.config.settings).unwrap();
    assert!(!saved.contains("hunter2"), "the password never goes into the settings file");
}

fn run(rt: &tokio::runtime::Runtime, task: iced::Task<Message>) -> Vec<Message> {
    let Some(mut stream) = iced_runtime::task::into_stream(task) else { return vec![] };
    rt.block_on(async move {
        let mut out = Vec::new();
        while let Some(action) = stream.next().await {
            if let iced_runtime::Action::Output(m) = action {
                out.push(m);
            }
        }
        out
    })
}

fn basic(user: &str, pass: &str) -> String {
    use base64::Engine;
    format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("{user}:{pass}")))
}

type Client = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn event(c: &mut Client) -> (String, Value) {
    loop {
        let m = tokio::time::timeout(Duration::from_secs(3), c.next())
            .await
            .expect("in time")
            .expect("open")
            .unwrap();
        if let Ws::Text(t) = m {
            let v: Value = serde_json::from_str(t.as_str()).unwrap();
            return (v["event"].as_str().unwrap().to_string(), v["data"].clone());
        }
    }
}

#[test]
fn a_phone_controls_the_running_app() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut s = player();
    s.playback.volume = 70;
    {
        let r = &mut s.config.settings.integrations.remote;
        r.port = 0;
        r.bind = "127.0.0.1".into();
        r.username = "alex".into();
    }
    let _ = s.update(Message::Integrations(IntegrationsMsg::Input(Field::RemotePassword, "pw".into())));
    let task = s.update(Message::Integrations(IntegrationsMsg::Save(Field::RemotePassword)));
    assert!(run(&rt, task).is_empty());
    let started = run(&rt, s.update(Message::Setting(Setting::Integrations(I::RemoteEnabled(true)))));
    for m in started {
        let _ = s.update(m);
    }
    let addr = s.integ.remote_addr().expect("the server runs");
    assert!(s.integ.remote_error().is_none());
    let _ = s.update(Message::Noop); // publishes the player

    rt.block_on(async {
        let mut req = tokio_tungstenite::tungstenite::client::IntoClientRequest::into_client_request(
            format!("ws://{addr}/"),
        )
        .unwrap();
        req.headers_mut().insert("Authorization", basic("alex", "pw").parse().unwrap());
        let (mut c, _) = tokio_tungstenite::connect_async(req).await.unwrap();
        let (e, state) = event(&mut c).await;
        assert_eq!(e, "state");
        let first = demo::album_songs(0);
        assert_eq!(state["song"]["name"], json!(first[0].title));
        assert_eq!(state["song"]["artistName"], json!(first[0].artist_line()));
        assert_eq!(state["volume"], json!(70));
        assert_eq!(state["repeat"], json!("none"));
        let text = state.to_string();
        assert!(!text.contains("pw") || !text.contains("alex"), "no login in the state");
        let (e, queue) = event(&mut c).await;
        assert_eq!(e, "queue-state");
        let items = queue["items"].as_array().unwrap();
        assert_eq!(items.len(), first.len());
        assert_eq!(event(&mut c).await.0, "radio-status");
        assert_eq!(event(&mut c).await.0, "confirm-queue-changes-setting");
        // The desktop's accent colour for the phone's page (hex for both themes).
        let (e, accent) = event(&mut c).await;
        assert_eq!(e, "accent-color");
        assert!(accent["dark"].as_str().is_some_and(|c| c.starts_with('#')));
        assert!(accent["light"].as_str().is_some_and(|c| c.starts_with('#')));
        let third = items[2]["uniqueId"].as_str().unwrap().to_string();
        let second = items[1]["uniqueId"].as_str().unwrap().to_string();

        // Volume from the phone.
        c.send(Ws::text(json!({ "event": "volume", "volume": 25 }).to_string())).await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        while let Some(cmd) = s.integ.poll_remote_command() {
            let _ = s.update(Message::Integrations(IntegrationsMsg::Remote(cmd)));
        }
        assert_eq!(s.playback.volume, 25);
        let _ = s.update(Message::Noop);
        assert_eq!(event(&mut c).await, ("volume".to_string(), json!(25)));
        // Skip ahead in the queue.
        c.send(Ws::text(json!({ "event": "queue-jump", "uniqueId": third }).to_string())).await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        while let Some(cmd) = s.integ.poll_remote_command() {
            let _ = s.update(Message::Integrations(IntegrationsMsg::Remote(cmd)));
        }
        assert_eq!(s.queue.current().unwrap().song.title, first[2].title);
        let _ = s.update(Message::Noop);
        let mut saw_song = false;
        for _ in 0..6 {
            let (e, d) = event(&mut c).await;
            if e == "song" {
                assert_eq!(d["name"], json!(first[2].title));
                saw_song = true;
                break;
            }
        }
        assert!(saw_song, "the phone is told about the new song");
        // Remove a queued song and clear the queue.
        c.send(Ws::text(json!({ "event": "remove-from-queue", "uniqueId": second }).to_string()))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        while let Some(cmd) = s.integ.poll_remote_command() {
            let _ = s.update(Message::Integrations(IntegrationsMsg::Remote(cmd)));
        }
        assert_eq!(s.queue.len(), first.len() - 1);
    });

    // A phone without the password is turned away.
    rt.block_on(async {
        let (mut c, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/")).await.unwrap();
        c.send(Ws::text(json!({ "event": "authenticate", "header": basic("alex", "wrong") }).to_string()))
            .await
            .unwrap();
        match tokio::time::timeout(Duration::from_secs(3), c.next()).await {
            Ok(Some(Ok(Ws::Close(Some(frame))))) => assert_eq!(u16::from(frame.code), 4004),
            other => panic!("expected a close, got {other:?}"),
        }
    });

    // Switching it off stops the server.
    let _ = s.update(Message::Setting(Setting::Integrations(I::RemoteEnabled(false))));
    assert!(s.integ.remote_addr().is_none());
    rt.block_on(async {
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(tokio::net::TcpStream::connect(addr).await.is_err());
    });
}

#[test]
fn demo_mode_never_listens() {
    let (mut s, _) = Listen::boot(&Args { demo: true, ephemeral: true, ..Args::default() });
    s.config.settings.integrations.remote.enabled = true;
    let rt = tokio::runtime::Runtime::new().unwrap();
    let task = s.update(Message::Setting(Setting::Integrations(I::RemoteEnabled(true))));
    assert!(run(&rt, task).is_empty());
    assert!(s.integ.remote_addr().is_none(), "demo mode opens no port");
}

fn page_has(s: &Listen, text: &str) -> bool {
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings(), iced::Size::new(1600.0, 4000.0), s.view());
    ui.find(text).is_ok()
}

#[test]
fn settings_pages_show_the_new_rows() {
    let (mut s, _) = Listen::boot(&Args { demo: true, ephemeral: true, ..Args::default() });
    let _ = s.update(Message::Navigate(listen::app::Route::Settings));
    // General: the scrobble section.
    for t in [
        "Scrobble minimum mode",
        "Minimum scrobble percentage",
        "Minimum scrobble seconds",
        "Song change notifications",
    ] {
        assert!(page_has(&s, t), "{t}");
    }
    set(&mut s, I::ScrobbleMode(ScrobbleMode::Seconds));
    assert!(!page_has(&s, "Minimum scrobble percentage"));
    assert!(page_has(&s, "Minimum scrobble seconds"));
    set(&mut s, I::ScrobbleMode(ScrobbleMode::Percentage));
    assert!(page_has(&s, "Minimum scrobble percentage"));
    assert!(!page_has(&s, "Minimum scrobble seconds"));
    // Window: Discord and the remote server.
    let _ = s.update(Message::Shell(listen::app::settings_shell::ShellMsg::Tab(SettingsTab::Window)));
    for t in [
        "Private mode",
        "Discord application ID",
        "Show presence when paused",
        "Show playing icon",
        "Show status as listening",
        "Discord display type",
        "Discord link type",
        "Show server cover image",
        "Enable remote control server",
        "Remote server port",
        "Remote server address",
        "Remote server username",
        "Remote server password",
    ] {
        assert!(page_has(&s, t), "{t}");
    }
}

#[test]
fn the_discord_presence_follows_the_settings_pause_and_private_mode() {
    use listen::discord::{Kind, StatusLine};
    let mut s = player();
    s.config.settings.window.discord_rpc = true;
    s.playback.playing = true;
    s.playback.position = Duration::from_secs(20);
    let _ = s.update(Message::Noop);
    let p = s.integ.discord_presence().expect("a presence while playing");
    let first = &demo::album_songs(0)[0];
    assert_eq!(p.details.trim_end(), first.title);
    assert_eq!((p.kind, p.status), (Kind::Playing, StatusLine::Name));
    assert_eq!(p.small_image, Some("playing"));
    assert!(p.started_at.is_some());

    set(&mut s, I::DiscordListening(true));
    set(&mut s, I::DiscordDisplay(DiscordDisplay::Artist));
    set(&mut s, I::DiscordLink(DiscordLink::LastFm));
    let _ = s.update(Message::Noop);
    let p = s.integ.discord_presence().unwrap();
    assert_eq!((p.kind, p.status), (Kind::Listening, StatusLine::State));
    assert!(p.details_url.as_deref().is_some_and(|u| u.starts_with("https://www.last.fm/music/")));

    // Paused: the paused icon and no timer; or nothing with the option off.
    s.playback.playing = false;
    let _ = s.update(Message::Noop);
    let p = s.integ.discord_presence().unwrap();
    assert_eq!((p.small_image, p.started_at), (Some("paused"), None));
    set(&mut s, I::DiscordPaused(false));
    let _ = s.update(Message::Noop);
    assert!(s.integ.discord_presence().is_none());
    set(&mut s, I::DiscordPaused(true));
    s.playback.playing = true;
    let _ = s.update(Message::Noop);
    assert!(s.integ.discord_presence().is_some());

    // Private mode clears it, and the end of private mode brings it back.
    let _ = s.update(Message::Integrations(IntegrationsMsg::SetPrivate(true)));
    let _ = s.update(Message::Noop);
    assert!(s.integ.discord_presence().is_none());
    let _ = s.update(Message::Integrations(IntegrationsMsg::SetPrivate(false)));
    let _ = s.update(Message::Noop);
    assert!(s.integ.discord_presence().is_some());
    // Switched off: cleared.
    s.config.settings.window.discord_rpc = false;
    let _ = s.update(Message::Noop);
    assert!(s.integ.discord_presence().is_none());
}
