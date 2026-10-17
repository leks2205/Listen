//! Platform-independent types of the desktop media controls (MPRIS, Windows SMTC).

/// Commands from the desktop.
#[derive(Debug, Clone)]
pub enum MprisCmd {
    Play,
    Pause,
    PlayPause,
    Stop,
    Next,
    Previous,
    /// Relative seek in seconds.
    SeekBy(f64),
    SetPosition(f64),
    Volume(f64),
    Shuffle(bool),
    Loop(LoopStatus),
    Raise,
}

/// What the app shows to the desktop.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MprisState {
    pub status: Status,
    pub track: Option<Track>,
    pub position_secs: f64,
    pub volume: f64,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub has_next: bool,
    pub has_previous: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum Status {
    #[default]
    Stopped,
    Playing,
    Paused,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum RepeatMode {
    #[default]
    None,
    Track,
    Playlist,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    /// Unique per queue entry.
    pub uid: u64,
    pub title: String,
    pub artists: Vec<String>,
    pub album: Option<String>,
    pub album_artists: Vec<String>,
    pub length_secs: f64,
    pub art_url: Option<String>,
    pub track_number: Option<i32>,
    /// Extra fields Feishin also publishes (`xesam:*`).
    pub extra: Extra,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Extra {
    pub genres: Vec<String>,
    pub disc_number: Option<i32>,
    pub bpm: Option<i32>,
    pub comment: Option<String>,
    /// ISO 8601 (the song's year).
    pub content_created: Option<String>,
    /// ISO 8601, when it was last played.
    pub last_used: Option<String>,
    pub use_count: Option<i32>,
    /// 0.0 to 1.0.
    pub user_rating: Option<f64>,
    /// The current lyrics (`xesam:asText`): LRC when synchronized.
    pub lyrics: Option<String>,
}

/// A loop mode requested by the desktop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopStatus {
    None,
    Track,
    Playlist,
}
