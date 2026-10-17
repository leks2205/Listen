//! DLNA casting against a fake renderer over loopback (no real device): the
//! connect / play / pause / seek / volume / end-of-song flow, YouTube Music
//! served from its local copy, the DJ features sitting out, and the popover.

use async_trait::async_trait;
use iced::Task;
use iced_test::simulator::Simulator;
use listen::app::dlna::DlnaMsg;
use listen::app::{INTER, Listen, Message, SourceHandle};
use listen::{Args, FONTS, demo};
use listen_core::config::ServerConfig;
use listen_core::model::{
    Album, Artist, ImageRef, Page, Playlist, SearchResults, ServerId, Song, SourceKind,
};
use listen_dlna::Renderer;
use listen_dlna::testing::FakeRenderer;
use listen_source::{
    AlbumQuery, Capabilities, ListQuery, MusicSource, Result, SongQuery, SourceError, StreamSource,
};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

static GPU: Mutex<()> = Mutex::new(());

fn gpu() -> std::sync::MutexGuard<'static, ()> {
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

/// One runtime for the fake renderer, the proxy and the app's tasks.
fn rt() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap()
    })
}

struct Src {
    kind: SourceKind,
    stream_url: String,
    headers: Vec<(String, String)>,
    local: Option<std::path::PathBuf>,
}

#[async_trait]
impl MusicSource for Src {
    fn kind(&self) -> SourceKind {
        self.kind
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::empty()
    }
    async fn ping(&self) -> Result<()> {
        Ok(())
    }
    async fn albums(&self, _: &AlbumQuery) -> Result<Page<Album>> {
        Err(SourceError::Unsupported)
    }
    async fn album(&self, _: &str) -> Result<Album> {
        Err(SourceError::Unsupported)
    }
    async fn artists(&self, _: &ListQuery) -> Result<Page<Artist>> {
        Err(SourceError::Unsupported)
    }
    async fn artist(&self, _: &str) -> Result<Artist> {
        Err(SourceError::Unsupported)
    }
    async fn artist_albums(&self, _: &str) -> Result<Vec<Album>> {
        Ok(vec![])
    }
    async fn top_songs(&self, _: &Artist, _: usize) -> Result<Vec<Song>> {
        Ok(vec![])
    }
    async fn songs(&self, _: &SongQuery) -> Result<Page<Song>> {
        Err(SourceError::Unsupported)
    }
    async fn random_songs(&self, _: usize, _: Option<&str>) -> Result<Vec<Song>> {
        Ok(vec![])
    }
    async fn search(&self, _: &str, _: usize) -> Result<SearchResults> {
        Ok(SearchResults::default())
    }
    async fn playlists(&self) -> Result<Vec<Playlist>> {
        Ok(vec![])
    }
    async fn playlist_songs(&self, _: &str) -> Result<Vec<Song>> {
        Ok(vec![])
    }
    async fn set_favorite(&self, _: &[String], _: bool) -> Result<()> {
        Ok(())
    }
    async fn stream(&self, _: &Song) -> Result<StreamSource> {
        if self.stream_url.is_empty() {
            return Err(SourceError::Other("no stream".into()));
        }
        Ok(StreamSource {
            url: self.stream_url.clone(),
            headers: self.headers.iter().cloned().collect(),
            expires_at: None,
        })
    }
    async fn local_copy(&self, _: &Song) -> Result<Option<std::path::PathBuf>> {
        Ok(self.local.clone())
    }
    fn image_url(&self, _: &ImageRef, _: u32) -> Option<String> {
        None
    }
}

fn songs(kind: SourceKind) -> Vec<Song> {
    demo::album_songs(0)
        .into_iter()
        .take(3)
        .map(|mut s| {
            s.server_id = ServerId("srv".into());
            s.source = kind;
            s
        })
        .collect()
}

fn app(src: Src) -> Listen {
    let (mut s, _) =
        Listen::boot(&Args { ephemeral: true, theme: Some("tokyoNight".into()), ..Args::default() });
    let id = ServerId("srv".into());
    s.config.servers = vec![ServerConfig {
        id: id.clone(),
        name: "srv".into(),
        kind: src.kind,
        url: "https://x.example".into(),
        username: "me".into(),
    }];
    s.queue.set(songs(src.kind), 0);
    let _ = s.update(Message::Connected(id, Ok(SourceHandle(Arc::new(src)))));
    s
}

fn run(task: Task<Message>) -> Vec<Message> {
    use iced::futures::StreamExt;
    let Some(mut stream) = iced_runtime::task::into_stream(task) else { return vec![] };
    rt().block_on(async move {
        let mut out = Vec::new();
        while let Some(action) = stream.next().await {
            if let iced_runtime::Action::Output(m) = action {
                out.push(m);
            }
        }
        out
    })
}

/// Sends `m`, then feeds the answers back.
fn settle(s: &mut Listen, m: Message) {
    let mut pending = run(s.update(m));
    for _ in 0..4 {
        let next: Vec<Message> = pending.drain(..).flat_map(|m| run(s.update(m))).collect();
        pending = next;
    }
}

async fn fake_and_renderer() -> (FakeRenderer, Renderer) {
    let fake = FakeRenderer::start().await;
    let r = Renderer::fetch(&fake.location).await.unwrap();
    (fake, r)
}

fn connected(src: Src) -> (Listen, FakeRenderer) {
    let mut s = app(src);
    let (fake, r) = rt().block_on(fake_and_renderer());
    s.dlna_found_for_test(vec![r]);
    // The song plays locally first.
    s.playback.playing = true;
    s.playback.position = Duration::from_secs(40);
    settle(&mut s, Message::Dlna(DlnaMsg::Connect(0)));
    (s, fake)
}

fn plain() -> Src {
    Src {
        kind: SourceKind::Subsonic,
        stream_url: "http://nas.lan/rest/stream?id=1".into(),
        headers: vec![],
        local: None,
    }
}

#[test]
fn casting_hands_the_song_over_and_the_transport_goes_to_the_device() {
    let (mut s, fake) = connected(plain());
    assert!(s.dlna_casting());
    assert_eq!(s.dlna_device(), Some("Fake Speaker"));
    // A plain http stream goes out directly; the song continues at its position.
    {
        let st = fake.state();
        assert_eq!(st.uri, "http://nas.lan/rest/stream?id=1");
        assert_eq!(st.state, "PLAYING");
        assert!(st.didl.contains("<dc:title>"));
    }
    let actions = fake.actions();
    assert!(
        actions.contains(&"SetAVTransportURI".to_string()) && actions.contains(&"Play".to_string()),
        "{actions:?}"
    );
    assert_eq!(fake.state().position, 40, "resumed where the local player was");
    assert!(s.playback.playing);

    // The player bar's volume is the device's; the local volume is kept for later.
    let local = s.config.settings.playback.volume;
    assert_eq!(s.playback.volume, 30, "the device's volume is shown");
    settle(&mut s, Message::Volume(55.0));
    assert_eq!(fake.state().volume, 55);
    assert_eq!(s.config.settings.playback.volume, local, "the saved local volume is not touched");
    settle(&mut s, Message::ToggleMute);
    assert!(fake.state().muted);
    settle(&mut s, Message::ToggleMute);
    assert!(!fake.state().muted);

    settle(&mut s, Message::TogglePlay);
    assert_eq!(fake.state().state, "PAUSED_PLAYBACK");
    assert!(!s.playback.playing);
    settle(&mut s, Message::TogglePlay);
    assert_eq!(fake.state().state, "PLAYING");
    settle(&mut s, Message::Seek(100.0));
    assert_eq!(fake.state().position, 100);

    // The poll brings the device's position back.
    fake.state().position = 123;
    settle(&mut s, Message::Dlna(DlnaMsg::Poll));
    assert_eq!(s.playback.position, Duration::from_secs(123));
    assert_eq!(s.playback.duration, Some(Duration::from_secs(205)));

    // The device ends the song (after a moment of playing): the queue moves on.
    std::thread::sleep(Duration::from_millis(2200));
    fake.state().state = "STOPPED";
    let first = s.queue.current().map(|i| i.uid);
    settle(&mut s, Message::Dlna(DlnaMsg::Poll));
    assert_ne!(s.queue.current().map(|i| i.uid), first, "advanced to the next song");
    assert_eq!(fake.actions().iter().filter(|a| *a == "SetAVTransportURI").count(), 2);
    assert_eq!(fake.state().state, "PLAYING");

    // Next / Previous load on the device too.
    settle(&mut s, Message::Next);
    assert_eq!(fake.actions().iter().filter(|a| *a == "SetAVTransportURI").count(), 3);

    // Back to this computer: the local volume returns and the device stops.
    settle(&mut s, Message::Dlna(DlnaMsg::Disconnect));
    assert!(!s.dlna_casting());
    assert_eq!(s.playback.volume, local);
    assert!(fake.actions().contains(&"Stop".to_string()));
}

#[test]
fn the_end_of_the_queue_stops_the_device() {
    let (mut s, fake) = connected(plain());
    s.queue.set(songs(SourceKind::Subsonic).into_iter().take(1).collect(), 0);
    settle(&mut s, Message::Stop);
    assert_eq!(fake.state().state, "STOPPED");
    assert!(!s.playback.playing);
    assert_eq!(s.playback.position, Duration::ZERO);
}

#[test]
fn https_and_headers_go_through_the_proxy_without_cookies() {
    let src = Src {
        kind: SourceKind::Subsonic,
        stream_url: "https://music.example/rest/stream?id=1".into(),
        headers: vec![("Cookie".into(), "SID=secret".into()), ("X-Key".into(), "k".into())],
        local: None,
    };
    let (_s, fake) = connected(src);
    let uri = fake.state().uri.clone();
    assert!(uri.starts_with("http://127.0.0.1:"), "{uri}");
    assert!(uri.contains("/s/"), "{uri}");
    assert!(!uri.contains("secret") && !uri.contains("music.example"), "{uri}");
}

#[test]
fn youtube_music_is_served_from_its_local_copy() {
    let dir = std::env::temp_dir().join(format!("listen-dlna-app-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("abc.m4a");
    let bytes: Vec<u8> = (0..70_000u32).map(|i| (i % 253) as u8).collect();
    std::fs::write(&file, &bytes).unwrap();
    let src = Src {
        kind: SourceKind::YouTubeMusic,
        // The resolved googlevideo address would expire and needs headers: it must not be used.
        stream_url: "https://rr1.googlevideo.com/videoplayback?expire=1".into(),
        headers: vec![],
        local: Some(file),
    };
    let (_s, fake) = connected(src);
    let uri = fake.state().uri.clone();
    assert!(uri.starts_with("http://127.0.0.1:") && uri.ends_with(".m4a"), "{uri}");
    assert!(fake.state().didl.contains("audio/mp4"), "{}", fake.state().didl);
    rt().block_on(async {
        let http = reqwest::Client::new();
        let whole = http.get(&uri).send().await.unwrap();
        assert_eq!(whole.status(), 200);
        assert_eq!(whole.bytes().await.unwrap().as_ref(), bytes.as_slice());
        let part = http.get(&uri).header("Range", "bytes=65000-").send().await.unwrap();
        assert_eq!(part.status(), 206);
        assert_eq!(part.bytes().await.unwrap().as_ref(), &bytes[65_000..]);
    });
}

#[test]
fn dj_transitions_and_voice_sit_out_while_casting() {
    let (mut s, _fake) = connected(plain());
    s.config.settings.mixing.enabled = true;
    s.config.settings.auto_dj.enabled = true;
    assert!(!s.mixing_possible(), "no DJ transitions on a renderer");
    settle(&mut s, Message::Dlna(DlnaMsg::Disconnect));
    assert!(s.mixing_possible(), "they are back on this computer");
}

#[test]
fn a_device_that_vanishes_hands_playback_back() {
    let (mut s, _fake) = connected(plain());
    // The device stops answering: after a few failed polls the app plays on.
    for _ in 0..5 {
        let _ = s.dlna_poll_failed_for_test();
    }
    assert!(!s.dlna_casting());
}

fn settings() -> iced::Settings {
    iced::Settings {
        fonts: FONTS.iter().map(|f| std::borrow::Cow::Borrowed(*f)).collect(),
        default_font: INTER,
        ..Default::default()
    }
}

fn has_text(s: &Listen, text: &str) -> bool {
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings(), s.window_size, s.view());
    ui.find(text).is_ok()
}

fn demo_state() -> Listen {
    let (mut s, _) = Listen::boot(&Args { demo: true, theme: Some("tokyoNight".into()), ..Args::default() });
    s.queue.set(demo::album_songs(0), 0);
    s
}

#[test]
fn the_cast_button_shows_only_when_devices_were_found() {
    let mut s = demo_state();
    assert!(!s.dlna_visible());
    let (_fake, r) = rt().block_on(fake_and_renderer());
    s.dlna_found_for_test(vec![r]);
    assert!(s.dlna_visible());
    s.config.settings.playback.dlna_enabled = false;
    assert!(!s.dlna_visible(), "the setting hides it");
}

#[test]
fn the_popover_lists_this_computer_and_the_devices() {
    let mut s = demo_state();
    let (_fake, r) = rt().block_on(fake_and_renderer());
    s.dlna_found_for_test(vec![r]);
    assert!(!has_text(&s, "This computer"));
    let _ = s.update(Message::Dlna(DlnaMsg::Close));
    s.dlna.open = true;
    assert!(has_text(&s, "This computer"));
    assert!(has_text(&s, "Fake Speaker"));
    assert!(has_text(&s, "Refresh"));
}

fn snapshot(name: &str, s: &Listen) {
    let theme = s.theme();
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings(), iced::Size::new(1600.0, 950.0), s.view());
    let snap = ui.snapshot(&theme).expect("snapshot renders");
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/snapshots");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    for entry in std::fs::read_dir(&dir).unwrap().flatten() {
        if entry.file_name().to_string_lossy().starts_with(&format!("{name}-")) {
            std::fs::remove_file(entry.path()).unwrap();
        }
    }
    assert!(snap.matches_image(&path).expect("png written"));
}

#[test]
fn snapshots_of_the_popover() {
    let mut s = demo_state();
    let (_fake, r) = rt().block_on(fake_and_renderer());
    let other =
        Renderer::new("Kitchen TV", "uuid:tv", url::Url::parse("http://127.0.0.1:9/avt").unwrap(), None);
    s.dlna_found_for_test(vec![r, other]);
    s.dlna.open = true;
    snapshot("dlna-popover", &s);

    let (mut c, _fake) = connected(plain());
    c.dlna.open = true;
    snapshot("dlna-popover-casting", &c);
}
