//! Item details, share links, the tag editor, saving files and their menu
//! entries: the flows through `Listen::update` with fake servers and a
//! real local folder, plus Simulator clicks on the modals.

use async_trait::async_trait;
use iced::Point;
use iced_test::simulator::Simulator;
use listen::app::item::ItemMsg;
use listen::app::item::details::{DetailsMsg, Load};
use listen::app::item::save::SaveMsg;
use listen::app::item::share::ShareMsg;
use listen::app::item::tags::{Step, TagsMsg};
use listen::app::menu::{ContextMenu, MenuAction, MenuMsg, MenuTarget};
use listen::app::{INTER, Listen, Message, SourceHandle};
use listen::{Args, FONTS, demo};
use listen_core::config::ServerConfig;
use listen_core::model::{
    Album, Artist, ImageRef, Page, Playlist, SearchResults, ServerId, Song, SourceKind,
};
use listen_core::settings::ExpiryUnit;
use listen_local::tags::{self as lofty_tags, TagField};
use listen_source::{
    AlbumQuery, Capabilities, ItemDetails, ItemKind, ListQuery, MusicSource, Result, Share, SongQuery,
    SourceError, StreamSource,
};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

static GPU: Mutex<()> = Mutex::new(());

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

type ShareCall = (ItemKind, Vec<String>, Option<String>, Option<SystemTime>);

#[derive(Default, Debug)]
struct Calls {
    shares: Vec<ShareCall>,
    details: Vec<(ItemKind, String)>,
}

struct Fake {
    kind: SourceKind,
    caps: Capabilities,
    calls: Arc<Mutex<Calls>>,
    download: Option<String>,
}

#[async_trait]
impl MusicSource for Fake {
    fn kind(&self) -> SourceKind {
        self.kind
    }
    fn capabilities(&self) -> Capabilities {
        self.caps
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
        Err(SourceError::Unsupported)
    }
    fn image_url(&self, _: &ImageRef, _: u32) -> Option<String> {
        None
    }
    fn share_url(&self, song: &Song) -> Option<String> {
        (self.kind == SourceKind::YouTubeMusic)
            .then(|| format!("https://music.youtube.com/watch?v={}", song.id))
    }
    fn download_url(&self, song: &Song) -> Option<String> {
        self.download.as_ref().map(|base| format!("{base}{}", song.id))
    }
    async fn item_details(&self, kind: ItemKind, id: &str) -> Result<ItemDetails> {
        self.calls.lock().unwrap().details.push((kind, id.to_string()));
        Ok(ItemDetails {
            path: Some("Artist/Album/01.flac".into()),
            bit_rate: Some(912),
            bit_depth: Some(24),
            play_count: Some(7),
            ..Default::default()
        })
    }
    async fn create_share(
        &self,
        kind: ItemKind,
        ids: &[String],
        description: Option<&str>,
        expires: Option<SystemTime>,
    ) -> Result<Share> {
        self.calls.lock().unwrap().shares.push((kind, ids.to_vec(), description.map(String::from), expires));
        Ok(Share {
            id: "sh1".into(),
            url: "https://music.example.com/share/sh1".into(),
            description: None,
            expires,
        })
    }
}

fn boot(source: impl MusicSource + 'static, kind: SourceKind) -> Listen {
    let (mut s, _) =
        Listen::boot(&Args { ephemeral: true, theme: Some("tokyoNight".into()), ..Args::default() });
    let id = ServerId("srv".into());
    s.config.servers = vec![ServerConfig {
        id: id.clone(),
        name: "srv".into(),
        kind,
        url: "https://x.example".into(),
        username: "me".into(),
    }];
    let _ = s.update(Message::Connected(id, Ok(SourceHandle(Arc::new(source)))));
    let _ = s.update(Message::WindowResized(iced::Size::new(1200.0, 800.0)));
    s
}

fn app(kind: SourceKind, caps: Capabilities) -> (Listen, Arc<Mutex<Calls>>) {
    app_with(kind, caps, None)
}

fn app_with(kind: SourceKind, caps: Capabilities, download: Option<String>) -> (Listen, Arc<Mutex<Calls>>) {
    let calls = Arc::new(Mutex::new(Calls::default()));
    let s = boot(Fake { kind, caps, calls: calls.clone(), download }, kind);
    (s, calls)
}

fn songs_of(kind: SourceKind, n: usize) -> Vec<Song> {
    demo::album_songs(0)
        .into_iter()
        .take(n)
        .map(|mut s| {
            s.server_id = ServerId("srv".into());
            s.source = kind;
            s
        })
        .collect()
}

/// Runs a task to completion and returns the messages it produced.
fn run(task: iced::Task<Message>) -> Vec<Message> {
    use iced::futures::StreamExt;
    let Some(mut stream) = iced_runtime::task::into_stream(task) else { return vec![] };
    tokio::runtime::Runtime::new().unwrap().block_on(async move {
        let mut out = Vec::new();
        while let Some(action) = stream.next().await {
            if let iced_runtime::Action::Output(m) = action {
                out.push(m);
            }
        }
        out
    })
}

/// Sends `msg` and feeds everything the tasks answer back in, until quiet.
fn send(s: &mut Listen, msg: Message) {
    let mut pending = std::collections::VecDeque::from([msg]);
    while let Some(m) = pending.pop_front() {
        pending.extend(run(s.update(m)));
    }
}

fn do_action(s: &mut Listen, target: MenuTarget, action: MenuAction) {
    send(s, Message::Menu(MenuMsg::Open(target)));
    send(s, Message::Menu(MenuMsg::Do(action)));
}

fn labels(s: &Listen, target: MenuTarget) -> Vec<String> {
    let menu = ContextMenu::new(target, Point::new(100.0, 100.0), None);
    listen::app::menu::entries(s, &menu).into_iter().map(|e| e.label).filter(|l| !l.is_empty()).collect()
}

fn has(labels: &[String], l: &str) -> bool {
    labels.iter().any(|x| x == l)
}

fn modal_key(s: &Listen) -> Option<&'static str> {
    match &s.overlay.modal {
        Some(listen::app::overlay::Modal::Custom(k)) => Some(k),
        _ => None,
    }
}

fn toasts(s: &Listen) -> Vec<String> {
    s.overlay.toasts.iter().map(|t| t.message.clone()).collect()
}

// --- menu entries per source ---

#[test]
fn entries_follow_what_the_source_can_do() {
    let (nd, _) = app(SourceKind::Subsonic, listen_subsonic::capabilities());
    let songs = songs_of(SourceKind::Subsonic, 1);
    let l = labels(&nd, MenuTarget::songs(&songs));
    assert!(has(&l, "Get info") && has(&l, "Share…") && has(&l, "Save file…"), "{l:?}");
    assert!(!has(&l, "Edit metadata…"), "servers have no tag editor: {l:?}");
    assert!(!has(&l, "Show in file manager"), "{l:?}");
    let album = demo::albums().remove(0);
    let l = labels(&nd, MenuTarget::album(&album));
    assert!(has(&l, "Get info") && has(&l, "Share…") && has(&l, "Save file…"), "{l:?}");

    // A server without shares has no Share entry.
    let (plain, _) = app(SourceKind::Subsonic, listen_subsonic::capabilities() - Capabilities::SHARES);
    assert!(!has(&labels(&plain, MenuTarget::songs(&songs)), "Share…"));

    // YouTube Music: details and its own link, no file saving, no downloads, no tag editor.
    let (ytm, _) = app(SourceKind::YouTubeMusic, listen_ytmusic::capabilities(false));
    let ysongs = songs_of(SourceKind::YouTubeMusic, 1);
    let l = labels(&ytm, MenuTarget::songs(&ysongs));
    assert!(has(&l, "Get info") && has(&l, "Copy link") && !has(&l, "Download"), "{l:?}");
    assert!(!has(&l, "Share…") && !has(&l, "Save file…") && !has(&l, "Edit metadata…"), "{l:?}");
    let l = labels(&ytm, MenuTarget::album(&demo::ytm_album()));
    assert!(has(&l, "Copy link") && has(&l, "Get info") && !has(&l, "Save file…"), "{l:?}");

    // Genres have nothing of it.
    let genre = listen_core::model::Genre {
        id: "g".into(),
        name: "Rock".into(),
        album_count: None,
        song_count: None,
    };
    let l = labels(&nd, MenuTarget::genre(&genre));
    assert!(!has(&l, "Get info") && !has(&l, "Share…") && !has(&l, "Save file…"), "{l:?}");
}

// --- details ---

#[test]
fn get_info_shows_the_model_then_the_servers_facts() {
    let (mut s, calls) = app(SourceKind::Subsonic, listen_subsonic::capabilities());
    let songs = songs_of(SourceKind::Subsonic, 2);
    do_action(&mut s, MenuTarget::songs(&songs), MenuAction::GetInfo);
    assert_eq!(modal_key(&s), Some("item-details"));
    let d = s.item.details.as_ref().unwrap();
    assert_eq!(d.entries.len(), 2);
    assert!(matches!(d.entries[0].load, Load::Done));
    let rows = &d.entries[0].rows;
    assert!(rows.iter().any(|r| r.label == "Bitrate" && r.value == "912 kbps"), "{rows:?}");
    assert!(rows.iter().any(|r| r.label == "Path" && r.value == "Artist/Album/01.flac"));
    assert!(rows.iter().any(|r| r.label == "Title" && r.value == songs[0].title));
    assert_eq!(calls.lock().unwrap().details, [(ItemKind::Song, songs[0].id.clone())]);

    // The second item is asked for when it is shown.
    send(&mut s, Message::Item(ItemMsg::Details(DetailsMsg::Select(1))));
    assert_eq!(calls.lock().unwrap().details.len(), 2);
    assert_eq!(s.item.details.as_ref().unwrap().selected, 1);

    {
        let _gpu = gpu();
        let ui = Simulator::with_size(settings(), iced::Size::new(1200.0, 800.0), s.view());
        let mut ui = ui;
        assert!(ui.find("Bitrate").is_ok() && ui.find("912 kbps").is_ok());
        assert!(ui.find("2 of 2").is_ok());
    }
}

#[test]
fn get_info_works_without_server_details() {
    let (mut s, calls) = app(SourceKind::YouTubeMusic, Capabilities::empty());
    let songs = songs_of(SourceKind::YouTubeMusic, 1);
    do_action(&mut s, MenuTarget::songs(&songs), MenuAction::GetInfo);
    assert!(calls.lock().unwrap().details.is_empty());
    let d = s.item.details.as_ref().unwrap();
    assert!(d.entries[0].rows.iter().any(|r| r.label == "Title"));
    // Playlists are described by their own model.
    let (mut s, _) = app(SourceKind::Subsonic, listen_subsonic::capabilities());
    let p = Playlist {
        id: "p".into(),
        server_id: ServerId("srv".into()),
        source: SourceKind::Subsonic,
        name: "Mix".into(),
        description: Some("d".into()),
        owner: Some("me".into()),
        image: None,
        song_count: Some(3),
        duration: Duration::from_secs(600),
        public: false,
        editable: true,
    };
    do_action(&mut s, MenuTarget::playlist(&p), MenuAction::GetInfo);
    let rows = &s.item.details.as_ref().unwrap().entries[0].rows;
    assert!(rows.iter().any(|r| r.label == "Owner" && r.value == "me"));
}

// --- sharing ---

#[test]
fn share_creates_a_link_with_the_chosen_expiry_and_description() {
    let (mut s, calls) = app(SourceKind::Subsonic, listen_subsonic::capabilities());
    let songs = songs_of(SourceKind::Subsonic, 2);
    let ids: Vec<String> = songs.iter().map(|x| x.id.clone()).collect();
    do_action(&mut s, MenuTarget::songs(&songs), MenuAction::ShareItem);
    assert_eq!(modal_key(&s), Some("item-share"));
    // The default (one year) is preselected.
    let st = s.item.share.as_ref().unwrap();
    assert_eq!((st.expiry.unit, st.amount.as_str()), (ExpiryUnit::Year, "1"));

    send(&mut s, Message::Item(ItemMsg::Share(ShareMsg::Preset(Some((1, ExpiryUnit::Week))))));
    send(&mut s, Message::Item(ItemMsg::Share(ShareMsg::Description("for Anna".into()))));
    let before = SystemTime::now();
    send(&mut s, Message::Item(ItemMsg::Share(ShareMsg::Submit)));
    let c = calls.lock().unwrap();
    assert_eq!(c.shares.len(), 1);
    let (kind, got_ids, description, expires) = &c.shares[0];
    assert_eq!((*kind, got_ids, description.as_deref()), (ItemKind::Song, &ids, Some("for Anna")));
    let wait = expires.unwrap().duration_since(before).unwrap();
    assert!(
        wait > Duration::from_secs(7 * 86_400 - 5) && wait < Duration::from_secs(7 * 86_400 + 5),
        "{wait:?}"
    );
    drop(c);
    assert!(modal_key(&s).is_none(), "the modal closes");
    assert!(toasts(&s).iter().any(|t| t.contains("Share link copied")), "{:?}", toasts(&s));
}

#[test]
fn share_with_the_server_default_sends_no_expiry_and_a_bad_amount_is_refused() {
    let (mut s, calls) = app(SourceKind::Subsonic, listen_subsonic::capabilities());
    let songs = songs_of(SourceKind::Subsonic, 1);
    do_action(&mut s, MenuTarget::songs(&songs), MenuAction::ShareItem);
    send(&mut s, Message::Item(ItemMsg::Share(ShareMsg::Amount("0".into()))));
    send(&mut s, Message::Item(ItemMsg::Share(ShareMsg::Submit)));
    assert!(calls.lock().unwrap().shares.is_empty());
    assert!(s.item.share.as_ref().unwrap().error.is_some());

    send(&mut s, Message::Item(ItemMsg::Share(ShareMsg::Preset(None))));
    send(&mut s, Message::Item(ItemMsg::Share(ShareMsg::Submit)));
    let c = calls.lock().unwrap();
    assert_eq!(c.shares.len(), 1);
    assert!(c.shares[0].2.is_none() && c.shares[0].3.is_none());
}

#[test]
fn the_default_expiry_can_be_kept_and_starts_the_next_share() {
    let (mut s, _) = app(SourceKind::Subsonic, listen_subsonic::capabilities());
    let songs = songs_of(SourceKind::Subsonic, 1);
    do_action(&mut s, MenuTarget::songs(&songs), MenuAction::ShareItem);
    send(&mut s, Message::Item(ItemMsg::Share(ShareMsg::Preset(Some((1, ExpiryUnit::Month))))));
    send(&mut s, Message::Item(ItemMsg::Share(ShareMsg::MakeDefault)));
    assert_eq!(s.config.settings.sharing.expiration.unit, ExpiryUnit::Month);
    send(&mut s, Message::Overlay(listen::app::overlay::OverlayMsg::CloseModal));
    do_action(&mut s, MenuTarget::songs(&songs), MenuAction::ShareItem);
    assert_eq!(s.item.share.as_ref().unwrap().expiry.unit, ExpiryUnit::Month);
}

#[test]
fn youtube_music_share_copies_the_link() {
    let (mut s, _) = app(SourceKind::YouTubeMusic, listen_ytmusic::capabilities(false));
    let album = demo::ytm_album();
    do_action(&mut s, MenuTarget::album(&album), MenuAction::ShareItem);
    assert!(modal_key(&s).is_none());
    assert!(toasts(&s).contains(&"Link copied.".to_string()), "{:?}", toasts(&s));
}

// --- saving the original file ---

fn file_server(body: &'static [u8]) -> String {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for sock in listener.incoming() {
            let Ok(mut sock) = sock else { return };
            let mut buf = [0u8; 2048];
            let _ = sock.read(&mut buf);
            let head =
                format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            let _ = sock.write_all(head.as_bytes());
            let _ = sock.write_all(body);
        }
    });
    base
}

#[test]
fn save_file_writes_the_original_next_to_a_progress_toast() {
    let base = file_server(b"FLACDATA-FLACDATA");
    let (mut s, _) = app_with(SourceKind::Subsonic, listen_subsonic::capabilities(), Some(base));
    let dir = std::env::temp_dir().join(format!("listen-item-save-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    s.item.save.dir = Some(dir.clone());
    let mut songs = songs_of(SourceKind::Subsonic, 1);
    songs[0].path = Some("A/B/01 Track.flac".into());
    do_action(&mut s, MenuTarget::songs(&songs), MenuAction::SaveFile);
    // Done: the progress toast is gone, a success toast offers the folder.
    assert!(s.item.save.active.is_none());
    assert_eq!(std::fs::read(dir.join("01 Track.flac")).unwrap(), b"FLACDATA-FLACDATA");
    assert!(toasts(&s).iter().any(|t| t.contains("Saved “01 Track.flac”")), "{:?}", toasts(&s));
    assert!(!toasts(&s).iter().any(|t| t.starts_with("Downloading")));

    // Once more: the name is not taken over.
    do_action(&mut s, MenuTarget::songs(&songs), MenuAction::SaveFile);
    assert!(dir.join("01 Track (2).flac").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn save_file_reports_a_failing_download() {
    let (mut s, _) =
        app_with(SourceKind::Subsonic, listen_subsonic::capabilities(), Some("http://127.0.0.1:9/".into()));
    s.item.save.dir = Some(std::env::temp_dir().join("listen-item-save-fail"));
    let songs = songs_of(SourceKind::Subsonic, 1);
    do_action(&mut s, MenuTarget::songs(&songs), MenuAction::SaveFile);
    assert!(s.item.save.active.is_none());
    assert!(toasts(&s).iter().any(|t| t.starts_with("Could not download “")), "{:?}", toasts(&s));
}

#[test]
fn youtube_music_keeps_only_its_offline_download() {
    let (mut s, _) = app_with(SourceKind::YouTubeMusic, listen_ytmusic::capabilities(false), None);
    let songs = songs_of(SourceKind::YouTubeMusic, 1);
    do_action(&mut s, MenuTarget::songs(&songs), MenuAction::SaveFile);
    assert!(s.item.save.active.is_none());
    assert!(toasts(&s).iter().any(|t| t.contains("can't be downloaded")), "{:?}", toasts(&s));
}

// --- the tag editor on a real local folder ---

fn mp3() -> Vec<u8> {
    let mut out = Vec::new();
    for _ in 0..12 {
        out.extend_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);
        out.extend(std::iter::repeat_n(0u8, 413));
    }
    out
}

fn local_app(files: &[&str]) -> (Listen, std::path::PathBuf) {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("listen-item-tags-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for f in files {
        std::fs::write(dir.join(f), mp3()).unwrap();
    }
    let folder = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(listen_local::LocalFolder::connect(ServerId("srv".into()), dir.to_str().unwrap()))
        .unwrap();
    let s = boot(folder, SourceKind::Local);
    (s, dir)
}

fn local_songs(s: &Listen) -> Vec<Song> {
    let source = s.sources.get(&ServerId("srv".into())).unwrap().clone();
    let page = tokio::runtime::Runtime::new().unwrap().block_on(source.songs(&SongQuery::default())).unwrap();
    page.items
}

#[test]
fn local_songs_get_the_tag_editor_and_show_in_file_manager() {
    let (s, dir) = local_app(&["a.mp3", "b.mp3"]);
    let songs = local_songs(&s);
    assert_eq!(songs.len(), 2);
    let l = labels(&s, MenuTarget::songs(&songs[..1]));
    assert!(has(&l, "Edit metadata…") && has(&l, "Show in file manager") && has(&l, "Get info"), "{l:?}");
    assert!(!has(&l, "Share…") && !has(&l, "Save file…"), "{l:?}");
    let l = labels(&s, MenuTarget::songs(&songs));
    assert!(has(&l, "Edit metadata…") && !has(&l, "Show in file manager"), "{l:?}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_tag_editor_previews_and_writes_many_files() {
    let (mut s, dir) = local_app(&["a.mp3", "b.mp3"]);
    let songs = local_songs(&s);
    // One file already has an album.
    lofty_tags::write(
        &dir.join("a.mp3"),
        &lofty_tags::TagEdit {
            fields: [(TagField::Album, "Old".to_string())].into_iter().collect(),
            cover: Default::default(),
        },
    )
    .unwrap();

    do_action(&mut s, MenuTarget::songs(&songs), MenuAction::EditTags);
    assert_eq!(modal_key(&s), Some("item-tags"));
    let t = s.item.tags.as_ref().unwrap();
    assert!(t.before.is_some());
    assert!(t.fields[&TagField::Album].mixed, "a has an album, b has none");

    // Nothing changed yet: no preview.
    send(&mut s, Message::Item(ItemMsg::Tags(TagsMsg::Preview)));
    assert_eq!(s.item.tags.as_ref().unwrap().step, Step::Edit);
    assert!(s.item.tags.as_ref().unwrap().error.is_some());

    send(&mut s, Message::Item(ItemMsg::Tags(TagsMsg::Field(TagField::Album, "Greatest".into()))));
    send(&mut s, Message::Item(ItemMsg::Tags(TagsMsg::Field(TagField::Year, "19x9".into()))));
    assert_eq!(s.item.tags.as_ref().unwrap().fields[&TagField::Year].value, "199", "non-digits are dropped");
    send(&mut s, Message::Item(ItemMsg::Tags(TagsMsg::Field(TagField::Year, "1999".into()))));
    send(&mut s, Message::Item(ItemMsg::Tags(TagsMsg::Preview)));
    let t = s.item.tags.as_ref().unwrap();
    assert_eq!(t.step, Step::Preview);
    let rows = t.preview();
    assert_eq!(rows.len(), 2);
    assert_eq!((rows[0].field.as_str(), rows[0].files), ("Album", 2));
    assert_eq!((rows[1].field.as_str(), rows[1].new.as_str()), ("Year", "1999"));

    send(&mut s, Message::Item(ItemMsg::Tags(TagsMsg::Write)));
    assert!(s.item.tags.is_none(), "closed after the last file");
    assert!(modal_key(&s).is_none());
    for f in ["a.mp3", "b.mp3"] {
        let t = lofty_tags::read(&dir.join(f)).unwrap();
        assert_eq!((t.get(TagField::Album), t.get(TagField::Year)), ("Greatest", "1999"), "{f}");
    }
    assert!(toasts(&s).iter().any(|t| t == "Tags written to 2 files."), "{:?}", toasts(&s));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn servers_never_open_the_tag_editor() {
    let (mut s, _) = app(SourceKind::Subsonic, listen_subsonic::capabilities());
    let songs = songs_of(SourceKind::Subsonic, 1);
    send(&mut s, Message::Menu(MenuMsg::Open(MenuTarget::songs(&songs))));
    send(&mut s, Message::Menu(MenuMsg::Do(MenuAction::EditTags)));
    assert!(s.item.tags.is_none() && modal_key(&s).is_none());
}

// --- clicking the modals ---

fn click_text(s: &Listen, text: &str) -> Vec<String> {
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings(), iced::Size::new(1200.0, 800.0), s.view());
    ui.click(text).unwrap_or_else(|_| panic!("{text} not found"));
    ui.into_messages().map(|m| format!("{m:?}")).collect()
}

#[test]
fn the_modals_react_to_clicks() {
    let (mut s, _) = app(SourceKind::Subsonic, listen_subsonic::capabilities());
    let songs = songs_of(SourceKind::Subsonic, 1);
    do_action(&mut s, MenuTarget::songs(&songs), MenuAction::GetInfo);
    let m = click_text(&s, "Close");
    assert!(m.iter().any(|m| m.contains("CloseModal")), "{m:?}");

    send(&mut s, Message::Overlay(listen::app::overlay::OverlayMsg::CloseModal));
    do_action(&mut s, MenuTarget::songs(&songs), MenuAction::ShareItem);
    let m = click_text(&s, "1 week");
    assert!(m.iter().any(|m| m.contains("Preset(Some((1, Week)))")), "{m:?}");
    let m = click_text(&s, "Share");
    assert!(m.iter().any(|m| m.contains("Submit")), "{m:?}");
    let m = click_text(&s, "Server default");
    assert!(m.iter().any(|m| m.contains("Preset(None)")), "{m:?}");
}

#[test]
fn the_tag_editor_buttons_click() {
    let (mut s, dir) = local_app(&["c.mp3"]);
    let songs = local_songs(&s);
    do_action(&mut s, MenuTarget::songs(&songs), MenuAction::EditTags);
    let m = click_text(&s, "Preview changes");
    assert!(m.iter().any(|m| m.contains("Preview")), "{m:?}");
    let _ = std::fs::remove_dir_all(dir);
    let _ = SaveMsg::Finished;
}
