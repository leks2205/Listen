//! Auto-DJ, analysis, mixing and the AI DJ.

use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoDj {
    pub enabled: bool,
    /// Songs to add each time the queue runs low.
    pub item_count: u32,
    /// Refill when this many (or fewer) songs remain.
    pub timing: u32,
    pub allow_duplicates: bool,
    /// `similar` or `random`.
    pub strategy: String,
}

impl Default for AutoDj {
    fn default() -> Self {
        Self { enabled: true, item_count: 5, timing: 1, allow_duplicates: false, strategy: "similar".into() }
    }
}

/// Background analysis of Navidrome songs (see `listen_core::analysis`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Analysis {
    pub enabled: bool,
    /// Work through the whole library at low priority (not only the queue).
    pub analyze_library: bool,
    /// Results folder, may be shared between machines; empty = Listen's data folder.
    pub shared_dir: String,
    /// Also analyse YouTube Music songs (via a temporary copy); needs `shared_dir`.
    pub youtube_music: bool,
    /// Library analysis threads (0 = automatic).
    pub threads: u32,
    /// Keep analysing the library while on battery.
    pub on_battery: bool,
}

impl Default for Analysis {
    fn default() -> Self {
        Self {
            enabled: true,
            analyze_library: true,
            shared_dir: String::new(),
            youtube_music: false,
            threads: 0,
            on_battery: false,
        }
    }
}

/// How strictly a song's start and end must be free of vocals to be blended over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionEdges {
    /// Vocals never block a blend.
    Always,
    /// Blends need instrumental edges.
    Instrumental,
    /// Like `Always` for club genres, like `Instrumental` otherwise.
    #[default]
    Smart,
}

impl TransitionEdges {
    pub const ALL: [TransitionEdges; 3] = [Self::Always, Self::Instrumental, Self::Smart];
}

impl std::fmt::Display for TransitionEdges {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Always => "Always",
            Self::Instrumental => "Instrumental only",
            Self::Smart => "Smart",
        })
    }
}

/// DJ transitions between songs the Auto-DJ / AI DJ picked (Navidrome).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Mixing {
    pub enabled: bool,
    /// Order the DJs' picks by tempo, key and energy flow.
    pub flow_select: bool,
    /// Largest tempo difference that is beat-matched (percent); beyond: fade.
    pub max_tempo_pct: f32,
    /// Length of long blends in bars.
    pub long_blend_bars: u8,
    pub tempo_ramps: bool,
    pub bass_swap: bool,
    /// Also mix into songs added with "Add to queue" while a DJ is on.
    pub mix_manual_queue: bool,
    /// Length of the plain fade used when songs can't be beat-matched (s).
    pub fallback_fade_secs: f32,
    /// Whether vocals at a song's start or end may be blended over.
    pub transition_edges: TransitionEdges,
}

impl Default for Mixing {
    fn default() -> Self {
        Self {
            enabled: true,
            flow_select: true,
            max_tempo_pct: 6.0,
            long_blend_bars: 16,
            tempo_ramps: true,
            bass_swap: true,
            mix_manual_queue: true,
            fallback_fade_secs: 4.0,
            transition_edges: TransitionEdges::default(),
        }
    }
}

/// The AI DJ (Gemini): picks sets and talks between them. The API key lives in the keyring.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiDj {
    pub enabled: bool,
    /// `late_night`, `morning` or `calm` (see `listen_dj::PERSONAS`).
    pub persona: String,
    pub text_model: String,
    pub tts_model: String,
    /// Songs per set.
    pub set_min: u32,
    pub set_max: u32,
    /// Talk after every n-th set (1 = every set).
    pub talk_every: u32,
    /// Spoken breaks per day at most (small free-tier quota); after that lines are shown as text.
    pub max_voiced_per_day: u32,
    /// Voice loudness relative to the default level, in dB.
    pub voice_gain_db: f32,
    /// Where the show airs, e.g. "Kiel" (empty = no home town).
    pub location: String,
    /// How the voice says the location's name (e.g. "Keel" for Kiel).
    pub location_pronunciation: String,
    /// Language code the hosts speak (empty = the app's language).
    pub language: String,
    /// Free-text notes for the hosts (names, likes, things to avoid).
    pub notes: String,
}

impl AiDj {
    /// The location's place name (text before any comma or bracket) and how to say it, if both are set.
    pub fn place_pronunciation(&self) -> Option<(&str, &str)> {
        let place = self.location.split(['(', ',']).next().unwrap_or("").trim();
        let how = self.location_pronunciation.trim();
        (!place.is_empty() && !how.is_empty()).then_some((place, how))
    }
}

impl Default for AiDj {
    fn default() -> Self {
        Self {
            enabled: false,
            persona: "late_night".into(),
            text_model: "gemini-3.5-flash-lite".into(),
            tts_model: "gemini-3.8-flash-lite-tts".into(),
            set_min: 2,
            set_max: 5,
            talk_every: 1,
            max_voiced_per_day: 15,
            voice_gain_db: 0.0,
            location: String::new(),
            location_pronunciation: String::new(),
            language: String::new(),
            notes: String::new(),
        }
    }
}

/// `af-command` label of the DJ mix filters in [`deck_audio_filters`].
pub const DECK_MIX_LABEL: &str = "mix";
/// `af-command` targets in that graph (ffmpeg biquads, `f` / `g` / `w` live). Idle: shelf at 0 dB,
/// passes at 10 Hz / 20 kHz. Move them only by sweeping `f`; switching a biquad's `mix` clicks.
pub const DECK_BASS: &str = "bass@lo";
pub const DECK_HIGHPASS: &str = "highpass@hp";
pub const DECK_LOWPASS: &str = "lowpass@lp";
/// Tempo filter of a deck: rubberband R3 keeps beats in place off 1.0× (WSOLA shifts them ±5–7 ms).
pub const DECK_TEMPO: &str = "rubberband=engine=finer";
/// For mpv builds without librubberband: clean, but beats jitter off 1.0×.
pub const DECK_TEMPO_FALLBACK: &str = "scaletempo2";

/// mpv `af` for a DJ deck, with [`DECK_TEMPO`]; see [`deck_audio_filters_with`].
pub fn deck_audio_filters(p: &Playback) -> String {
    deck_audio_filters_with(p, DECK_TEMPO)
}

/// mpv `af` for a DJ deck: the user's EQ ([`mpv_audio_filters`]), the live mix filters, then a
/// permanent tempo filter `ts`. Without one, mpv adds and removes its own around 1.0×, which jumps.
pub fn deck_audio_filters_with(p: &Playback, tempo: &str) -> String {
    let deck = format!(
        "@{DECK_MIX_LABEL}:lavfi=[{DECK_BASS}=g=0:f=120:w=0.7,{DECK_HIGHPASS}=f=10,{DECK_LOWPASS}=f=20000],@ts:{tempo}"
    );
    match mpv_audio_filters(p) {
        user if user.is_empty() => deck,
        user => format!("{user},{deck}"),
    }
}
