//! Playback: replay gain, stream quality, equaliser, audio filters.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReplayGain {
    No,
    Track,
    Album,
}

impl std::fmt::Display for ReplayGain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ReplayGain::No => "Off",
            ReplayGain::Track => "Track",
            ReplayGain::Album => "Album",
        })
    }
}

/// How much quality to stream (lower saves bandwidth). Local files and the
/// analysis copy used for DJ mixing always stay original.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StreamQuality {
    /// The file as stored, i.e. the best audio.
    #[default]
    Original,
    /// Up to 320 kbit/s.
    High,
    /// Up to 192 kbit/s.
    Normal,
    /// Up to 96 kbit/s.
    Low,
}

impl StreamQuality {
    pub const ALL: [StreamQuality; 4] =
        [StreamQuality::Original, StreamQuality::High, StreamQuality::Normal, StreamQuality::Low];

    /// Bit-rate cap in kbit/s; `None` = no cap.
    pub fn max_kbps(self) -> Option<u32> {
        match self {
            StreamQuality::Original => None,
            StreamQuality::High => Some(320),
            StreamQuality::Normal => Some(192),
            StreamQuality::Low => Some(96),
        }
    }

    pub fn to_u8(self) -> u8 {
        self as u8
    }

    pub fn from_u8(v: u8) -> Self {
        Self::ALL.get(usize::from(v)).copied().unwrap_or_default()
    }
}

impl std::fmt::Display for StreamQuality {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            StreamQuality::Original => "Original (best)",
            StreamQuality::High => "High (320 kbit/s)",
            StreamQuality::Normal => "Normal (192 kbit/s)",
            StreamQuality::Low => "Low (96 kbit/s)",
        })
    }
}

/// Where the sound comes from (Feishin's "Audio player"): local mpv, or the server's jukebox
/// (Subsonic `jukeboxControl`, offered only when the server answers it).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioPlayer {
    #[default]
    Mpv,
    Jukebox,
}

impl AudioPlayer {
    pub const ALL: [AudioPlayer; 2] = [AudioPlayer::Mpv, AudioPlayer::Jukebox];
}

impl std::fmt::Display for AudioPlayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            AudioPlayer::Mpv => "mpv (this computer)",
            AudioPlayer::Jukebox => "Jukebox (the server plays)",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EqBand {
    pub freq: f32,
    pub gain: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Playback {
    /// mpv `gapless-audio`: `weak`, `yes` or `no`.
    pub gapless: String,
    pub crossfade_secs: f32,
    /// Songs fade in at their start and out at their end over this many
    /// seconds (0 = off). The AI DJ turns the music down/up at this speed too.
    pub track_fade_secs: f32,
    pub replay_gain: ReplayGain,
    pub replay_gain_preamp_db: f32,
    pub replay_gain_clip: bool,
    pub audio_device: String,
    pub mpv_extra_args: Vec<String>,
    pub fade_on_pause: bool,
    pub equalizer_enabled: bool,
    pub equalizer_preamp: f32,
    pub equalizer: Vec<EqBand>,
    /// Name of the last applied equalizer preset (shown in the preset picker).
    pub equalizer_preset: String,
    /// Equalizer presets the user saved.
    pub custom_eq_presets: BTreeMap<String, Vec<EqBand>>,
    pub compressor: Compressor,
    pub compressor_preset: String,
    pub custom_compressor_presets: BTreeMap<String, CompressorPreset>,
    pub stream_quality: StreamQuality,
    /// Explicit transcoding (format, bit rate, sample rate); overrides `stream_quality`.
    pub transcode: Transcode,
    /// Path of the mpv binary; empty = `mpv` from the PATH. Needs a restart (or a reload).
    pub mpv_path: String,
    /// Keep the pitch while the speed changes (mpv `audio-pitch-correction`).
    pub preserve_pitch: bool,
    /// Pitch steps of 10 cents besides the semitone buttons (when the pitch is not preserved).
    pub microtonal_pitch: bool,
    /// mpv `replaygain-fallback`: gain in dB for songs without ReplayGain tags.
    pub replay_gain_fallback_db: f32,
    /// mpv `audio-exclusive`.
    pub audio_exclusive: bool,
    /// mpv `audio-samplerate`; below 8000 = the device's default.
    pub sample_rate_hz: u32,
    /// mpv `audio-format` (`s16`, `s32`, `float`, ...); empty = automatic.
    pub audio_format: String,
    /// Audio player choice; the jukebox needs a server that supports it.
    pub audio_player: AudioPlayer,
    /// Look for DLNA / UPnP media renderers on the network and offer the cast button.
    pub dlna_enabled: bool,
    pub scrobble: bool,
    pub scrobble_at_percent: u8,
    pub scrobble_at_secs: u32,
    pub volume: u8,
}

impl Default for Playback {
    fn default() -> Self {
        Self {
            gapless: "weak".into(),
            crossfade_secs: 0.0,
            track_fade_secs: 1.5,
            replay_gain: ReplayGain::No,
            replay_gain_preamp_db: 0.0,
            replay_gain_clip: true,
            audio_device: "auto".into(),
            mpv_extra_args: vec![],
            fade_on_pause: true,
            equalizer_enabled: false,
            equalizer_preamp: 0.0,
            equalizer: EQ_FREQS.iter().map(|&freq| EqBand { freq, gain: 0.0 }).collect(),
            equalizer_preset: String::new(),
            custom_eq_presets: BTreeMap::new(),
            compressor: Compressor::default(),
            compressor_preset: String::new(),
            custom_compressor_presets: BTreeMap::new(),
            stream_quality: StreamQuality::Original,
            transcode: Transcode::default(),
            mpv_path: String::new(),
            preserve_pitch: true,
            microtonal_pitch: false,
            replay_gain_fallback_db: 0.0,
            audio_exclusive: false,
            sample_rate_hz: 0,
            audio_format: String::new(),
            audio_player: AudioPlayer::Mpv,
            dlna_enabled: true,
            scrobble: true,
            scrobble_at_percent: 75,
            scrobble_at_secs: 240,
            volume: 70,
        }
    }
}

/// mpv `af` value for the equalizer, as Feishin builds it
/// (`mpv-audio-filters.ts`): a preamp `volume` filter plus one `equalizer`
/// per non-zero band, gains clamped to ±12 dB. Empty = pass-through.
pub fn mpv_audio_filters(p: &Playback) -> String {
    let mut parts = Vec::new();
    if p.equalizer_enabled {
        eq_filters(p, &mut parts);
    }
    if p.compressor.enabled {
        parts.push(p.compressor.filter());
    }
    parts.join(",")
}

fn eq_filters(p: &Playback, parts: &mut Vec<String>) {
    // Feishin's per-band widths (in octaves).
    let width = |f: f32| match f {
        f if (f - 31.5).abs() < 0.1 => 1.9,
        f if (f - 63.0).abs() < 0.1 => 1.3,
        f if (f - 6300.0).abs() < 1.0 || (f - 10000.0).abs() < 1.0 => 1.2,
        f if (f - 16000.0).abs() < 1.0 => 1.5,
        _ => 1.0,
    };
    let preamp = p.equalizer_preamp.clamp(-12.0, 12.0);
    if preamp != 0.0 {
        parts.push(format!("volume={preamp}dB"));
    }
    for band in &p.equalizer {
        let gain = band.gain.clamp(-12.0, 12.0);
        if gain != 0.0 {
            parts.push(format!(
                "lavfi=[equalizer=f={}:width_type=o:w={}:g={gain}]",
                band.freq,
                width(band.freq)
            ));
        }
    }
}

/// Centre frequencies of the equalizer bands (Feishin's twelve).
pub const EQ_FREQS: [f32; 12] =
    [31.5, 63.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 3000.0, 4000.0, 6300.0, 10000.0, 16000.0];

/// Built-in equalizer presets (Feishin's `eq-settings.tsx`): one gain per band.
pub const EQ_PRESETS: [(&str, [f32; 12]); 15] = [
    ("Acoustic", [2.0, 2.0, 3.0, 2.0, 1.0, 0.0, 1.0, 2.0, 2.0, 2.0, 2.0, 1.0]),
    ("Bass Boost", [6.0, 5.0, 4.0, 2.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
    ("Bass Cut", [-6.0, -5.0, -4.0, -2.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
    ("Classical", [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, -1.0, 0.0, 0.0, 0.0, -3.0]),
    ("Electronic", [4.0, 3.0, 1.0, 0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 2.0, 3.0, 4.0]),
    ("Flat", [0.0; 12]),
    ("Hip-Hop", [5.0, 4.0, 2.0, 1.0, 0.0, -1.0, 0.0, 1.0, 0.0, 1.0, 2.0, 3.0]),
    ("Jazz", [2.0, 1.0, 0.0, 1.0, 2.0, 2.0, 1.0, 0.0, 0.0, 1.0, 2.0, 2.0]),
    ("Loudness", [5.0, 3.0, 1.0, 0.0, -1.0, -2.0, -2.0, -1.0, 0.0, 1.0, 3.0, 6.0]),
    ("Pop", [-1.0, 0.0, 2.0, 3.0, 3.0, 2.0, 0.0, -1.0, -1.0, 0.0, 0.0, 0.0]),
    ("Rock", [3.0, 2.0, 1.0, 0.0, -1.0, 0.0, 1.0, 2.0, 2.0, 2.0, 3.0, 3.0]),
    ("Treble Boost", [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0]),
    ("Treble Cut", [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, -2.0, -3.0, -4.0, -5.0, -6.0]),
    ("V-Shape", [5.0, 3.0, 1.0, 0.0, -1.0, -2.0, -2.0, -1.0, 0.0, 1.0, 3.0, 5.0]),
    ("Vocal Boost", [-1.0, 0.0, 1.0, 2.0, 3.0, 4.0, 4.0, 3.0, 2.0, 1.0, 0.0, -1.0]),
];

/// The gain of the preset band nearest (in octaves) to `freq`.
fn nearest_gain(preset: &[EqBand], freq: f32) -> f32 {
    preset
        .iter()
        .min_by(|a, b| {
            let d = |x: &EqBand| (x.freq.max(1.0).ln() - freq.max(1.0).ln()).abs();
            d(a).total_cmp(&d(b))
        })
        .map_or(0.0, |b| b.gain)
}

impl Playback {
    /// Sets the band gains from `preset` and the preamp to 0, as Feishin does.
    /// The bands keep their frequencies: a config from before the twelve-band
    /// layout (ten bands) takes the gain of the nearest preset band.
    pub fn apply_eq_preset(&mut self, name: &str, preset: &[EqBand]) {
        for band in &mut self.equalizer {
            band.gain = nearest_gain(preset, band.freq);
        }
        self.equalizer_preamp = 0.0;
        self.equalizer_preset = name.to_string();
    }

    /// A built-in or saved preset (saved ones first).
    pub fn eq_preset(&self, name: &str) -> Option<Vec<EqBand>> {
        self.custom_eq_presets.get(name).cloned().or_else(|| {
            EQ_PRESETS.iter().find(|(n, _)| *n == name).map(|(_, gains)| {
                EQ_FREQS.iter().zip(gains).map(|(&freq, &gain)| EqBand { freq, gain }).collect()
            })
        })
    }

    /// Saves the current band gains under `name`.
    pub fn save_eq_preset(&mut self, name: &str) {
        self.custom_eq_presets.insert(name.to_string(), self.equalizer.clone());
        self.equalizer_preset = name.to_string();
    }

    /// All bands and the preamp back to 0.
    pub fn reset_eq(&mut self) {
        self.apply_eq_preset("", &[]);
    }

    /// A built-in or saved compressor preset (saved ones first).
    pub fn compressor_preset(&self, name: &str) -> Option<CompressorPreset> {
        self.custom_compressor_presets
            .get(name)
            .copied()
            .or_else(|| COMPRESSOR_PRESETS.iter().find(|(n, _)| *n == name).map(|(_, p)| *p))
    }
}

/// The part of a compressor setting a preset holds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CompressorPreset {
    /// dB, -60..=0.
    pub threshold: f32,
    /// n:1, 1..=20.
    pub ratio: f32,
    /// ms.
    pub attack: f32,
    /// ms.
    pub release: f32,
    /// dB, 0..=30.
    pub makeup: f32,
    /// dB soft-knee width, 1..=10.
    pub knee: f32,
}

impl Default for CompressorPreset {
    fn default() -> Self {
        COMPRESSOR_PRESETS[1].1
    }
}

const fn cp(
    threshold: f32,
    ratio: f32,
    attack: f32,
    release: f32,
    makeup: f32,
    knee: f32,
) -> CompressorPreset {
    CompressorPreset { threshold, ratio, attack, release, makeup, knee }
}

/// Built-in compressor presets (Feishin's `eq-settings.tsx`).
pub const COMPRESSOR_PRESETS: [(&str, CompressorPreset); 8] = [
    ("Broadcast", cp(-20.0, 5.0, 15.0, 200.0, 6.0, 3.0)),
    ("Default", cp(-24.0, 4.0, 20.0, 250.0, 6.0, 2.83)),
    ("Gentle", cp(-15.0, 1.5, 50.0, 500.0, 2.0, 6.0)),
    ("Heavy", cp(-30.0, 8.0, 10.0, 150.0, 8.0, 2.0)),
    ("Light", cp(-18.0, 2.0, 30.0, 400.0, 3.0, 4.0)),
    ("Limiter", cp(-3.0, 20.0, 1.0, 100.0, 0.0, 1.0)),
    ("Loud Master", cp(-28.0, 6.0, 5.0, 100.0, 10.0, 2.0)),
    ("Moderate", cp(-24.0, 4.0, 20.0, 300.0, 5.0, 3.0)),
];

/// ffmpeg `acompressor` through mpv's `lavfi` filter.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Compressor {
    pub enabled: bool,
    pub params: CompressorPreset,
}

impl Compressor {
    /// The `af` entry, as Feishin builds it: threshold and makeup are linear
    /// amplitudes in `acompressor`, so the dB values are converted.
    pub fn filter(&self) -> String {
        let c = &self.params;
        let threshold = 10f32.powf(c.threshold.clamp(-60.0, 0.0) / 20.0);
        let makeup = 10f32.powf(c.makeup.clamp(0.0, 30.0) / 20.0);
        format!(
            "lavfi=[acompressor=threshold={threshold:.6}:ratio={}:attack={}:release={}:makeup={makeup:.6}:knee={}]",
            c.ratio.clamp(1.0, 20.0),
            c.attack.clamp(0.01, 2000.0),
            c.release.clamp(0.01, 9000.0),
            c.knee.clamp(1.0, 8.0),
        )
    }
}

/// Explicit transcoding (Feishin's "Transcode" settings).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Transcode {
    pub enabled: bool,
    /// `mp3`, `opus`, `flac`, ...; empty = the server's choice (Subsonic) or MP3 (Jellyfin).
    pub format: String,
    /// Maximum bit rate in kbit/s; 0 = the server picks.
    pub bitrate: u32,
    /// Maximum sample rate in Hz (Jellyfin); 0 = no cap.
    pub max_sample_rate: u32,
}

impl Transcode {
    /// Transcoding is on and asks for something.
    pub fn active(&self) -> bool {
        self.enabled
    }

    /// Format in lower case, if set.
    pub fn format(&self) -> Option<String> {
        let f = self.format.trim().to_lowercase();
        (!f.is_empty()).then_some(f)
    }
}

/// Parses mpv's extra parameters: one per line; blank lines are dropped and a
/// missing `--` is added (`gapless-audio=weak` -> `--gapless-audio=weak`).
pub fn parse_mpv_extra_args(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| if l.starts_with('-') { l.to_string() } else { format!("--{l}") })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compressor_follows_feishin_in_linear_amplitudes() {
        let mut p = Playback::default();
        assert_eq!(mpv_audio_filters(&p), "", "off by default");
        p.compressor.enabled = true;
        // Defaults: -24 dB threshold, +6 dB makeup.
        assert_eq!(
            mpv_audio_filters(&p),
            "lavfi=[acompressor=threshold=0.063096:ratio=4:attack=20:release=250:makeup=1.995262:knee=2.83]"
        );
    }

    #[test]
    fn equalizer_comes_before_the_compressor() {
        let mut p = Playback { equalizer_enabled: true, ..Default::default() };
        p.equalizer[0].gain = 2.0;
        p.compressor.enabled = true;
        let af = mpv_audio_filters(&p);
        assert!(af.starts_with("lavfi=[equalizer=f=31.5"), "{af}");
        assert!(af.contains(",lavfi=[acompressor="), "{af}");
        // The DJ decks keep the user's chain in front of the mix filters.
        let deck = deck_audio_filters(&p);
        assert!(
            deck.starts_with(&af) && deck.contains(",@mix:") && deck.ends_with("@ts:rubberband=engine=finer")
        );
    }

    #[test]
    fn compressor_values_are_clamped_to_what_ffmpeg_accepts() {
        let mut p = Playback::default();
        p.compressor.enabled = true;
        p.compressor.params = CompressorPreset {
            threshold: -90.0,
            ratio: 99.0,
            attack: 0.0,
            release: 99999.0,
            makeup: 99.0,
            knee: 10.0,
        };
        let af = mpv_audio_filters(&p);
        assert!(af.contains("ratio=20:attack=0.01:release=9000:") && af.contains("knee=8]"), "{af}");
    }

    #[test]
    fn eq_presets_have_one_gain_per_band() {
        assert_eq!(EQ_FREQS.len(), 12);
        assert_eq!(Playback::default().equalizer.len(), 12);
        let mut p = Playback { equalizer_preamp: 3.0, ..Default::default() };
        let preset = p.eq_preset("Bass Boost").unwrap();
        p.apply_eq_preset("Bass Boost", &preset);
        assert_eq!(p.equalizer[0].gain, 6.0);
        assert_eq!(p.equalizer[11].gain, 0.0);
        assert_eq!(p.equalizer_preamp, 0.0, "a preset resets the preamp, as in Feishin");
        p.reset_eq();
        assert!(p.equalizer.iter().all(|b| b.gain == 0.0) && p.equalizer_preset.is_empty());
    }

    #[test]
    fn custom_presets_are_saved_and_found_before_built_in_ones() {
        let mut p = Playback::default();
        p.equalizer[2].gain = 4.0;
        p.save_eq_preset("Mine");
        assert_eq!(p.eq_preset("Mine").unwrap()[2].gain, 4.0);
        p.save_eq_preset("Flat");
        assert_eq!(p.eq_preset("Flat").unwrap()[2].gain, 4.0, "a saved preset shadows the built-in one");
        assert_eq!(p.compressor_preset("Limiter").unwrap().ratio, 20.0);
        assert!(p.compressor_preset("nope").is_none());
    }

    #[test]
    fn presets_fit_the_old_ten_band_layout_by_frequency() {
        let old = [31.5, 63.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];
        let mut p = Playback {
            equalizer: old.iter().map(|&freq| EqBand { freq, gain: 9.0 }).collect(),
            ..Default::default()
        };
        let bass = p.eq_preset("Bass Boost").unwrap();
        p.apply_eq_preset("Bass Boost", &bass);
        assert_eq!(p.equalizer.len(), 10, "the layout is kept");
        assert_eq!(p.equalizer[0].gain, 6.0);
        assert_eq!(p.equalizer[5].gain, 0.0);
        // 8 kHz sits between 6.3 k and 10 k (both 0 here).
        assert_eq!(p.equalizer[8].gain, 0.0);
        // A saved preset of the same layout comes back unchanged.
        p.equalizer[2].gain = 3.5;
        p.save_eq_preset("Mine");
        p.reset_eq();
        let mine = p.eq_preset("Mine").unwrap();
        p.apply_eq_preset("Mine", &mine);
        assert_eq!(p.equalizer[2].gain, 3.5);
        // The old config file still reads (and keeps its ten bands).
        let toml = toml::to_string(&p).unwrap();
        assert_eq!(toml::from_str::<Playback>(&toml).unwrap().equalizer.len(), 10);
        assert!(p.preserve_pitch && !p.transcode.enabled && p.mpv_path.is_empty());
    }

    #[test]
    fn extra_parameters_are_one_per_line() {
        assert_eq!(
            parse_mpv_extra_args("--gapless-audio=weak\n\n  prefetch-playlist=yes \r\n-v"),
            ["--gapless-audio=weak", "--prefetch-playlist=yes", "-v"]
        );
        assert!(parse_mpv_extra_args("  \n").is_empty());
    }

    #[test]
    fn transcode_format_is_trimmed_and_lowercase() {
        let t = Transcode { enabled: true, format: " Opus ".into(), ..Default::default() };
        assert_eq!(t.format().as_deref(), Some("opus"));
        assert_eq!(Transcode::default().format(), None);
    }
}
