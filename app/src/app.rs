//! Application state, messages and the update loop.

use crate::images::{self, Images};
#[cfg(any(target_os = "linux", windows))]
use crate::mpris::{self, MprisCmd, MprisHandle, MprisMsg, MprisState};
use crate::playback::{self, PlayerConfig, PlayerMsg};
use crate::secrets::Secrets;
use crate::style::{ShadeChoice, Tokens, metrics};
use crate::ui;
use crate::{Args, demo};
use iced::theme::Mode;
use iced::widget::{column, container, row, scrollable};
use iced::{Element, Font, Length, Subscription, Task, Theme, window};
use listen_core::config::{Config, Paths, ServerConfig};
use listen_core::model::{
    Album, Artist, ImageRef, Page, Playlist, SearchResults, ServerId, Song, SourceKind,
};
use listen_core::queue::{Queue, SavedQueue};
use listen_core::theme;
use listen_player::{Player, PlayerEvent, Track};
use listen_source::{AlbumQuery, AlbumSort, HomeShelf, ListQuery, MusicSource, ScrobbleEvent, SongQuery};
use std::sync::Arc;
use std::time::Duration;

pub mod advanced;
pub mod album_page;
pub mod analysis;
pub mod appearance;
pub mod artist_page;
pub mod audio;
pub mod decks;
mod dj;
mod dj_mix;
pub mod dlna;
mod fade;
mod flow;
pub mod fullscreen_state;
pub mod helpers;
pub mod history;
pub mod home;
pub mod hotkeys;
pub mod integrations;
pub mod item;
pub mod library_extra;
pub mod list;
pub mod list_query;
pub mod lyrics_state;
pub mod mcp;
pub mod menu;
pub mod mix;
pub mod offline;
pub mod onboarding;
pub mod overlay;
pub mod palette;
pub mod play_mode;
pub mod playlists;
pub mod queue_ui;
pub mod radio;
pub mod rip;
pub mod sidebar_state;
pub use settings_msg::Setting;
pub use window_state::{WindowSetting, WindowStateMsg};
pub mod milkdrop;
pub mod search;
pub mod servers;
mod settings_msg;
pub mod settings_shell;
pub mod sleep_timer;
pub mod smart_playlist;
pub mod substitute;
mod transport;
pub mod updater;
mod video;
pub mod viz;
mod window_state;

pub const INTER: Font = Font::with_name("Inter");
/// Artwork size requested from servers (Feishin's `itemCard` resolution).
pub const ART_SIZE: u32 = 300;
const ALBUM_PAGE: usize = 200;
const SONG_PAGE: usize = 200;

/// Pages, matching Feishin's routes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    Home,
    Favorites,
    Albums,
    Album(String),
    Artist(String),
    /// A page under an artist: discography, all tracks, top / favorite songs.
    ArtistList(String, artist_page::ArtistView),
    Playlist(String),
    Genre(String),
    /// Folder browser; `None` is the root (music folders / indexes).
    Folder(Option<String>),
    Tracks,
    AlbumArtists,
    Artists,
    Genres,
    Folders,
    Playlists,
    /// Songs downloaded for offline play (works without a server).
    Downloads,
    NowPlaying,
    Search,
    Settings,
    Recent,
    Stats,
    Mixes,
    Mix(listen_core::history::MixKind),
    /// Internet radio stations.
    Radio,
}

impl Route {
    /// Maps a Feishin sidebar item id to a route.
    pub fn from_sidebar_id(id: &str) -> Option<Self> {
        Some(match id {
            "Home" => Route::Home,
            "Favorites" => Route::Favorites,
            "Albums" => Route::Albums,
            "Tracks" => Route::Tracks,
            "Artists" => Route::AlbumArtists,
            "Artists-all" => Route::Artists,
            "Genres" => Route::Genres,
            "Folders" => Route::Folders,
            "Playlists" => Route::Playlists,
            "Now Playing" => Route::NowPlaying,
            "Search" => Route::Search,
            "Recently Played" => Route::Recent,
            "Statistics" => Route::Stats,
            "Mixes" => Route::Mixes,
            "Radio" => Route::Radio,
            "Settings" => Route::Settings,
            _ => return None,
        })
    }

    pub fn title(&self) -> &'static str {
        match self {
            Route::Home => "Home",
            Route::Favorites => "Favorites",
            Route::Albums | Route::Album(_) => "Albums",
            Route::Artist(_) | Route::ArtistList(..) => "Artist",
            Route::Genre(_) => "Genre",
            Route::Folder(_) => "Folders",
            Route::Playlist(_) => "Playlist",
            Route::Tracks => "Tracks",
            Route::AlbumArtists => "Album Artists",
            Route::Artists => "Artists",
            Route::Genres => "Genres",
            Route::Folders => "Folders",
            Route::Playlists => "Playlists",
            Route::Downloads => "Downloads",
            Route::NowPlaying => "Now Playing",
            Route::Search => "Search",
            Route::Settings => "Settings",
            Route::Recent => "Recently Played",
            Route::Stats => "Statistics",
            Route::Mixes | Route::Mix(_) => "Mixes",
            Route::Radio => "Radio",
        }
    }
}

/// What the player is doing (driven by mpv; simulated in demo mode).
#[derive(Debug, Clone)]
pub struct Playback {
    pub playing: bool,
    pub position: Duration,
    /// Duration reported by mpv, if any (more precise than the tag).
    pub duration: Option<Duration>,
    pub volume: u8,
    pub muted: bool,
    /// Current track has been scrobbled as played.
    scrobbled: bool,
    /// Time of the current track actually listened to (seeks do not count).
    played: Duration,
    /// Last reported position, to measure the step to the next report.
    last_pos: Option<Duration>,
}

impl Playback {
    /// A new track begins: nothing played yet, not scrobbled.
    pub(crate) fn reset_scrobble(&mut self) {
        self.scrobbled = false;
        self.played = Duration::ZERO;
        self.last_pos = None;
    }

    /// Adds the step to `pos` to the played time when it is a normal tick
    /// (forward, at most [`MAX_PLAY_STEP`]); seeks and rewinds add nothing.
    pub(crate) fn track_played(&mut self, pos: Duration) {
        if let Some(last) = self.last_pos
            && pos > last
            && pos - last <= MAX_PLAY_STEP
        {
            self.played += pos - last;
        }
        self.last_pos = Some(pos);
    }
}

/// Largest position step that still counts as listening (bigger ones are seeks).
const MAX_PLAY_STEP: Duration = Duration::from_secs(2);

/// A browser profile on this computer and whether its YouTube sign-in can be used.
pub type BrowserStatus = (listen_ytmusic::browser::Profile, Result<(), String>);

#[derive(Debug, Clone)]
pub struct LoginForm {
    pub kind: SourceKind,
    pub url: String,
    pub username: String,
    pub password: String,
    pub busy: bool,
    pub error: Option<String>,
    /// Confirmation shown after a cookies file was loaded (YouTube Music).
    pub info: Option<String>,
    /// YouTube Music: the signed-in browser profile found on this computer
    /// (None = still looking).
    pub browser: Option<Result<listen_ytmusic::browser::Profile, String>>,
    /// Every browser profile found, and whether it can be used (for "Use another browser").
    pub browsers: Vec<BrowserStatus>,
    /// Show the list of browsers.
    pub show_browsers: bool,
    /// The session's channels to choose from (after "Sign in with …").
    pub channels: Vec<listen_ytmusic::channels::Channel>,
    /// Show the cookie-file alternative.
    pub show_cookies: bool,
    /// Legacy authentication, ignore SSL, prefer instant mix (see `servers`).
    pub options: listen_core::config::ServerOptions,
}

impl Default for LoginForm {
    fn default() -> Self {
        Self {
            kind: SourceKind::Subsonic,
            url: String::new(),
            username: String::new(),
            password: String::new(),
            busy: false,
            error: None,
            info: None,
            browser: None,
            browsers: Vec::new(),
            show_browsers: false,
            channels: Vec::new(),
            show_cookies: false,
            options: Default::default(),
        }
    }
}

/// Custom window bar actions.
#[derive(Debug, Clone, Copy)]
pub enum WindowAction {
    Drag,
    ToggleMaximize,
    Minimize,
    Close,
    Resize(window::Direction),
    /// The desktop's window menu (KWin: move to desktop, keep above, …).
    SystemMenu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullscreenTab {
    Queue,
    /// Similar songs.
    Related,
    Lyrics,
    Visualizer,
    /// No panel: the cover is centred.
    Hidden,
}

/// All connected sources by server, so a queue can mix servers and keeps
/// playing while you browse another one.
pub type Sources = std::collections::HashMap<ServerId, Arc<dyn MusicSource>>;

/// Resolves a song's stream through the source it came from.
async fn stream_track(sources: &Sources, song: &Song) -> Result<Track, String> {
    if song.server_id.0 == "demo" {
        let id = song.id.clone();
        let path = tokio::task::spawn_blocking(move || demo::audio_path(&id)).await.map_err(err)??;
        return Ok(Track::url(path.to_string_lossy().into_owned()));
    }
    if let Some(path) = offline::local_track(song) {
        tracing::debug!("playing the offline copy of “{}” [{}]", song.title, song.id);
        return Ok(Track {
            url: path.to_string_lossy().into_owned(),
            headers: Vec::new(),
            options: Vec::new(),
        });
    }
    let source =
        sources.get(&song.server_id).ok_or_else(|| "the song's server is not connected".to_string())?;
    let started = std::time::Instant::now();
    let result = source.stream(song).await;
    match &result {
        Ok(s) => tracing::debug!(
            "stream for “{}” [{}] from {} in {:.1}s",
            song.title,
            song.id,
            s.url.split('/').nth(2).unwrap_or("?"),
            started.elapsed().as_secs_f32()
        ),
        Err(e) => tracing::warn!("no stream for “{}” [{}]: {e}", song.title, song.id),
    }
    result.map(to_track).map_err(err)
}

/// A connected source, passed around in messages.
#[derive(Clone)]
pub struct SourceHandle(pub Arc<dyn MusicSource>);

impl std::fmt::Debug for SourceHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SourceHandle({:?})", self.0.kind())
    }
}

#[derive(Debug, Clone)]
pub struct ArtistDetail {
    pub artist: Artist,
    pub albums: Vec<Album>,
    pub top_songs: Vec<Song>,
}

#[derive(Debug, Clone)]
pub struct PlaylistDetail {
    pub playlist: Playlist,
    pub songs: Vec<Song>,
}

#[derive(Debug, Default)]
pub struct SearchState {
    pub query: String,
    pub results: SearchResults,
    pub loading: bool,
    pub error: Option<String>,
    generation: u64,
    /// The tab of the search page.
    pub tab: list::SearchTab,
    pub debounce: search::Debounce,
}

/// Loaded library data for the pages.
#[derive(Debug, Default)]
pub struct Library {
    pub albums: Vec<Album>,
    pub albums_done: bool,
    pub albums_loading: bool,
    pub songs: Vec<Song>,
    pub songs_done: bool,
    pub songs_loading: bool,
    pub album: Option<Album>,
    pub album_error: Option<String>,
    /// Home rows in display order (the source's own feed first, e.g. YouTube
    /// Music's Quick picks and mixes, then the album rows from settings).
    pub home: Vec<HomeShelf>,
    pub home_loaded: bool,
    /// Home sections, feature carousel and genres (`app/home.rs`).
    pub home_state: home::HomeState,
    pub artists: Vec<Artist>,
    pub artists_loading: bool,
    pub artists_done: bool,
    pub artist: Option<ArtistDetail>,
    pub playlists: Vec<Playlist>,
    pub playlist: Option<PlaylistDetail>,
    /// Bumped per opened playlist so pages of an older one are dropped.
    pub playlist_gen: u64,
    /// More pages of the open playlist are on their way.
    pub playlist_loading_more: bool,
    pub favorites: Vec<Song>,
    pub favorites_loaded: bool,
    pub genres: Vec<listen_core::model::Genre>,
    pub genres_loaded: bool,
}

/// Scroll position of a virtualized list.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScrollState {
    pub offset: f32,
    pub height: f32,
    pub width: f32,
}

/// Cheap keys of what the tray, MPRIS and Discord were last built from:
/// their full state (strings, the cover file) is only built when a key
/// changes. `None` = build next time.
#[derive(Debug, Default)]
struct Published {
    /// (playing, current entry).
    #[cfg(any(target_os = "linux", windows))]
    tray: Option<(bool, Option<u64>)>,
    #[cfg(any(target_os = "linux", windows))]
    mpris: Option<MprisKey>,
    /// The cover file of the entry it was looked up for (MPRIS).
    #[cfg(any(target_os = "linux", windows))]
    art: Option<(u64, Option<String>)>,
}

#[cfg(any(target_os = "linux", windows))]
#[derive(Debug, Clone, Copy, PartialEq)]
struct MprisKey {
    uid: Option<u64>,
    playing: bool,
    audible: bool,
    duration: Duration,
    volume: (u8, bool),
    shuffle: bool,
    repeat: listen_core::queue::Repeat,
    has_next: bool,
    art: bool,
    /// Changes whenever the shown lyrics do (they go to MPRIS as `xesam:asText`).
    lyrics_rev: u64,
}

pub struct Listen {
    pub offline: offline::OfflineState,
    pub paths: Option<Paths>,
    pub history: listen_core::history::History,
    pub history_view: history::HistoryView,
    pub adv: advanced::AdvancedState,
    pub onboard: onboarding::OnboardState,
    pub helpers: helpers::HelpersState,
    pub mcp: mcp::McpState,
    pub radio: radio::RadioState,
    /// DLNA casting.
    pub dlna: dlna::DlnaState,
    pub config: Config,
    secrets: Option<Secrets>,
    pub servers: servers::ServersState,
    pub tokens: Tokens,
    theme: Theme,
    system_mode: Mode,
    /// A panel divider being dragged.
    resizing: Option<Resize>,
    /// Current window size (logical), for fitting text into panels.
    pub window_size: iced::Size,
    /// Libraries of servers switched away from, with the page and when they
    /// were loaded, so switching back is instant.
    library_cache: std::collections::HashMap<ServerId, (Library, Route, std::time::Instant)>,
    /// When the current library started loading.
    library_loaded_at: std::time::Instant,
    /// The window's display scale (e.g. 1.35), for touchpad scroll scaling.
    scale_factor: f32,
    /// The desktop's theme, for "Follow desktop theme" (`None`: nothing to follow).
    pub desktop_theme: Option<listen_core::theme::desktop::DesktopTheme>,
    pub appearance: appearance::AppearanceState,
    pub route: Route,
    back: Vec<Route>,
    forward: Vec<Route>,
    pub queue: Queue,
    /// The queue panel, drawer and page: search, drawer hover, server sync (`app/queue_ui.rs`).
    pub queue_ui: queue_ui::QueueUi,
    pub playback: Playback,
    pub show_queue: bool,
    pub demo: bool,
    pub video: video::Video,
    pub demo_audio: bool,
    demo_started: bool,
    screenshot: Option<std::path::PathBuf>,

    /// The source being browsed (the active server).
    pub source: Option<Arc<dyn MusicSource>>,
    /// Every connected server (the active one included).
    pub sources: Sources,
    /// Showing the sign-in page to add another server.
    pub adding_server: bool,
    /// Server whose "Remove?" confirmation is showing.
    pub confirm_remove: Option<ServerId>,
    pub server_name: Option<String>,
    pub login: LoginForm,
    pub library: Library,
    /// Selection, scroll position and page of every item list (`ui::list`).
    pub lists: list::Lists,
    /// The album page: track search / sort, carousels (`ui::album_detail`).
    pub album_page: album_page::AlbumPage,
    /// Genres, folders, favourite albums / artists (`library_extra.rs`).
    pub extra: library_extra::Extra,
    pub list_queries: list_query::ListQueries,
    /// Scroll position of the playlist / favorites song list.
    pub list_scroll: ScrollState,
    /// A song of the open playlist is being dragged: (row it came from, row it is over).
    pub playlist_drag: Option<(usize, usize)>,
    /// The Playlists page, the playlist page, their dialogs and the picker (`app/playlists`).
    pub playlist_page: playlists::PlaylistPage,
    pub artist_page: artist_page::ArtistPage,
    /// Scroll position of the queue list (panel or fullscreen, never both).
    pub queue_scroll: ScrollState,
    pub search: SearchState,
    pub smart: smart_playlist::SmartState,
    pub palette: palette::PaletteState,
    /// Item details, share, tag editor and file saving.
    pub item: item::ItemState,
    /// Reconnecting to the saved server at start-up.
    pub connecting: bool,
    pub images: Images,
    http: reqwest::Client,
    /// For servers set to ignore certificate errors (artwork).
    insecure_http: reqwest::Client,
    /// The music players (deck 0 plays alone unless a DJ transition runs).
    pub decks: decks::Decks,
    /// The DJ transition between the decks.
    pub mix: mix::MixState,
    player_config: PlayerConfig,
    /// Bumped whenever the current track is replaced, to drop stale results.
    play_token: u64,
    /// `play_token`, readable from background tasks (drops stale results).
    live_token: Arc<std::sync::atomic::AtomicU64>,
    /// Own copies of YouTube Music songs (see `substitute.rs`).
    pub substitutes: substitute::Substitutes,
    /// Retries so far for a queue entry that failed to play: (uid, attempts,
    /// position to resume at).
    retried: Option<(u64, u8, Duration)>,
    /// Play the next resolved stream from its backup server.
    use_backup: bool,
    /// Start position for the play request with this token (resuming a
    /// track that failed mid-way).
    resume_at: Option<(u64, Duration)>,
    /// Restored from the last run: nothing is loaded in mpv yet; the first play resumes here.
    restored: Option<Duration>,
    /// What was last written to queue.json (to skip identical saves).
    queue_saved: Option<(usize, u64, Option<u64>, u64, bool, listen_core::queue::Repeat)>,
    /// Stop mpv before resolving the next stream (after a failure, so nothing
    /// keeps playing from mpv's own playlist).
    stop_first: bool,
    pub overlay: overlay::OverlayState,
    pub fullscreen: bool,
    pub fullscreen_tab: FullscreenTab,
    /// Compact layout: the sidebar drawer is open.
    pub drawer_open: bool,
    /// The vertical volume slider above the compact bar's speaker is open.
    pub volume_popup: bool,
    /// Compact full-screen player: the panel shown instead of the cover.
    pub mobile_panel: Option<FullscreenTab>,
    pub lyrics: Option<listen_core::model::Lyrics>,
    lyrics_song: Option<String>,
    pub lyrics_loading: bool,
    pub lyric_active: Option<usize>,
    pub lyrics_ui: lyrics_state::LyricsUi,
    pub spectrum: Option<listen_visualizer::Frame>,
    pub visualizer_error: Option<String>,
    autodj_busy: bool,
    /// The running Auto-DJ fetch was started as the AI DJ's fallback.
    autodj_for_ai: bool,
    /// Keyboard modifiers currently held (the player-bar buttons don't report them).
    pub modifiers: iced::keyboard::Modifiers,
    /// Time the seek slider is being dragged to; the seek happens on release.
    pub seek_preview: Option<f32>,
    /// Version keys of every song played since Listen started: the Auto-DJ
    /// and the AI DJ don't pick them (or another version of them) again.
    session_played: std::collections::HashSet<String>,
    /// The AI DJ (Gemini).
    pub dj: dj::DjState,
    /// The open right-click menu.
    pub menu: Option<menu::ContextMenu>,
    /// Output device, speed, popover (see `app/audio.rs`).
    pub audio: audio::AudioState,
    /// Audio analysis results (Navidrome songs), once the store is loaded.
    pub analysis: Option<analysis::AnalysisHandle>,
    analysis_tx: Option<tokio::sync::mpsc::UnboundedSender<analysis::CommandBox>>,
    /// What the analysis worker is doing (settings page).
    pub analysis_status: Option<analysis::AnalysisStatus>,
    /// Where the right mouse button was last pressed.
    right_click_at: iced::Point,
    /// CD ripping (drives, running rips).
    pub rip: rip::RipState,
    /// The AI DJ's voice player (talking over music).
    voice: Option<Player>,
    /// Volume moves: song fades, the DJ's ducking, pause fades.
    fader: fade::Fader,
    /// Text of the accent field on the settings page.
    pub accent_input: String,
    /// Path typed into "Import from Feishin".
    pub import_path: String,
    /// Text in the settings page's search box.
    pub settings_search: String,
    /// Discord application id being typed (saved on Enter).
    pub discord_id_input: Option<String>,
    /// Action whose row in the hotkey editor is recording the next key combo.
    pub hotkey_recording: Option<String>,
    /// Visualizer settings UI and the player bar's waveform.
    pub viz: viz::VizState,
    /// The Milkdrop visualizer.
    pub milk: milkdrop::MilkdropState,
    /// Triggers the desktop actually assigned (action → description), once bound.
    pub global_triggers: Vec<(String, String)>,
    /// Why system-wide shortcuts are unavailable, if the portal refused.
    pub global_error: Option<String>,
    /// The shared analysis folder being typed (settings).
    pub analysis_dir_input: Option<String>,
    /// YouTube Music analysis switches on once the folder dialog returns a folder.
    pub pending_ytm_analysis: bool,
    #[cfg(any(target_os = "linux", windows))]
    tray: Option<crate::tray::TrayHandle>,
    #[cfg(any(target_os = "linux", windows))]
    tray_published: (bool, String),
    discord: crate::discord::Discord,
    pub integ: integrations::State,
    /// Hidden to the tray (close-to-tray).
    hidden: bool,
    /// Geometry of the main window (saved between runs).
    win: window_state::WinTrack,
    /// Keeps the system awake while playing.
    inhibitor: crate::inhibit::Inhibitor,
    /// The main window (the app is a daemon: windows are opened explicitly).
    pub main_window: Option<window::Id>,
    /// Minimized, as far as the platform tells (asked when focus is lost).
    minimized: bool,
    /// What the desktop integrations were last built from (see `sync_*`).
    #[cfg_attr(not(any(target_os = "linux", windows)), allow(dead_code))]
    published: Published,
    /// The window has keyboard focus (the visualizer slows down without it).
    focused: bool,
    pub sleep_timer: Option<sleep_timer::SleepTimer>,
    /// Full-screen player extras: backdrop, full-window visualizer, related songs, sleep popover.
    pub fs: fullscreen_state::FullscreenState,
    /// Left sidebar: folded sections, playlist folders, drags and popups.
    pub sb: sidebar_state::SidebarState,
    /// Signed in via `--login`: never persist config or secrets.
    ephemeral: bool,
    /// `--onboarding`: show the wizard regardless of the config.
    force_onboarding: bool,
    autoplay: bool,
    /// `--autoplay` with the AI DJ on: start it from the album's first song.
    autoplay_dj: bool,
    start_route: Option<Route>,
    #[cfg(any(target_os = "linux", windows))]
    mpris: Option<MprisHandle>,
    #[cfg(any(target_os = "linux", windows))]
    mpris_published: MprisState,
}

#[derive(Debug, Clone)]
pub enum LoginMsg {
    Kind(SourceKind),
    Url(String),
    Username(String),
    Password(String),
    Submit,
    /// Open a file dialog for the YouTube Music cookies file.
    Browse,
    /// Result of the file dialog.
    FileChosen(Option<std::path::PathBuf>),
    /// Open a folder dialog for a local music folder.
    BrowseFolder,
    /// Result of the folder dialog.
    FolderChosen(Option<std::path::PathBuf>),
    /// Result of looking for a signed-in browser.
    BrowserFound(Result<Vec<BrowserStatus>, String>),
    /// Sign in with another browser instead (an index into `LoginForm::browsers`):
    /// its cookies are read again first (it may have been closed or signed in since).
    OtherBrowser(usize),
    BrowserChecked(usize, Result<(), String>),
    ToggleBrowsers,
    /// Look again (after signing in in the browser).
    FindBrowser,
    /// Sign in with the found browser's cookies: list its channels first.
    UseBrowser,
    Channels(Result<Vec<listen_ytmusic::channels::Channel>, String>),
    /// Use this channel's library.
    PickChannel(usize),
    ToggleCookies,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// A file was dropped onto the window (cookies file / Feishin settings).
    FileDropped(std::path::PathBuf),
    Navigate(Route),
    Back,
    Forward,
    SystemTheme(Mode),
    /// A library result for this server (dropped if another one is active by now).
    ForServer(ServerId, Box<Message>),
    /// The window's display scale changed (or was first reported).
    ScaleFactor(f32),
    WindowResized(iced::Size),
    /// The window gained (true) or lost focus.
    WindowFocused(bool),
    /// Whether the window is minimized (asked when it loses focus).
    Minimized(bool),
    /// The queue list (panel or fullscreen) scrolled.
    QueueScrolled(scrollable::Viewport),
    /// The desktop theme changed (or disappeared).
    DesktopTheme(Option<listen_core::theme::desktop::DesktopTheme>),
    TogglePlay,
    Stop,
    Next,
    Previous,
    SkipBackward,
    SkipForward,
    Seek(f32),
    /// The seek slider is being dragged: only the shown time moves.
    SeekPreview(f32),
    /// The slider was let go: one seek to the previewed time.
    SeekRelease,
    /// Player-bar Next / Previous buttons: Alt+click jumps by album.
    NextClick,
    PreviousClick,
    /// To the first track of the next / previous album in the queue.
    NextAlbum,
    PreviousAlbum,
    /// Plays a random selection of songs shuffled (the bar's shuffle-all button).
    ShuffleAll,
    Volume(f32),
    ToggleMute,
    ToggleShuffle,
    SetPlayMode(play_mode::PlayMode),
    CycleRepeat,
    ToggleQueue,
    /// A panel divider was grabbed; the cursor moved while dragging; released.
    ResizeStart(PanelSide),
    ResizeMove(f32),
    ResizeEnd,
    /// Collapse the left sidebar to an icon rail, or expand it.
    ToggleSidebar,
    /// Show the playing song's cover large in the sidebar, or back in the player bar.
    ToggleCoverExpanded,
    PlayQueueItem(u64),
    /// Demo: play a demo album by index.
    PlayDemoAlbum(usize),
    Tick,
    /// Periodic save of the queue and playback position.
    SaveQueue,
    ScreenshotTaken(window::Screenshot),

    Login(LoginMsg),
    LoggedIn(Result<(ServerConfig, String, SourceHandle), String>),
    Connected(ServerId, Result<SourceHandle, String>),
    /// Browse another configured server.
    SwitchServer(ServerId),
    /// Scans a local folder again (files added, removed or retagged).
    RescanServer(ServerId),
    /// Open the sign-in page to add a server (keeps the current ones).
    AddServer,
    CancelAddServer,
    /// Ask "Remove?" for a server (None = cancel).
    ConfirmRemoveServer(Option<ServerId>),
    /// Forget a server (and its stored credentials).
    RemoveServer(ServerId),
    ArtistsLoaded(Result<Page<Artist>, String>),
    Artist(artist_page::ArtistMsg),
    ArtistLoaded(Result<ArtistDetail, String>),
    PlaylistsLoaded(Result<Vec<Playlist>, String>),
    PlaylistLoaded(Result<PlaylistDetail, String>),
    /// First page of a playlist (generation, detail, cursor for the rest).
    PlaylistFirstPage(u64, Result<(PlaylistDetail, Option<String>), String>),
    /// A further page (generation, songs, cursor for the next).
    PlaylistMore(u64, Result<(Vec<Song>, Option<String>), String>),
    /// Edit mode of the playlist page: a row is dragged by its grip, over a row, dropped.
    PlaylistDragStart(usize),
    PlaylistDragOver(usize),
    PlaylistDragEnd,
    PlaylistPage(playlists::PlaylistPageMsg),
    Queue(queue_ui::QueueMsg),
    FavoritesLoaded(Result<Vec<Song>, String>),
    GenresLoaded(Result<Vec<listen_core::model::Genre>, String>),
    SearchInput(String),
    SearchSubmit,
    SearchLoaded(u64, Result<SearchResults, String>),
    PlaySearchSong(usize),
    ToggleFavorite,
    Hotkeys(hotkeys::HotkeysMsg),
    /// Advanced settings tab (log level, caches, export / import, diagnostics).
    Advanced(advanced::AdvancedMsg),
    /// The in-app updater.
    Update(updater::UpdateMsg),
    Onboard(onboarding::OnboardMsg),
    Helpers(helpers::HelpersMsg),
    Mcp(mcp::McpMsg),
    Viz(viz::VizMsg),
    Milkdrop(milkdrop::MilkdropMsg),
    /// Zoom hotkeys; the appearance settings apply the step.
    Zoom(hotkeys::ZoomStep),
    Window(WindowAction),
    WindowState(window_state::WindowStateMsg),
    /// A second launch asked this instance to come forward.
    SingleInstance(crate::single_instance::Forward),
    Setting(Setting),
    Integrations(integrations::IntegrationsMsg),
    #[cfg(any(target_os = "linux", windows))]
    Tray(crate::tray::TrayMsg),
    Keyboard(iced::keyboard::Event),
    #[cfg(target_os = "linux")]
    GlobalShortcut(crate::global_shortcuts::ShortcutMsg),
    CloseRequested(window::Id),
    WindowClosed(window::Id),
    WindowEvent(window::Id, window::Event),
    DiscordDone(Result<(), String>),
    ToggleFullscreen,
    /// Compact layout: open / close the sidebar drawer.
    ToggleDrawer,
    CloseDrawer,
    /// Compact bar: open / close the volume popover.
    ToggleVolumePopup,
    CloseVolumePopup,
    /// Mouse wheel over a volume control: positive scroll = louder, by `volume_wheel_step`.
    VolumeWheel(f32),
    /// Compact full-screen player: toggle the queue / lyrics panel.
    MobilePanel(FullscreenTab),
    FullscreenTab(FullscreenTab),
    /// Opens the fullscreen player on a tab, or closes it if already there.
    OpenFullscreenTab(FullscreenTab),
    LyricsLoaded(String, Option<listen_core::model::Lyrics>),
    Lyrics(lyrics_state::LyricsMsg),
    Spectrum(listen_visualizer::Frame),
    VisualizerFailed(String),
    AutoDjLoaded(Result<Vec<Song>, String>),
    /// A planned AI DJ set (generation, result).
    DjPlanned(u64, Result<dj::Planned, listen_dj::DjError>),
    /// Periodic AI DJ refill check.
    DjCheck,
    DjPreviewDone(Result<(), listen_dj::DjError>),
    /// The AI DJ button in the queue panel.
    ToggleAiDj,
    Fullscreen(fullscreen_state::FullscreenMsg),
    Sidebar(sidebar_state::SidebarMsg),
    Logout,
    OpenAlbum(String),
    PlayAlbum(String),
    /// Add an album next / last (the Play button of the home feature follows the setting).
    PlayAlbumWith(Box<Album>, listen_core::settings::PlayBehavior),
    AlbumLoaded(Result<Album, String>),
    /// Album fetched by `PlayAlbum`, to be played right away.
    AlbumForPlay(Result<Album, String>),
    /// Play the loaded album detail starting at a track index.
    PlayAlbumTrack(usize),
    /// Play the loaded song list starting at an index.
    PlaySongAt(usize),
    /// A page of the Albums / Tracks list (generation of the query it was asked for).
    AlbumsLoaded(u64, Result<Page<Album>, String>),
    SongsLoaded(u64, Result<Page<Song>, String>),
    ListQuery(list_query::ListQueryMsg),
    HomeLoaded(Vec<HomeShelf>),
    /// Play song `index` of home row `row`.
    PlayHomeSong(usize, usize),
    /// The item lists (`ui::list`): selection, scrolling, play actions, drag.
    List(list::ListMsg),
    AlbumPage(album_page::AlbumPageMsg),
    Home(home::HomeMsg),
    Palette(palette::PaletteMsg),
    Item(item::ItemMsg),
    Search(search::SearchMsg),
    Extra(library_extra::ExtraMsg),
    /// Smart playlists: the editor and the badge ids (`app/smart_playlist.rs`).
    Smart(smart_playlist::SmartMsg),
    /// The playlist / favorites list scrolled.
    ListScrolled(scrollable::Viewport),
    ImageLoaded(images::Key, Option<Arc<Vec<u8>>>),
    BlurLoaded(images::Key, Option<iced::widget::image::Handle>),
    /// The lookup for an own copy of this YouTube Music song finished.
    Substituted(String),
    /// Switch the playing song between its own copy and the YouTube Music version.
    SubstituteToggle,
    StreamsResolved {
        token: u64,
        current: Result<Track, String>,
        next: Option<Track>,
    },
    /// A music deck (0 or 1) reports.
    Deck(u8, PlayerMsg),
    /// The AI DJ's voice player.
    Voice(PlayerMsg),
    /// The volume-fade clock.
    FadeTick,
    PlayerDone(Result<(), String>),
    Overlay(overlay::OverlayMsg),
    /// Settings page shell: tabs, search, reset, relaunch, player-bar/sidebar controls.
    Shell(settings_shell::ShellMsg),
    #[cfg(any(target_os = "linux", windows))]
    Mpris(MprisMsg),
    /// CD ripping.
    Rip(rip::RipMsg),
    /// Offline downloads.
    Offline(offline::OfflineMsg),
    /// The right mouse button went down here (comes before a menu's `Open`).
    RightClickAt(iced::Point),
    /// Right-click menus.
    Menu(menu::MenuMsg),
    /// Audio output, player-config popover, mpv options.
    Audio(audio::AudioMsg),
    /// The audio analysis worker.
    Analysis(analysis::AnalysisMsg),
    /// DJ transitions.
    Mix(mix::MixMsg),
    History(history::HistoryMsg),
    Radio(radio::RadioMsg),
    Dlna(dlna::DlnaMsg),
    Servers(servers::ServersMsg),
    /// Appearance settings (accent, font, zoom, backgrounds) and zoom hotkeys.
    Appearance(appearance::AppearanceMsg),
    Noop,
    /// The song/video switch in the player bar.
    ToggleVideo,
    /// Time to bring the video in step with the audio.
    VideoTick,
    /// The video address of a song (song id) or why there is none.
    VideoUrl(String, Result<String, String>),
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

impl Listen {
    pub fn boot(args: &Args) -> (Self, Task<Message>) {
        let paths = if args.demo { None } else { Paths::discover().ok() };
        // An `--ephemeral` run (tests, checks) starts from the defaults and ignores the user's settings;
        // `--login` still reads the config.
        let isolated = args.ephemeral && args.login.is_none();
        let config = paths
            .as_ref()
            .filter(|_| !isolated)
            .and_then(|p| match Config::load(&p.config_file()) {
                Ok(cfg) => Some(cfg),
                Err(e) => {
                    tracing::warn!("using default config: {e}");
                    None
                }
            })
            .unwrap_or_default();
        // The tests render in English whatever the machine's language is.
        // `LISTEN_LANG=de` overrides the setting (used to look at a translation).
        crate::i18n::set_language(match std::env::var("LISTEN_LANG") {
            Ok(code) if !code.is_empty() => crate::i18n::resolve(&code),
            _ if args.demo || args.ephemeral => "en",
            _ => crate::i18n::resolve(&config.settings.appearance.language),
        });
        let list_queries = list_query::ListQueries::from_settings(&config.settings.lists);

        let http = reqwest::Client::builder()
            .user_agent(concat!("Listen/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("http client");
        let insecure_http = reqwest::Client::builder()
            .user_agent(concat!("Listen/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            .danger_accept_invalid_certs(true)
            .build()
            .expect("http client");
        let volume = config.settings.playback.volume;
        let player_config = PlayerConfig::from_settings(&config.settings.playback);
        let audio = audio::AudioState::new(&config.settings.playback);
        let history = match (&paths, args.demo || args.login.is_some() || args.ephemeral) {
            (Some(p), false) => {
                listen_core::history::History::load(&p.data_dir.join(listen_core::history::FILE_NAME))
            }
            _ => listen_core::history::History::in_memory(),
        };
        let lyrics_ui = lyrics_state::LyricsUi::new(
            paths
                .as_ref()
                .filter(|_| !(args.demo || args.login.is_some() || args.ephemeral))
                .map(|p| p.data_dir.as_path()),
        );
        let substitutes = substitute::Substitutes::default();
        substitutes.set_enabled(config.settings.playback.prefer_own_servers);
        let mut state = Self {
            history,
            history_view: history::HistoryView::default(),
            adv: advanced::AdvancedState::default(),
            onboard: onboarding::OnboardState::default(),
            helpers: helpers::HelpersState::default(),
            mcp: mcp::McpState::default(),
            radio: radio::RadioState::default(),
            dlna: dlna::DlnaState::default(),
            secrets: paths.as_ref().map(|p| Secrets::with_store(&p.config_dir, config.secret_store)),
            servers: Default::default(),
            images: Images::new(paths.as_ref().map(|p| p.cache_dir.clone())),
            paths,
            config,
            tokens: Tokens::from_palette(&theme::resolve("defaultDark", None, true)),
            theme: Theme::Dark,
            system_mode: Mode::Dark,
            resizing: None,
            window_size: iced::Size::new(args.size.0, args.size.1),
            library_cache: std::collections::HashMap::new(),
            library_loaded_at: std::time::Instant::now(),
            scale_factor: 1.0,
            desktop_theme: None,
            appearance: appearance::AppearanceState::default(),
            route: Route::Home,
            back: Vec::new(),
            forward: Vec::new(),
            queue: Queue::new(),
            queue_ui: queue_ui::QueueUi::default(),
            playback: Playback {
                playing: false,
                position: Duration::ZERO,
                duration: None,
                volume,
                muted: false,
                scrobbled: false,
                played: Duration::ZERO,
                last_pos: None,
            },
            show_queue: false,
            demo: args.demo,
            video: Default::default(),
            demo_audio: args.demo && args.demo_audio && args.screenshot.is_none(),
            demo_started: false,
            screenshot: args.screenshot.clone(),
            source: None,
            sources: Sources::new(),
            adding_server: false,
            confirm_remove: None,
            server_name: None,
            login: LoginForm::default(),
            library: Library::default(),
            lists: list::Lists::default(),
            album_page: album_page::AlbumPage::default(),
            extra: library_extra::Extra::default(),
            list_queries,
            list_scroll: ScrollState::default(),
            playlist_drag: None,
            playlist_page: playlists::PlaylistPage::default(),
            artist_page: artist_page::ArtistPage::default(),
            queue_scroll: ScrollState::default(),
            search: SearchState::default(),
            smart: Default::default(),
            palette: palette::PaletteState::default(),
            item: item::ItemState::default(),
            connecting: false,
            http,
            insecure_http,
            decks: decks::Decks::default(),
            mix: mix::MixState::default(),
            player_config,
            play_token: 0,
            live_token: Arc::default(),
            substitutes,
            retried: None,
            use_backup: false,
            resume_at: None,
            restored: None,
            queue_saved: None,
            stop_first: false,
            overlay: overlay::OverlayState::default(),
            fullscreen: false,
            fullscreen_tab: FullscreenTab::Lyrics,
            drawer_open: false,
            volume_popup: false,
            mobile_panel: None,
            lyrics: None,
            lyrics_song: None,
            lyrics_loading: false,
            lyric_active: None,
            lyrics_ui,
            spectrum: None,
            visualizer_error: None,
            autodj_busy: false,
            autodj_for_ai: false,
            modifiers: iced::keyboard::Modifiers::default(),
            seek_preview: None,
            session_played: Default::default(),
            menu: None,
            audio,
            analysis: None,
            analysis_tx: None,
            analysis_status: None,
            analysis_dir_input: None,
            pending_ytm_analysis: false,
            right_click_at: iced::Point::ORIGIN,
            dj: dj::DjState::default(),
            rip: rip::RipState::default(),
            offline: offline::OfflineState::default(),
            voice: None,
            fader: fade::Fader::default(),
            sleep_timer: None,
            fs: Default::default(),
            sb: Default::default(),
            accent_input: String::new(),
            import_path: String::new(),
            settings_search: String::new(),
            discord_id_input: None,
            hotkey_recording: None,
            viz: Default::default(),
            milk: Default::default(),
            global_triggers: Vec::new(),
            global_error: None,
            #[cfg(any(target_os = "linux", windows))]
            tray: None,
            #[cfg(any(target_os = "linux", windows))]
            tray_published: (false, String::new()),
            discord: crate::discord::Discord::default(),
            integ: integrations::State::default(),
            hidden: false,
            win: window_state::WinTrack::default(),
            inhibitor: if args.demo || args.ephemeral || args.screenshot.is_some() {
                crate::inhibit::Inhibitor::disabled()
            } else {
                crate::inhibit::Inhibitor::system()
            },
            main_window: None,
            minimized: false,
            focused: true,
            published: Published::default(),
            ephemeral: args.login.is_some() || args.ephemeral,
            force_onboarding: args.onboarding,
            autoplay: args.autoplay,
            autoplay_dj: false,
            start_route: args.route.as_deref().and_then(parse_route),
            #[cfg(any(target_os = "linux", windows))]
            mpris: None,
            #[cfg(any(target_os = "linux", windows))]
            mpris_published: MprisState::default(),
        };
        if let Some(id) = &args.theme {
            state.config.settings.appearance.follow_desktop_theme = false;
            state.config.settings.appearance.follow_system_theme = false;
            state.config.settings.appearance.theme = id.clone();
        }
        // Read once so the first frame already has the desktop's colours.
        if state.config.settings.appearance.follow_desktop_theme {
            state.desktop_theme =
                listen_core::theme::desktop::config_dir().and_then(|d| listen_core::theme::desktop::read(&d));
        }
        if state.demo {
            let albums = demo::albums();
            state.library.home_state = home::HomeState::demo(&albums);
            state.library.home_loaded = true;
            state.library.albums = albums;
            state.library.albums_done = true;
            state.library.songs = (0..demo::ALBUMS.len()).flat_map(demo::album_songs).collect();
            state.library.songs_done = true;
            state.queue.set(demo::album_songs(0), 2);
            state.history.entries =
                listen_core::history::demo_entries(&state.library.songs, listen_core::history::now_secs());
            if !state.demo_audio {
                state.playback.position = Duration::from_secs(83);
            }
            state.show_queue = args.show_queue;
            state.video.on = args.video && state.demo_audio;
            state.fullscreen = args.fullscreen;
        }
        state.history_boot();
        state.restore_queue();
        state.dj_boot();
        state.rip_boot();
        state.offline_boot();
        state.appearance = appearance::AppearanceState::boot(
            state.paths.as_ref().filter(|_| !state.demo).map(|p| p.config_dir.clone()),
            &state.config.settings.appearance.font_path,
        );
        state.apply_theme();
        state.accent_input = state
            .config
            .settings
            .appearance
            .accent
            .map(|c| format!("rgb({}, {}, {})", c.0, c.1, c.2))
            .unwrap_or_default();

        state.win_restore(args);
        // Start minimized: only the tray icon (needs the tray, and never in demos).
        let w = &state.config.settings.window;
        let start_hidden = w.start_minimized && w.tray && !state.demo && state.screenshot.is_none();
        let mut tasks = vec![state.appearance_boot(), iced::system::theme().map(Message::SystemTheme)];
        tasks.push(state.advanced_boot());
        let mut main_id = None;
        if start_hidden {
            state.hidden = true;
        } else {
            let (id, open_main) = window::open(state.main_window_settings());
            state.main_window = Some(id);
            main_id = Some(id);
            tasks.push(open_main.then(|id| {
                Task::batch([
                    window::scale_factor(id).map(Message::ScaleFactor),
                    window::size(id).map(Message::WindowResized),
                ])
            }));
        }
        if let Some((url, user)) = &args.login {
            state.login.url = url.clone();
            state.login.username = user.clone();
            state.login.password = std::env::var("LISTEN_PASSWORD").unwrap_or_default();
            tasks.push(state.submit_login());
        } else if !state.demo {
            tasks.push(state.restore_session());
            if state.config.servers.is_empty() {
                tasks.push(state.start_discovery());
            }
            tasks.push(state.probe_keyring());
        }
        if state.demo
            && let Some(route) = state.start_route.take()
        {
            tasks.push(state.navigate(route));
        }
        if let (true, Some(main_id)) = (state.screenshot.is_some(), main_id) {
            // Give the first frames (fonts, images) time to settle.
            tasks.push(
                Task::future(tokio::time::sleep(Duration::from_millis(args.screenshot_delay_ms)))
                    .then(move |_| window::screenshot(main_id))
                    .map(Message::ScreenshotTaken),
            );
        }
        tasks.push(state.integrations_boot());
        tasks.push(state.mcp_boot());
        tasks.push(state.dlna_boot());
        tasks.push(state.milkdrop_boot());
        (state, Task::batch(tasks))
    }

    pub fn window_title(&self, _id: window::Id) -> String {
        self.title()
    }

    pub fn window_theme(&self, _id: window::Id) -> Theme {
        self.theme()
    }

    pub fn window_view(&self, _id: window::Id) -> Element<'_, Message> {
        self.view()
    }

    fn main_window_settings(&self) -> window::Settings {
        let w = &self.config.settings.window;
        let open = self.win_open_state();
        let settings = window::Settings {
            size: iced::Size::new(open.size.0, open.size.1),
            position: open.position.map_or(window::Position::Default, |(x, y)| {
                window::Position::Specific(iced::Point::new(x, y))
            }),
            maximized: open.maximized,
            fullscreen: open.fullscreen,
            min_size: Some(iced::Size::new(480.0, 120.0)),
            decorations: !w.custom_title_bar,
            // Close requests are answered by `CloseRequested` (tray or quit).
            exit_on_close_request: false,
            #[cfg(target_os = "linux")]
            platform_specific: iced::window::settings::PlatformSpecific {
                application_id: listen_core::config::APP_ID.into(),
                ..Default::default()
            },
            ..Default::default()
        };
        pin_screenshot_size(settings, self.screenshot.is_some())
    }

    /// Shows and focuses the main window. Wayland can neither un-hide nor un-minimize,
    /// so a window minimized there is replaced.
    fn reveal_main_window(&mut self) -> Task<Message> {
        self.hidden = false;
        let mut close = Task::none();
        if let Some(id) = self.main_window {
            if !(self.minimized && window_state::is_wayland()) {
                return Task::batch([window::minimize(id, false), window::gain_focus(id)]);
            }
            self.minimized = false;
            self.main_window = None;
            close = window::close(id);
        }
        let (id, open) = window::open(self.main_window_settings());
        self.main_window = Some(id);
        Task::batch([
            close,
            open.then(|id| {
                Task::batch([
                    window::scale_factor(id).map(Message::ScaleFactor),
                    window::size(id).map(Message::WindowResized),
                ])
            }),
        ])
    }

    pub fn theme(&self) -> Theme {
        self.theme.clone()
    }

    /// Whether the connected source can serve a page (e.g. no Folders on YouTube Music).
    pub fn supports_route(&self, route: &Route) -> bool {
        use listen_source::Capabilities as C;
        let Some(source) = &self.source else { return true };
        let caps = source.capabilities();
        match route {
            Route::Folders | Route::Folder(_) => caps.contains(C::FOLDERS),
            Route::Genres | Route::Genre(_) => caps.contains(C::GENRES),
            // YouTube Music has no album artists (only the artists you follow).
            Route::AlbumArtists => caps.contains(C::ALBUM_ARTISTS),
            Route::Radio => caps.contains(C::INTERNET_RADIO),
            _ => true,
        }
    }

    /// Full or compact layout, from the main window's width.
    pub fn layout_mode(&self) -> ui::LayoutMode {
        ui::LayoutMode::for_width(self.window_size.width)
    }

    /// Width of the middle page area (window minus sidebar and queue panel).
    pub fn page_width(&self) -> f32 {
        let sidebar = &self.config.settings.sidebar;
        let left = if sidebar.collapsed { ui::sidebar::RAIL_WIDTH + 1.0 } else { sidebar.width + 5.0 };
        let right = if self.queue_attached()
            && self.config.settings.queue.side_layout == listen_core::settings::SideQueueLayout::Horizontal
        {
            sidebar.queue_width + 5.0
        } else {
            0.0
        };
        (self.window_size.width - left - right).max(200.0)
    }

    /// The visualizer strip at the bottom of the queue panel is on screen.
    pub fn queue_visualizer_visible(&self) -> bool {
        (self.queue_attached() || self.queue_drawer_visible())
            && self.config.settings.visualizer.in_queue_panel
            && self.visualizer_error.is_none()
    }

    /// Demo mode without real audio: playback is simulated, no mpv.
    pub fn mock_playback(&self) -> bool {
        self.demo && !self.demo_audio
    }

    pub fn visualizer_running(&self) -> bool {
        self.visualizer_wanted() && self.playback.playing && !self.hidden && !self.minimized
    }

    /// True when the login page should be shown instead of the library.
    pub fn needs_login(&self) -> bool {
        // Downloads play without any server.
        let offline_page = self.route == Route::Downloads && !self.adding_server;
        !self.demo && ((self.source.is_none() && !self.connecting && !offline_page) || self.adding_server)
    }

    /// Current track duration: mpv's value if known, else the tag.
    pub fn duration(&self) -> Duration {
        self.playback.duration.or_else(|| self.queue.current().map(|i| i.song.duration)).unwrap_or_default()
    }

    /// Picks the palette: the desktop's (when following it), else from
    /// settings and the system light/dark mode.
    fn apply_theme(&mut self) {
        let a = &self.config.settings.appearance;
        let loaded = &self.appearance.loaded;
        let ov = &loaded.overrides;
        if a.follow_desktop_theme
            && let Some(desktop) = &self.desktop_theme
        {
            let mut palette = desktop.palette;
            ov.apply_palette(&mut palette);
            self.tokens = Tokens::build(&palette, ShadeChoice::default(), ov);
            crate::style::metrics::apply(&self.tokens);
            self.theme = self.tokens.iced_theme(&format!("desktop: {}", desktop.name));
            return;
        }
        let id = if a.follow_system_theme {
            match self.system_mode {
                Mode::Light => &a.theme_light,
                Mode::Dark | Mode::None => &a.theme_dark,
            }
        } else {
            &a.theme
        };
        let base = loaded.palette(id).unwrap_or_else(|| theme::resolve("defaultDark", None, true));
        let mut palette = theme::with_accent(base, a.accent, a.use_theme_accent);
        ov.apply_palette(&mut palette);
        let shade = ShadeChoice((!a.use_theme_primary_shade).then_some(a.primary_shade));
        self.tokens = Tokens::build(&palette, shade, ov);
        crate::style::metrics::apply(&self.tokens);
        self.theme = self.tokens.iced_theme(id);
    }

    /// The iced scale factor: the zoom setting (50-300 %).
    pub fn ui_scale(&self, _id: window::Id) -> f32 {
        self.config.settings.appearance.scale_factor()
    }

    fn save_config(&self) {
        if self.ephemeral || self.demo {
            return;
        }
        if let Some(paths) = &self.paths
            && let Err(e) = self.config.save(&paths.config_file())
        {
            tracing::error!("saving config: {e}");
        }
    }

    fn restore_queue(&mut self) {
        if self.ephemeral || self.demo || !self.config.settings.queue.save_play_queue {
            return;
        }
        let Some(saved) = self.paths.as_ref().and_then(|p| SavedQueue::load(&p.queue_file())) else { return };
        if saved.queue.current().is_none() {
            return;
        }
        let at = Duration::from_secs_f64(saved.position_secs.max(0.0));
        self.queue = saved.queue;
        self.playback.position = at;
        self.restored = Some(at);
    }

    /// Test hook: the state right after a queue was restored at start-up.
    #[doc(hidden)]
    pub fn restore_for_test(&mut self, at: Duration) {
        self.playback.position = at;
        self.restored = Some(at);
    }

    /// Writes the queue and the playback position (only when they changed).
    fn save_queue(&mut self) {
        self.write_queue(false);
    }

    /// Like `save_queue`, but blocks until the file is written (for quit/close).
    fn save_queue_now(&mut self) {
        self.write_queue(true);
    }

    fn write_queue(&mut self, blocking: bool) {
        if self.ephemeral || self.demo || !self.config.settings.queue.save_play_queue {
            return;
        }
        let Some(paths) = &self.paths else { return };
        let secs = self.playback.position.as_secs();
        let order = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            self.queue.iter().for_each(|i| i.uid.hash(&mut h));
            h.finish()
        };
        let key = (
            self.queue.len(),
            order,
            self.queue.current().map(|i| i.uid),
            secs / 5,
            self.queue.shuffle(),
            self.queue.repeat,
        );
        if self.queue_saved == Some(key) {
            return;
        }
        self.queue_saved = Some(key);
        let saved =
            SavedQueue { queue: self.queue.clone(), position_secs: self.playback.position.as_secs_f64() };
        let path = paths.queue_file();
        let write = move || {
            if let Err(e) = saved.save(&path) {
                tracing::error!("saving queue: {e}");
            }
        };
        if blocking {
            write();
        } else {
            std::thread::spawn(write);
        }
    }

    // ---- session ------------------------------------------------------------

    /// Connects a source; YouTube Music signs in asynchronously.
    async fn connect_source(
        server: ServerConfig,
        secret: String,
        options: listen_core::config::ServerOptions,
        full_scan: bool,
    ) -> Result<SourceHandle, String> {
        let source: Arc<dyn MusicSource> = match server.kind {
            SourceKind::Subsonic => {
                let s = listen_subsonic::Subsonic::with_options(
                    server.id.clone(),
                    &server.url,
                    &server.username,
                    &secret,
                    listen_subsonic::Options {
                        legacy_auth: options.legacy_auth,
                        ignore_ssl: options.ignore_ssl,
                    },
                )
                .map_err(err)?;
                Arc::new(s)
            }
            SourceKind::YouTubeMusic => {
                let yt =
                    listen_ytmusic::YouTubeMusic::connect(server.id.clone(), &secret).await.map_err(err)?;
                if let Err(e) = yt.check_ytdlp().await {
                    tracing::warn!("{e}");
                }
                Arc::new(yt)
            }
            SourceKind::Local => {
                let folder = if full_scan {
                    listen_local::LocalFolder::connect_full(server.id.clone(), &server.url).await
                } else {
                    listen_local::LocalFolder::connect(server.id.clone(), &server.url).await
                };
                Arc::new(folder.map_err(err)?)
            }
            SourceKind::Jellyfin => Arc::new(
                listen_jellyfin::Jellyfin::connect_with(
                    server.id.clone(),
                    &server.url,
                    &server.username,
                    &secret,
                    servers::jf_options(options),
                )
                .await
                .map_err(err)?,
            ),
        };
        source.ping().await.map_err(|e| match e {
            listen_source::SourceError::Auth(m) if server.kind != SourceKind::YouTubeMusic => {
                format!("Wrong username or password. ({m})")
            }
            other => other.to_string(),
        })?;
        Ok(SourceHandle(source))
    }

    /// Reconnects on start-up: the active server first, the others in the
    /// background (so switching is instant and queued songs can play).
    fn restore_session(&mut self) -> Task<Message> {
        let Some(active) = self.config.active_server.clone() else { return Task::none() };
        let others: Vec<ServerId> =
            self.config.servers.iter().map(|s| s.id.clone()).filter(|id| *id != active).collect();
        let mut tasks = vec![self.connect_server(active)];
        for id in others {
            tasks.push(self.connect_background(id));
        }
        Task::batch(tasks)
    }

    /// The stored credential; local folders have none.
    fn secret_of(&self, server: &ServerConfig) -> Option<String> {
        if server.kind == SourceKind::Local {
            return Some(String::new());
        }
        self.secrets.as_ref().and_then(|s| s.get(&server.id))
    }

    /// Connects a server without switching to it (silently skipped without credentials).
    fn connect_background(&mut self, id: ServerId) -> Task<Message> {
        self.connect_background_mode(id, false)
    }

    fn connect_background_mode(&mut self, id: ServerId, full_scan: bool) -> Task<Message> {
        let Some(server) = self.config.servers.iter().find(|s| s.id == id).cloned() else {
            return Task::none();
        };
        let Some(secret) = self.secret_of(&server) else { return Task::none() };
        let options = self.config.options_of(&id);
        Task::perform(Self::connect_source(server, secret, options, full_scan), move |r| {
            Message::Connected(id.clone(), r)
        })
    }

    /// Connects a configured server with its stored credentials.
    fn connect_server(&mut self, id: ServerId) -> Task<Message> {
        self.connect_server_mode(id, false)
    }

    fn connect_server_mode(&mut self, id: ServerId, full_scan: bool) -> Task<Message> {
        let Some(server) = self.config.servers.iter().find(|s| s.id == id).cloned() else {
            return Task::none();
        };
        let Some(secret) = self.secret_of(&server) else {
            if self.source.is_some() {
                // Another server is in use: ask for the password in the edit dialog.
                self.toast_warning(format!("Please sign in to {} again.", server.name));
                return self.on_servers(servers::ServersMsg::Edit(id));
            }
            self.servers.failure = Some(servers::Failure::Credentials { id });
            return Task::none();
        };
        self.connecting = true;
        let options = self.config.options_of(&id);
        Task::perform(Self::connect_source(server, secret, options, full_scan), move |r| {
            Message::Connected(id.clone(), r)
        })
    }

    /// Looks for a browser that is signed in to YouTube (off the UI thread).
    fn find_browser(&mut self) -> Task<Message> {
        self.login.browser = None;
        Task::perform(
            async {
                tokio::task::spawn_blocking(|| {
                    listen_ytmusic::browser::all()
                        .map(|found| found.into_iter().map(|(p, header)| (p, header.map(|_| ()))).collect())
                })
                .await
                .unwrap_or_else(|e| Err(e.to_string()))
            },
            |r| Message::Login(LoginMsg::BrowserFound(r)),
        )
    }

    fn load_cookie_file(&mut self, path: &std::path::Path) -> bool {
        self.login.kind = SourceKind::YouTubeMusic;
        self.login.url = path.display().to_string();
        self.login.info = None;
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                self.login.error = Some(format!("Could not read {}: {e}", path.display()));
                return false;
            }
        };
        match listen_ytmusic::cookies::to_header(&text) {
            Ok(header) => {
                let count = header.split("; ").count();
                self.login.password = text;
                self.login.error = None;
                self.login.info = Some(format!("✓ {count} youtube.com cookies loaded — click Connect."));
                true
            }
            Err(e) => {
                self.login.password.clear();
                self.login.error = Some(e);
                false
            }
        }
    }

    fn submit_login(&mut self) -> Task<Message> {
        let now = unix_millis();
        let (server, secret) = match self.login.kind {
            SourceKind::Local => {
                let path = shellexpand_home(self.login.url.trim());
                if self.login.url.trim().is_empty() || !path.is_dir() {
                    self.login.error = Some("Choose an existing music folder.".into());
                    return Task::none();
                }
                let name = path
                    .file_name()
                    .map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
                let server = ServerConfig {
                    id: ServerId(format!("local-{now:x}")),
                    name: format!("{name} (local)"),
                    kind: SourceKind::Local,
                    url: path.display().to_string(),
                    username: String::new(),
                };
                (server, String::new())
            }
            SourceKind::YouTubeMusic => {
                // A typed/picked cookies file is read now (unless cookies were pasted).
                if self.login.password.trim().is_empty() && !self.login.url.trim().is_empty() {
                    let path = shellexpand_home(self.login.url.trim());
                    if !self.load_cookie_file(&path) {
                        return Task::none();
                    }
                }
                let guest = self.login.password.trim().is_empty();
                let server = ServerConfig {
                    id: ServerId(format!("ytm-{now:x}")),
                    name: if guest { "YouTube Music (guest)".into() } else { "YouTube Music".into() },
                    kind: SourceKind::YouTubeMusic,
                    url: String::new(),
                    username: String::new(),
                };
                let secret = if guest {
                    listen_ytmusic::GUEST_COOKIE.to_string()
                } else {
                    self.login.password.trim().to_string()
                };
                (server, secret)
            }
            _ => {
                let url = servers::normalize_url(&self.login.url);
                if url.is_empty() || self.login.username.trim().is_empty() {
                    self.login.error = Some("Server URL and username are required.".into());
                    return Task::none();
                }
                let name = servers::host_of(&url);
                let kind = self.login.kind;
                let prefix = if kind == SourceKind::Jellyfin { "jf" } else { "nd" };
                let server = ServerConfig {
                    id: ServerId(format!("{prefix}-{now:x}")),
                    name,
                    kind,
                    url,
                    username: self.login.username.trim().to_string(),
                };
                (server, self.login.password.clone())
            }
        };
        self.connect_new(server, secret)
    }

    /// Signs in to a new server from the sign-in page.
    fn connect_new(&mut self, server: ServerConfig, secret: String) -> Task<Message> {
        self.login.busy = true;
        self.login.error = None;
        let options = self.login.options;
        self.note_pending_options(&server.id, options);
        Task::perform(
            async move {
                let handle = Self::connect_source(server.clone(), secret.clone(), options, false).await?;
                Ok((server, secret, handle))
            },
            Message::LoggedIn,
        )
    }

    // ---- loading --------------------------------------------------------------

    fn load_albums(&mut self) -> Task<Message> {
        let Some(source) = self.source.clone() else { return Task::none() };
        if self.library.albums_loading || self.library.albums_done {
            return Task::none();
        }
        self.library.albums_loading = true;
        let q = self.album_page_query(self.library.albums.len());
        let gen_ = self.list_queries.albums_gen;
        Task::perform(
            async move { source.albums(&q).await.map_err(err) },
            self.scoped(move |r| Message::AlbumsLoaded(gen_, r)),
        )
    }

    fn load_songs(&mut self) -> Task<Message> {
        let Some(source) = self.source.clone() else { return Task::none() };
        if self.library.songs_loading || self.library.songs_done {
            return Task::none();
        }
        self.library.songs_loading = true;
        let q = self.song_page_query(self.library.songs.len());
        let gen_ = self.list_queries.songs_gen;
        Task::perform(
            async move { source.songs(&q).await.map_err(err) },
            self.scoped(move |r| Message::SongsLoaded(gen_, r)),
        )
    }

    fn open_album(&mut self, id: String) -> Task<Message> {
        if self.demo {
            self.library.album = demo::albums().into_iter().find(|a| a.id == id);
            return Task::none();
        }
        let Some(source) = self.source.clone() else { return Task::none() };
        if self.library.album.as_ref().map(|a| &a.id) != Some(&id) {
            self.library.album = None;
        }
        self.library.album_error = None;
        Task::perform(async move { source.album(&id).await.map_err(err) }, self.scoped(Message::AlbumLoaded))
    }

    /// Requests artwork (from the active server) if it is not cached yet.
    pub fn request_image(&mut self, image: Option<&ImageRef>) -> Task<Message> {
        match self.config.active_server.clone() {
            Some(server) => self.request_image_on(&server, image),
            None => Task::none(),
        }
    }

    /// Requests artwork from a specific server (queue items may come from another one).
    pub fn request_image_on(&mut self, server: &ServerId, image: Option<&ImageRef>) -> Task<Message> {
        let (Some(image), Some(source)) = (image, self.sources.get(server).cloned()) else {
            return Task::none();
        };
        let key = images::key(&server.0, image, ART_SIZE);
        if !self.images.should_fetch(&key) {
            return Task::none();
        }
        if self.config.settings.appearance.image_placeholder
            == listen_core::settings::ImagePlaceholder::ThumbHash
        {
            self.images.prime_preview(&key);
        }
        let Some(url) = source.image_url(image, ART_SIZE) else { return Task::none() };
        let disk = self.images.disk_path(&key);
        let http =
            if self.config.options_of(server).ignore_ssl { &self.insecure_http } else { &self.http }.clone();
        Task::perform(images::fetch(http, url, disk), move |bytes| Message::ImageLoaded(key.clone(), bytes))
    }

    /// Cache key of an image on the active server.
    pub fn image_key(&self, image: &ImageRef) -> images::Key {
        let server = self.config.active_server.as_ref().map_or("", |s| s.0.as_str());
        images::key(server, image, ART_SIZE)
    }

    /// Cache key of an image on a given server.
    pub fn image_key_on(&self, server: &ServerId, image: &ImageRef) -> images::Key {
        images::key(&server.0, image, ART_SIZE)
    }

    /// Starts a background job producing the blurred album header version of an image.
    pub fn request_blur(&mut self, image: Option<&ImageRef>) -> Task<Message> {
        self.request_blur_for(image, appearance::BlurKind::Album)
    }

    fn request_images<'a>(&mut self, refs: impl IntoIterator<Item = Option<&'a ImageRef>>) -> Task<Message> {
        let refs: Vec<ImageRef> = refs.into_iter().flatten().cloned().collect();
        Task::batch(refs.iter().map(|r| self.request_image(Some(r))).collect::<Vec<_>>())
    }

    /// Images and more data for the visible part of the albums list.
    fn albums_visible(&mut self) -> Task<Message> {
        let (first, last) = self.list_range(&list::ListId::Albums, self.library.albums.len());
        let refs: Vec<ImageRef> =
            self.library.albums[first..last].iter().filter_map(|a| a.image.clone()).collect();
        let mut tasks: Vec<Task<Message>> = refs.iter().map(|r| self.request_image(Some(r))).collect();
        if last + 24 >= self.library.albums.len() {
            tasks.push(self.load_albums());
        }
        tasks.push(self.load_detail(&list::ListId::Albums, first, last));
        Task::batch(tasks)
    }

    fn songs_visible(&mut self) -> Task<Message> {
        let (first, last) = self.list_range(&list::ListId::Songs, self.library.songs.len());
        let refs: Vec<ImageRef> =
            self.library.songs[first..last].iter().filter_map(|s| s.image.clone()).collect();
        let mut tasks: Vec<Task<Message>> = refs.iter().map(|r| self.request_image(Some(r))).collect();
        if last + 40 >= self.library.songs.len() {
            tasks.push(self.load_songs());
        }
        Task::batch(tasks)
    }

    /// Covers for the visible rows of the playlist / favorites list.
    fn list_visible(&mut self) -> Task<Message> {
        let songs = match self.route {
            Route::Recent => &self.history_view.recent,
            Route::Mix(_) => &self.history_view.mix,
            Route::Playlist(_) => return self.playlist_list_visible(),
            _ => return Task::none(),
        };
        let (first, last) = ui::playlist_detail::visible_rows(self.list_scroll, songs.len());
        let refs: Vec<ImageRef> = songs[first..last].iter().filter_map(|s| s.image.clone()).collect();
        self.request_images(refs.iter().map(Some))
    }

    /// A new song list opens: start at the top.
    fn reset_list_scroll(&mut self) -> Task<Message> {
        self.list_scroll.offset = 0.0;
        iced::widget::operation::scroll_to(
            iced::widget::Id::new(ui::playlist_detail::LIST_ID),
            scrollable::AbsoluteOffset { x: 0.0, y: 0.0 },
        )
    }

    /// A source is ready (sign-in, start-up or switch): browse it.
    fn on_connected(&mut self, id: ServerId, source: Arc<dyn MusicSource>) -> Task<Message> {
        self.servers.failure = None;
        self.sources.insert(id.clone(), source.clone());
        let covers = self.queue_covers(&id);
        Task::batch([covers, self.activate(id, source)])
    }

    /// Loads the covers of the restored queue (around the current song) from `server`.
    fn queue_covers(&mut self, server: &ServerId) -> Task<Message> {
        const AHEAD: usize = 100;
        let from = self.queue.current_position().unwrap_or(0).saturating_sub(10);
        let refs: Vec<ImageRef> = self
            .queue
            .iter()
            .skip(from)
            .take(AHEAD)
            .filter(|i| i.song.server_id == *server)
            .filter_map(|i| i.song.image.clone())
            .collect();
        Task::batch(refs.iter().map(|r| self.request_image_on(server, Some(r))).collect::<Vec<_>>())
    }

    /// Makes a connected server the one being browsed. Playback continues.
    fn activate(&mut self, id: ServerId, source: Arc<dyn MusicSource>) -> Task<Message> {
        /// Kept libraries older than this are shown at once but refreshed.
        const FRESH: Duration = Duration::from_secs(10 * 60);
        self.server_name = self.config.servers.iter().find(|s| s.id == id).map(|s| s.name.clone());
        let previous_server = self.config.active_server.clone().filter(|p| *p != id);
        // Keep the library we leave, so switching back is instant.
        if let Some(previous) = previous_server.clone()
            && self.source.is_some()
        {
            let library = std::mem::take(&mut self.library);
            let loaded = self.library_loaded_at;
            self.library_cache.insert(previous, (library, self.route.clone(), loaded));
        }
        if self.config.active_server.as_ref() != Some(&id) {
            self.config.active_server = Some(id.clone());
            self.save_config();
        }
        self.source = Some(source);
        self.swap_list_queries(&id, previous_server);
        // Analyse this server's library in the background (Navidrome only).
        self.analysis_crawl();
        self.adding_server = false;
        self.connecting = false;
        self.search = SearchState::default();
        self.back.clear();
        self.forward.clear();
        if let Some((library, route, loaded)) = self.library_cache.remove(&id) {
            tracing::info!("switched back to {}: library from memory", id.0);
            self.library = library;
            self.library_loaded_at = loaded;
            self.route = route.clone();
            let mut tasks = vec![self.on_enter(route)];
            if loaded.elapsed() > FRESH {
                self.library_loaded_at = std::time::Instant::now();
                tasks.push(self.load_home());
                tasks.push(self.load_playlists());
            }
            return Task::batch(tasks);
        }
        self.library = Library::default();
        self.library_loaded_at = std::time::Instant::now();
        self.route = Route::Home;
        let mut tasks = vec![self.load_home(), self.load_playlists()];
        if let Some(route) = self.start_route.take().filter(|r| *r != Route::Home) {
            tasks.push(self.navigate(route));
        }
        Task::batch(tasks)
    }

    /// Tags a load result with the active server, so a result that arrives
    /// after switching servers doesn't land in the other server's library.
    fn scoped<T: 'static>(
        &self,
        f: impl FnOnce(T) -> Message + Send + 'static,
    ) -> impl FnOnce(T) -> Message + Send + 'static {
        let id = self.config.active_server.clone();
        move |t| match id {
            Some(id) => Message::ForServer(id, Box::new(f(t))),
            None => f(t),
        }
    }

    fn load_artists(&mut self) -> Task<Message> {
        let Some(source) = self.source.clone() else { return Task::none() };
        if self.library.artists_loading || self.library.artists_done {
            return Task::none();
        }
        self.library.artists_loading = true;
        let q = ListQuery { start: self.library.artists.len(), limit: 500, ..Default::default() };
        Task::perform(
            async move { source.artists(&q).await.map_err(err) },
            self.scoped(Message::ArtistsLoaded),
        )
    }

    fn open_artist(&mut self, id: String) -> Task<Message> {
        let Some(source) = self.source.clone() else { return Task::none() };
        if self.library.artist.as_ref().map(|a| &a.artist.id) != Some(&id) {
            self.library.artist = None;
        }
        Task::perform(
            async move {
                let artist = source.artist(&id).await.map_err(err)?;
                let (albums, top) = tokio::join!(source.artist_albums(&id), source.top_songs(&artist, 20));
                let mut albums = albums.unwrap_or_default();
                albums.sort_by_key(|a| std::cmp::Reverse(a.year.unwrap_or(0)));
                Ok(ArtistDetail { artist, albums, top_songs: top.unwrap_or_default() })
            },
            self.scoped(Message::ArtistLoaded),
        )
    }

    fn load_playlists(&mut self) -> Task<Message> {
        let Some(source) = self.source.clone() else { return Task::none() };
        Task::perform(
            async move { source.playlists().await.map_err(err) },
            self.scoped(Message::PlaylistsLoaded),
        )
    }

    fn open_playlist(&mut self, id: String) -> Task<Message> {
        if self.demo {
            // No server: demo fixtures (a playlist without songs of its own gets the Navidrome ones).
            let mut detail = if id == demo::ytm_playlist().playlist.id {
                demo::ytm_playlist()
            } else {
                demo::navidrome_playlist()
            };
            if let Some(known) = self.library.playlists.iter().find(|p| p.id == id) {
                detail.playlist = known.clone();
            }
            self.library.playlist_gen += 1;
            return Task::done(Message::PlaylistLoaded(Ok(detail)));
        }
        let Some(source) = self.source.clone() else { return Task::none() };
        if self.library.playlist.as_ref().map(|p| &p.playlist.id) != Some(&id) {
            self.library.playlist = None;
        }
        let known = self
            .library
            .playlists
            .iter()
            .chain(self.library.home.iter().flat_map(|r| &r.playlists))
            .find(|p| p.id == id)
            .cloned();
        self.library.playlist_gen += 1;
        self.library.playlist_loading_more = false;
        let generation = self.library.playlist_gen;
        Task::perform(
            async move {
                let (songs, next) = source.playlist_page(&id, None).await.map_err(err)?;
                let playlist = known.unwrap_or_else(|| Playlist {
                    id: id.clone(),
                    server_id: ServerId(String::new()),
                    source: source.kind(),
                    name: "Playlist".into(),
                    description: None,
                    owner: None,
                    image: songs.first().and_then(|s| s.image.clone()),
                    song_count: None,
                    duration: Duration::ZERO,
                    public: false,
                    editable: false,
                });
                Ok((PlaylistDetail { playlist, songs }, next))
            },
            self.scoped(move |r| Message::PlaylistFirstPage(generation, r)),
        )
    }

    /// Loads the next page of the open playlist.
    fn more_playlist(&mut self, cursor: String) -> Task<Message> {
        let (Some(source), Some(detail)) = (self.source.clone(), &self.library.playlist) else {
            return Task::none();
        };
        let id = detail.playlist.id.clone();
        let generation = self.library.playlist_gen;
        self.library.playlist_loading_more = true;
        Task::perform(
            async move { source.playlist_page(&id, Some(&cursor)).await.map_err(err) },
            self.scoped(move |r| Message::PlaylistMore(generation, r)),
        )
    }

    fn load_favorites(&mut self) -> Task<Message> {
        let Some(source) = self.source.clone() else { return Task::none() };
        let q = SongQuery { favorites_only: true, limit: 5000, ..Default::default() };
        Task::perform(
            async move { source.songs(&q).await.map(|p| p.items).map_err(err) },
            self.scoped(Message::FavoritesLoaded),
        )
    }

    /// Called whenever the current track changes.
    fn on_track_changed(&mut self) -> Task<Message> {
        if let Some(item) = self.queue.current() {
            self.session_played.insert(listen_core::versions::song_key(&item.song));
        }
        self.analysis_want();
        let refill = if self.ai_dj_on() { self.dj_track_changed() } else { self.maybe_autodj() };
        // Both songs may be analysed already (no `Analysed` will come): plan now.
        let mix = self.mix_consider();
        // YouTube Music: analyse the DJ's next candidates ahead of the pick.
        let pool = self.analysis_pool();
        let waveform = self.waveform_sync();
        Task::batch([
            self.load_lyrics(),
            refill,
            mix,
            pool,
            self.fs_backdrop(),
            self.queue_track_changed(),
            waveform,
            self.related_track_changed(),
        ])
    }

    /// Feishin's Auto-DJ: when few songs are left, append similar (or random) ones.
    fn maybe_autodj(&mut self) -> Task<Message> {
        let s = &self.config.settings.auto_dj;
        if !s.enabled || self.queue.upcoming() > s.timing as usize {
            return Task::none();
        }
        self.fetch_autodj()
    }

    /// Appends similar (or random) songs to the current one (the Auto-DJ, and
    /// the AI DJ's fallback).
    fn fetch_autodj(&mut self) -> Task<Message> {
        let s = &self.config.settings.auto_dj;
        if self.demo || self.autodj_busy {
            return Task::none();
        }
        let seed = self.queue.current().map(|i| i.song.clone());
        let (Some(source), Some(seed)) =
            (seed.as_ref().and_then(|s| self.sources.get(&s.server_id).cloned()), seed)
        else {
            return Task::none();
        };
        self.autodj_busy = true;
        self.autodj_for_ai = self.ai_dj_on();
        // More candidates when they get ordered by flow (only the best are queued).
        let spread = if self.config.settings.mixing.flow_select { 6 } else { 3 };
        let count = s.item_count.max(1) as usize * spread / 3;
        let similar = s.strategy != "random";
        Task::perform(
            async move {
                let mut songs = if similar {
                    source.similar_songs(&seed, count * 3).await.unwrap_or_default()
                } else {
                    vec![]
                };
                // Random songs fill in when the similar ones already played this session.
                match source.random_songs(count * 3, None).await {
                    Ok(random) => songs.extend(random),
                    Err(e) if songs.is_empty() => return Err(err(e)),
                    Err(_) => {}
                }
                Ok(songs)
            },
            Message::AutoDjLoaded,
        )
    }

    // ---- playback ---------------------------------------------------------------

    /// A single song clicked in a list. With the AI DJ on, it plays alone
    /// and becomes the DJ's new mood seed; otherwise the list plays from it.
    fn play_picked(&mut self, songs: Vec<Song>, start: usize) -> Task<Message> {
        if self.queue_confirm_needed() {
            return self.queue_confirm_play(songs, start);
        }
        if self.ai_dj_on()
            && let Some(song) = songs.get(start).cloned()
        {
            self.queue.set(vec![song], 0);
            self.dj_reseed();
            return self.start_current();
        }
        self.play_songs(songs, start)
    }

    fn play_songs(&mut self, songs: Vec<Song>, start: usize) -> Task<Message> {
        if songs.is_empty() {
            return Task::none();
        }
        self.queue.set(songs, start);
        self.start_current()
    }

    /// Replaces what mpv plays with the queue's current (and next) track.
    fn bump_token(&mut self) {
        self.play_token += 1;
        self.live_token.store(self.play_token, std::sync::atomic::Ordering::SeqCst);
    }

    fn start_current(&mut self) -> Task<Message> {
        // A mix in progress stops; the new song plays on the active deck.
        let cancel = self.mix_cancel("another song was started");
        Task::batch([cancel, self.start_current_track()])
    }

    fn start_current_track(&mut self) -> Task<Message> {
        self.radio.active = None;
        self.restored = None;
        self.demo_started = true;
        self.sleep_manual_start();
        self.dj_not_stalled();
        self.dj_interrupt();
        self.bump_token();
        self.playback.position = Duration::ZERO;
        self.playback.duration = None;
        self.playback.reset_scrobble();
        self.seek_preview = None;
        self.fader.pos_at = None;
        let Some(current) = self.queue.current().map(|i| i.song.clone()) else {
            return self.stop_player();
        };
        let image_task = Task::batch([
            self.request_image_on(&current.server_id, current.image.as_ref()),
            self.on_track_changed(),
        ]);
        if self.dlna_casting() {
            return Task::batch([image_task, self.dlna_load(), self.scrobble(ScrobbleEvent::NowPlaying)]);
        }
        if self.jukebox_active() {
            return Task::batch([image_task, self.jukebox_start()]);
        }
        if self.mock_playback() {
            self.playback.playing = true;
            return image_task;
        }
        let sources = self.sources.clone();
        let subs = self.substitutes.clone();
        let token = self.play_token;
        let stop = std::mem::take(&mut self.stop_first).then(|| self.decks.active_player()).flatten();
        let backup = std::mem::take(&mut self.use_backup);
        tracing::info!("playing “{}” [{}]", current.title, current.id);
        // Only the current track is resolved before playback starts; the next
        // one is prepared while it plays (see `StreamsResolved`).
        let resolve = Task::perform(
            async move {
                if let Some(player) = stop {
                    let _ = player.stop().await;
                }
                let mut track = substitute::stream_for(&sources, &subs, &current).await?;
                if backup {
                    let from = subs.used(&current).map_or(current.server_id.clone(), |own| own.server_id);
                    match sources.get(&from).and_then(|s| s.alternate_url(&track.url)) {
                        Some(url) => {
                            tracing::info!(
                                "using the backup server {}",
                                url.split('/').nth(2).unwrap_or("?")
                            );
                            track.url = url;
                        }
                        None => tracing::info!("no backup server; retrying the same stream"),
                    }
                }
                Ok(track)
            },
            move |current| Message::StreamsResolved { token, current, next: None },
        );
        Task::batch([resolve, image_task, self.scrobble(ScrobbleEvent::NowPlaying), self.substitute_warm()])
    }

    /// The current track couldn't be resolved or played: retry it once with a
    /// fresh stream (URLs expire, requests fail), then skip to the next one.
    /// Title and audio always stay in step: mpv is stopped first.
    fn playback_failed(&mut self, reason: String) -> Task<Message> {
        let Some(item) = self.queue.current() else {
            self.playback.playing = false;
            return Task::none();
        };
        let (uid, song) = (item.uid, item.song.clone());
        tracing::warn!(
            "playback failed for “{}” [{} on {}]: {reason}",
            song.title,
            song.id,
            song.server_id.0
        );
        self.stop_first = true;
        // The own copy broke: straight back to the YouTube Music version, no retries on the server.
        if self.substitutes.used(&song).is_some() {
            self.substitutes.set_denied(&song, true);
            let resume = self.playback.position;
            tracing::info!("own copy of “{}” failed; playing the YouTube Music version", song.title);
            let task = self.start_current();
            if resume > Duration::from_secs(3) {
                self.resume_at = Some((self.play_token, resume));
            }
            return task;
        }
        let (attempts, resume) = match self.retried {
            Some((u, n, at)) if u == uid => (n, at.max(self.playback.position)),
            _ => (0, self.playback.position),
        };
        // Retry ladder: backup server → fresh URL → its backup server → skip.
        if attempts < 3 {
            self.retried = Some((uid, attempts + 1, resume));
            match attempts {
                0 | 2 => self.use_backup = true,
                _ => {
                    if let Some(source) = self.sources.get(&song.server_id) {
                        source.forget_stream(&song);
                    }
                }
            }
            tracing::info!("retry {} for “{}” (resume at {resume:?})", attempts + 1, song.title);
            let task = self.start_current();
            if resume > Duration::from_secs(3) {
                self.resume_at = Some((self.play_token, resume));
            }
            return task;
        }
        self.resume_at = None;
        self.toast_error(format!("Couldn't play “{}”, skipped it. ({reason})", song.title));
        if self.queue.skip_next().is_some() {
            return self.start_current();
        }
        self.stop_first = false;
        self.stop_player()
    }

    /// Re-queues mpv's "next" slot after the queue changed.
    fn refresh_next(&mut self) -> Task<Message> {
        Task::batch([self.substitute_warm(), self.refresh_next_slot()])
    }

    fn refresh_next_slot(&mut self) -> Task<Message> {
        // The queue changed: analyse what's coming up.
        self.analysis_want();
        let Some(player) = self.decks.active_player() else { return Task::none() };
        if self.queue.current().is_none() || self.mock_playback() || self.dj_airing() || self.dlna_casting() {
            return Task::none();
        }
        if self.mix.owns_next() {
            // The second deck brings the next song in; mpv must not advance by itself.
            return Task::perform(
                async move { player.set_next(None).await.map_err(err) },
                Message::PlayerDone,
            );
        }
        let sources = self.sources.clone();
        let subs = self.substitutes.clone();
        let next = self.next_slot();
        let plan = self.mix_consider();
        if self.mix.owns_next() {
            return plan;
        }
        Task::batch([
            plan,
            Task::perform(
                async move {
                    let track = dj::resolve_slot(&sources, &subs, next).await?;
                    player.set_next(track.as_ref()).await.map_err(err)
                },
                Message::PlayerDone,
            ),
        ])
    }

    fn player_task<F, Fut>(&self, f: F) -> Task<Message>
    where
        F: FnOnce(Player) -> Fut,
        Fut: Future<Output = Result<(), listen_player::MpvError>> + Send + 'static,
    {
        match self.decks.active_player() {
            Some(p) => Task::perform(f(p), |r| Message::PlayerDone(r.map_err(err))),
            None => Task::none(),
        }
    }

    fn voice_task<F, Fut>(&self, f: F) -> Task<Message>
    where
        F: FnOnce(Player) -> Fut,
        Fut: Future<Output = Result<(), listen_player::MpvError>> + Send + 'static,
    {
        match self.voice.clone() {
            Some(p) => Task::perform(f(p), |r| Message::PlayerDone(r.map_err(err))),
            None => Task::none(),
        }
    }

    fn stop_player(&mut self) -> Task<Message> {
        self.playback.playing = false;
        self.dj_not_stalled();
        if let Some(task) = self.dlna_stop_task() {
            return task;
        }
        self.player_task(|p| async move { p.stop().await })
    }

    fn scrobble(&self, event: ScrobbleEvent) -> Task<Message> {
        if self.demo || !self.config.settings.playback.scrobble || self.private_mode() {
            return Task::none();
        }
        let Some(song) = self.queue.current().map(|i| i.song.clone()) else { return Task::none() };
        // The YouTube Music account hears it as before; the server whose copy plays counts it too.
        let own = self.substitutes.used(&song);
        let tasks: Vec<_> = [Some(song), own]
            .into_iter()
            .flatten()
            .filter_map(|song| {
                let source = self.sources.get(&song.server_id).cloned()?;
                Some(Task::perform(
                    async move { source.scrobble(&song, event).await.map_err(err) },
                    Message::PlayerDone,
                ))
            })
            .collect();
        Task::batch(tasks)
    }

    fn on_player_event(&mut self, event: PlayerEvent) -> Task<Message> {
        // A DLNA renderer plays: mpv is parked, its late events mean nothing.
        if self.dlna_casting() {
            return Task::none();
        }
        self.apply_player_event(event)
    }

    /// What a player event does to the app's state (also fed by the DLNA poll).
    fn apply_player_event(&mut self, event: PlayerEvent) -> Task<Message> {
        if let Some(task) = self.radio_on_player_event(&event) {
            return task; // a radio stream is playing: nothing of the queue's business
        }
        match event {
            PlayerEvent::Position(pos) => {
                self.playback.position = pos;
                self.fader.pos_at = Some(std::time::Instant::now());
                self.mix_position(self.decks.active, pos);
                if let Some(task) = self.dj_check_overlap() {
                    return task;
                }
                if self.dj_airing() {
                    return Task::none(); // the DJ's clip: no lyrics, no scrobbling
                }
                // Playing fine again for 10 s past the resume point: a later
                // failure of this entry starts the retry ladder afresh.
                if self.retried.is_some_and(|(_, _, at)| pos > at + Duration::from_secs(10)) {
                    self.retried = None;
                }
                if let Some(task) = self.lyrics_tick(pos) {
                    return task;
                }
                let settings = &self.config.settings;
                let (s, mode) = (&settings.playback, settings.integrations.scrobble_mode);
                let total = self.duration();
                self.playback.track_played(pos);
                if !self.playback.scrobbled
                    && total > Duration::ZERO
                    && mode.reached(s.scrobble_at_percent, s.scrobble_at_secs, self.playback.played, total)
                {
                    self.playback.scrobbled = true;
                    self.record_play();
                    return self.scrobble(ScrobbleEvent::Submission);
                }
            }
            PlayerEvent::Duration(d) => self.playback.duration = Some(d),
            PlayerEvent::Paused(paused) => {
                // A fresh mpv reports "not paused" while idle, and a restored queue stays paused
                // until the user presses play.
                if !(self.demo_audio && !self.demo_started) && self.restored.is_none() {
                    self.playback.playing = !paused;
                }
            }
            PlayerEvent::Volume(v) => tracing::trace!("mpv volume {v:.1}"),
            PlayerEvent::Muted(_)
            | PlayerEvent::Codec(_)
            | PlayerEvent::StreamTitle(_)
            | PlayerEvent::Restarted => {}
            PlayerEvent::Advanced => {
                if let Some(task) = self.dj_on_advanced() {
                    return task; // the AI DJ's break went on air
                }
                // mpv moved to the prefetched track: rotate the queue and mpv's slots.
                self.queue.advance();
                let sleep = self.sleep_advanced();
                tracing::info!(
                    "gapless advance to {:?}",
                    self.queue.current().map(|i| i.song.title.as_str()).unwrap_or("<end>")
                );
                self.bump_token();
                self.playback.position = Duration::ZERO;
                // Keep the old duration: mpv only reports a change, so a next song of
                // the same length would leave it unset and MPRIS stuck on Paused.
                self.playback.reset_scrobble();
                self.seek_preview = None;
                self.fader.pos_at = None;
                let current = self.queue.current().map(|i| (i.song.server_id.clone(), i.song.image.clone()));
                let Some(player) = self.decks.active_player() else { return Task::none() };
                // First: a DJ transition planned into the next song keeps mpv's next slot empty.
                let changed = self.on_track_changed();
                let sources = self.sources.clone();
                let subs = self.substitutes.clone();
                let next = if self.mix.owns_next() { None } else { self.next_slot() };
                let rotate = Task::perform(
                    async move {
                        let track = dj::resolve_slot(&sources, &subs, next).await.ok().flatten();
                        player.advanced(track.as_ref()).await.map_err(err)
                    },
                    Message::PlayerDone,
                );
                return Task::batch([
                    rotate,
                    match current {
                        Some((server, image)) => self.request_image_on(&server, image.as_ref()),
                        None => Task::none(),
                    },
                    self.scrobble(ScrobbleEvent::NowPlaying),
                    changed,
                    sleep,
                ]);
            }
            PlayerEvent::Ended if self.mix.owns_next() => {
                let cancel = self.mix_cancel("the song ended before the transition");
                self.queue.advance();
                return Task::batch([cancel, self.start_current()]);
            }
            PlayerEvent::Ended => {
                self.dj_interrupt();
                self.playback.playing = false;
                self.playback.position = Duration::ZERO;
                self.dj_queue_ended();
            }
            PlayerEvent::Error(e) => {
                if let Some(task) = self.dj_clip_failed() {
                    return task;
                }
                return self.playback_failed(format!("mpv: {e}"));
            }
            PlayerEvent::Crashed => {
                self.decks.set(self.decks.active, None);
                self.playback.playing = false;
                self.set_player_error("The audio player (mpv) stopped unexpectedly.".into());
            }
        }
        Task::none()
    }

    // ---- update -------------------------------------------------------------------

    fn navigate(&mut self, route: Route) -> Task<Message> {
        self.drawer_open = false;
        // A page change closes the full-screen player (Feishin: on every location change).
        self.fullscreen = false;
        self.fs.visualizer_expanded = false;
        if route != self.route {
            self.back.push(self.route.clone());
            self.forward.clear();
            self.route = route.clone();
        }
        self.on_enter(route)
    }

    /// Loads whatever a page needs when it is shown, and brings the scroll
    /// position of its item list back.
    fn on_enter(&mut self, route: Route) -> Task<Message> {
        let restore = self.list_enter(&route);
        Task::batch([restore, self.on_enter_page(route)])
    }

    fn on_enter_page(&mut self, route: Route) -> Task<Message> {
        match route {
            Route::Home => self.home_enter(),
            Route::Albums if self.library.albums.is_empty() => self.load_albums(),
            Route::Tracks if self.library.songs.is_empty() => self.load_songs(),
            Route::Album(id) => self.open_album(id),
            Route::AlbumArtists | Route::Artists if self.library.artists.is_empty() => self.load_artists(),
            Route::Artist(id) | Route::ArtistList(id, _) => self.open_artist(id),
            Route::Playlist(id) => self.open_playlist(id),
            Route::Playlists => self.playlists_enter(),
            Route::Radio => self.radio_enter(),
            Route::Settings => self.jukebox_probe(),
            Route::Recent | Route::Stats | Route::Mixes | Route::Mix(_) => {
                self.refresh_history_views();
                Task::batch([self.reset_list_scroll(), self.list_visible()])
            }
            Route::Genres | Route::Genre(_) | Route::Favorites | Route::Folders | Route::Folder(_) => {
                self.extra_enter(route)
            }
            _ => Task::none(),
        }
    }

    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let task = self.update_message(message);
        // The queue panel's search result and selection follow the queue.
        self.queue_ui_sync();
        self.queue_drawer_settle();
        match self.queue_surface_changed() {
            Some(follow) => Task::batch([task, follow]),
            None => task,
        }
    }

    fn update_message(&mut self, message: Message) -> Task<Message> {
        if matches!(message, Message::ImageLoaded(..)) {
            self.integ.cover_dirty();
        }
        #[cfg(any(target_os = "linux", windows))]
        if matches!(message, Message::ImageLoaded(..)) {
            // Maybe the playing song's cover: MPRIS looks for its file again.
            self.published.art.take_if(|(_, art)| art.is_none());
        }
        let video = matches!(
            &message,
            Message::ToggleVideo
                | Message::TogglePlay
                | Message::Stop
                | Message::Seek(_)
                | Message::Next
                | Message::Previous
        );
        let toggled = matches!(message, Message::ToggleVideo) && !self.dlna_casting();
        self.video.seeked |= matches!(message, Message::Seek(_));
        if toggled {
            self.video.on = !self.video.on;
            self.fullscreen |= self.video.on;
        }
        let task = self.handle(message);
        let task = if video { Task::batch([task, self.video_sync()]) } else { task };
        let task = if std::mem::take(&mut self.dj.stop_voice) {
            Task::batch([task, self.voice_task(|v| async move { v.stop().await })])
        } else {
            task
        };
        #[cfg(any(target_os = "linux", windows))]
        let task = Task::batch([task, self.sync_mpris(), self.sync_tray()]);
        self.sync_inhibit();
        Task::batch([task, self.sync_integrations()])
    }

    #[cfg(any(target_os = "linux", windows))]
    fn sync_tray(&mut self) -> Task<Message> {
        let Some(handle) = self.tray.clone() else { return Task::none() };
        let key = (self.playback.playing, self.queue.current().map(|i| i.uid));
        if self.published.tray == Some(key) {
            return Task::none();
        }
        self.published.tray = Some(key);
        let title = self
            .queue
            .current()
            .map(|i| format!("{} — {}", i.song.title, i.song.artist_line()))
            .unwrap_or_default();
        let state = (self.playback.playing, title);
        if state == self.tray_published {
            return Task::none();
        }
        self.tray_published = state.clone();
        Task::future(handle.set(state.0, state.1)).discard()
    }

    fn on_key(&mut self, event: iced::keyboard::Event) -> Task<Message> {
        match &event {
            iced::keyboard::Event::KeyPressed { modifiers, .. }
            | iced::keyboard::Event::KeyReleased { modifiers, .. }
            | iced::keyboard::Event::ModifiersChanged(modifiers) => self.modifiers = *modifiers,
        }
        let iced::keyboard::Event::KeyPressed { key, modifiers, physical_key, .. } = event else {
            return Task::none();
        };
        // The command palette has its own keys (palette::subscription) and eats the rest.
        if self.palette_open() {
            return Task::none();
        }
        if self.record_hotkey(&key, &physical_key, modifiers) {
            return Task::none();
        }
        // The open context menu takes the arrows and Enter (its pages with a text field keep them).
        if self.menu.as_ref().is_some_and(|m| matches!(m.page, menu::MenuPage::Main))
            && let Some(k) = menu::MenuKey::from_key(&key)
        {
            return self.on_menu(menu::MenuMsg::Key(k));
        }
        if key == iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape)
            && self.menu.is_none()
            && !self.drawer_open
            && !self.volume_popup
            && let Some(task) = self.fullscreen_escape()
        {
            return task;
        }
        if key == iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape)
            && (self.menu.is_some() || self.drawer_open || self.volume_popup)
        {
            if self.menu.is_some() {
                return self.on_menu(menu::MenuMsg::Back);
            } else {
                self.drawer_open = false;
                self.volume_popup = false;
            }
            return Task::none();
        }
        // Delete / Backspace take the selected rows out of the queue on screen.
        if let Some(task) = self.queue_key(&key, modifiers) {
            return task;
        }
        // Arrows, Home / End, Page Up / Down and Ctrl+A act on the shown item list.
        if let Some(task) = self.list_key(&key, modifiers) {
            return task;
        }
        let Some(name) = crate::hotkeys::press_name(&key, &physical_key) else { return Task::none() };
        // Actions the portal handles system-wide do not also fire from the window.
        let global = if self.portal_active() && self.global_error.is_none() {
            self.config.settings.hotkeys_global.clone()
        } else {
            Default::default()
        };
        let actions: Vec<String> =
            crate::hotkeys::actions_for(&self.config.settings.hotkeys, &global, &name, modifiers)
                .into_iter()
                .map(str::to_string)
                .collect();
        let mut tasks = Vec::new();
        for action in actions {
            if action == "globalSearch" {
                tasks.push(self.open_palette());
            } else if action == "localSearch" {
                tasks.push(iced::widget::operation::focus(iced::widget::Id::new(ui::sidebar::SEARCH_ID)));
            } else if let Some(message) = self.hotkey_message(&action) {
                tasks.push(self.handle(message));
            }
        }
        Task::batch(tasks)
    }

    /// Pushes playback settings (gapless, ReplayGain, EQ) to mpv.
    fn apply_audio(&self) -> Task<Message> {
        Task::batch([self.apply_audio_to(0), self.apply_audio_to(1), self.apply_speed()])
    }

    /// Audio settings for one deck; with DJ transitions on, its filter chain
    /// includes the mix filters and a permanent tempo filter.
    fn apply_audio_to(&self, deck: usize) -> Task<Message> {
        let audio = self.audio_settings_for(deck);
        match self.decks.d[deck.min(1)].player.clone() {
            Some(p) => {
                Task::perform(async move { p.apply(&audio).await }, |r| Message::PlayerDone(r.map_err(err)))
            }
            None => Task::none(),
        }
    }

    #[cfg(any(target_os = "linux", windows))]
    fn show_window(&mut self) -> Task<Message> {
        self.reveal_main_window()
    }

    /// Publishes player state to the desktop when it changed.
    #[cfg(any(target_os = "linux", windows))]
    fn sync_mpris(&mut self) -> Task<Message> {
        let Some(handle) = self.mpris.clone() else { return Task::none() };
        let position = self.playback.position.as_secs_f64();
        handle.set_position(position);
        // Everything the state depends on but the position, cheap to compare
        // (this runs after every message).
        let art = self.mpris_art();
        let key = MprisKey {
            uid: self.queue.current().map(|i| i.uid),
            playing: self.playback.playing,
            duration: self.duration(),
            audible: self.playback.duration.is_some() || self.mock_playback(),
            volume: (self.playback.volume, self.playback.muted),
            shuffle: self.play_mode() == play_mode::PlayMode::Shuffle,
            repeat: self.queue.repeat,
            has_next: self.queue.peek_next().is_some(),
            art: art.is_some(),
            lyrics_rev: self.lyrics_ui.rev,
        };
        if self.published.mpris == Some(key) {
            return Task::none();
        }
        self.published.mpris = Some(key);
        let state = self.mpris_state(art);
        let mut published = state.clone();
        published.position_secs = 0.0;
        let mut previous = self.mpris_published.clone();
        previous.position_secs = 0.0;
        if published == previous {
            return Task::none();
        }
        self.mpris_published = state.clone();
        Task::future(handle.publish(state)).discard()
    }

    /// The playing song's cover file as a `file://` URL, looked up once per
    /// song (and again when an image arrives while it has none).
    #[cfg(any(target_os = "linux", windows))]
    fn mpris_art(&mut self) -> Option<String> {
        let item = self.queue.current()?;
        if let Some((uid, art)) = &self.published.art
            && *uid == item.uid
        {
            return art.clone();
        }
        let art = item
            .song
            .image
            .as_ref()
            .and_then(|i| self.images.disk_path(&self.image_key(i)))
            .filter(|p| p.exists())
            .map(|p| format!("file://{}", p.display()));
        self.published.art = Some((item.uid, art.clone()));
        art
    }

    #[cfg(any(target_os = "linux", windows))]
    fn mpris_state(&self, art_url: Option<String>) -> MprisState {
        use listen_core::queue::Repeat;
        let track = self.queue.current().map(|item| {
            let s = &item.song;
            mpris::Track {
                uid: item.uid,
                title: s.title.clone(),
                artists: s.artists.iter().map(|a| a.name.clone()).collect(),
                album: s.album.clone(),
                album_artists: s.album_artists.iter().map(|a| a.name.clone()).collect(),
                length_secs: self.duration().as_secs_f64(),
                art_url,
                track_number: s.track_number.map(|n| n as i32),
                extra: mpris::Extra {
                    genres: s.genres.clone(),
                    disc_number: s.disc_number.map(|n| n as i32),
                    bpm: s.bpm.map(|n| n as i32),
                    comment: s.comment.clone().filter(|c| !c.is_empty()),
                    content_created: s.year.map(|y| format!("{y:04}-01-01T00:00:00Z")),
                    last_used: self
                        .history
                        .entries
                        .iter()
                        .filter(|e| e.song_id == s.id && e.server_id == s.server_id)
                        .map(|e| e.ts)
                        .max()
                        .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
                        .map(|d| d.to_rfc3339()),
                    use_count: s.play_count.map(|n| n as i32),
                    // Navidrome/Subsonic rate 1-5.
                    user_rating: s.rating.filter(|r| *r > 0).map(|r| f64::from(r.min(5)) / 5.0),
                    lyrics: self.lyrics_mpris_text(),
                },
            }
        });
        // "Playing" only once mpv has opened the file (it reports a duration): clients like
        // Music Presence treat "playing at 0:00" as a broken player.
        let audible = self.playback.duration.is_some() || self.mock_playback();
        MprisState {
            status: match (&track, self.playback.playing && audible) {
                (None, _) => mpris::Status::Stopped,
                (Some(_), true) => mpris::Status::Playing,
                (Some(_), false) => mpris::Status::Paused,
            },
            track,
            position_secs: self.playback.position.as_secs_f64(),
            volume: if self.playback.muted { 0.0 } else { f64::from(self.playback.volume) / 100.0 },
            shuffle: self.play_mode() == play_mode::PlayMode::Shuffle,
            repeat: match self.queue.repeat {
                Repeat::Off => mpris::RepeatMode::None,
                Repeat::One => mpris::RepeatMode::Track,
                Repeat::All => mpris::RepeatMode::Playlist,
            },
            has_next: self.queue.peek_next().is_some() || self.queue.repeat != Repeat::Off,
            has_previous: self.queue.current().is_some(),
        }
    }

    #[cfg(any(target_os = "linux", windows))]
    fn on_mpris(&mut self, msg: MprisMsg) -> Task<Message> {
        use listen_core::queue::Repeat;
        let cmd = match msg {
            MprisMsg::Ready(handle) => {
                self.mpris = Some(handle);
                self.mpris_published = MprisState::default();
                self.published.mpris = None;
                return Task::none();
            }
            MprisMsg::Command(cmd) => cmd,
        };
        let message = match cmd {
            MprisCmd::PlayPause => Message::TogglePlay,
            MprisCmd::Play if !self.playback.playing => Message::TogglePlay,
            MprisCmd::Pause if self.playback.playing => Message::TogglePlay,
            MprisCmd::Play | MprisCmd::Pause => return Task::none(),
            MprisCmd::Stop => Message::Stop,
            MprisCmd::Next => Message::Next,
            MprisCmd::Previous => Message::Previous,
            MprisCmd::SeekBy(delta) => {
                Message::Seek((self.playback.position.as_secs_f64() + delta).max(0.0) as f32)
            }
            MprisCmd::SetPosition(pos) => Message::Seek(pos as f32),
            MprisCmd::Volume(v) => Message::Volume((v * 100.0) as f32),
            MprisCmd::Shuffle(on) if on != (self.play_mode() == play_mode::PlayMode::Shuffle) => {
                Message::SetPlayMode(if on { play_mode::PlayMode::Shuffle } else { play_mode::PlayMode::Off })
            }
            MprisCmd::Shuffle(_) => return Task::none(),
            MprisCmd::Loop(status) => {
                self.queue.repeat = match status {
                    mpris::LoopStatus::None => Repeat::Off,
                    mpris::LoopStatus::Track => Repeat::One,
                    mpris::LoopStatus::Playlist => Repeat::All,
                };
                return self.refresh_next();
            }
            MprisCmd::Raise => return self.reveal_main_window(),
        };
        let seek = matches!(message, Message::Seek(_));
        let task = self.handle(message);
        if seek && let Some(handle) = self.mpris.clone() {
            let pos = self.playback.position.as_secs_f64();
            return Task::batch([task, Task::future(handle.seeked(pos)).discard()]);
        }
        task
    }

    fn handle(&mut self, message: Message) -> Task<Message> {
        if let Some(task) = self.playback_intercept(&message) {
            return task; // a radio station or the jukebox owns playback
        }
        match message {
            Message::Navigate(route) => return self.navigate(route),
            Message::Back => {
                if let Some(route) = self.back.pop() {
                    self.forward.push(std::mem::replace(&mut self.route, route.clone()));
                    return self.on_enter(route);
                }
            }
            Message::Forward => {
                if let Some(route) = self.forward.pop() {
                    self.back.push(std::mem::replace(&mut self.route, route.clone()));
                    return self.on_enter(route);
                }
            }
            Message::SystemTheme(mode) => {
                self.system_mode = mode;
                self.apply_theme();
            }
            Message::ScaleFactor(scale) => self.scale_factor = scale,
            Message::WindowResized(size) => {
                self.window_size = size;
                if self.layout_mode() == ui::LayoutMode::Full {
                    self.drawer_open = false;
                    self.volume_popup = false;
                    self.mobile_panel = None;
                }
            }
            Message::WindowFocused(true) => {
                self.focused = true;
                self.minimized = false;
            }
            Message::WindowFocused(false) => {
                self.focused = false;
                let Some(id) = self.main_window else { return Task::none() };
                return window::is_minimized(id)
                    .map(|minimized| Message::Minimized(minimized.unwrap_or(false)));
            }
            Message::Minimized(minimized) => {
                self.minimized = minimized;
                // Minimized by the desktop's own button (X11): go to the tray.
                if minimized
                    && self.minimize_to_tray()
                    && let Some(task) = self.hide_to_tray()
                {
                    return task;
                }
            }
            Message::QueueScrolled(viewport) => self.queue_scroll = scroll_state(&viewport),
            Message::ForServer(id, message) => {
                if self.config.active_server.as_ref() == Some(&id) {
                    return self.handle(*message);
                }
                tracing::debug!("dropping a late result for {} (switched servers)", id.0);
            }
            Message::DesktopTheme(theme) => {
                self.desktop_theme = theme;
                self.apply_theme();
            }
            Message::TogglePlay => {
                if self.queue.current().is_none() {
                    return Task::none();
                }
                if let Some(at) = self.restored.take() {
                    let task = self.start_current();
                    if at > Duration::from_secs(3) {
                        self.resume_at = Some((self.play_token, at));
                    }
                    return task;
                }
                if self.demo_audio && !self.demo_started && !self.playback.playing {
                    return self.start_current();
                }
                if self.mock_playback() || !self.decks.has_active() {
                    self.playback.playing = !self.playback.playing;
                    return Task::none();
                }
                let pause = self.playback.playing;
                self.playback.playing = !pause;
                // Pausing mid-transition drops the transition (one deck keeps the song).
                // Resuming plans the transition again from where the song is.
                let cancel = if pause { self.mix_cancel("paused") } else { self.mix_consider() };
                if self.config.settings.playback.fade_on_pause {
                    return Task::batch([cancel, self.fade_pause(pause)]);
                }
                return Task::batch([
                    cancel,
                    self.player_task(move |p| async move { p.set_paused(pause).await }),
                    self.voice_task(move |v| async move { v.set_paused(pause).await }),
                ]);
            }
            Message::Stop => {
                self.playback.position = Duration::ZERO;
                if self.mock_playback() {
                    self.playback.playing = false;
                    return Task::none();
                }
                // Keep the track loaded but rewind and pause (Feishin's stop).
                self.playback.playing = false;
                let cancel = self.mix_cancel("stopped");
                self.reset_pause_fade();
                self.dj_interrupt();
                return Task::batch([
                    cancel,
                    self.player_task(|p| async move {
                        p.set_paused(true).await?;
                        p.seek(Duration::ZERO).await
                    }),
                    self.push_volume(true),
                ]);
            }
            Message::Next => {
                let (mix, done) = self.mix_skip_next();
                if done {
                    return mix;
                }
                self.dj_on_next();
                if self.queue.skip_next().is_some() {
                    return self.start_current();
                }
                return self.stop_player();
            }
            Message::Previous => {
                // Like Feishin: restart the song if more than 10s in.
                if self.playback.position > Duration::from_secs(10) {
                    return self.update(Message::Seek(0.0));
                }
                self.queue.skip_previous();
                return self.start_current();
            }
            Message::SeekPreview(_)
            | Message::SeekRelease
            | Message::NextClick
            | Message::PreviousClick
            | Message::NextAlbum
            | Message::PreviousAlbum
            | Message::ShuffleAll => return self.handle_transport(message),
            Message::SkipBackward => {
                let secs = self.config.settings.player_bar.skip_backward_secs;
                let pos = self.playback.position.saturating_sub(Duration::from_secs(secs.into()));
                return self.update(Message::Seek(pos.as_secs_f32()));
            }
            Message::SkipForward => {
                let secs = self.config.settings.player_bar.skip_forward_secs;
                let pos = self.playback.position + Duration::from_secs(secs.into());
                return self.update(Message::Seek(pos.as_secs_f32()));
            }
            Message::Seek(secs) => {
                let pos = Duration::from_secs_f32(secs.max(0.0));
                if self.mock_playback() {
                    self.demo_seek(pos);
                    return Task::none();
                }
                self.playback.position = pos;
                let cancel = self.mix_cancel("seek");
                return Task::batch([cancel, self.player_task(move |p| async move { p.seek(pos).await })]);
            }
            Message::Volume(v) => {
                self.playback.volume = v.round().clamp(0.0, 100.0) as u8;
                self.playback.muted = false;
                self.config.settings.playback.volume = self.playback.volume;
                let volume = f32::from(self.playback.volume);
                let music = self.effective_volume();
                self.fader.last_sent = Some(music);
                let idle = self.push_idle_volume(true);
                return Task::batch([
                    idle,
                    self.player_task(move |p| async move {
                        p.set_muted(false).await?;
                        p.set_volume(music).await
                    }),
                    self.voice_task(move |v| async move {
                        v.set_muted(false).await?;
                        v.set_volume(volume).await
                    }),
                ]);
            }
            Message::ToggleMute => {
                self.playback.muted = !self.playback.muted;
                let muted = self.playback.muted;
                let idle = self.decks.idle_player();
                return Task::batch([
                    self.player_task(move |p| async move { p.set_muted(muted).await }),
                    self.voice_task(move |v| async move { v.set_muted(muted).await }),
                    match idle {
                        Some(p) => Task::perform(
                            async move { p.set_muted(muted).await.map_err(err) },
                            Message::PlayerDone,
                        ),
                        None => Task::none(),
                    },
                ]);
            }
            Message::ToggleShuffle => return self.set_play_mode(self.next_play_mode()),
            Message::SetPlayMode(mode) => return self.set_play_mode(mode),
            Message::CycleRepeat => {
                self.queue.repeat = self.queue.repeat.cycle();
                return self.refresh_next();
            }
            Message::ToggleQueue => {
                self.show_queue = !self.show_queue;
                self.queue_scroll.offset = 0.0;
            }
            Message::ResizeStart(side) => {
                let sidebar = &self.config.settings.sidebar;
                let width = match side {
                    PanelSide::Left => sidebar.width,
                    PanelSide::Right => sidebar.queue_width,
                };
                self.resizing = Some(Resize { side, start_x: None, start_width: width });
            }
            Message::ResizeMove(x) => {
                if let Some(drag) = &mut self.resizing {
                    // The first move after the press is the anchor.
                    let start_x = *drag.start_x.get_or_insert(x);
                    let delta = x - start_x;
                    let sidebar = &mut self.config.settings.sidebar;
                    match drag.side {
                        // Feishin: dragging under 160 px folds the sidebar to the rail, dragging
                        // back brings it out again.
                        PanelSide::Left => {
                            let wanted = drag.start_width + delta;
                            sidebar.collapsed = wanted < SIDEBAR_COLLAPSE_BELOW;
                            if !sidebar.collapsed {
                                sidebar.width = wanted.clamp(SIDEBAR_MIN, SIDEBAR_MAX);
                            }
                        }
                        PanelSide::Right => {
                            sidebar.queue_width = (drag.start_width - delta).clamp(QUEUE_MIN, QUEUE_MAX)
                        }
                    }
                }
            }
            Message::ResizeEnd => {
                if self.resizing.take().is_some() {
                    self.save_config();
                }
            }
            Message::ToggleSidebar => {
                let sidebar = &mut self.config.settings.sidebar;
                sidebar.collapsed = !sidebar.collapsed;
                self.save_config();
            }
            Message::ToggleCoverExpanded => {
                let sidebar = &mut self.config.settings.sidebar;
                sidebar.cover_expanded = !sidebar.cover_expanded;
                self.save_config();
            }
            Message::PlayQueueItem(uid) => {
                self.queue.jump_to(uid);
                return self.start_current();
            }
            Message::PlayDemoAlbum(index) => {
                self.queue.set(demo::album_songs(index), 0);
                return self.start_current();
            }
            Message::SaveQueue => self.save_queue(),
            Message::Tick => {
                // Simulated playback clock (demo mode only).
                if self.playback.playing {
                    self.demo_seek(self.playback.position + Duration::from_millis(500));
                }
            }
            Message::ScreenshotTaken(shot) => {
                if let Some(path) = self.screenshot.take() {
                    match save_png(&path, &shot) {
                        Ok(()) => tracing::info!("screenshot saved to {}", path.display()),
                        Err(e) => tracing::error!("screenshot failed: {e}"),
                    }
                }
                return iced::exit();
            }

            Message::Login(msg) => match msg {
                LoginMsg::Url(v) => self.login.url = v,
                LoginMsg::Username(v) => self.login.username = v,
                LoginMsg::Password(v) => self.login.password = v,
                LoginMsg::Kind(kind) => {
                    self.login.kind = kind;
                    self.login.url.clear();
                    self.login.password.clear();
                    self.login.error = None;
                    self.login.info = None;
                    if kind == SourceKind::YouTubeMusic {
                        return self.find_browser();
                    }
                }
                LoginMsg::Submit if !self.login.busy => return self.submit_login(),
                LoginMsg::Submit => {}
                LoginMsg::Browse => {
                    return Task::perform(pick_cookie_file(), |p| Message::Login(LoginMsg::FileChosen(p)));
                }
                LoginMsg::FileChosen(Some(path)) => {
                    self.load_cookie_file(&path);
                }
                LoginMsg::FileChosen(None) => {}
                LoginMsg::BrowseFolder => {
                    return Task::perform(pick_music_folder(), |p| Message::Login(LoginMsg::FolderChosen(p)));
                }
                LoginMsg::FolderChosen(Some(path)) => self.login.url = path.display().to_string(),
                LoginMsg::FolderChosen(None) => {}
                LoginMsg::BrowserFound(Ok(found)) => {
                    let first = found.iter().find(|(_, usable)| usable.is_ok()).map(|(p, _)| p.clone());
                    let problems: Vec<String> =
                        found.iter().filter_map(|(_, usable)| usable.clone().err()).collect();
                    self.login.browser =
                        Some(first.ok_or_else(|| listen_ytmusic::browser::why_unusable(&problems)));
                    self.login.browsers = found;
                }
                LoginMsg::BrowserFound(Err(e)) => {
                    self.login.browsers.clear();
                    self.login.browser = Some(Err(e));
                }
                LoginMsg::OtherBrowser(i) => {
                    let Some((profile, _)) = self.login.browsers.get(i).cloned() else { return Task::none() };
                    self.login.busy = true;
                    return Task::perform(
                        async move {
                            tokio::task::spawn_blocking(move || profile.youtube_header().map(|_| ()))
                                .await
                                .unwrap_or_else(|e| Err(e.to_string()))
                        },
                        move |r| Message::Login(LoginMsg::BrowserChecked(i, r)),
                    );
                }
                LoginMsg::BrowserChecked(i, usable) => {
                    self.login.busy = false;
                    let Some(entry) = self.login.browsers.get_mut(i) else { return Task::none() };
                    entry.1 = usable;
                    if entry.1.is_ok() {
                        self.login.browser = Some(Ok(entry.0.clone()));
                        self.login.channels.clear();
                        self.login.error = None;
                        self.login.show_browsers = false;
                    }
                }
                LoginMsg::ToggleBrowsers => self.login.show_browsers = !self.login.show_browsers,
                LoginMsg::FindBrowser => return self.find_browser(),
                LoginMsg::UseBrowser => {
                    let Some(Ok(profile)) = self.login.browser.clone() else { return Task::none() };
                    self.login.busy = true;
                    self.login.error = None;
                    return Task::perform(
                        async move {
                            let header = tokio::task::spawn_blocking(move || profile.youtube_header())
                                .await
                                .map_err(|e| e.to_string())??;
                            listen_ytmusic::channels::list(&header).await
                        },
                        |r| Message::Login(LoginMsg::Channels(r)),
                    );
                }
                LoginMsg::Channels(Ok(channels)) => {
                    self.login.busy = false;
                    self.login.channels = channels;
                    if self.login.channels.len() == 1 {
                        return self.update(Message::Login(LoginMsg::PickChannel(0)));
                    }
                }
                LoginMsg::Channels(Err(e)) => {
                    self.login.busy = false;
                    self.login.error = Some(e);
                }
                LoginMsg::PickChannel(i) => {
                    let (Some(Ok(mut profile)), Some(channel)) =
                        (self.login.browser.clone(), self.login.channels.get(i).cloned())
                    else {
                        return Task::none();
                    };
                    profile.page_id = channel.page_id;
                    profile.channel = Some(channel.name.clone());
                    let server = ServerConfig {
                        id: ServerId(format!("ytm-{:x}", unix_millis())),
                        name: format!("YouTube Music ({})", channel.name),
                        kind: SourceKind::YouTubeMusic,
                        url: String::new(),
                        username: String::new(),
                    };
                    return self.connect_new(server, profile.to_secret());
                }
                LoginMsg::ToggleCookies => self.login.show_cookies = !self.login.show_cookies,
            },
            Message::FileDropped(path) => {
                // On the sign-in page: a cookies file. On Settings: a Feishin export.
                if self.needs_login() {
                    self.load_cookie_file(&path);
                } else if self.route == Route::Settings {
                    self.import_path = path.display().to_string();
                    return self.change_setting(Setting::ImportFeishin);
                }
            }
            Message::LoggedIn(Ok((server, secret, handle))) => {
                if let Some(secrets) =
                    self.secrets.as_ref().filter(|_| !self.ephemeral && server.kind != SourceKind::Local)
                {
                    secrets.set(&server.id, &secret);
                }
                // Signing in to the same account again replaces the old entry.
                let replaced: Vec<(ServerId, String)> = self
                    .config
                    .servers
                    .iter()
                    .filter(|s| s.kind == server.kind && s.url == server.url && s.username == server.username)
                    .map(|s| (s.id.clone(), s.url.clone()))
                    .collect();
                let replaced_ids: Vec<ServerId> = replaced.iter().map(|(id, _)| id.clone()).collect();
                self.config.servers.retain(|s| !replaced_ids.contains(&s.id));
                for (id, url) in &replaced {
                    self.sources.remove(id);
                    self.config.server_options.remove(&id.0);
                    self.config.music_dirs.remove(&id.0);
                    if server.kind == SourceKind::Local {
                        listen_local::forget_cache(id, std::path::Path::new(url));
                    }
                }
                let id = server.id.clone();
                self.commit_pending_options(&id);
                self.config.servers.push(server);
                self.save_config();
                self.login = LoginForm::default();
                return self.on_connected(id, handle.0);
            }
            Message::LoggedIn(Err(e)) => {
                self.login.busy = false;
                self.login.error = Some(e);
            }
            Message::Connected(id, Ok(handle)) => {
                handle.0.set_stream_quality(self.config.settings.playback.stream_quality);
                handle.0.set_transcode(&self.config.settings.playback.transcode);
                tracing::info!("connected to {}", id.0);
                if handle.0.kind() == SourceKind::YouTubeMusic
                    && handle.0.capabilities().contains(listen_source::Capabilities::DOWNLOAD)
                    && let Some(store) = self.offline_store()
                {
                    store.mark_verified(&id);
                }
                if let Some(store) = self.offline_store()
                    && store.expire_unverified() > 0
                {
                    self.offline_refresh();
                }
                let browse = self.source.is_none() || self.config.active_server.as_ref() == Some(&id);
                if browse {
                    self.connecting = false;
                    return self.on_connected(id, handle.0);
                }
                // A background connection: available for switching and playback.
                self.sources.insert(id.clone(), handle.0);
                return self.queue_covers(&id);
            }
            Message::Connected(id, Err(e)) => {
                // A YouTube Music sign-in that no longer works: its offline copies go.
                if e.starts_with("authentication failed")
                    && self.config.servers.iter().any(|s| s.id == id && s.kind == SourceKind::YouTubeMusic)
                    && let Some(store) = self.offline_store()
                {
                    self.offline_cancel(Some(&id));
                    store.remove_server(&id);
                    self.offline_refresh();
                }
                if self.config.active_server.as_ref() != Some(&id) && self.source.is_some() {
                    tracing::warn!("background connection to {} failed: {e}", id.0);
                    return Task::none();
                }
                self.connecting = false;
                let name = self
                    .config
                    .servers
                    .iter()
                    .find(|s| s.id == id)
                    .map(|s| s.name.clone())
                    .unwrap_or_default();
                if self.source.is_some() {
                    // Another server is still usable; just report it.
                    self.toast_error(format!("Could not connect to {name}: {e}"));
                } else if self.config.servers.iter().any(|s| s.id == id) {
                    // No server works: the action-required page, with what to do.
                    self.servers.failure = Some(if e.starts_with("Wrong username or password") {
                        servers::Failure::Credentials { id }
                    } else {
                        servers::Failure::Unreachable { id, reason: e }
                    });
                }
            }
            Message::RescanServer(id) => {
                self.sources.remove(&id);
                if self.config.active_server.as_ref() == Some(&id) {
                    self.connecting = true;
                    return self.connect_server_mode(id, true);
                }
                return self.connect_background_mode(id, true);
            }
            Message::SwitchServer(id) => {
                self.adding_server = false;
                if self.config.active_server.as_ref() == Some(&id) && self.source.is_some() {
                    return self.navigate(Route::Home);
                }
                if let Some(source) = self.sources.get(&id).cloned() {
                    return self.activate(id, source);
                }
                return self.connect_server(id);
            }
            Message::AddServer => {
                self.login = LoginForm::default();
                self.servers.quick_connect = None;
                self.adding_server = true;
                return self.start_discovery();
            }
            Message::CancelAddServer => {
                self.adding_server = false;
                self.servers.quick_connect = None;
                self.login = LoginForm::default();
            }
            Message::ConfirmRemoveServer(id) => self.confirm_remove = id,
            Message::RemoveServer(id) => {
                self.confirm_remove = None;
                self.offline_cancel(Some(&id));
                if let Some(store) = self.offline_store() {
                    store.remove_server(&id);
                    self.offline_refresh();
                }
                if let Some(secrets) = &self.secrets {
                    secrets.delete(&id);
                }
                if let Some(server) = self.config.servers.iter().find(|s| s.id == id)
                    && server.kind == SourceKind::Local
                {
                    listen_local::forget_cache(&id, std::path::Path::new(&server.url));
                }
                self.config.servers.retain(|s| s.id != id);
                self.sources.remove(&id);
                self.library_cache.remove(&id);
                // Songs from that server can't play any more.
                let doomed: Vec<u64> =
                    self.queue.iter().filter(|i| i.song.server_id == id).map(|i| i.uid).collect();
                let current_removed = self.queue.current().is_some_and(|i| i.song.server_id == id);
                for uid in doomed {
                    self.queue.remove(uid);
                }
                let mut tasks = vec![if current_removed { self.stop_player() } else { self.refresh_next() }];
                if self.config.active_server.as_ref() == Some(&id) {
                    self.config.active_server = None;
                    self.source = None;
                    self.library = Library::default();
                    self.route = Route::Home;
                    // Fall back to another configured server, if any.
                    if let Some(next) = self.config.servers.first().map(|s| s.id.clone()) {
                        tasks.push(self.handle(Message::SwitchServer(next)));
                    }
                }
                self.save_config();
                return Task::batch(tasks);
            }
            Message::Logout => {
                if let Some(id) = self.config.active_server.clone() {
                    return self.handle(Message::RemoveServer(id));
                }
            }
            Message::OpenAlbum(id) => return self.navigate(Route::Album(id)),
            Message::PlayAlbum(id) => {
                if let Some(index) = demo::album_index(&id).filter(|_| self.demo) {
                    return self.update(Message::PlayDemoAlbum(index));
                }
                let Some(source) = self.source.clone() else { return Task::none() };
                return Task::perform(
                    async move { source.album(&id).await.map_err(err) },
                    Message::AlbumForPlay,
                );
            }
            Message::PlayAlbumWith(album, behavior) => {
                return self.play_items(vec![list::Picked::Album(*album)], behavior);
            }
            Message::AlbumForPlay(Ok(album)) => {
                if std::mem::take(&mut self.autoplay_dj) {
                    return self.play_picked(album.songs, 0);
                }
                return self.play_songs(album.songs, 0);
            }
            Message::AlbumForPlay(Err(e)) => self.toast_error(format!("Could not load the album: {e}")),
            Message::AlbumLoaded(Ok(album)) => {
                let image = album.image.clone();
                self.library.album = Some(album);
                let page = self.album_page_loaded();
                return Task::batch([
                    self.request_image(image.as_ref()),
                    self.request_blur(image.as_ref()),
                    page,
                ]);
            }
            Message::AlbumLoaded(Err(e)) => self.library.album_error = Some(e),
            Message::PlayAlbumTrack(index) => {
                if let Some(album) = &self.library.album {
                    return self.play_picked(album.songs.clone(), index);
                }
            }
            Message::PlaySongAt(index) => {
                // Like Feishin's "play from here": queue the loaded list.
                return self.play_picked(self.library.songs.clone(), index);
            }
            Message::AlbumsLoaded(gen_, _) if gen_ != self.list_queries.albums_gen => {}
            Message::AlbumsLoaded(_, result) => {
                self.library.albums_loading = false;
                match result {
                    Ok(page) => {
                        self.library.albums_done = !page.has_more() || page.items.len() < ALBUM_PAGE;
                        self.library.albums.extend(page.items);
                        return self.albums_visible();
                    }
                    Err(e) => {
                        self.library.albums_done = true;
                        self.toast_error(format!("Loading albums failed: {e}"));
                    }
                }
            }
            Message::SongsLoaded(gen_, _) if gen_ != self.list_queries.songs_gen => {}
            Message::SongsLoaded(_, result) => {
                self.library.songs_loading = false;
                match result {
                    Ok(page) => {
                        self.library.songs_done = page.items.len() < SONG_PAGE;
                        self.library.songs.extend(page.items);
                        return self.songs_visible();
                    }
                    Err(e) => {
                        self.library.songs_done = true;
                        self.toast_error(format!("Loading tracks failed: {e}"));
                    }
                }
            }
            Message::HomeLoaded(rows) => {
                self.library.home_loaded = true;
                let refs: Vec<ImageRef> = rows
                    .iter()
                    .flat_map(|row| {
                        let albums = row.albums.iter().take(10).map(|a| a.image.clone());
                        let playlists = row.playlists.iter().take(10).map(|p| p.image.clone());
                        let songs = row.songs.iter().take(10).map(|s| s.image.clone());
                        albums.chain(playlists).chain(songs)
                    })
                    .flatten()
                    .collect();
                // `--autoplay`: the feed's first album, else the first section that arrives (home.rs).
                let autoplay = rows
                    .iter()
                    .find_map(|r| r.albums.first())
                    .map(|a| a.id.clone())
                    .filter(|_| std::mem::take(&mut self.autoplay));
                self.library.home_state.feed_first.clear();
                self.library.home = rows;
                let images = self.request_images(refs.iter().map(Some));
                return match autoplay {
                    Some(id) => self.start_autoplay(id, images),
                    None => images,
                };
            }
            Message::List(msg) => return self.on_list(msg),
            Message::AlbumPage(msg) => return self.on_album_page(msg),
            Message::Home(msg) => return self.on_home(msg),
            Message::Palette(msg) => return self.on_palette(msg),
            Message::Item(msg) => return self.on_item(msg),
            Message::Search(msg) => return self.on_search(msg),
            Message::Extra(msg) => return self.on_extra(msg),
            Message::Smart(msg) => return self.on_smart(msg),
            Message::ListQuery(msg) => return self.on_list_query(msg),
            Message::ListScrolled(viewport) => {
                self.list_scroll = scroll_state(&viewport);
                return self.list_visible();
            }
            Message::ImageLoaded(key, bytes) => {
                let explicit = self.explicit_blur_job(&key, bytes.as_ref());
                self.images.loaded(key, bytes);
                return explicit;
            }
            Message::BlurLoaded(key, handle) => self.images.blur_loaded(key, handle),
            Message::StreamsResolved { token, current, next } => {
                if token != self.play_token {
                    return Task::none(); // the user already moved on
                }
                match current {
                    Ok(current) => {
                        self.playback.playing = true;
                        self.reset_pause_fade();
                        // Now that it plays: a DJ transition into the next song may be due.
                        let plan = self.mix_consider();
                        let volume = self.effective_volume();
                        self.fader.last_sent = Some(volume);
                        let start = self.resume_at.take().filter(|(t, _)| *t == token).map(|(_, at)| at);
                        let sources = self.sources.clone();
                        let subs = self.substitutes.clone();
                        // A planned DJ transition brings the next song in on the other deck.
                        let upcoming =
                            if next.is_none() && !self.mix.owns_next() { self.next_slot() } else { None };
                        let live = self.live_token.clone();
                        let play = self.player_task(move |p| async move {
                            p.set_volume(volume).await?;
                            p.play_from(&current, next.as_ref(), start).await?;
                            // Prefetch the next track for a gapless transition, unless
                            // the user moved on meanwhile.
                            if let Ok(Some(track)) = dj::resolve_slot(&sources, &subs, upcoming).await
                                && live.load(std::sync::atomic::Ordering::SeqCst) == token
                            {
                                p.set_next(Some(&track)).await?;
                            }
                            Ok(())
                        });
                        return Task::batch([plan, play]);
                    }
                    Err(e) => return self.playback_failed(e),
                }
            }
            Message::Deck(deck, PlayerMsg::Ready(player)) => {
                tracing::info!("mpv ready (deck {deck})");
                self.decks.set(usize::from(deck), Some(player));
                if usize::from(deck) != self.decks.active {
                    // The second deck: same audio settings, silent until a mix.
                    return self.apply_audio_to(usize::from(deck));
                }
                self.clear_player_error();
                let volume = self.effective_volume();
                self.fader.last_sent = Some(volume);
                return Task::batch([
                    self.apply_audio(),
                    self.player_task(move |p| async move { p.set_volume(volume).await }),
                ]);
            }
            Message::Deck(deck, PlayerMsg::Failed(e)) => {
                tracing::error!("mpv failed (deck {deck}): {e}");
                if usize::from(deck) == self.decks.active {
                    self.set_player_error(e);
                }
            }
            Message::Deck(deck, PlayerMsg::Event(event)) => {
                if usize::from(deck) != self.decks.active {
                    // The idle deck (DJ transitions) reports to the mixer.
                    return self.mix_on_idle_event(usize::from(deck), event);
                }
                return self.on_player_event(event);
            }
            Message::Voice(msg) => return self.on_voice(msg),
            Message::FadeTick => return self.fade_tick(),
            Message::VideoTick => return self.video_sync(),
            Message::VideoUrl(id, url) => return self.video_url(id, url),
            Message::Substituted(id) => return self.on_substituted(&id),
            Message::SubstituteToggle => return self.substitute_toggle(),
            Message::PlayerDone(Ok(())) => {}
            Message::PlayerDone(Err(e)) => tracing::warn!("player/server command failed: {e}"),
            Message::ArtistsLoaded(result) => {
                self.library.artists_loading = false;
                self.library.artists_done = true;
                match result {
                    Ok(page) => {
                        let refs: Vec<ImageRef> =
                            page.items.iter().take(60).filter_map(|a| a.image.clone()).collect();
                        self.library.artists.extend(page.items);
                        return self.request_images(refs.iter().map(Some));
                    }
                    Err(e) => self.toast_error(format!("Loading artists failed: {e}")),
                }
            }
            Message::Artist(msg) => return self.on_artist(msg),
            Message::ArtistLoaded(Ok(detail)) => {
                let mut refs: Vec<ImageRef> = detail.albums.iter().filter_map(|a| a.image.clone()).collect();
                refs.extend(detail.artist.image.clone());
                let blur = self.request_blur_for(detail.artist.image.as_ref(), appearance::BlurKind::Artist);
                refs.extend(detail.top_songs.iter().take(10).filter_map(|s| s.image.clone()));
                let artist = detail.artist.clone();
                self.library.artist = Some(detail);
                let begin = self.artist_begin(&artist);
                return Task::batch([blur, self.request_images(refs.iter().map(Some)), begin]);
            }
            Message::ArtistLoaded(Err(e)) => {
                tracing::warn!("loading the artist page failed: {e}");
                self.toast_error(format!("Could not load the artist page: {e}"))
            }
            Message::PlaylistsLoaded(Ok(list)) => {
                let refs: Vec<ImageRef> = list.iter().filter_map(|p| p.image.clone()).collect();
                self.library.playlists = list;
                return Task::batch([self.request_images(refs.iter().map(Some)), self.smart_ids_task()]);
            }
            Message::PlaylistsLoaded(Err(e)) => tracing::warn!("playlists: {e}"),
            Message::PlaylistLoaded(Ok(detail)) => {
                let image = detail.playlist.image.clone();
                let covers: Vec<ImageRef> =
                    detail.songs.iter().take(4).filter_map(|s| s.image.clone()).collect();
                self.library.playlist = Some(detail);
                return Task::batch([
                    self.playlist_page_loaded(),
                    self.request_image(image.as_ref()),
                    self.request_images(covers.iter().map(Some)),
                ]);
            }
            Message::PlaylistFirstPage(generation, _) | Message::PlaylistMore(generation, _)
                if generation != self.library.playlist_gen => {}
            Message::PlaylistFirstPage(_, Ok((detail, next))) => {
                let task = self.update(Message::PlaylistLoaded(Ok(detail)));
                return match next {
                    Some(cursor) => Task::batch([task, self.more_playlist(cursor)]),
                    None => task,
                };
            }
            Message::PlaylistFirstPage(_, Err(e)) => return self.update(Message::PlaylistLoaded(Err(e))),
            Message::PlaylistMore(_, Ok((songs, next))) => {
                self.library.playlist_loading_more = false;
                if let Some(detail) = &mut self.library.playlist {
                    detail.songs.extend(songs);
                }
                let task = self.playlist_page_loaded();
                return match next {
                    Some(cursor) => Task::batch([task, self.more_playlist(cursor)]),
                    None => task,
                };
            }
            Message::PlaylistMore(_, Err(e)) => {
                self.library.playlist_loading_more = false;
                self.toast_error(format!("Could not load the rest of the playlist: {e}"));
            }
            Message::PlaylistLoaded(Err(e)) => self.toast_error(format!("Could not load the playlist: {e}")),
            Message::PlaylistDragStart(i) => self.playlist_drag_start(i),
            Message::PlaylistDragOver(i) => self.playlist_drag_over(i),
            Message::PlaylistDragEnd => self.playlist_drag_end(),
            Message::PlaylistPage(msg) => return self.on_playlist_page(msg),
            Message::Queue(msg) => return self.on_queue(msg),
            Message::FavoritesLoaded(result) => {
                self.library.favorites_loaded = true;
                match result {
                    Ok(songs) => {
                        self.library.favorites = songs;
                        return self.extra_visible(&list::ListId::Favorites(list::FavoritesTab::Songs));
                    }
                    Err(e) => self.toast_error(format!("Loading favorites failed: {e}")),
                }
            }
            Message::GenresLoaded(result) => {
                self.library.genres_loaded = true;
                match result {
                    Ok(genres) => self.library.genres = genres,
                    Err(e) => self.toast_error(format!("Loading genres failed: {e}")),
                }
                self.refresh_genres_view();
            }
            Message::History(m) => return self.on_history(m),
            Message::Radio(m) => return self.on_radio(m),
            Message::Dlna(m) => return self.on_dlna(m),
            Message::Servers(m) => return self.on_servers(m),
            Message::Appearance(m) => return self.on_appearance(m),
            Message::SearchInput(text) => return self.search_input(text),
            Message::SearchSubmit => return self.run_search(),
            Message::SearchLoaded(generation, result) => {
                if generation == self.search.generation {
                    self.search.loading = false;
                    match result {
                        Ok(results) => {
                            self.search.results = results;
                            return self.search_results_changed();
                        }
                        Err(e) => self.search.error = Some(e),
                    }
                }
            }
            Message::PlaySearchSong(i) => return self.play_picked(self.search.results.songs.clone(), i),
            Message::PlayHomeSong(row, i) => {
                let songs = self.library.home.get(row).map(|r| r.songs.clone()).unwrap_or_default();
                return self.play_picked(songs, i);
            }
            Message::ToggleFavorite => {
                let Some(song) = self.queue.current().map(|i| i.song.clone()) else { return Task::none() };
                return self.set_song_favorite(&song, !song.favorite);
            }
            Message::RightClickAt(at) => {
                self.right_click_at = at;
                // A right-click elsewhere closes an open menu (a new one may open right after).
                self.menu = None;
                self.lists.menu = None;
            }
            Message::Menu(msg) => return self.on_menu(msg),
            Message::Audio(msg) => return self.on_audio(msg),
            Message::Analysis(msg) => return self.on_analysis(msg),
            Message::Mix(msg) => return self.on_mix(msg),
            Message::Setting(change) => return self.change_setting(change),
            Message::Integrations(m) => return self.on_integrations(m),
            #[cfg(any(target_os = "linux", windows))]
            Message::Tray(crate::tray::TrayMsg::Ready(handle)) => {
                self.tray = Some(handle);
                self.tray_published = (false, String::from("\u{0}"));
                self.published.tray = None;
            }
            #[cfg(any(target_os = "linux", windows))]
            Message::Tray(crate::tray::TrayMsg::Unavailable) => {
                // Started hidden but there is no tray to bring the window back from.
                if self.main_window.is_none() && self.hidden {
                    return self.reveal_main_window();
                }
            }
            #[cfg(any(target_os = "linux", windows))]
            Message::Tray(crate::tray::TrayMsg::Command(cmd)) => {
                use crate::tray::TrayCmd;
                return match cmd {
                    TrayCmd::PlayPause => self.handle(Message::TogglePlay),
                    TrayCmd::Next => self.handle(Message::Next),
                    TrayCmd::Previous => self.handle(Message::Previous),
                    TrayCmd::Stop => self.handle(Message::Stop),
                    TrayCmd::Show => self.show_window(),
                    // A click on the icon hides the window again when minimize-to-tray is on.
                    TrayCmd::Toggle => {
                        if self.minimize_to_tray() && self.main_window.is_some() && !self.minimized {
                            self.hide_to_tray().unwrap_or_else(Task::none)
                        } else {
                            self.show_window()
                        }
                    }
                    TrayCmd::Quit => {
                        self.save_queue_now();
                        self.save_window_state_now();
                        iced::exit()
                    }
                };
            }
            Message::Keyboard(event) => return self.on_key(event),
            Message::Hotkeys(msg) => return self.on_hotkeys(msg),
            Message::Advanced(msg) => return self.on_advanced(msg),
            Message::Update(msg) => return self.on_update(msg),
            Message::Onboard(msg) => return self.on_onboard(msg),
            Message::Helpers(msg) => return self.on_helpers(msg),
            Message::Mcp(msg) => return self.on_mcp(msg),
            Message::Viz(msg) => return self.on_viz(msg),
            Message::Milkdrop(msg) => return self.on_milkdrop(msg),
            // The appearance settings apply the step.
            Message::Zoom(step) => {
                use appearance::AppearanceMsg as A;
                let msg = match step {
                    hotkeys::ZoomStep::In => A::ZoomIn,
                    hotkeys::ZoomStep::Out => A::ZoomOut,
                    hotkeys::ZoomStep::Reset => A::ZoomReset,
                };
                return self.update(Message::Appearance(msg));
            }
            #[cfg(target_os = "linux")]
            Message::GlobalShortcut(msg) => {
                use crate::global_shortcuts::ShortcutMsg;
                match msg {
                    ShortcutMsg::Activated(action) => {
                        return match self.hotkey_message(crate::global_shortcuts::action_of(&action)) {
                            Some(message) => self.handle(message),
                            None => Task::none(),
                        };
                    }
                    ShortcutMsg::Bound(triggers) => self.global_triggers = triggers,
                    ShortcutMsg::Unavailable(e) => self.global_error = Some(e),
                }
            }
            Message::WindowClosed(id) => {
                if Some(id) == self.main_window {
                    return iced::exit();
                }
            }
            Message::WindowEvent(id, event) => {
                if Some(id) != self.main_window {
                    return Task::none();
                }
                let tracked = self.win_event(id, &event);
                let message = match event {
                    window::Event::FileDropped(path) => Message::FileDropped(path),
                    window::Event::Rescaled(scale) => Message::ScaleFactor(scale),
                    window::Event::Resized(size) => Message::WindowResized(size),
                    window::Event::Focused => Message::WindowFocused(true),
                    window::Event::Unfocused => Message::WindowFocused(false),
                    _ => return tracked,
                };
                return Task::batch([self.handle(message), tracked]);
            }
            Message::CloseRequested(id) => {
                if Some(id) != self.main_window {
                    return window::close(id);
                }
                self.save_queue_now();
                self.save_window_state_now();
                if self.config.settings.window.exit_to_tray
                    && self.config.settings.window.tray
                    && let Some(task) = self.hide_to_tray()
                {
                    return task;
                }
                return window::close(id);
            }
            Message::DiscordDone(Err(e)) => tracing::debug!("discord: {e}"),
            Message::DiscordDone(Ok(())) => {}
            Message::WindowState(msg) => return self.on_window_state(msg),
            Message::SingleInstance(forward) => return self.on_single_instance(forward),
            Message::Window(action) => {
                let Some(main) = self.main_window else { return Task::none() };
                let latest = Task::done(main);
                return match action {
                    WindowAction::Drag => latest.then(window::drag),
                    WindowAction::ToggleMaximize => latest.then(window::toggle_maximize),
                    WindowAction::Minimize => {
                        if self.minimize_to_tray()
                            && let Some(task) = self.hide_to_tray()
                        {
                            return task;
                        }
                        self.minimized = true;
                        latest.then(|id| window::minimize(id, true))
                    }
                    WindowAction::Close => Task::done(Message::CloseRequested(main)),
                    WindowAction::Resize(dir) => latest.then(move |id| window::drag_resize(id, dir)),
                    WindowAction::SystemMenu => latest.then(window::show_system_menu),
                };
            }
            // The compact layout has no full-screen player.
            Message::ToggleFullscreen if self.layout_mode() == ui::LayoutMode::Compact => {}
            Message::ToggleFullscreen => return self.fullscreen_toggled(),
            Message::ToggleDrawer => self.drawer_open = !self.drawer_open,
            Message::CloseDrawer => self.drawer_open = false,
            Message::ToggleVolumePopup => self.volume_popup = !self.volume_popup,
            Message::CloseVolumePopup => self.volume_popup = false,
            Message::VolumeWheel(dy) => {
                if dy == 0.0 {
                    return Task::none();
                }
                let step = i16::from(self.config.settings.player_bar.volume_wheel_step.max(1));
                let delta = if dy > 0.0 { step } else { -step };
                let v = (i16::from(self.playback.volume) + delta).clamp(0, 100);
                return self.handle(Message::Volume(f32::from(v)));
            }
            Message::MobilePanel(tab) => {
                self.mobile_panel = if self.mobile_panel == Some(tab) { None } else { Some(tab) };
                if self.mobile_panel.is_some() {
                    self.queue_scroll.offset = 0.0;
                    let lyrics = self.load_lyrics();
                    return Task::batch([lyrics, self.handle(Message::FullscreenTab(tab))]);
                }
            }
            Message::OpenFullscreenTab(tab) => {
                // Closed: opens on the tab. Open: the button toggles the panel, the player stays.
                if self.fullscreen {
                    return self.on_fullscreen(fullscreen_state::FullscreenMsg::TogglePanel(tab));
                }
                self.fullscreen = true;
                self.fs.visualizer_expanded = false;
                self.queue_scroll.offset = 0.0;
                let lyrics = self.load_lyrics();
                return Task::batch([lyrics, self.handle(Message::FullscreenTab(tab)), self.fs_backdrop()]);
            }
            Message::FullscreenTab(tab) => {
                self.fullscreen_tab = tab;
                self.queue_scroll.offset = 0.0;
                if tab != FullscreenTab::Visualizer && !self.queue_visualizer_visible() {
                    self.spectrum = None;
                }
                return self.scroll_lyrics();
            }
            Message::Fullscreen(m) => return self.on_fullscreen(m),
            Message::Sidebar(m) => return self.on_sidebar(m),
            Message::LyricsLoaded(id, lyrics) => {
                return self.on_lyrics(lyrics_state::LyricsMsg::Loaded(id, lyrics.into_iter().collect()));
            }
            Message::Lyrics(msg) => return self.on_lyrics(msg),
            Message::Spectrum(frame) => {
                self.visualizer_error = None;
                self.spectrum = Some(frame);
            }
            Message::VisualizerFailed(e) => self.visualizer_error = Some(e),
            Message::AutoDjLoaded(result) => {
                self.autodj_busy = false;
                match result {
                    // Started before the AI DJ went on: its picks are not wanted.
                    Ok(_) if self.ai_dj_on() && !self.autodj_for_ai => {
                        tracing::info!("Auto-DJ: dropped a stale fetch (AI DJ is on)");
                    }
                    Ok(songs) => {
                        use listen_core::versions::{dedupe_versions, song_key};
                        let s = &self.config.settings.auto_dj;
                        let queued: std::collections::HashSet<String> =
                            self.queue.iter().map(|i| song_key(&i.song)).collect();
                        let mut picked = Vec::new();
                        // One version per song (radio edits preferred), nothing
                        // that already played this session.
                        for song in dedupe_versions(songs) {
                            let key = song_key(&song);
                            if !self.session_played.contains(&key)
                                && (s.allow_duplicates || !queued.contains(&key))
                            {
                                picked.push(song);
                            }
                        }
                        // Best flow from the current song first (tempo, key, energy), when analysed.
                        let n = s.item_count.max(1) as usize;
                        let picked = match self.queue.current().map(|i| i.song.clone()) {
                            Some(current) => self.flow_order(&current, picked, n),
                            None => picked.into_iter().take(n).collect(),
                        };
                        if picked.is_empty() {
                            return Task::none();
                        }
                        tracing::info!(
                            "Auto-DJ added {} songs{}",
                            picked.len(),
                            if self.autodj_for_ai { " (fallback for the AI DJ)" } else { "" }
                        );
                        let had_next = self.queue.peek_next().is_some();
                        let refs: Vec<ImageRef> = picked.iter().filter_map(|s| s.image.clone()).collect();
                        self.queue.add_last(picked, true);
                        let images = self.request_images(refs.iter().map(Some));
                        if let Some(play) = self.dj_after_fill() {
                            return Task::batch([images, play]);
                        }
                        return if had_next { images } else { Task::batch([images, self.refresh_next()]) };
                    }
                    Err(e) => tracing::warn!("Auto-DJ: {e}"),
                }
            }
            Message::DjPlanned(generation, result) => return self.dj_planned(generation, result),
            Message::DjCheck => return self.dj_watch(),
            Message::DjPreviewDone(result) => self.dj_preview_done(result),
            Message::Rip(m) => return self.on_rip(m),
            Message::Offline(m) => return self.on_offline(m),
            Message::ToggleAiDj => {
                let switch = self.dj_switch(!self.config.settings.ai_dj.enabled);
                return Task::batch([switch, self.apply_audio()]);
            }
            Message::Overlay(m) => return self.on_overlay(m),
            Message::Shell(m) => return self.on_shell(m),
            #[cfg(any(target_os = "linux", windows))]
            Message::Mpris(msg) => return self.on_mpris(msg),
            Message::Noop | Message::ToggleVideo => {}
        }
        Task::none()
    }

    fn demo_seek(&mut self, pos: Duration) {
        let Some(item) = self.queue.current() else { return };
        if pos >= item.song.duration {
            self.queue.advance();
            self.playback.position = Duration::ZERO;
            if self.queue.current().is_none() {
                self.playback.playing = false;
            }
        } else {
            self.playback.position = pos;
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let mut subs = vec![iced::system::theme_changes().map(Message::SystemTheme)];
        if self.config.settings.appearance.follow_desktop_theme {
            subs.push(crate::desktop_theme::subscription());
        }
        if self.library.home_state.feature.anim.is_some() {
            // Frames while the feature carousel slides.
            subs.push(
                iced::time::every(Duration::from_millis(16))
                    .map(|_| Message::Home(home::HomeMsg::FeatureFrame)),
            );
        }
        if let Some(dir) = &self.appearance.themes_dir {
            subs.push(appearance::themes_subscription(dir.clone()));
        }
        if self.mock_playback() {
            if self.playback.playing {
                subs.push(iced::time::every(Duration::from_millis(500)).map(|_| Message::Tick));
            }
        } else {
            subs.push(playback::subscription(self.player_config.clone()));
            if !self.ephemeral {
                subs.push(iced::time::every(Duration::from_secs(5)).map(|_| Message::SaveQueue));
            }
            if self.deck_b_needed() {
                subs.push(playback::subscription(self.player_config.deck_b()));
            }
            if self.ai_dj_on() {
                subs.push(playback::subscription(self.player_config.voice()));
                subs.push(iced::time::every(Duration::from_secs(5)).map(|_| Message::DjCheck));
            }
            if self.fade_active() {
                subs.push(iced::time::every(fade::TICK).map(|_| Message::FadeTick));
            }
            if self.video.on {
                subs.push(iced::time::every(Duration::from_millis(20)).map(|_| Message::VideoTick));
            }
            #[cfg(any(target_os = "linux", windows))]
            subs.push(mpris::subscription());
            #[cfg(target_os = "linux")]
            if self.visualizer_running() && self.milkdrop_active() {
                self.milkdrop_sync();
                subs.push(crate::milkdrop::sub::subscription(self.deck_b_needed()));
            }
            if self.visualizer_running() && !self.milkdrop_active() {
                // The fullscreen visualizer keeps its full rate unfocused (e.g. on a
                // second screen); the queue panel's strip slows down in the background.
                let fullscreen_visualizer = self.fs.visualizer_expanded
                    || (self.fullscreen && self.fullscreen_tab == FullscreenTab::Visualizer);
                crate::visualizer::set_background(!self.focused && !fullscreen_visualizer);
                crate::visualizer::set_config(crate::visualizer::analyzer_config(
                    &self.config.settings.visualizer,
                ));
                subs.push(crate::visualizer::subscription(self.deck_b_needed()));
            }
        }
        subs.extend(self.radio_subscriptions());
        subs.extend(self.dlna_subscriptions());
        subs.push(self.remote_subscription());
        subs.push(self.mcp_subscription());
        subs.push(self.discord_subscription());
        subs.push(iced::keyboard::listen().map(Message::Keyboard));
        subs.push(palette::subscription(self));
        #[cfg(target_os = "linux")]
        {
            let wanted = self.portal_wanted();
            if !wanted.is_empty() {
                subs.push(crate::global_shortcuts::subscription(wanted));
            }
        }
        if self.overlay.timer_needed() {
            subs.push(
                iced::time::every(Duration::from_millis(500))
                    .map(|_| Message::Overlay(overlay::OverlayMsg::Tick)),
            );
        }
        if self.playlist_drag.is_some() {
            subs.push(iced::event::listen_with(|event, _, _| match event {
                iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left))
                | iced::Event::Mouse(iced::mouse::Event::CursorLeft) => Some(Message::PlaylistDragEnd),
                _ => None,
            }));
        }
        subs.extend(self.sidebar_subscription());
        subs.extend(self.list_subscription());
        if self.resizing.is_some() {
            subs.push(iced::event::listen_with(|event, _, _| match event {
                iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                    Some(Message::ResizeMove(position.x))
                }
                iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left))
                | iced::Event::Mouse(iced::mouse::Event::CursorLeft) => Some(Message::ResizeEnd),
                _ => None,
            }));
        }
        subs.push(iced::event::listen_with(|event, _, id| match event {
            iced::Event::Window(
                e @ (window::Event::FileDropped(_)
                | window::Event::Rescaled(_)
                | window::Event::Resized(_)
                | window::Event::Moved(_)
                | window::Event::Focused
                | window::Event::Unfocused),
            ) => Some(Message::WindowEvent(id, e)),
            _ => None,
        }));
        subs.push(window::close_events().map(Message::WindowClosed));
        subs.push(window::close_requests().map(Message::CloseRequested));
        if !self.ephemeral && !self.demo {
            subs.push(crate::single_instance::subscription());
        }
        #[cfg(any(target_os = "linux", windows))]
        if self.config.settings.window.tray && !self.demo {
            subs.push(crate::tray::subscription());
        }
        // CD drives (runs in the tray too: the app keeps running there).
        #[cfg(target_os = "linux")]
        if !self.demo && self.screenshot.is_none() && !self.ephemeral {
            subs.push(rip::subscription());
        }
        if self.rip_ticking() {
            subs.push(iced::time::every(Duration::from_secs(1)).map(|_| Message::Rip(rip::RipMsg::Tick)));
        }
        subs.extend(self.fullscreen_subscriptions());
        if let Some(config) = self.analysis_config() {
            subs.push(analysis::subscription(config));
        }
        subs.extend(self.queue_subscription());
        Subscription::batch(subs)
    }

    pub fn view(&self) -> Element<'_, Message> {
        let t = self.tokens;
        let body: Element<'_, Message> = if self.onboarding_pending() {
            ui::onboarding::view(self)
        } else if self.needs_login() {
            ui::login::view(self)
        } else if self.layout_mode() == ui::LayoutMode::Compact {
            ui::compact::body(self)
        } else if self.fs.visualizer_expanded {
            ui::fullscreen::visualizer_overlay(self)
        } else {
            let left_divider = if self.config.settings.sidebar.collapsed {
                ui::divider_v(t)
            } else {
                ui::resize_handle(t, PanelSide::Left)
            };
            let middle: Element<'_, Message> = if self.fullscreen {
                row![ui::fullscreen::view(self)].into()
            } else {
                // The queue panel sits at the right or under the page, as set.
                ui::queue::attach(
                    self,
                    row![ui::sidebar::view(self), left_divider, ui::page::view(self)].into(),
                )
            };
            // Always a stack, so showing the cover keeps the page's scroll position.
            let mut middle = iced::widget::stack![container(middle).height(Length::Fill)];
            // Collapsed sidebar: the expanded cover floats next to the icon rail.
            if self.config.settings.sidebar.collapsed
                && !self.fullscreen
                && let Some(cover) = ui::expanded_cover(self, 240.0)
            {
                let cover = container(cover).style(|_| container::Style {
                    border: iced::border::rounded(metrics::radius_md()),
                    shadow: iced::Shadow {
                        color: iced::Color { a: 0.5, ..iced::Color::BLACK },
                        offset: iced::Vector::new(0.0, 8.0),
                        blur_radius: 24.0,
                    },
                    ..Default::default()
                });
                let corner = container(cover)
                    .padding(iced::Padding {
                        left: ui::sidebar::RAIL_WIDTH + metrics::space_md(),
                        bottom: metrics::space_md(),
                        ..iced::Padding::ZERO
                    })
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_y(iced::alignment::Vertical::Bottom);
                middle = middle.push(corner);
            }
            column![
                middle,
                ui::divider_h(t),
                container(ui::player_bar::view(self))
                    .height(metrics::PLAYER_BAR_HEIGHT)
                    .style(crate::style::player_bar(t)),
            ]
            .into()
        };
        let custom_bar = self.config.settings.window.custom_title_bar;
        let content: Element<'_, Message> =
            if custom_bar { column![ui::title_bar::view(self), ui::divider_h(t), body].into() } else { body };
        let root: Element<'_, Message> = container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(crate::style::app_background(t))
            .into();
        let root = ui::compact::overlays(self, root);
        let root = ui::context_menu::overlay(self, root);
        let root = ui::list::overlay(self, root);
        let root = ui::queue::overlay(self, root);
        let root = ui::list::header::popups(self, root);
        let root = ui::player_config::overlay(self, root);
        let root = ui::sleep_popover::overlay(self, root);
        let root = ui::dlna::overlay(self, root);
        let root = ui::sidebar::overlay(self, root);
        let root = ui::overlay::overlay(self, root);
        // Touchpad momentum for every scrollable in the window.
        let factor = ui::kinetic::factor(self.config.settings.appearance.scroll_speed, self.scale_factor);
        let root: Element<'_, Message> = ui::kinetic::kinetic(root, factor).into();
        let root =
            if custom_bar && self.screenshot.is_none() { ui::title_bar::resize_edges(root) } else { root };
        ui::right_click::right_clicks(root).into()
    }
}

fn to_track(mut s: listen_source::StreamSource) -> Track {
    // "Ignore certificate errors" of the server: mpv must not check either.
    let insecure = s.headers.remove(listen_source::INSECURE_TLS_HEADER).is_some();
    let options = if insecure { vec![("tls-verify".to_string(), "no".to_string())] } else { Vec::new() };
    Track { url: s.url, headers: s.headers.into_iter().collect(), options }
}

fn scroll_state(viewport: &scrollable::Viewport) -> ScrollState {
    ScrollState {
        offset: viewport.absolute_offset().y,
        height: viewport.bounds().height,
        width: viewport.bounds().width,
    }
}

/// A window for `--screenshot --size WxH` must come out at exactly that size:
/// compositors (KDE) ignore the requested size of a resizable toplevel, but
/// they honour equal min and max size hints, so pin both and drop resizing.
fn pin_screenshot_size(mut settings: window::Settings, screenshot: bool) -> window::Settings {
    if screenshot {
        settings.min_size = Some(settings.size);
        settings.max_size = Some(settings.size);
        settings.resizable = false;
    }
    settings
}

fn save_png(path: &std::path::Path, shot: &window::Screenshot) -> Result<(), String> {
    image::save_buffer(path, &shot.rgba, shot.size.width, shot.size.height, image::ExtendedColorType::Rgba8)
        .map_err(|e| e.to_string())
}

/// Native "open file" dialog for the cookies export (see [`crate::dialog`]).
async fn pick_cookie_file() -> Option<std::path::PathBuf> {
    crate::dialog::open_file(crate::dialog::Pick {
        title: "Choose the cookies file",
        start: listen_core::platform::home_dir().map(|h| h.join("Downloads")),
        filter_name: "Cookie exports",
        patterns: &["*.json", "*.txt"],
    })
    .await
}

async fn pick_music_folder() -> Option<std::path::PathBuf> {
    crate::dialog::open_folder(crate::dialog::Pick {
        title: "Choose your music folder",
        start: listen_core::platform::home_dir().map(|h| h.join("Music")),
        filter_name: "",
        patterns: &[],
    })
    .await
}

/// A page named on the command line (`--route`) or by a second launch.
pub fn parse_route(name: &str) -> Option<Route> {
    match name {
        "home" => Some(Route::Home),
        "albums" => Some(Route::Albums),
        "tracks" => Some(Route::Tracks),
        "settings" => Some(Route::Settings),
        "artists" => Some(Route::AlbumArtists),
        "favorites" => Some(Route::Favorites),
        "recent" => Some(Route::Recent),
        "stats" => Some(Route::Stats),
        "mixes" => Some(Route::Mixes),
        "radio" => Some(Route::Radio),
        _ => None,
    }
}

fn unix_millis() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis()
}

/// Which resizable panel a divider belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelSide {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy)]
struct Resize {
    side: PanelSide,
    /// Cursor x when the drag started (set by the first move).
    start_x: Option<f32>,
    start_width: f32,
}

/// Feishin's `constrainSidebarWidth`.
const SIDEBAR_MIN: f32 = 260.0;
const SIDEBAR_MAX: f32 = 400.0;
const SIDEBAR_COLLAPSE_BELOW: f32 = 160.0;
const QUEUE_MIN: f32 = 300.0;
const QUEUE_MAX: f32 = 900.0;

/// Expands a leading `~/` to the home directory.
fn shellexpand_home(p: &str) -> std::path::PathBuf {
    match (p.strip_prefix("~/"), listen_core::platform::home_dir()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => std::path::PathBuf::from(p),
    }
}

#[cfg(test)]
mod screenshot_size_tests {
    use super::*;

    #[test]
    fn screenshot_windows_are_pinned_to_the_requested_size() {
        let base = window::Settings { size: iced::Size::new(1280.0, 720.0), ..Default::default() };
        let pinned = pin_screenshot_size(base.clone(), true);
        assert_eq!(pinned.min_size, Some(iced::Size::new(1280.0, 720.0)));
        assert_eq!(pinned.max_size, Some(iced::Size::new(1280.0, 720.0)));
        assert!(!pinned.resizable);
        let normal = pin_screenshot_size(base, false);
        assert!(normal.resizable);
        assert_eq!(normal.max_size, None);
    }
}
