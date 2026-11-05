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
    /// One pick in [`FAMILIAR_EVERY`] is a forgotten favourite from the history (`listen_core::memory`).
    pub forgotten_favourites: bool,
    /// Focus mode: automatic picks prefer little lead vocal, calm energy and instrumental versions;
    /// the AI DJ doesn't talk.
    pub focus: bool,
}

/// The Auto-DJ's forgotten-favourite slot: one pick in this many.
pub const FAMILIAR_EVERY: u8 = 8;

impl AutoDj {
    /// What the Auto-DJ aims for beyond flow (the forgotten-favourite slot).
    /// None in focus mode (a forgotten favourite is rarely a quiet one).
    pub fn familiar_slot(&self) -> Option<u8> {
        (self.forgotten_favourites && !self.focus).then_some(FAMILIAR_EVERY)
    }
}

impl Default for AutoDj {
    fn default() -> Self {
        Self {
            enabled: true,
            item_count: 5,
            timing: 1,
            allow_duplicates: false,
            strategy: "similar".into(),
            forgotten_favourites: true,
            focus: false,
        }
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
    /// Put a short stretch of an analysed song between two DJ picks that are far apart (bridge).
    pub bridges: bool,
    /// How often (percent) a far-apart pair gets a bridge.
    pub bridge_chance_pct: u8,
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
            bridges: true,
            bridge_chance_pct: 50,
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
    /// No longer used (spoken breaks have no daily limit); kept so existing configs round-trip.
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
/// The deck's echo (DJ echo-out): a send (`volume`) into four delay taps (`adelay`, `delays` in ms, live),
/// mixed with the dry signal by `amix` (`weights` "dry tap1 … tap4", live). Idle: send 0, weights
/// "1 0 0 0 0", delays 0 — bit-exact passthrough.
///
/// `amix` and `volume` change in one sample, which clicks, so the send and the dry signal are switched by
/// `afade` gates instead ([`DECK_ECHO_OPEN`], [`DECK_ECHO_CLOSE`], [`DECK_ECHO_DRY`]): a command sets the
/// gate's `start_time` to a file time (the deck's own pts, before the tempo filter) and the gate ramps
/// over [`DECK_ECHO_GLIDE_S`] exactly there, as `listen_mix::echo` does. See [`deck_echo_commands`].
pub const DECK_ECHO_SEND: &str = "volume@es";
pub const DECK_ECHO_TAPS: [&str; 4] = ["adelay@e1", "adelay@e2", "adelay@e3", "adelay@e4"];
pub const DECK_ECHO_MIX: &str = "amix@em";
/// Fade-in gate of the send (closed until its `start_time`).
pub const DECK_ECHO_OPEN: &str = "afade@eo";
/// Fade-out gate of the send (open until its `start_time`).
pub const DECK_ECHO_CLOSE: &str = "afade@ec";
/// Fade-out gate of the dry signal (down to its `silence` level from its `start_time`).
pub const DECK_ECHO_DRY: &str = "afade@ed";
/// Length of the gates' ramps (s), the sample echo's glide.
pub const DECK_ECHO_GLIDE_S: f64 = 0.005;
/// `start_time` of a gate that never switches (s; `afade` passes frames before a fade-out untouched).
const DECK_ECHO_NEVER: &str = "1000000";

/// A deck echo's settings as the deck chain takes them (`listen_mix::echo::EchoParams` with its tap gains).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeckEcho {
    pub delay_s: f32,
    /// Level of each repeat relative to the send.
    pub taps: [f32; 4],
    pub send: f32,
    pub dry: f32,
}

impl DeckEcho {
    pub const IDLE: DeckEcho = DeckEcho { delay_s: 0.0, taps: [0.0; 4], send: 0.0, dry: 1.0 };
}

/// `af-command`s (target, option, value) on [`DECK_MIX_LABEL`] that take a deck's echo from `from` to
/// `to` at file time `at_s` (sent ahead; the gates switch at `at_s` with a [`DECK_ECHO_GLIDE_S`] ramp).
/// Delays and repeat levels change at once: they come first and while the send is still closed. The
/// send opens once and closes once per echo; [`DeckEcho::IDLE`] resets every gate at once (the deck
/// stops or the mix is cancelled).
pub fn deck_echo_commands(
    from: &DeckEcho,
    to: &DeckEcho,
    at_s: f64,
) -> Vec<(&'static str, &'static str, String)> {
    let mut c = Vec::new();
    let weights = |e: &DeckEcho| {
        std::iter::once(1.0).chain(e.taps).map(|g| format!("{g:.4}")).collect::<Vec<_>>().join(" ")
    };
    if *to == DeckEcho::IDLE {
        for gate in [DECK_ECHO_OPEN, DECK_ECHO_CLOSE, DECK_ECHO_DRY] {
            c.push((gate, "start_time", DECK_ECHO_NEVER.to_string()));
        }
        c.push((DECK_ECHO_MIX, "weights", weights(to)));
        return c;
    }
    if (from.delay_s - to.delay_s).abs() > 1e-4 {
        // adelay's `delays` command sets only the listed channels (`all` counts at init only): one entry
        // per channel, up to 7.1.
        for (k, tap) in DECK_ECHO_TAPS.into_iter().enumerate() {
            let ms = format!("{:.1}", (k + 1) as f32 * to.delay_s * 1000.0);
            c.push((tap, "delays", vec![ms; 8].join("|")));
        }
    }
    if from.taps != to.taps {
        c.push((DECK_ECHO_MIX, "weights", weights(to)));
    }
    let at = format!("{at_s:.6}");
    if to.send > 0.0 && (to.send - from.send).abs() > 1e-6 {
        c.push((DECK_ECHO_SEND, "volume", format!("{:.3}", to.send)));
    }
    if to.send > 0.0 && from.send <= 0.0 {
        c.push((DECK_ECHO_OPEN, "start_time", at.clone()));
    } else if to.send <= 0.0 && from.send > 0.0 {
        c.push((DECK_ECHO_CLOSE, "start_time", at.clone()));
    }
    if to.dry < from.dry {
        c.push((DECK_ECHO_DRY, "silence", format!("{:.4}", to.dry.max(0.0))));
        c.push((DECK_ECHO_DRY, "start_time", at));
    } else if to.dry > from.dry {
        c.push((DECK_ECHO_DRY, "start_time", DECK_ECHO_NEVER.to_string()));
    }
    c
}
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
    let [e1, e2, e3, e4] = DECK_ECHO_TAPS;
    // Bass shelf, echo (before the passes, so a high-pass sweep thins its repeats), high-pass, low-pass.
    let echo = format!(
        "asplit=2[dry][wet];[dry]{DECK_ECHO_DRY}=t=out:st={DECK_ECHO_NEVER}:d={DECK_ECHO_GLIDE_S}[dg];\
         [wet]{DECK_ECHO_SEND}=0,{DECK_ECHO_OPEN}=t=in:st={DECK_ECHO_NEVER}:d={DECK_ECHO_GLIDE_S},\
         {DECK_ECHO_CLOSE}=t=out:st={DECK_ECHO_NEVER}:d={DECK_ECHO_GLIDE_S},asplit=4[t1][t2][t3][t4];\
         [t1]{e1}=delays=0:all=1[u1];[t2]{e2}=delays=0:all=1[u2];\
         [t3]{e3}=delays=0:all=1[u3];[t4]{e4}=delays=0:all=1[u4];\
         [dg][u1][u2][u3][u4]{DECK_ECHO_MIX}=inputs=5:normalize=0:duration=first:weights=1 0 0 0 0"
    );
    let deck = format!(
        "@{DECK_MIX_LABEL}:lavfi=[{DECK_BASS}=g=0:f=120:w=0.7,{echo},{DECK_HIGHPASS}=f=10,{DECK_LOWPASS}=f=20000],@ts:{tempo}"
    );
    match mpv_audio_filters(p) {
        user if user.is_empty() => deck,
        user => format!("{user},{deck}"),
    }
}
