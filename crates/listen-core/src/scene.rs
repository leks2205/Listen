//! Scenes: which musical world a song belongs to (K-pop, US hip hop, Brazilian samba, …), so a DJ bridge
//! between two K-pop songs is never an American rap record that merely blends well.
//!
//! A [`Scene`] is a language (or "instrumental") plus a set of genre [`Families`]. It is read from the
//! best signal a song has, strongest first:
//!
//! 1. the describe-it tags (`tags_en` of `embeddings/v1/clamp3/*.jsonl`, Gemini's "K-pop, dance-pop,
//!    2020s, South Korea, Korean, girl group, …"), parsed by [`Scene::from_tags`];
//! 2. the song's own genres (Navidrome / Jellyfin / local tags), [`Scene::from_genres`];
//! 3. the artist's other tagged songs ([`SceneIndex`] keeps one scene per artist);
//! 4. the script of the title and artist (Hangul → Korean, kana → Japanese), language only.
//!
//! Where two songs can't be compared on tags, their CLaMP 3 audio embeddings can: on the index of
//! 2026-11-06 (305 songs, 184 K-pop, 14 US hip hop) K-pop↔K-pop pairs have a median cosine of 0.59
//! (p25 0.49) and K-pop↔US hip hop 0.41 (p95 0.56, 2 % ≥ 0.60), so ≥ [`AUDIO_SAME`] counts as the same
//! scene. Same artist is the same scene unless both eras are known and more than a decade apart; an artist
//! heard right next to another in the listening history ([`Neighbours`]) is too when nothing else is known.
//!
//! A scene also has an **era** (the decade: before 1980, the 1980s, …, the 2020s), read from the tags
//! ("2020s", "90s ballad", "1st-generation K-pop"), else the song's year, else the artist's other tagged
//! songs, else the eras of the songs that sound closest (CLaMP 3, ≥ [`AUDIO_SAME`]). Two songs whose eras
//! are both known must be at most one decade apart (the 2010s and the 2020s are one scene, the 1980s and
//! the 2020s are not). When only one era is known, language and genre must both match, and the score is
//! lower.
//!
//! [`Gate`] applies this to a bridge: when the two songs it joins share a scene the bridge must belong to
//! it, when they differ it must match one of them, and a candidate whose scene is unknown is rejected
//! when the songs' scenes are known (no bridge is better than a wrong one). [`Session`] applies it to the
//! songs the heart-rate tempo mode adds: they must belong to the scene of what is playing (the majority of
//! the last few known scenes, or one of them when the session is mixed).

use crate::describe::{EmbedRecord, decode_vec, split_hint};
use crate::history::Entry as Play;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Audio-embedding cosine at or above which two songs without comparable tags count as one scene
/// (measured: 48 % of K-pop↔K-pop pairs, 2 % of K-pop↔US hip hop, 8 % of K-pop↔other English songs).
pub const AUDIO_SAME: f32 = 0.60;
/// Plays at most this far apart (seconds, start to start) count as heard next to each other.
const NEIGHBOUR_GAP_S: i64 = 20 * 60;
/// Two known eras at most this many years apart (decade starts) are one scene.
pub const ERA_SPAN: u16 = 10;
/// Eras before this decade count as one ("before 1980").
const EARLY_ERA: u16 = 1970;
/// A [`Session`] is read from at most this many songs with a known scene.
pub const SESSION_LEN: usize = 5;
/// Score factor when only one of two songs has a known era.
const ONE_ERA: f32 = 0.85;

/// Genre families as bits; a song may have several ("K-pop, hip hop" is pop and hip hop).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Families(pub u16);

impl Families {
    pub const POP: u16 = 1 << 0;
    pub const HIPHOP: u16 = 1 << 1;
    pub const RNB: u16 = 1 << 2;
    pub const ROCK: u16 = 1 << 3;
    pub const ELECTRONIC: u16 = 1 << 4;
    pub const JAZZ: u16 = 1 << 5;
    pub const FOLK: u16 = 1 << 6;
    pub const TROT: u16 = 1 << 7;
    pub const LATIN: u16 = 1 << 8;
    pub const CLASSICAL: u16 = 1 << 9;
    pub const AMBIENT: u16 = 1 << 10;
    pub const BLUES: u16 = 1 << 11;
    pub const COUNTRY: u16 = 1 << 12;
    pub const REGGAE: u16 = 1 << 13;
    const NAMES: [&'static str; 14] = [
        "pop",
        "hip hop",
        "r&b",
        "rock",
        "electronic",
        "jazz",
        "folk",
        "trot",
        "latin",
        "classical",
        "ambient",
        "blues",
        "country",
        "reggae",
    ];
    /// Not a family: set by a bare "dance" tag, which is pop unless the song is electronic ("dance, house").
    const DANCE: u16 = 1 << 15;
    /// Families too broad to place a song on their own: an instrumental record shares a scene with vocal
    /// songs only through another family (a K-pop song and a house track are both "pop"/"electronic").
    pub const GENERIC: u16 = Self::POP | Self::ELECTRONIC;
    /// Families close enough that one may bridge the other (half a match). Pop is near none: Korean R&B
    /// or hip hop, or an electronic record, never stands in for K-pop.
    const NEAR: [(u16, u16); 5] = [
        (Self::HIPHOP, Self::RNB),
        (Self::ROCK, Self::FOLK),
        (Self::JAZZ, Self::LATIN),
        (Self::RNB, Self::BLUES),
        (Self::JAZZ, Self::BLUES),
    ];

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// 1 when they share a family, 0.5 when two are near, 0 otherwise.
    fn overlap(self, other: Families) -> f32 {
        if self.0 & other.0 != 0 {
            return 1.0;
        }
        let near = Self::NEAR
            .iter()
            .any(|&(a, b)| (self.0 & a != 0 && other.0 & b != 0) || (self.0 & b != 0 && other.0 & a != 0));
        if near { 0.5 } else { 0.0 }
    }

    /// "pop, hip hop" for logs.
    pub fn label(self) -> String {
        let names: Vec<&str> =
            (0..Self::NAMES.len()).filter(|i| self.0 & (1 << i) != 0).map(|i| Self::NAMES[i]).collect();
        names.join(", ")
    }
}

/// Where a scene was read from (weakest first).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Source {
    #[default]
    None,
    Script,
    Artist,
    Genres,
    Tags,
}

/// A song's scene: language and genre families; either may be unknown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Scene {
    /// ISO 639-1 code of the sung language.
    pub lang: Option<&'static str>,
    /// No vocals to speak of: fits any language.
    pub instrumental: bool,
    pub families: Families,
    pub source: Source,
    /// The decade it comes from (1980, 1990, …; 1970 stands for everything before 1980).
    pub era: Option<u16>,
}

/// The era of a year: its decade, everything before 1980 as one.
pub fn era_of_year(year: i32) -> Option<u16> {
    if !(1900..=2099).contains(&year) {
        return None;
    }
    Some((year as u16 / 10 * 10).max(EARLY_ERA))
}

/// "2020s", "before 1980" for logs.
pub fn era_label(era: u16) -> String {
    if era <= EARLY_ERA { "before 1980".into() } else { format!("{era}s") }
}

/// The era a tag names: a decade ("2020s", "90s ballad", "90skpop"), a year ("1999"), or a K-pop
/// generation ("1st-generation K-pop": the 1990s … "5th-generation": the 2020s). The second value is
/// true for an explicit decade or year (it beats a generation).
fn era_of_tag(tag: &str) -> Option<(u16, bool)> {
    let w = words(tag);
    for x in &w {
        let digits: String = x.chars().take_while(|c| c.is_ascii_digit()).collect();
        let rest = &x[digits.len()..];
        let decade = match (digits.len(), rest.starts_with('s')) {
            (4, true) => digits.parse::<i32>().ok().filter(|y| y % 10 == 0),
            // "80s", "90s", "00s", "10s"; "20s" could be the 1920s.
            (2, true) => match digits.as_str() {
                "30" | "40" | "50" | "60" | "70" | "80" | "90" => {
                    digits.parse::<i32>().ok().map(|d| 1900 + d)
                }
                "00" | "10" => digits.parse::<i32>().ok().map(|d| 2000 + d),
                _ => None,
            },
            (4, false) if rest.is_empty() => digits.parse::<i32>().ok().filter(|y| (1930..=2029).contains(y)),
            _ => None,
        };
        if let Some(e) = decade.and_then(era_of_year) {
            return Some((e, true));
        }
    }
    if w.iter().any(|x| x.starts_with("generation")) {
        let gen_era = |x: &str| match x {
            "1st" | "first" => Some(1990),
            "2nd" | "second" => Some(2000),
            "3rd" | "third" => Some(2010),
            "4th" | "fourth" | "5th" | "fifth" => Some(2020),
            _ => None,
        };
        if let Some(e) = w.iter().find_map(|x| gen_era(x)) {
            return Some((e, false));
        }
    }
    None
}

/// Language words and country names as they appear in tags and genres.
const LANGS: [(&str, &str); 40] = [
    ("korean", "ko"),
    ("k-pop", "ko"),
    ("kpop", "ko"),
    ("k-indie", "ko"),
    ("k-rap", "ko"),
    ("k-hip hop", "ko"),
    ("k-r&b", "ko"),
    ("k-drama", "ko"),
    ("south korea", "ko"),
    ("korea", "ko"),
    ("english", "en"),
    ("united states", "en"),
    ("usa", "en"),
    ("american", "en"),
    ("united kingdom", "en"),
    ("uk", "en"),
    ("british", "en"),
    ("canada", "en"),
    ("australia", "en"),
    ("japanese", "ja"),
    ("japan", "ja"),
    ("j-pop", "ja"),
    ("j-rock", "ja"),
    ("mandarin", "zh"),
    ("chinese", "zh"),
    ("cantonese", "zh"),
    ("c-pop", "zh"),
    ("mandopop", "zh"),
    ("china", "zh"),
    ("taiwan", "zh"),
    ("portuguese", "pt"),
    ("brazil", "pt"),
    ("brazilian", "pt"),
    ("spanish", "es"),
    ("mexico", "es"),
    ("latin america", "es"),
    ("french", "fr"),
    ("france", "fr"),
    ("german", "de"),
    ("germany", "de"),
];

/// Words of a tag (lower case; "dance-pop" → "dance", "pop").
fn words(tag: &str) -> Vec<&str> {
    tag.split(|c: char| !(c.is_alphanumeric() || c == '&')).filter(|w| !w.is_empty()).collect()
}

/// The genre families one tag names. Instrument tags ("acoustic guitar", "synthesizer") name none.
fn families_of(tag: &str) -> u16 {
    let w = words(tag);
    let has = |x: &str| w.contains(&x);
    let has_seq = |a: &str, b: &str| w.windows(2).any(|p| p[0] == a && p[1] == b);
    if w.iter()
        .any(|x| matches!(*x, "guitar" | "piano" | "synthesizer" | "violin" | "saxophone" | "turntable"))
    {
        return 0;
    }
    let mut f = 0;
    if w.iter().any(|x| x.ends_with("pop") || matches!(*x, "ballad" | "balad" | "ost" | "idol" | "disco"))
        || has_seq("girl", "group")
        || has_seq("boy", "group")
    {
        f |= Families::POP;
    } else if has("dance") {
        // Pop unless the song turns out electronic (see `Scene::from_labels`).
        f |= Families::DANCE;
    }
    if has_seq("hip", "hop")
        || w.iter().any(|x| matches!(*x, "hiphop" | "rap" | "trap" | "drill" | "grime" | "rapper"))
        || has_seq("boom", "bap")
    {
        f |= Families::HIPHOP;
    }
    if w.iter().any(|x| matches!(*x, "r&b" | "rnb" | "soul" | "funk" | "gospel" | "motown")) {
        f |= Families::RNB;
    }
    if w.iter().any(|x| {
        matches!(*x, "rock" | "punk" | "metal" | "grunge" | "shoegaze" | "emo" | "hardcore" | "britpop")
            || x.ends_with("rock")
    }) {
        f |= Families::ROCK;
    }
    if w.iter().any(|x| {
        matches!(
            *x,
            "electronic"
                | "edm"
                | "house"
                | "techno"
                | "trance"
                | "dubstep"
                | "electro"
                | "synthwave"
                | "idm"
        )
    }) || has_seq("drum", "bass")
    {
        f |= Families::ELECTRONIC;
    }
    if w.iter().any(|x| matches!(*x, "jazz" | "swing" | "bebop" | "fusion")) {
        f |= Families::JAZZ;
    }
    // "singer-songwriter" is a genre, "female singer" is not.
    if has("folk") || has("americana") || has("songwriter") {
        f |= Families::FOLK;
    }
    if w.iter().any(|x| matches!(*x, "trot" | "enka")) {
        f |= Families::TROT;
    }
    if w.iter().any(|x| {
        matches!(
            *x,
            "samba"
                | "bossa"
                | "bossanova"
                | "latin"
                | "reggaeton"
                | "salsa"
                | "choro"
                | "flamenco"
                | "mpb"
                | "cumbia"
                | "tango"
                | "bachata"
        )
    }) {
        f |= Families::LATIN;
    }
    if w.iter().any(|x| matches!(*x, "classical" | "orchestral" | "opera" | "baroque" | "symphony")) {
        f |= Families::CLASSICAL;
    }
    if has("ambient") || has("lofi") || has("chillout") || has_seq("lo", "fi") || has_seq("new", "age") {
        f |= Families::AMBIENT;
    }
    if has("blues") {
        f |= Families::BLUES;
    }
    if has("country") && !has_seq("country", "of") {
        f |= Families::COUNTRY;
    }
    if w.iter().any(|x| matches!(*x, "reggae" | "dancehall" | "ska" | "dub")) {
        f |= Families::REGGAE;
    }
    f
}

/// The language a tag names (a language, a country, or a scene like "K-pop").
fn lang_of(tag: &str) -> Option<&'static str> {
    let t = tag.trim();
    if let Some((_, l)) = LANGS.iter().find(|(k, _)| *k == t) {
        return Some(l);
    }
    // "80s Korean rock", "5th-generation K-pop", "Korean hip hop": a language word inside a scene label.
    let w = words(t);
    LANGS
        .iter()
        .find(|(k, _)| {
            !k.contains(' ') && (w.contains(k) || (k.contains('-') && t.split_whitespace().any(|x| x == *k)))
        })
        .map(|(_, l)| *l)
}

impl Scene {
    /// Parses describe-it tags ("K-pop, dance-pop, 2020s, South Korea, Korean, girl group, …").
    pub fn from_tags(tags: &str) -> Scene {
        Self::from_labels(tags.split(',').map(str::trim), Source::Tags)
    }

    /// Reads a song's own genres (Navidrome, Jellyfin, local tags).
    pub fn from_genres(genres: &[String]) -> Scene {
        Self::from_labels(genres.iter().flat_map(|g| g.split([',', ';', '/'])).map(str::trim), Source::Genres)
    }

    fn from_labels<'a>(labels: impl Iterator<Item = &'a str>, source: Source) -> Scene {
        let mut s = Scene::default();
        let mut langs: Vec<&'static str> = Vec::new();
        let mut generation = None;
        for label in labels {
            let l = label.to_lowercase();
            if l.is_empty() || l == "unknown" {
                continue;
            }
            match era_of_tag(&l) {
                Some((e, true)) if s.era.is_none() => s.era = Some(e),
                Some((e, false)) if generation.is_none() => generation = Some(e),
                _ => {}
            }
            if l == "instrumental" {
                s.instrumental = true;
                continue;
            }
            if let Some(code) = lang_of(&l) {
                langs.push(code);
            }
            s.families.0 |= families_of(&l);
        }
        // A bare "dance" is pop, except on an electronic record ("dance, house, techno").
        if s.families.0 & Families::DANCE != 0 {
            s.families.0 &= !Families::DANCE;
            if s.families.0 & Families::ELECTRONIC == 0 {
                s.families.0 |= Families::POP;
            }
        }
        // The explicit language tag (or the first one named) wins; "K-pop" in English is still Korean pop.
        s.lang = langs.first().copied();
        s.era = s.era.or(generation);
        if s.lang.is_some() || s.instrumental || !s.families.is_empty() {
            s.source = source;
        }
        s
    }

    /// The language a title and artist are written in, when the script says it (Hangul, kana).
    pub fn from_script(title: &str, artist: &str) -> Scene {
        let text = format!("{title} {artist}");
        let hangul = text.chars().filter(|c| ('\u{ac00}'..='\u{d7a3}').contains(c)).count();
        let kana = text.chars().filter(|c| ('\u{3040}'..='\u{30ff}').contains(c)).count();
        let lang = if hangul >= 2 && hangul >= kana {
            Some("ko")
        } else if kana >= 2 {
            Some("ja")
        } else {
            None
        };
        Scene {
            lang,
            source: if lang.is_some() { Source::Script } else { Source::None },
            ..Default::default()
        }
    }

    /// Something is known.
    pub fn is_known(&self) -> bool {
        self.lang.is_some() || self.instrumental || !self.families.is_empty()
    }

    /// Fills what `self` doesn't know from a weaker `other`.
    pub fn or(self, other: Scene) -> Scene {
        if !self.is_known() {
            return Scene { era: self.era.or(other.era), ..other };
        }
        Scene {
            lang: self.lang.or(if self.instrumental { None } else { other.lang }),
            instrumental: self.instrumental,
            families: if self.families.is_empty() { other.families } else { self.families },
            source: self.source,
            era: self.era.or(other.era),
        }
    }

    /// "ko · pop, hip hop · 2020s (tags)" for logs.
    pub fn label(&self) -> String {
        if !self.is_known() {
            return "unknown".into();
        }
        let lang = if self.instrumental { "instrumental" } else { self.lang.unwrap_or("?") };
        let era = self.era.map(|e| format!(" · {}", era_label(e))).unwrap_or_default();
        format!("{lang} · {}{era} ({:?})", self.families.label(), self.source).to_lowercase()
    }
}

/// How two songs' scenes relate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fit {
    /// One scene; the score (0..1) says how close (more known and more shared is higher).
    Same(f32),
    /// Different scenes, and why ("language", "genre", "sound").
    Other(&'static str),
    /// Nothing comparable is known.
    Unknown,
}

impl Fit {
    pub fn is_same(self) -> bool {
        matches!(self, Fit::Same(_))
    }
}

/// Compares two scenes on what both know: the language (where both name one) and the genre
/// families. One dimension alone gives a weaker score. Eras known on both sides must be at most
/// [`ERA_SPAN`] apart; when only one side's era is known, language and genre must both match (else nothing
/// comparable is known) and the score is lower.
pub fn compare(a: &Scene, b: &Scene) -> Fit {
    let mut parts: Vec<f32> = Vec::new();
    // An instrumental song has no language of its own unless its tags name one (an instrumental version
    // of a Korean song is still Korean in scene).
    if let (Some(x), Some(y)) = (a.lang, b.lang) {
        if x != y {
            return Fit::Other("language");
        }
        parts.push(1.0);
    }
    if !a.families.is_empty() && !b.families.is_empty() {
        // Trot is its own scene: a trot song (even one also tagged pop) never sits next to a song that
        // isn't trot, and the other way round.
        let trot = |f: Families| f.0 & Families::TROT != 0;
        if trot(a.families) != trot(b.families) {
            return Fit::Other("genre");
        }
        let o = a.families.overlap(b.families);
        if o == 0.0 {
            return Fit::Other("genre");
        }
        parts.push(o);
    }
    if let (Some(x), Some(y)) = (a.era, b.era)
        && x.abs_diff(y) > ERA_SPAN
    {
        return Fit::Other("era");
    }
    let score = match parts.len() {
        0 => return Fit::Unknown,
        1 => 0.6 * parts[0],
        _ => parts.iter().sum::<f32>() / parts.len() as f32,
    };
    match (a.era, b.era) {
        // A decade apart is one scene, a little less close.
        (Some(x), Some(y)) => Fit::Same(if x == y { score } else { 0.9 * score }),
        (None, None) => Fit::Same(score),
        // One era unknown: only a full match (language and genre) places it.
        _ if parts.len() >= 2 => Fit::Same(ONE_ERA * score),
        _ => Fit::Unknown,
    }
}

/// A unit vector stored as bytes (a quarter of f32's memory; cosines stay within ≈0.01).
#[derive(Debug, Clone, PartialEq)]
pub struct Quant {
    v: Vec<i8>,
    scale: f32,
}

impl Quant {
    pub fn new(v: &[f32]) -> Option<Quant> {
        let max = v.iter().fold(0f32, |m, x| m.max(x.abs()));
        if !max.is_finite() || max == 0.0 {
            return None;
        }
        let q: Vec<i8> = v.iter().map(|x| (x / max * 127.0).round() as i8).collect();
        let norm = q.iter().map(|&x| f32::from(x).powi(2)).sum::<f32>().sqrt();
        Some(Quant { v: q, scale: 1.0 / norm })
    }

    pub fn cos(&self, other: &Quant) -> f32 {
        if self.v.len() != other.v.len() {
            return 0.0;
        }
        let dot: i32 = self.v.iter().zip(&other.v).map(|(&a, &b)| i32::from(a) * i32::from(b)).sum();
        dot as f32 * self.scale * other.scale
    }

    /// Base64 of the bytes (the wire format of [`SceneRecord::audio`]).
    pub fn encode(&self) -> String {
        let bytes: Vec<u8> = self.v.iter().map(|&x| x as u8).collect();
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    pub fn decode(b64: &str) -> Option<Quant> {
        let bytes = base64::engine::general_purpose::STANDARD.decode(b64.trim()).ok()?;
        let v: Vec<f32> = bytes.iter().map(|&b| f32::from(b as i8)).collect();
        Quant::new(&v)
    }
}

/// The scene facts of one song as listen-server hands them to the phone (`GET /v1/scenes`, one JSON object
/// per line): the describe-it record without its tag embedding, the audio embedding as bytes.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SceneRecord {
    pub song_id: String,
    #[serde(default)]
    pub title_hint: String,
    #[serde(default)]
    pub tags_en: String,
    /// [`Quant::encode`] of the CLaMP 3 audio embedding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<String>,
    /// When listen-embed wrote the song (Unix seconds; `GET /v1/scenes?since=` hands out newer ones).
    #[serde(default)]
    pub at: i64,
}

impl SceneRecord {
    pub fn of(r: &EmbedRecord) -> SceneRecord {
        SceneRecord {
            song_id: r.song_id.clone(),
            title_hint: r.title_hint.clone(),
            tags_en: r.tags_en.clone(),
            audio: r.audio.as_deref().and_then(decode_vec).and_then(|v| Quant::new(&v)).map(|q| q.encode()),
            at: r.at,
        }
    }
}

/// What the gate knows about one song.
#[derive(Debug, Clone, Default)]
pub struct Profile {
    pub scene: Scene,
    /// [`artist_key`] of the main artist ("" unknown).
    pub artist: String,
    pub audio: Option<Quant>,
}

impl Profile {
    pub fn is_known(&self) -> bool {
        self.scene.is_known() || self.audio.is_some()
    }
}

/// An artist name as a key: lower case, the first of several artists, without "feat." parts.
pub fn artist_key(name: &str) -> String {
    let l = name.to_lowercase();
    let mut s = l.as_str();
    for sep in [" feat.", " feat ", " ft.", " featuring ", ", ", " & ", " x ", " with "] {
        if let Some((a, _)) = s.split_once(sep) {
            s = a;
        }
    }
    s.trim().trim_end_matches(" - topic").trim().to_string()
}

/// Artists heard right after one another in the listening history (both orders).
#[derive(Debug, Clone, Default)]
pub struct Neighbours(HashSet<(String, String)>);

impl Neighbours {
    /// Consecutive plays at most [`NEIGHBOUR_GAP_S`] apart, by different artists.
    pub fn from_history(entries: &[Play]) -> Neighbours {
        let mut sorted: Vec<&Play> = entries.iter().collect();
        sorted.sort_by_key(|e| e.ts);
        let mut set = HashSet::new();
        for p in sorted.windows(2) {
            if p[1].ts - p[0].ts > NEIGHBOUR_GAP_S {
                continue;
            }
            let (Some(a), Some(b)) = (p[0].artists.first(), p[1].artists.first()) else { continue };
            let (a, b) = (artist_key(&a.name), artist_key(&b.name));
            if a.is_empty() || b.is_empty() || a == b {
                continue;
            }
            set.insert((a.clone(), b.clone()));
            set.insert((b, a));
        }
        Neighbours(set)
    }

    pub fn contains(&self, a: &str, b: &str) -> bool {
        self.0.contains(&(a.to_string(), b.to_string()))
    }

    pub fn len(&self) -> usize {
        self.0.len() / 2
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// The scenes of the describe-it index: per song (analysis-store song id) and per artist.
#[derive(Debug, Clone, Default)]
pub struct SceneIndex {
    songs: HashMap<String, (Scene, Option<Quant>, String)>,
    artists: HashMap<String, Scene>,
    /// Eras of songs without one of their own (nor their artist's), from the songs that sound closest.
    audio_eras: HashMap<String, u16>,
}

/// The era most of `eras` agree on: the median when at least 60 % are within a decade of it.
fn common_era(eras: &[u16]) -> Option<u16> {
    if eras.is_empty() {
        return None;
    }
    let mut v = eras.to_vec();
    v.sort_unstable();
    let mid = v[(v.len() - 1) / 2];
    let near = v.iter().filter(|e| e.abs_diff(mid) <= ERA_SPAN).count();
    (near as f32 >= 0.6 * v.len() as f32).then_some(mid)
}

impl SceneIndex {
    /// Builds the index; a later record for a song replaces an earlier one.
    pub fn from_records(records: impl IntoIterator<Item = SceneRecord>) -> SceneIndex {
        let mut songs = HashMap::new();
        for r in records {
            if r.song_id.is_empty() {
                continue;
            }
            let scene = Scene::from_tags(&r.tags_en);
            let audio = r.audio.as_deref().and_then(Quant::decode);
            let (_, artist) = split_hint(&r.title_hint);
            if !scene.is_known() && audio.is_none() {
                continue;
            }
            songs.insert(r.song_id, (scene, audio, artist_key(&artist)));
        }
        // Per artist: the language most of their tagged songs have (≥ 60 %), and the families at least a
        // third of them have.
        let mut by_artist: HashMap<&str, Vec<&Scene>> = HashMap::new();
        for (scene, _, artist) in songs.values() {
            if !artist.is_empty() && scene.is_known() {
                by_artist.entry(artist.as_str()).or_default().push(scene);
            }
        }
        let mut artists = HashMap::new();
        for (artist, scenes) in by_artist {
            let n = scenes.len() as f32;
            let mut langs: HashMap<&str, usize> = HashMap::new();
            for s in &scenes {
                if let Some(l) = s.lang {
                    *langs.entry(l).or_default() += 1;
                }
            }
            let lang = langs
                .into_iter()
                .max_by_key(|(l, c)| (*c, std::cmp::Reverse(*l)))
                .filter(|(_, c)| *c as f32 >= 0.6 * n)
                .map(|(l, _)| l);
            let mut families = 0u16;
            for bit in 0..16 {
                let have = scenes.iter().filter(|s| s.families.0 & (1 << bit) != 0).count();
                if have as f32 >= n / 3.0 {
                    families |= 1 << bit;
                }
            }
            let instrumental = scenes.iter().all(|s| s.instrumental);
            let eras: Vec<u16> = scenes.iter().filter_map(|s| s.era).collect();
            let era = common_era(&eras);
            let scene =
                Scene { lang, instrumental, families: Families(families), source: Source::Artist, era };
            if scene.is_known() {
                artists.insert(artist.to_string(), scene);
            }
        }
        // Songs that still have no era: the era of the (up to 5) songs that sound closest, when they agree.
        let dated: Vec<(&Quant, u16)> =
            songs.values().filter_map(|(s, q, _)| Some((q.as_ref()?, s.era?))).collect();
        let mut audio_eras = HashMap::new();
        for (id, (scene, audio, artist)) in &songs {
            let Some(q) = audio else { continue };
            if scene.era.is_some() || artists.get(artist).is_some_and(|a| a.era.is_some()) {
                continue;
            }
            let mut near: Vec<(f32, u16)> =
                dated.iter().map(|(d, e)| (q.cos(d), *e)).filter(|(c, _)| *c >= AUDIO_SAME).collect();
            near.sort_by(|a, b| b.0.total_cmp(&a.0));
            near.truncate(5);
            if near.len() >= 3
                && let Some(e) = common_era(&near.iter().map(|n| n.1).collect::<Vec<_>>())
            {
                audio_eras.insert(id.clone(), e);
            }
        }
        SceneIndex { songs, artists, audio_eras }
    }

    /// Reads the describe-it index folder (`…/embeddings/v1/clamp3`, every `*.jsonl` in name order).
    pub fn load_embeddings(dir: &Path) -> SceneIndex {
        Self::from_records(read_scene_records(dir))
    }

    /// Reads [`SceneRecord`] lines (the phone's copy of `GET /v1/scenes`).
    pub fn load_scene_records(text: &str) -> SceneIndex {
        Self::from_records(text.lines().filter_map(|l| serde_json::from_str::<SceneRecord>(l).ok()))
    }

    /// [`SceneRecord`] lines with one line per song (the last one), for compacting the phone's copy.
    pub fn dedup_lines(text: &str) -> String {
        let mut last: HashMap<String, usize> = HashMap::new();
        let lines: Vec<&str> = text.lines().collect();
        for (i, l) in lines.iter().enumerate() {
            if let Ok(r) = serde_json::from_str::<SceneRecord>(l) {
                last.insert(r.song_id, i);
            }
        }
        let keep: HashSet<usize> = last.into_values().collect();
        let mut out = String::new();
        for (i, l) in lines.iter().enumerate() {
            if keep.contains(&i) {
                out.push_str(l);
                out.push('\n');
            }
        }
        out
    }

    /// Songs with a scene or an audio embedding.
    pub fn len(&self) -> usize {
        self.songs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.songs.is_empty()
    }

    /// Songs whose tags say their scene.
    pub fn tagged(&self) -> usize {
        self.songs.values().filter(|(s, _, _)| s.is_known()).count()
    }

    /// The scene of a song: its own tags, else its genres, else its artist's, else its script.
    /// `song_id` is the analysis store's id; `artist` and `title` as shown.
    pub fn profile(&self, song_id: &str, title: &str, artist: &str, genres: &[String]) -> Profile {
        self.profile_dated(song_id, title, artist, genres, None)
    }

    /// [`Self::profile`] with the song's release `year` where the source has one: the era comes from the
    /// tags, else the year, else the artist's other songs, else the songs that sound closest.
    pub fn profile_dated(
        &self,
        song_id: &str,
        title: &str,
        artist: &str,
        genres: &[String],
        year: Option<i32>,
    ) -> Profile {
        let own = self.songs.get(song_id);
        let mut key = artist_key(artist);
        if key.is_empty() {
            key = own.map(|o| o.2.clone()).unwrap_or_default();
        }
        let tags = own.map(|o| o.0).unwrap_or_default();
        let by_artist = self.artists.get(&key).copied().unwrap_or_default();
        let mut scene =
            tags.or(Scene::from_genres(genres)).or(by_artist).or(Scene::from_script(title, artist));
        scene.era = tags
            .era
            .or(year.and_then(era_of_year))
            .or(by_artist.era)
            .or_else(|| self.audio_eras.get(song_id).copied());
        Profile { scene, artist: key, audio: own.and_then(|o| o.1.clone()) }
    }
}

/// The describe-it index folder as [`SceneRecord`]s, one per song (the last line for a song wins), in file
/// order (listen-server's `GET /v1/scenes`).
pub fn read_scene_records(dir: &Path) -> Vec<SceneRecord> {
    let mut by_id: HashMap<String, usize> = HashMap::new();
    let mut out: Vec<SceneRecord> = Vec::new();
    for r in read_jsonl::<EmbedRecord>(dir) {
        if r.song_id.is_empty() {
            continue;
        }
        let rec = SceneRecord::of(&r);
        match by_id.get(&r.song_id) {
            Some(&i) => out[i] = rec,
            None => {
                by_id.insert(r.song_id.clone(), out.len());
                out.push(rec);
            }
        }
    }
    out
}

fn read_jsonl<T: serde::de::DeserializeOwned>(dir: &Path) -> Vec<T> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
        .filter(|p| !p.to_string_lossy().contains("conflicted copy"))
        .collect();
    files.sort();
    files
        .iter()
        .flat_map(|p| {
            std::fs::read_to_string(p)
                .unwrap_or_default()
                .lines()
                .filter_map(|l| serde_json::from_str::<T>(l).ok())
                .collect::<Vec<_>>()
        })
        .collect()
}

/// How a candidate relates to one song: the artist first (the same artist is one scene within a decade),
/// then the tags, then the sound, then the listening history.
pub fn fit(a: &Profile, b: &Profile, neighbours: &Neighbours) -> Fit {
    if !a.artist.is_empty() && a.artist == b.artist {
        // The era is part of the scene for the same artist too: their 1980s ballad and 2020s single are
        // two scenes, so a session or a bridge never mixes an artist's decades.
        return match (a.scene.era, b.scene.era) {
            (Some(x), Some(y)) if x.abs_diff(y) > ERA_SPAN => Fit::Other("era"),
            _ => Fit::Same(1.0),
        };
    }
    let cos = match (&a.audio, &b.audio) {
        (Some(x), Some(y)) => Some(x.cos(y)),
        _ => None,
    };
    let by_tags = compare(&a.scene, &b.scene);
    let f = match (by_tags, cos) {
        // Sound refines a match: a close one scores a little higher.
        (Fit::Same(s), Some(c)) => Fit::Same(0.75 * s + 0.25 * ((c - 0.4) / 0.4).clamp(0.0, 1.0)),
        (Fit::Unknown, Some(c)) if c >= AUDIO_SAME => Fit::Same(0.5),
        (Fit::Unknown, Some(_)) => Fit::Other("sound"),
        (f, _) => f,
    };
    // History neighbours fill an unknown, never a known other scene, and never across eras: when one
    // song's era is known the other's must be too.
    if !f.is_same()
        && f != Fit::Other("language")
        && f != Fit::Other("genre")
        && f != Fit::Other("era")
        && a.scene.era.is_some() == b.scene.era.is_some()
        && !a.artist.is_empty()
        && !b.artist.is_empty()
        && neighbours.contains(&a.artist, &b.artist)
    {
        return Fit::Same(0.5);
    }
    f
}

/// Why a candidate may not bridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reject {
    /// Its scene is known and is neither song's.
    OtherScene,
    /// Language and genre fit, but it is from another era (more than a decade apart).
    OtherEra,
    /// Its scene is unknown while the songs' are known.
    UnknownScene,
    /// The songs around it are sung in a known language and it has none (an instrumental record or an
    /// unknown language), and it isn't one the listener played in a family of theirs beyond pop/electronic.
    NoLanguage,
    /// A record the listener never played whose scene fits only loosely ([`MIN_UNHEARD_BRIDGE`]).
    Weak,
}

/// A bridge the listener never played must fit the pair's scene at least this well (0..1, `Gate::judge`):
/// two tagged songs of one language, genre and decade score 0.75 or more, a loose match (one shared
/// dimension: 0.6, half a genre: 0.75 × …) less.
pub const MIN_UNHEARD_BRIDGE: f32 = 0.75;

/// The languages songs are sung in (`None`: none known; an instrumental song has none).
fn sung(p: &Profile) -> Option<&'static str> {
    if p.scene.instrumental { None } else { p.scene.lang }
}

/// The vocal rule between songs sung in known languages (`langs`, the songs' `families`): a candidate must
/// be sung in one of those languages (a bridge plays the instrumental stretch of a sung song anyway); a
/// record tagged instrumental (even with a country, like a Korean game soundtrack) or of unknown language
/// only when the listener played it (`heard`) and it shares a family with every song beyond the generic
/// pop/electronic. The same artist as one of the songs always passes.
fn vocal_rule(c: &Profile, langs: &[&'static str], families: &[Families], heard: bool) -> Result<(), Reject> {
    if langs.is_empty() {
        return Ok(());
    }
    match sung(c) {
        Some(l) if langs.contains(&l) => Ok(()),
        Some(_) => Err(Reject::OtherScene),
        None => {
            let own = c.scene.families.0 & !Families::GENERIC;
            let shared = own != 0 && families.iter().all(|f| f.0 & own != 0);
            if heard && shared { Ok(()) } else { Err(Reject::NoLanguage) }
        }
    }
}

/// Why a candidate that fits none of `fits` is rejected: another era when that is the only known
/// difference, another scene for any other known difference, else unknown.
fn reject_of(fits: &[Fit]) -> Reject {
    let others: Vec<&str> =
        fits.iter().filter_map(|f| if let Fit::Other(w) = f { Some(*w) } else { None }).collect();
    if others.is_empty() {
        Reject::UnknownScene
    } else if others.iter().all(|w| *w == "era") {
        Reject::OtherEra
    } else {
        Reject::OtherScene
    }
}

/// The scene rule for one pair: built from the outgoing and incoming songs, asked about each candidate.
#[derive(Debug, Clone)]
pub struct Gate<'a> {
    out: Profile,
    inn: Profile,
    pair: Fit,
    neighbours: &'a Neighbours,
}

impl<'a> Gate<'a> {
    pub fn new(out: Profile, inn: Profile, neighbours: &'a Neighbours) -> Gate<'a> {
        let pair = fit(&out, &inn, neighbours);
        Gate { out, inn, pair, neighbours }
    }

    /// Whether the gate knows anything (else every candidate passes, as before scenes).
    pub fn active(&self) -> bool {
        self.out.is_known() || self.inn.is_known()
    }

    /// "ko · pop (tags) → ko · pop (tags), same scene" for logs.
    pub fn describe(&self) -> String {
        let rel = match self.pair {
            Fit::Same(_) => "one scene".to_string(),
            Fit::Other(why) => format!("different scenes ({why})"),
            Fit::Unknown => "scenes not comparable".to_string(),
        };
        format!("{} → {}, {rel}", self.out.scene.label(), self.inn.scene.label())
    }

    /// A bridge candidate's scene score with the bridge rules on top of [`Self::judge`]: between two songs
    /// sung in known languages it must be sung in one of them (an instrumental record only when the
    /// listener played it, `heard`, and it shares a family beyond pop/electronic with both songs), and a
    /// record the listener never played must score at least [`MIN_UNHEARD_BRIDGE`]. The same artist as one
    /// of the songs passes when it is from that song's era (within a decade).
    pub fn judge_bridge(&self, c: &Profile, heard: bool) -> Result<f32, Reject> {
        let score = self.judge(c)?;
        if !self.active() {
            return Ok(score);
        }
        // `fit` puts an artist's song from another decade in another scene, so this exemption never
        // carries an artist's other era across.
        let same_artist = !c.artist.is_empty()
            && [&self.out, &self.inn]
                .iter()
                .any(|s| c.artist == s.artist && fit(c, s, self.neighbours).is_same());
        if same_artist {
            return Ok(score);
        }
        if let (Some(a), Some(b)) = (sung(&self.out), sung(&self.inn)) {
            let langs = if a == b { vec![a] } else { vec![a, b] };
            vocal_rule(c, &langs, &[self.out.scene.families, self.inn.scene.families], heard)?;
        }
        if !heard && score < MIN_UNHEARD_BRIDGE {
            return Err(Reject::Weak);
        }
        Ok(score)
    }

    /// The candidate's scene score (0..1, higher is closer) or why it is rejected.
    pub fn judge(&self, c: &Profile) -> Result<f32, Reject> {
        if !self.active() {
            return Ok(0.0);
        }
        let (fo, fi) = (fit(c, &self.out, self.neighbours), fit(c, &self.inn, self.neighbours));
        let score = |f: Fit| if let Fit::Same(s) = f { s } else { 0.0 };
        let reject = || reject_of(&[fo, fi]);
        match self.pair {
            // One scene: the bridge must be in it.
            Fit::Same(_) => {
                if fo.is_same() && fi.is_same() {
                    Ok((score(fo) + score(fi)) / 2.0)
                } else {
                    Err(reject())
                }
            }
            // Two scenes: the bridge belongs to one of them.
            Fit::Other(_) => {
                if fo.is_same() || fi.is_same() {
                    Ok(score(fo).max(score(fi)))
                } else {
                    Err(reject())
                }
            }
            // One side unknown (or nothing comparable): match every known side.
            Fit::Unknown => {
                let sides = [(self.out.is_known(), fo), (self.inn.is_known(), fi)];
                let known: Vec<Fit> = sides.iter().filter(|(k, _)| *k).map(|(_, f)| *f).collect();
                if known.iter().any(|f| matches!(f, Fit::Other(_))) {
                    Err(reject_of(&known))
                } else if known.iter().any(|f| f.is_same()) {
                    Ok(known.iter().map(|&f| score(f)).sum::<f32>() / known.len() as f32)
                } else {
                    Err(Reject::UnknownScene)
                }
            }
        }
    }
}

/// Counts of a gate's decisions over the candidates, for the log line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tally {
    pub candidates: usize,
    pub other_scene: usize,
    pub unknown_scene: usize,
    pub other_era: usize,
    /// Rejected by the vocal rule ([`Reject::NoLanguage`]).
    pub no_language: usize,
    /// Never-heard records that fit only loosely ([`Reject::Weak`]).
    pub weak: usize,
}

impl Tally {
    pub fn rejected(&self) -> usize {
        self.other_scene + self.unknown_scene + self.other_era + self.no_language + self.weak
    }

    /// Counts one decision.
    pub fn count(&mut self, r: Reject) {
        match r {
            Reject::OtherScene => self.other_scene += 1,
            Reject::OtherEra => self.other_era += 1,
            Reject::UnknownScene => self.unknown_scene += 1,
            Reject::NoLanguage => self.no_language += 1,
            Reject::Weak => self.weak += 1,
        }
    }

    /// "30 other scene, 2 other era, 3 unknown scene" (empty when none was rejected).
    pub fn reasons(&self) -> String {
        let mut parts = Vec::new();
        if self.other_scene > 0 {
            parts.push(format!("{} other scene", self.other_scene));
        }
        if self.other_era > 0 {
            parts.push(format!("{} other era", self.other_era));
        }
        if self.unknown_scene > 0 {
            parts.push(format!("{} unknown scene", self.unknown_scene));
        }
        if self.no_language > 0 {
            parts.push(format!("{} without the language", self.no_language));
        }
        if self.weak > 0 {
            parts.push(format!("{} unheard and too loose", self.weak));
        }
        parts.join(", ")
    }

    /// "41 candidates, 33 rejected (30 other scene, 3 unknown scene)".
    pub fn describe(&self) -> String {
        if self.rejected() == 0 {
            return format!("{} candidates, none rejected by scene", self.candidates);
        }
        format!("{} candidates, {} rejected ({})", self.candidates, self.rejected(), self.reasons())
    }
}

/// The scene of what is playing, for the songs the heart-rate tempo mode adds from outside the DJ's
/// pool: read from the current song and the songs around it (at most [`SESSION_LEN`] with a known
/// scene, the current song first). When most of them share one scene, a candidate must belong to it (its
/// language and genre, its era within a decade of theirs); a mixed session takes a candidate from any of
/// its scenes. A candidate of unknown scene is rejected; when nothing is known there is no gate.
#[derive(Debug, Clone)]
pub struct Session<'a> {
    members: Vec<Profile>,
    /// The majority's scene (`None`: mixed).
    anchor: Option<Profile>,
    /// Indices of the members in the majority.
    cluster: Vec<usize>,
    /// The artists of every song given, known scene or not.
    artists: HashSet<String>,
    neighbours: &'a Neighbours,
}

impl<'a> Session<'a> {
    /// `profiles`: the current song first, then the songs before and after it; unknown ones are skipped.
    pub fn new(profiles: impl IntoIterator<Item = Profile>, neighbours: &'a Neighbours) -> Session<'a> {
        let mut artists = HashSet::new();
        let mut members: Vec<Profile> = Vec::new();
        for p in profiles {
            if members.len() >= SESSION_LEN {
                break;
            }
            if !p.artist.is_empty() {
                artists.insert(p.artist.clone());
            }
            if p.is_known() {
                members.push(p);
            }
        }
        let n = members.len();
        let same: Vec<Vec<usize>> = (0..n)
            .map(|i| {
                (0..n).filter(|&j| i == j || fit(&members[i], &members[j], neighbours).is_same()).collect()
            })
            .collect();
        // The member most others share a scene with (the current song on a tie).
        let best = (0..n).max_by_key(|&i| (same[i].len(), std::cmp::Reverse(i)));
        let (anchor, cluster) = match best {
            Some(i) if same[i].len() * 2 > n => {
                let cluster = same[i].clone();
                (Some(merge(cluster.iter().map(|&j| &members[j]))), cluster)
            }
            _ => (None, (0..n).collect()),
        };
        Session { members, anchor, cluster, artists, neighbours }
    }

    /// A session led by what plays now (the AI DJ's pool, the Auto-DJ's fill, the tempo picks): when the
    /// current song's (the first profile's) scene is known it decides, and the songs before it only count
    /// where they share that scene, so a song the listener just picked from another decade isn't outvoted
    /// by the ones before it. When the current song's scene is unknown, the others decide as in
    /// [`Self::new`] (its artist still belongs).
    pub fn led(profiles: impl IntoIterator<Item = Profile>, neighbours: &'a Neighbours) -> Session<'a> {
        let mut it = profiles.into_iter();
        let Some(current) = it.next() else { return Session::new([], neighbours) };
        if !current.is_known() {
            return Session::new(std::iter::once(current).chain(it), neighbours);
        }
        let rest: Vec<Profile> =
            it.filter(|p| p.is_known() && fit(&current, p, neighbours).is_same()).collect();
        Session::new(std::iter::once(current).chain(rest), neighbours)
    }

    /// Whether anything is known (else every candidate passes, as before scenes).
    pub fn active(&self) -> bool {
        !self.members.is_empty()
    }

    /// The majority's scene (language, families, era), `None` when the session is mixed or unknown.
    pub fn scene(&self) -> Option<&Scene> {
        self.anchor.as_ref().map(|a| &a.scene)
    }

    /// [`Self::judge`] plus the vocal rule: when the session's songs are sung in a known language, a
    /// candidate must be too (an instrumental record only when the listener played it, `heard`, and it
    /// shares a family beyond pop/electronic with the session). The session's artists always pass.
    pub fn judge_with(&self, c: &Profile, heard: bool) -> Result<f32, Reject> {
        let score = self.judge(c)?;
        if !self.active() || (!c.artist.is_empty() && self.artists.contains(&c.artist)) {
            return Ok(score);
        }
        let (langs, families): (Vec<&'static str>, Vec<Families>) = match &self.anchor {
            Some(a) => (sung(a).into_iter().collect(), vec![a.scene.families]),
            None => {
                let mut langs: Vec<&'static str> = Vec::new();
                for m in &self.members {
                    match sung(m) {
                        Some(l) if !langs.contains(&l) => langs.push(l),
                        Some(_) => {}
                        // A mixed session with an instrumental or unknown-language member: no vocal rule.
                        None => return Ok(score),
                    }
                }
                // Any one member's families will do in a mixed session.
                let any = Families(self.members.iter().fold(0, |f, m| f | m.scene.families.0));
                (langs, vec![any])
            }
        };
        vocal_rule(c, &langs, &families, heard)?;
        Ok(score)
    }

    /// "ko · pop · 2020s (tags), 4 of 5 songs" or "mixed: ko · pop · 2020s (tags) / en · hip hop (tags)".
    pub fn describe(&self) -> String {
        match &self.anchor {
            _ if !self.active() => "session scene unknown".into(),
            Some(a) => format!(
                "session scene {}, {} of {} songs",
                a.scene.label(),
                self.cluster.len(),
                self.members.len()
            ),
            None => {
                let mut labels: Vec<String> = Vec::new();
                for m in &self.members {
                    let l = m.scene.label();
                    if !labels.contains(&l) {
                        labels.push(l);
                    }
                }
                format!("session scene mixed: {}", labels.join(" / "))
            }
        }
    }

    /// The candidate's scene score (0..1, higher is closer) or why it is rejected.
    pub fn judge(&self, c: &Profile) -> Result<f32, Reject> {
        if !self.active() {
            return Ok(0.0);
        }
        let score = |f: Fit| if let Fit::Same(s) = f { s } else { 0.0 };
        let in_cluster: Vec<&Profile> = self.cluster.iter().map(|&i| &self.members[i]).collect();
        // The same artist as a song of the session (the current one too, scene known or not) belongs,
        // unless the song is from another era than the session's (an artist's later decade).
        if !c.artist.is_empty() && self.artists.contains(&c.artist) {
            let era = self.anchor.as_ref().and_then(|a| a.scene.era);
            if let (Some(x), Some(y)) = (era, c.scene.era)
                && x.abs_diff(y) > ERA_SPAN
            {
                return Err(Reject::OtherEra);
            }
            return Ok(1.0);
        }
        let fits: Vec<Fit> = in_cluster.iter().map(|m| fit(c, m, self.neighbours)).collect();
        match &self.anchor {
            Some(anchor) => {
                // The majority's scene decides; where tags can't compare, most of its songs must fit (by
                // sound or the history) and none may be a known other scene.
                match compare(&c.scene, &anchor.scene) {
                    Fit::Same(s) => match fits.iter().find(|f| **f == Fit::Other("era")) {
                        // Within a decade of the session's middle, but far from one of its songs: lower.
                        Some(_) => Ok(0.8 * s),
                        None => Ok(s),
                    },
                    Fit::Other(why) => Err(reject_of(&[Fit::Other(why)])),
                    Fit::Unknown => {
                        let same = fits.iter().filter(|f| f.is_same()).count();
                        if fits.iter().any(|f| matches!(f, Fit::Other(_))) {
                            Err(reject_of(&fits))
                        } else if same * 2 > fits.len() {
                            Ok(0.8 * fits.iter().map(|&f| score(f)).sum::<f32>() / same as f32)
                        } else {
                            Err(Reject::UnknownScene)
                        }
                    }
                }
            }
            // Mixed: any of its scenes.
            None => match fits.iter().copied().filter(|f| f.is_same()).map(score).reduce(f32::max) {
                Some(s) => Ok(s),
                None => Err(reject_of(&fits)),
            },
        }
    }
}

/// A song's scene score (0..1) or why it doesn't belong ([`SceneFilter::judge`]).
pub type SongJudge<'a> = Box<dyn Fn(&crate::model::Song) -> Result<f32, Reject> + 'a>;

/// The scene rule a tempo fill applies to the songs it adds from outside the DJ's pool
/// (`listen_mix::tempo::tempo_fill`; built by `listen_analysis::tempo_scene_filter`).
pub struct SceneFilter<'a> {
    /// The song's scene score (0..1) or why it doesn't belong.
    pub judge: SongJudge<'a>,
    /// The listener played or liked it (such songs come first).
    pub heard: Box<dyn Fn(&crate::model::Song) -> bool + 'a>,
    /// Whether the session's scene is known (else `judge` passes everything).
    pub active: bool,
    /// [`Session::describe`], for the log.
    pub session: String,
    /// [`Session::scene`]: the session's scene when one leads (for the AI DJ's prompt).
    pub scene: Option<Scene>,
    /// A song's scene (for the AI DJ's notes on its candidates).
    pub scene_of: Box<dyn Fn(&crate::model::Song) -> Scene + 'a>,
}

impl std::fmt::Debug for SceneFilter<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SceneFilter").field("active", &self.active).field("session", &self.session).finish()
    }
}

/// One profile for a group sharing a scene: the language most have, the families at least half have (the
/// most common one when none reach half), the era most have (on a tie the first song's, i.e. the current
/// one); no artist or sound of its own.
fn merge<'p>(members: impl Iterator<Item = &'p Profile>) -> Profile {
    let members: Vec<&Profile> = members.collect();
    let scenes: Vec<&Scene> = members.iter().map(|m| &m.scene).filter(|s| s.is_known()).collect();
    let mut langs: HashMap<&str, usize> = HashMap::new();
    for s in &scenes {
        if let (Some(l), false) = (s.lang, s.instrumental) {
            *langs.entry(l).or_default() += 1;
        }
    }
    let lang = langs.into_iter().max_by_key(|(l, c)| (*c, std::cmp::Reverse(*l))).map(|(l, _)| l);
    let with_families: Vec<&&Scene> = scenes.iter().filter(|s| !s.families.is_empty()).collect();
    let counts: Vec<usize> =
        (0..16).map(|bit| with_families.iter().filter(|s| s.families.0 & (1 << bit) != 0).count()).collect();
    let mut families = 0u16;
    for (bit, &c) in counts.iter().enumerate() {
        if c * 2 >= with_families.len() && c > 0 {
            families |= 1 << bit;
        }
    }
    if families == 0
        && let Some((bit, _)) = counts.iter().enumerate().filter(|(_, c)| **c > 0).max_by_key(|(_, c)| **c)
    {
        families = 1 << bit;
    }
    let eras: Vec<u16> = scenes.iter().filter_map(|s| s.era).collect();
    let count = |e: u16| eras.iter().filter(|x| **x == e).count();
    // `max_by_key` keeps the last of equals: walk backwards so the first song wins a tie.
    let era = eras.iter().rev().copied().max_by_key(|&e| count(e));
    let scene = Scene {
        lang,
        instrumental: !scenes.is_empty() && scenes.iter().all(|s| s.instrumental),
        families: Families(families),
        source: scenes.iter().map(|s| s.source).max().unwrap_or_default(),
        era,
    };
    Profile { scene, artist: String::new(), audio: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(t: &str) -> Profile {
        Profile { scene: Scene::from_tags(t), ..Default::default() }
    }

    const KPOP: &str = "K-pop, dance-pop, 2020s, South Korea, Korean, girl group, synthesizer, energetic";
    const KPOP2: &str = "k-pop, ballad, 2010s, south korea, korean, female solo, piano, emotional, OST";
    const USRAP: &str = "Hip hop, boom bap, 1990s, United States, English, male solo, turntable, gritty";
    const SAMBA: &str = "samba, bossa nova, 1960s, brazil, portuguese, male solo, acoustic guitar";

    #[test]
    fn tags_give_language_and_families() {
        let s = Scene::from_tags(KPOP);
        assert_eq!(s.lang, Some("ko"));
        assert_eq!(s.families.0, Families::POP, "{}", s.families.label());
        let r = Scene::from_tags(USRAP);
        assert_eq!((r.lang, r.families.0), (Some("en"), Families::HIPHOP));
        let k = Scene::from_tags("Hip hop, Korean hip hop, 2010s, South Korea, Korean, male solo");
        assert_eq!((k.lang, k.families.0), (Some("ko"), Families::HIPHOP));
        let t = Scene::from_tags(
            "Trot, medley, 2020s, South Korea, Korean, female solo, traditional instruments",
        );
        assert_eq!(t.families.0, Families::TROT);
        let i = Scene::from_tags("lo-fi hip hop, chillhop, 2020s, instrumental, piano, calm");
        assert!(
            i.instrumental && i.families.0 & Families::HIPHOP != 0 && i.families.0 & Families::AMBIENT != 0
        );
        assert_eq!(Scene::from_tags(SAMBA).families.0, Families::LATIN);
        assert!(!Scene::from_tags("unknown, unknown").is_known());
    }

    #[test]
    fn genres_and_script_are_weaker_signals() {
        let g = Scene::from_genres(&["K-Pop".into(), "Dance".into()]);
        assert_eq!((g.lang, g.families.0, g.source), (Some("ko"), Families::POP, Source::Genres));
        assert_eq!(Scene::from_script("너만을 느끼며", "The Blue").lang, Some("ko"));
        assert_eq!(Scene::from_script("Magnetic", "ILLIT").lang, None);
    }

    #[test]
    fn kpop_pairs_are_one_scene_and_us_rap_is_another() {
        let n = Neighbours::default();
        assert!(fit(&tags(KPOP), &tags(KPOP2), &n).is_same());
        assert_eq!(fit(&tags(KPOP), &tags(USRAP), &n), Fit::Other("language"));
        let ko_rap = tags("hip hop, 2010s, south korea, korean, male solo");
        assert_eq!(fit(&tags(KPOP2), &ko_rap, &n), Fit::Other("genre"));
    }

    #[test]
    fn same_artist_is_one_scene_and_history_neighbours_fill_unknowns() {
        let mut a = tags(KPOP);
        a.artist = "bts".into();
        let mut b = tags("pop, disco, 2020s, english, boy group");
        b.artist = "bts".into();
        assert!(fit(&a, &b, &Neighbours::default()).is_same());
        // (Same era: a neighbour never bridges eras, see `history_neighbours_never_bridge_eras`.)
        let mut c = Profile { artist: "someone".into(), ..Default::default() };
        c.scene.era = Some(2020);
        let n =
            Neighbours(HashSet::from([("someone".into(), "bts".into()), ("bts".into(), "someone".into())]));
        assert_eq!(fit(&c, &a, &Neighbours::default()), Fit::Unknown);
        assert!(fit(&c, &a, &n).is_same());
        // A neighbour doesn't override a known other scene.
        c.scene = Scene::from_tags(USRAP);
        assert!(!fit(&c, &tags(KPOP), &n).is_same());
    }

    #[test]
    fn the_current_song_leads_the_session() {
        let n = Neighbours::default();
        // The listener picked a 1980s ballad after four 2020s idol songs.
        let ballad = tags(KPOP80);
        let idols: Vec<Profile> = (0..4).map(|_| tags(KPOP)).collect();
        let led = Session::led(std::iter::once(ballad.clone()).chain(idols.clone()), &n);
        assert_eq!(led.scene().and_then(|s| s.era), Some(1980), "{}", led.describe());
        assert!(led.judge(&tags(KPOP)).is_err(), "no 2020s idol song after the ballad");
        assert!(led.judge(&tags(KPOP90)).is_ok(), "1990s Korean pop is within a decade");
        // The old majority rule let the four idol songs outvote it.
        let majority = Session::new(std::iter::once(ballad).chain(idols), &n);
        assert_eq!(majority.scene().and_then(|s| s.era), Some(2020));
        // Unknown current song: the songs before it decide.
        let led = Session::led([Profile::default(), tags(KPOP), tags(KPOP)], &n);
        assert_eq!(led.scene().and_then(|s| s.era), Some(2020));
    }

    #[test]
    fn korean_rnb_house_and_dance_records_are_not_kpop() {
        let n = Neighbours::default();
        let krnb = tags("R&B, soul, 2020s, South Korea, Korean, female solo");
        assert_eq!(fit(&krnb, &tags(KPOP), &n), Fit::Other("genre"), "pop is near no other family");
        let house = Scene::from_tags("tech house, dance, electronic, 2020s, club");
        assert_eq!(
            house.families.0,
            Families::ELECTRONIC,
            "a bare \"dance\" on an electronic record isn't pop"
        );
        assert_eq!(Scene::from_tags("dance, 1990s, korean").families.0, Families::POP);
        // The vocal rule: a session sung in Korean takes no language-less record, unless played and sharing
        // a family beyond pop/electronic.
        let session = Session::led([tags(KPOP)], &n);
        let inst = tags("synthwave, electronic, pop, 2020s, instrumental");
        assert_eq!(session.judge_with(&inst, true), Err(Reject::NoLanguage));
        let lofi_rnb = tags("lo-fi, r&b, 2020s, instrumental");
        let krnb_session = Session::led([tags("R&B, 2020s, Korean, female solo")], &n);
        assert_eq!(krnb_session.judge_with(&lofi_rnb, false), Err(Reject::NoLanguage));
        assert!(krnb_session.judge_with(&lofi_rnb, true).is_ok(), "played, and R&B like the session");
        // A record tagged instrumental counts as language-less even with a Korean tag (a game soundtrack).
        let ko_inst = tags("soundtrack, electronic, 2020s, south korea, instrumental, video game ost");
        assert_eq!(session.judge_with(&ko_inst, true), Err(Reject::NoLanguage));
        assert!(session.judge_with(&tags(KPOP2), false).is_ok());
    }

    #[test]
    fn the_gate_rules() {
        let n = Neighbours::default();
        let g = Gate::new(tags(KPOP), tags(KPOP2), &n);
        assert!(g.judge(&tags(KPOP2)).is_ok());
        assert_eq!(g.judge(&tags(USRAP)), Err(Reject::OtherScene));
        assert_eq!(g.judge(&Profile::default()), Err(Reject::UnknownScene));
        // Mixed pair: a match for either side passes.
        let mixed = Gate::new(tags(KPOP), tags(USRAP), &n);
        assert!(mixed.judge(&tags(KPOP2)).is_ok());
        assert!(mixed.judge(&tags("hip hop, 2000s, united states, english, male solo")).is_ok());
        assert_eq!(mixed.judge(&tags(SAMBA)), Err(Reject::OtherScene));
        // Nothing known about the songs: no gate.
        let none = Gate::new(Profile::default(), Profile::default(), &n);
        assert!(!none.active());
        assert!(none.judge(&tags(USRAP)).is_ok());
        // One side known: the bridge matches it.
        let half = Gate::new(tags(KPOP), Profile::default(), &n);
        assert!(half.judge(&tags(KPOP2)).is_ok());
        assert_eq!(half.judge(&tags(USRAP)), Err(Reject::OtherScene));
    }

    #[test]
    fn audio_decides_when_tags_cannot() {
        let n = Neighbours::default();
        let v = |x: f32| {
            let mut v = vec![0.0f32; 8];
            v[0] = 1.0;
            v[1] = x;
            Quant::new(&v)
        };
        let a = Profile { audio: v(0.0), ..Default::default() };
        let near = Profile { audio: v(0.3), ..Default::default() };
        let far = Profile { audio: v(3.0), ..Default::default() };
        assert!(fit(&a, &near, &n).is_same());
        assert_eq!(fit(&a, &far, &n), Fit::Other("sound"));
        let q = v(0.5).unwrap();
        assert!((Quant::decode(&q.encode()).unwrap().cos(&q) - 1.0).abs() < 1e-3);
    }

    #[test]
    fn the_index_keeps_artist_scenes() {
        let rec = |id: &str, hint: &str, t: &str| SceneRecord {
            song_id: id.into(),
            title_hint: hint.into(),
            tags_en: t.into(),
            audio: None,
            at: 0,
        };
        let idx = SceneIndex::from_records([
            rec("ytm:a", "ILLIT – Magnetic", KPOP),
            rec("ytm:b", "ILLIT – Cherish", KPOP),
            rec("ytm:c", "Mos Def – UMI Says", USRAP),
        ]);
        assert_eq!(idx.len(), 3);
        let p = idx.profile("ytm:new", "Lucky Girl Syndrome", "ILLIT", &[]);
        assert_eq!((p.scene.lang, p.scene.source), (Some("ko"), Source::Artist));
        let own = idx.profile("ytm:c", "UMI Says", "Mos Def", &["Jazz".into()]);
        assert_eq!(own.scene.source, Source::Tags);
        let none = idx.profile("ytm:x", "Song", "Nobody", &[]);
        assert!(!none.is_known());
    }

    #[test]
    fn scene_records_round_trip_through_the_folder_and_the_phone_copy() {
        let dir = std::env::temp_dir().join(format!("listen-scenes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut v = vec![0.0f32; crate::describe::DIM];
        v[3] = 1.0;
        let line = |id: &str, t: &str, at: i64| {
            serde_json::to_string(&EmbedRecord {
                song_id: id.into(),
                title_hint: "ILLIT – Magnetic".into(),
                tags_en: t.into(),
                audio: Some(crate::describe::encode_vec(&v)),
                at,
                ..Default::default()
            })
            .unwrap()
        };
        let text = [line("ytm:a", USRAP, 1), line("ytm:b", KPOP, 2), line("ytm:a", KPOP, 3)].join("\n");
        std::fs::write(dir.join("fedora-PC.jsonl"), text).unwrap();
        let recs = read_scene_records(&dir);
        assert_eq!(recs.len(), 2);
        assert_eq!((recs[0].song_id.as_str(), recs[0].at), ("ytm:a", 3), "the later line wins");
        let wire: String = recs.iter().map(|r| serde_json::to_string(r).unwrap() + "\n").collect();
        let doubled = format!("{wire}{wire}");
        assert_eq!(SceneIndex::dedup_lines(&doubled).lines().count(), 2);
        let idx = SceneIndex::load_scene_records(&doubled);
        let p = idx.profile("ytm:a", "Magnetic", "ILLIT", &[]);
        assert_eq!(p.scene.lang, Some("ko"));
        let cos = p.audio.as_ref().unwrap().cos(idx.profile("ytm:b", "", "", &[]).audio.as_ref().unwrap());
        assert!((cos - 1.0).abs() < 1e-3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- eras ----

    const KPOP80: &str = "Korean pop, ballad, 1980s, South Korea, Korean, female solo, retro, 80s ballad";
    const KPOP90: &str = "K-pop, dance-pop, 1990s, South Korea, Korean, girl group, 1st-generation K-pop";
    const KPOP10: &str = "K-pop, dance-pop, 2010s, South Korea, Korean, girl group, 3rd-generation K-pop";

    #[test]
    fn eras_come_from_decades_years_and_generations() {
        assert_eq!(Scene::from_tags(KPOP).era, Some(2020));
        assert_eq!(Scene::from_tags(KPOP80).era, Some(1980));
        assert_eq!(Scene::from_tags(SAMBA).era, Some(1970), "before 1980 is one era");
        assert_eq!(Scene::from_tags("K-pop, girl group, 1st-generation K-pop").era, Some(1990));
        assert_eq!(Scene::from_tags("K-pop, 4th-generation K-pop").era, Some(2020));
        assert_eq!(
            Scene::from_tags("K-pop, 5th-generation K-pop, 2010s").era,
            Some(2010),
            "a decade beats a generation"
        );
        assert_eq!(Scene::from_tags("k-pop, 90skpop, dance").era, Some(1990));
        assert_eq!(Scene::from_tags("ballad, 1999, korean").era, Some(1990));
        assert_eq!(Scene::from_tags("pop, 20s jazz age").era, None, "\"20s\" is ambiguous");
        assert_eq!(Scene::from_tags("hip hop, 808s, english").era, None);
        assert_eq!(era_of_year(2024), Some(2020));
        assert_eq!(era_of_year(1964), Some(1970));
        assert_eq!(era_label(1970), "before 1980");
        assert!(Scene::from_tags(KPOP).label().contains("2020s"), "{}", Scene::from_tags(KPOP).label());
    }

    #[test]
    fn eras_more_than_a_decade_apart_are_other_scenes() {
        let n = Neighbours::default();
        assert_eq!(fit(&tags(KPOP80), &tags(KPOP), &n), Fit::Other("era"));
        assert_eq!(fit(&tags(KPOP90), &tags(KPOP), &n), Fit::Other("era"));
        let next = fit(&tags(KPOP10), &tags(KPOP), &n);
        let same = fit(&tags(KPOP2.replace("2010s", "2020s").as_str()), &tags(KPOP), &n);
        assert!(next.is_same() && same.is_same());
        let (Fit::Same(a), Fit::Same(b)) = (next, same) else { unreachable!() };
        assert!(a < b, "a decade apart scores a little lower: {a} {b}");
        // One era unknown: a full match (language and genre) passes with a lower score.
        let undated = tags("K-pop, dance-pop, South Korea, Korean, girl group");
        let Fit::Same(u) = fit(&undated, &tags(KPOP), &n) else { panic!("full match passes") };
        assert!(u < b);
        // Language only (the Hangul title of 바꿔), era unknown: not comparable.
        let script = Profile { scene: Scene::from_script("바꿔", "Lee Jung-hyun"), ..Default::default() };
        assert_eq!(fit(&script, &tags(KPOP), &n), Fit::Unknown);
        // Both eras unknown, language only: as before.
        assert!(
            fit(&script, &Profile { scene: Scene::from_script("너만을", "x"), ..Default::default() }, &n)
                .is_same()
        );
    }

    #[test]
    fn a_bridge_between_2020s_kpop_songs_is_from_the_2010s_or_2020s() {
        let n = Neighbours::default();
        let g = Gate::new(tags(KPOP), tags(KPOP), &n);
        assert!(g.judge(&tags(KPOP10)).is_ok());
        assert_eq!(g.judge(&tags(KPOP90)), Err(Reject::OtherEra));
        assert_eq!(g.judge(&tags(KPOP80)), Err(Reject::OtherEra));
        let script = Profile { scene: Scene::from_script("바꿔", "Lee Jung-hyun"), ..Default::default() };
        assert_eq!(g.judge(&script), Err(Reject::UnknownScene));
        // A 1990s → 2020s pair may take a bridge from either era.
        let mixed = Gate::new(tags(KPOP90), tags(KPOP), &n);
        assert!(mixed.judge(&tags(KPOP90)).is_ok() && mixed.judge(&tags(KPOP)).is_ok());
        assert!(mixed.judge(&tags(KPOP80)).is_ok(), "the 1980s are within a decade of the 1990s side");
    }

    fn by(artist: &str, t: &str) -> Profile {
        Profile { artist: artist.into(), ..tags(t) }
    }

    #[test]
    fn same_artist_from_another_decade_is_another_era() {
        let n = Neighbours::default();
        assert_eq!(fit(&by("x", KPOP80), &by("x", KPOP), &n), Fit::Other("era"));
        assert_eq!(fit(&by("x", KPOP10), &by("x", KPOP), &n), Fit::Same(1.0), "a decade apart is one era");
        let undated = Profile { artist: "x".into(), ..Default::default() };
        assert_eq!(fit(&undated, &by("x", KPOP80), &n), Fit::Same(1.0), "an unknown era doesn't split them");
        // A bridge between two of the artist's 2020s songs is never their 1990s song.
        let g = Gate::new(by("x", KPOP), by("x", KPOP), &n);
        assert_eq!(g.judge_bridge(&by("x", KPOP90), true), Err(Reject::OtherEra));
        assert!(g.judge_bridge(&by("x", KPOP10), true).is_ok());
    }

    #[test]
    fn led_session_of_one_artist_keeps_the_current_decade() {
        let n = Neighbours::default();
        let songs = std::iter::once(by("x", KPOP80)).chain((0..4).map(|_| by("x", KPOP)));
        let led = Session::led(songs, &n);
        assert_eq!(led.scene().and_then(|s| s.era), Some(1980), "{}", led.describe());
        assert_eq!(led.judge(&by("x", KPOP)), Err(Reject::OtherEra));
    }

    #[test]
    fn history_neighbours_never_bridge_eras() {
        let mut old = Profile { artist: "lee jung-hyun".into(), ..Default::default() };
        let mut new = tags(KPOP);
        new.artist = "aespa".into();
        let n = Neighbours(HashSet::from([
            ("lee jung-hyun".into(), "aespa".into()),
            ("aespa".into(), "lee jung-hyun".into()),
        ]));
        assert!(!fit(&old, &new, &n).is_same(), "the new song's era is known, the old one's is not");
        old.scene.era = Some(1990);
        assert_eq!(fit(&old, &new, &n), Fit::Other("era"));
        old.scene.era = Some(2020);
        assert!(fit(&old, &new, &n).is_same());
    }

    #[test]
    fn the_index_dates_songs_by_year_artist_and_sound() {
        let v = |x: f32| {
            let mut v = vec![0.0f32; 8];
            v[0] = 1.0;
            v[1] = x;
            Some(Quant::new(&v).unwrap().encode())
        };
        let rec = |id: &str, hint: &str, t: &str, audio: Option<String>| SceneRecord {
            song_id: id.into(),
            title_hint: hint.into(),
            tags_en: t.into(),
            audio,
            at: 0,
        };
        let idx = SceneIndex::from_records([
            rec("ytm:a", "Fin.K.L – Blue Rain", KPOP90, v(0.0)),
            rec("ytm:b", "Fin.K.L – White", KPOP90, v(0.05)),
            rec("ytm:c", "S.E.S. – Love", KPOP90, v(0.1)),
            rec("ytm:d", "Nobody – Untagged era", "K-pop, dance-pop, Korean", v(0.02)),
            rec("ytm:e", "aespa – Whiplash", KPOP, v(5.0)),
        ]);
        // The artist's other songs.
        assert_eq!(idx.profile("ytm:new", "Ruby", "Fin.K.L", &[]).scene.era, Some(1990));
        // The tags beat the year, the year beats the artist.
        assert_eq!(idx.profile_dated("ytm:e", "Whiplash", "aespa", &[], Some(1999)).scene.era, Some(2020));
        assert_eq!(idx.profile_dated("x", "Ruby", "Fin.K.L", &[], Some(2016)).scene.era, Some(2010));
        // No tag era, no artist era: the closest-sounding songs (three 1990s songs).
        assert_eq!(idx.profile("ytm:d", "Untagged era", "Nobody", &[]).scene.era, Some(1990));
    }

    // ---- the session gate (heart-rate tempo picks) ----

    const UKPOP: &str = "pop, dance-pop, 2000s, United Kingdom, English, girl group, electropop";
    const JROCK: &str = "J-rock, anime, 2020s, Japan, Japanese, male vocals, energetic";
    const USRAP2: &str = "hip hop, trap, 2020s, United States, English, male solo";

    fn session<'a>(t: &[&str], n: &'a Neighbours) -> Session<'a> {
        Session::new(t.iter().map(|x| tags(x)), n)
    }

    #[test]
    fn a_kpop_session_never_takes_uk_pop_japanese_or_unknown_songs() {
        let n = Neighbours::default();
        let s = session(&[KPOP, KPOP, KPOP2.replace("2010s", "2020s").as_str(), KPOP10, KPOP], &n);
        assert!(s.active());
        assert!(s.describe().contains("ko · pop · 2020s"), "{}", s.describe());
        assert!(s.judge(&tags(KPOP)).is_ok());
        assert!(s.judge(&tags(KPOP10)).is_ok());
        assert_eq!(s.judge(&tags(UKPOP)), Err(Reject::OtherScene));
        assert_eq!(s.judge(&tags(JROCK)), Err(Reject::OtherScene));
        assert_eq!(s.judge(&Profile::default()), Err(Reject::UnknownScene));
        assert_eq!(
            s.judge(&Profile {
                scene: Scene::from_script("シェイクスピアのダイアリー", "PIROKALPIN"),
                ..Default::default()
            }),
            Err(Reject::OtherScene)
        );
        // 1980s and 1990s K-pop: another era.
        assert_eq!(s.judge(&tags(KPOP80)), Err(Reject::OtherEra));
        assert_eq!(s.judge(&tags(KPOP90)), Err(Reject::OtherEra));
        // A Korean title without tags: language only, era unknown — not enough.
        assert_eq!(
            s.judge(&Profile { scene: Scene::from_script("바꿔", "Lee Jung-hyun"), ..Default::default() }),
            Err(Reject::UnknownScene)
        );
        // An undated full match passes, below a dated one.
        let undated = s.judge(&tags("K-pop, dance-pop, South Korea, Korean")).unwrap();
        assert!(undated < s.judge(&tags(KPOP)).unwrap());
    }

    #[test]
    fn the_majority_of_the_session_decides_and_a_mixed_one_takes_either() {
        let n = Neighbours::default();
        // One English song among four K-pop songs: still a K-pop session.
        let s = session(&[UKPOP, KPOP, KPOP, KPOP, KPOP], &n);
        assert!(s.judge(&tags(KPOP)).is_ok());
        assert!(s.judge(&tags(UKPOP)).is_err());
        // Mixed: K-pop and US rap, two each.
        let m = session(&[KPOP, USRAP2, KPOP, USRAP2], &n);
        assert!(m.describe().contains("mixed"), "{}", m.describe());
        assert!(m.judge(&tags(KPOP)).is_ok() && m.judge(&tags(USRAP2)).is_ok());
        assert_eq!(m.judge(&tags(SAMBA)), Err(Reject::OtherScene));
        assert_eq!(m.judge(&Profile::default()), Err(Reject::UnknownScene));
        // Unknown songs don't count; the first five known ones do.
        let u = Session::new([Profile::default(), tags(KPOP), Profile::default()], &n);
        assert!(u.active() && u.judge(&tags(UKPOP)).is_err());
    }

    #[test]
    fn nothing_known_means_no_session_gate() {
        let n = Neighbours::default();
        let s = Session::new([Profile::default(), Profile::default()], &n);
        assert!(!s.active());
        assert_eq!(s.judge(&tags(UKPOP)), Ok(0.0));
        assert_eq!(s.judge(&Profile::default()), Ok(0.0));
    }

    #[test]
    fn the_sessions_artists_and_history_neighbours_belong() {
        let mut cur = tags(KPOP);
        cur.artist = "tri.be".into();
        let n = Neighbours(HashSet::from([
            ("tri.be".into(), "kiiikiii".into()),
            ("kiiikiii".into(), "tri.be".into()),
        ]));
        let s = Session::new([cur], &n);
        let same_artist = Profile { artist: "tri.be".into(), ..Default::default() };
        assert_eq!(s.judge(&same_artist), Ok(1.0));
        // …but not with a song from another era than the session's.
        let mut old = tags(KPOP90);
        old.artist = "tri.be".into();
        assert_eq!(s.judge(&old), Err(Reject::OtherEra));
        // Heard next to the session's artist, but the session's era is known and the candidate's is not.
        let neighbour = Profile { artist: "kiiikiii".into(), ..Default::default() };
        assert_eq!(s.judge(&neighbour), Err(Reject::UnknownScene));
        let dated = Profile {
            artist: "kiiikiii".into(),
            scene: Scene { era: Some(2020), ..Default::default() },
            ..Default::default()
        };
        assert!(s.judge(&dated).is_ok());
        // The current song's artist belongs even when its scene is unknown.
        let unknown_now = Profile { artist: "tri.be".into(), ..Default::default() };
        let s = Session::new([unknown_now, tags(KPOP10), tags(KPOP)], &n);
        assert_eq!(s.judge(&Profile { artist: "tri.be".into(), ..Default::default() }), Ok(1.0));
        // A tie between the 2010s and the 2020s goes to the first known song's era.
        assert!(s.describe().contains("2010s"), "{}", s.describe());
        let s = Session::new([tags(KPOP), tags(KPOP10), tags(KPOP10), tags(KPOP)], &n);
        assert!(s.describe().contains("2020s"), "{}", s.describe());
    }
}

#[cfg(test)]
mod trot_tests {
    use super::*;

    #[test]
    fn trot_is_its_own_scene() {
        let idol = Scene::from_tags("K-pop, dance-pop, 2020s, South Korea, Korean");
        let trot_pop = Scene::from_tags("trot, K-pop, 2020s, South Korea, Korean");
        let trot = Scene::from_tags("trot, 2020s, Korean");
        assert!(!compare(&idol, &trot_pop).is_same(), "{:?}", compare(&idol, &trot_pop));
        assert!(!compare(&trot_pop, &idol).is_same());
        assert!(compare(&trot, &trot_pop).is_same());
        assert!(compare(&idol, &idol).is_same());
    }
}
