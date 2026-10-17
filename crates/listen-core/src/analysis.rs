//! Song audio analysis records (tempo, beat grid, key, energy, sections, timbre) shared by
//! `listen-analysis`, `listen-mix` and the DJs; serde types only. Keyed by song id plus
//! [`FileId`], not by the per-machine [`crate::model::ServerId`], so they work across machines.

use crate::model::Song;
use serde::{Deserialize, Serialize};

/// Bump when the analysis changes in a way that makes old records worse
/// (they are re-analysed at low priority).
pub const ANALYZER_VERSION: u32 = 3;
/// Records older than this are ignored entirely.
pub const MIN_COMPATIBLE_VERSION: u32 = 1;

/// Identifies the audio file behind a song id (a re-encoded or replaced file
/// gets a new analysis).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileId {
    /// 0 = unknown (some YouTube Music lists carry no duration).
    pub duration_ms: u32,
    /// Lower-case file suffix (`flac`, `mp3`, …), if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suffix: Option<String>,
    /// The exact audio variant, where a song comes in several (YouTube's
    /// itag, e.g. "251" Opus or "140" AAC).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// Size of the audio file in bytes (YouTube's `clen`), if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
}

impl FileId {
    pub fn of(song: &Song) -> Self {
        FileId {
            duration_ms: song.duration.as_millis().min(u128::from(u32::MAX)) as u32,
            suffix: song.codec.as_ref().map(|c| c.to_lowercase()),
            format: None,
            bytes: None,
        }
    }

    /// Same file: durations within a second (unless either is unknown), and the same
    /// suffix, format and size where both sides know them.
    pub fn matches(&self, other: &FileId) -> bool {
        fn same<T: PartialEq>(a: &Option<T>, b: &Option<T>) -> bool {
            match (a, b) {
                (Some(a), Some(b)) => a == b,
                _ => true,
            }
        }
        (self.duration_ms == 0
            || other.duration_ms == 0
            || self.duration_ms.abs_diff(other.duration_ms) <= 1000)
            && same(&self.suffix, &other.suffix)
            && same(&self.format, &other.format)
            && same(&self.bytes, &other.bytes)
    }
}

/// A key on the Camelot wheel: `number` 1–12, `minor` = the "A" ring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Camelot {
    pub number: u8,
    pub minor: bool,
}

impl Camelot {
    /// From a pitch class (0 = C … 11 = B) and mode.
    pub fn from_pitch(pitch_class: u8, minor: bool) -> Self {
        // Camelot numbers of C, C#, D, … for major (B ring) and minor (A ring).
        const MAJOR: [u8; 12] = [8, 3, 10, 5, 12, 7, 2, 9, 4, 11, 6, 1];
        const MINOR: [u8; 12] = [5, 12, 7, 2, 9, 4, 11, 6, 1, 8, 3, 10];
        let pc = (pitch_class % 12) as usize;
        Camelot { number: if minor { MINOR[pc] } else { MAJOR[pc] }, minor }
    }

    /// How well two keys mix: 0 = same key, 0.1 = ±1 on the wheel (same
    /// ring), 0.15 = relative major/minor, 0.35 = ±2 or diagonal (energy
    /// boost), 1 = clash.
    pub fn distance(self, other: Camelot) -> f32 {
        let step = {
            let d = (i16::from(self.number) - i16::from(other.number)).rem_euclid(12);
            d.min(12 - d)
        };
        match (step, self.minor == other.minor) {
            (0, true) => 0.0,
            (1, true) => 0.1,
            (0, false) => 0.15,
            (2, true) | (1, false) => 0.35,
            _ => 1.0,
        }
    }

    /// "8A", "11B".
    pub fn label(self) -> String {
        format!("{}{}", self.number, if self.minor { 'A' } else { 'B' })
    }
}

/// Beat positions and bar structure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BeatGrid {
    /// Every beat, seconds of file time (as mpv plays it).
    #[serde(with = "beat_codec")]
    pub beats: Vec<f32>,
    /// Index in `beats` of the first downbeat (0..beats_per_bar).
    pub downbeat_phase: u8,
    pub beats_per_bar: u8,
    pub bpm: f32,
    /// 0..1: how sure the tempo is.
    pub confidence: f32,
    /// 0..1: 1 − variation of the beat intervals (1 = machine-steady).
    pub stability: f32,
}

impl BeatGrid {
    /// Downbeat times (first beat of every bar).
    pub fn downbeats(&self) -> impl Iterator<Item = f32> + '_ {
        let per = usize::from(self.beats_per_bar.max(1));
        self.beats.iter().skip(usize::from(self.downbeat_phase)).step_by(per).copied()
    }

    /// Fractional beat index at time `t` (linear between beats; extrapolated
    /// with the average tempo outside the grid).
    pub fn beat_at(&self, t: f32) -> f32 {
        let b = &self.beats;
        let period = 60.0 / self.bpm.max(1.0);
        match b.len() {
            0 => t / period,
            1 => (t - b[0]) / period,
            n => {
                if t <= b[0] {
                    return (t - b[0]) / period;
                }
                if t >= b[n - 1] {
                    return (n - 1) as f32 + (t - b[n - 1]) / period;
                }
                let i = b.partition_point(|&x| x <= t) - 1;
                i as f32 + (t - b[i]) / (b[i + 1] - b[i]).max(1e-4)
            }
        }
    }

    /// Time of fractional beat index `i` (inverse of [`Self::beat_at`]).
    pub fn time_of(&self, i: f32) -> f32 {
        let b = &self.beats;
        let period = 60.0 / self.bpm.max(1.0);
        match b.len() {
            0 => i * period,
            1 => b[0] + i * period,
            n => {
                if i <= 0.0 {
                    return b[0] + i * period;
                }
                if i >= (n - 1) as f32 {
                    return b[n - 1] + (i - (n - 1) as f32) * period;
                }
                let k = i.floor() as usize;
                b[k] + (i - k as f32) * (b[k + 1] - b[k])
            }
        }
    }

    /// Time of the start of bar `bar` (0 = first downbeat).
    pub fn bar_time(&self, bar: usize) -> f32 {
        self.time_of((usize::from(self.downbeat_phase) + bar * usize::from(self.beats_per_bar.max(1))) as f32)
    }

    pub fn bar_count(&self) -> usize {
        self.beats.len().saturating_sub(usize::from(self.downbeat_phase))
            / usize::from(self.beats_per_bar.max(1))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SectionKind {
    Intro,
    Body,
    Breakdown,
    Build,
    Drop,
    Outro,
}

/// A stretch of bars (on the beat grid).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Section {
    pub start_bar: u16,
    pub bars: u16,
    pub kind: SectionKind,
}

/// One value per bar, 0–255, normalised within the song.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BarCurves {
    /// Overall loudness.
    pub energy: Vec<u8>,
    /// Bass (< 150 Hz): kick and bassline.
    pub low: Vec<u8>,
    /// How likely a lead vocal is present.
    pub vocal: Vec<u8>,
    /// Brightness (spectral centroid).
    pub centroid: Vec<u8>,
    /// How much changes at this bar (section boundaries peak).
    pub novelty: Vec<u8>,
}

/// Coarse descriptors for choosing songs and for the AI DJ prompt.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Mood {
    /// 0..1 (loudness, density).
    pub energy: f32,
    /// 0..1 (beat strength × steadiness).
    pub danceability: f32,
    /// 0..1 (mean spectral centroid, log scale 1.4–2.6 kHz).
    pub brightness: f32,
}

/// Everything the analyser found in a song.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Features {
    /// First / last moment above −60 dBFS (seconds).
    pub audio_start: f32,
    pub audio_end: f32,
    /// `None`: no steady beat (ambient, rubato, live) → plain fades only.
    pub grid: Option<BeatGrid>,
    /// Key and how sure the analyser is (0..1).
    pub key: Option<(Camelot, f32)>,
    /// Integrated loudness (LUFS) and loudness range (LU).
    pub lufs: f32,
    pub lra: f32,
    /// Bar 0 is the first downbeat. Without a grid, bars (here and in
    /// `sections`) are 2-second pseudo-bars from `audio_start`.
    pub bars: BarCurves,
    /// Phrase length in bars (4, 8 or 16) and the bar the lattice starts at.
    pub phrase_bars: u8,
    pub phrase_offset: u8,
    pub sections: Vec<Section>,
    /// Bars at the start / end without a lead vocal (mixable).
    pub intro_instrumental_bars: u16,
    pub outro_instrumental_bars: u16,
    /// Timbre/similarity vector (comparable only within one analyser version).
    pub timbre: Vec<f32>,
    pub mood: Mood,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Outcome {
    Ok(Box<Features>),
    /// `transient`: worth retrying (network); otherwise skipped for this version.
    Failed {
        reason: String,
        transient: bool,
    },
}

/// One analysis record (a line in a shard file).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackAnalysis {
    pub version: u32,
    /// Unix seconds.
    pub analyzed_at: u64,
    /// Machine that made it (shard directory name).
    pub machine: String,
    /// Decoder used, e.g. "mpv 0.41.0".
    pub decoder: String,
    pub song_id: String,
    /// Host of the server, informational.
    pub server_hint: String,
    pub file: FileId,
    /// "Artist – Title", for humans reading the files.
    pub title_hint: String,
    pub outcome: Outcome,
}

impl TrackAnalysis {
    pub fn features(&self) -> Option<&Features> {
        match &self.outcome {
            Outcome::Ok(f) => Some(f),
            Outcome::Failed { .. } => None,
        }
    }

    /// Is this record better than `other` for the same song? Ok beats
    /// Failed, then newer analyser, then newer, then a fixed machine order.
    pub fn better_than(&self, other: &TrackAnalysis) -> bool {
        let rank = |r: &TrackAnalysis| (r.features().is_some(), r.version, r.analyzed_at);
        match rank(self).cmp(&rank(other)) {
            std::cmp::Ordering::Equal => self.machine < other.machine,
            o => o.is_gt(),
        }
    }
}

/// The light view the song choosers need.
#[derive(Debug, Clone, PartialEq)]
pub struct FlowFeatures {
    pub bpm: Option<f32>,
    pub bpm_confidence: f32,
    pub key: Option<(Camelot, f32)>,
    pub lufs: f32,
    pub mood: Mood,
    pub timbre: Vec<f32>,
    /// Has an instrumental start / end to mix over.
    pub mixable_in: bool,
    pub mixable_out: bool,
}

impl FlowFeatures {
    pub fn of(f: &Features) -> Self {
        FlowFeatures {
            bpm: f.grid.as_ref().map(|g| g.bpm),
            bpm_confidence: f.grid.as_ref().map_or(0.0, |g| g.confidence),
            key: f.key,
            lufs: f.lufs,
            mood: f.mood.clone(),
            timbre: f.timbre.clone(),
            mixable_in: f.grid.is_some() && f.intro_instrumental_bars >= 8,
            mixable_out: f.grid.is_some() && f.outro_instrumental_bars >= 8,
        }
    }

    /// "124 bpm, 8A, energy 7/10" for the AI DJ prompt and the queue.
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if let Some(bpm) = self.bpm.filter(|_| self.bpm_confidence >= 0.35) {
            parts.push(format!("{} bpm", bpm.round()));
        }
        if let Some((key, conf)) = self.key
            && conf >= 0.3
        {
            parts.push(key.label());
        }
        parts.push(format!("energy {}/10", (self.mood.energy * 10.0).round().clamp(1.0, 10.0)));
        if self.mood.brightness >= 0.66 {
            parts.push("bright".into());
        } else if self.mood.brightness <= 0.33 {
            parts.push("dark".into());
        }
        parts.join(", ")
    }
}

/// Beat times as delta-encoded milliseconds in one string ("12,500,498,…"),
/// about a third of the size of a float list.
mod beat_codec {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(beats: &[f32], s: S) -> Result<S::Ok, S::Error> {
        let mut out = String::with_capacity(beats.len() * 4);
        let mut prev = 0i64;
        for (i, b) in beats.iter().enumerate() {
            let ms = (f64::from(*b) * 1000.0).round() as i64;
            if i > 0 {
                out.push(',');
            }
            out.push_str(&(ms - prev).to_string());
            prev = ms;
        }
        s.serialize_str(&out)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<f32>, D::Error> {
        let text = String::deserialize(d)?;
        let mut prev = 0i64;
        let mut beats = Vec::new();
        for part in text.split(',').filter(|p| !p.is_empty()) {
            let delta: i64 = part.trim().parse().map_err(serde::de::Error::custom)?;
            prev += delta;
            beats.push(prev as f32 / 1000.0);
        }
        Ok(beats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(bpm: f32, n: usize, offset: f32) -> BeatGrid {
        let p = 60.0 / bpm;
        BeatGrid {
            beats: (0..n).map(|i| offset + i as f32 * p).collect(),
            downbeat_phase: 1,
            beats_per_bar: 4,
            bpm,
            confidence: 0.9,
            stability: 0.99,
        }
    }

    #[test]
    fn camelot_wheel() {
        // C major = 8B, A minor = 8A, G major = 9B, E minor = 9A.
        assert_eq!(Camelot::from_pitch(0, false).label(), "8B");
        assert_eq!(Camelot::from_pitch(9, true).label(), "8A");
        assert_eq!(Camelot::from_pitch(7, false).label(), "9B");
        assert_eq!(Camelot::from_pitch(4, true).label(), "9A");
        let k = |n, m| Camelot { number: n, minor: m };
        assert_eq!(k(8, true).distance(k(8, true)), 0.0);
        assert_eq!(k(12, true).distance(k(1, true)), 0.1); // wraps around
        assert_eq!(k(8, true).distance(k(8, false)), 0.15);
        assert_eq!(k(8, true).distance(k(10, true)), 0.35);
        assert_eq!(k(8, true).distance(k(2, false)), 1.0);
    }

    #[test]
    fn grid_lookup_and_bars() {
        let g = grid(120.0, 33, 0.25);
        assert!((g.beat_at(0.25 + 1.0) - 2.0).abs() < 1e-4);
        assert!((g.time_of(2.5) - 1.5).abs() < 1e-4);
        assert!((g.beat_at(-0.25) + 1.0).abs() < 1e-4); // before the grid
        assert_eq!(g.bar_count(), 8);
        assert!((g.bar_time(1) - (0.25 + 5.0 * 0.5)).abs() < 1e-4);
        assert_eq!(g.downbeats().take(2).collect::<Vec<_>>(), [0.75, 2.75]);
    }

    #[test]
    fn records_roundtrip_compactly() {
        let rec = TrackAnalysis {
            version: ANALYZER_VERSION,
            analyzed_at: 1,
            machine: "desktop-abc".into(),
            decoder: "mpv".into(),
            song_id: "s1".into(),
            server_hint: "musik".into(),
            file: FileId { duration_ms: 180_000, suffix: Some("flac".into()), format: None, bytes: None },
            title_hint: "A – B".into(),
            outcome: Outcome::Ok(Box::new(Features {
                audio_start: 0.1,
                audio_end: 179.0,
                grid: Some(grid(128.0, 300, 0.1)),
                key: Some((Camelot { number: 8, minor: true }, 0.8)),
                lufs: -9.0,
                lra: 5.0,
                bars: BarCurves::default(),
                phrase_bars: 8,
                phrase_offset: 0,
                sections: vec![Section { start_bar: 0, bars: 8, kind: SectionKind::Intro }],
                intro_instrumental_bars: 8,
                outro_instrumental_bars: 16,
                timbre: vec![0.5; 20],
                mood: Mood { energy: 0.7, danceability: 0.8, brightness: 0.5 },
            })),
        };
        let json = serde_json::to_string(&rec).unwrap();
        assert!(json.len() < 2500, "{}", json.len());
        let back: TrackAnalysis = serde_json::from_str(&json).unwrap();
        let (a, b) = (rec.features().unwrap(), back.features().unwrap());
        let (ga, gb) = (a.grid.as_ref().unwrap(), b.grid.as_ref().unwrap());
        assert_eq!(ga.beats.len(), gb.beats.len());
        assert!(ga.beats.iter().zip(&gb.beats).all(|(x, y)| (x - y).abs() < 0.001));
        assert_eq!(FlowFeatures::of(b).describe(), "128 bpm, 8A, energy 7/10");
        let mut f = FlowFeatures::of(b);
        f.mood.brightness = 0.8;
        f.key = Some((Camelot::from_pitch(9, true), 0.2));
        assert_eq!(f.describe(), "128 bpm, energy 7/10, bright", "unsure key left out");
        f.mood.brightness = 0.2;
        assert!(f.describe().ends_with("dark"));
    }

    #[test]
    fn file_ids_and_record_preference() {
        let a = FileId { duration_ms: 200_000, suffix: Some("flac".into()), format: None, bytes: None };
        assert!(a.matches(&FileId { duration_ms: 200_900, suffix: None, format: None, bytes: None }));
        // Unknown duration (YouTube Music lists): matched on the other fields.
        assert!(a.matches(&FileId { duration_ms: 0, suffix: None, format: None, bytes: None }));
        let yt = FileId { duration_ms: 0, suffix: None, format: Some("251".into()), bytes: Some(3_445_462) };
        assert!(!yt.matches(&FileId { bytes: Some(3_400_000), ..yt.clone() }));
        assert!(!yt.matches(&FileId { format: Some("140".into()), ..yt.clone() }));
        assert!(!a.matches(&FileId {
            duration_ms: 200_000,
            suffix: Some("mp3".into()),
            format: None,
            bytes: None
        }));
        assert!(!a.matches(&FileId {
            duration_ms: 202_000,
            suffix: Some("flac".into()),
            format: None,
            bytes: None
        }));
        let base = TrackAnalysis {
            version: 1,
            analyzed_at: 10,
            machine: "b".into(),
            decoder: String::new(),
            song_id: "s".into(),
            server_hint: String::new(),
            file: a,
            title_hint: String::new(),
            outcome: Outcome::Failed { reason: "x".into(), transient: true },
        };
        let mut ok = base.clone();
        ok.outcome = Outcome::Ok(Box::new(Features {
            audio_start: 0.0,
            audio_end: 1.0,
            grid: None,
            key: None,
            lufs: -10.0,
            lra: 0.0,
            bars: BarCurves::default(),
            phrase_bars: 8,
            phrase_offset: 0,
            sections: vec![],
            intro_instrumental_bars: 0,
            outro_instrumental_bars: 0,
            timbre: vec![],
            mood: Mood::default(),
        }));
        ok.analyzed_at = 1;
        assert!(ok.better_than(&base));
        let mut newer = ok.clone();
        newer.version = 2;
        assert!(newer.better_than(&ok));
        let mut other_machine = ok.clone();
        other_machine.machine = "a".into();
        assert!(other_machine.better_than(&ok));
    }
}
