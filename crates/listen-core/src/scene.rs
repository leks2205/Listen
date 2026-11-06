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
//! scene. Same artist is always the same scene; an artist heard right next to another in the listening
//! history ([`Neighbours`]) is too when nothing else is known.
//!
//! [`Gate`] applies this to a bridge: when the two songs it joins share a scene the bridge must belong to
//! it, when they differ it must match one of them, and a candidate whose scene is unknown is rejected
//! when the songs' scenes are known (no bridge is better than a wrong one).

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
    /// Families close enough that one may bridge the other (half a match).
    const NEAR: [(u16, u16); 7] = [
        (Self::POP, Self::RNB),
        (Self::HIPHOP, Self::RNB),
        (Self::POP, Self::ELECTRONIC),
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
        || has("dance")
        || has_seq("girl", "group")
        || has_seq("boy", "group")
    {
        f |= Families::POP;
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
        for label in labels {
            let l = label.to_lowercase();
            if l.is_empty() || l == "unknown" {
                continue;
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
        // The explicit language tag (or the first one named) wins; "K-pop" in English is still Korean pop.
        s.lang = langs.first().copied();
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
            return other;
        }
        Scene {
            lang: self.lang.or(if self.instrumental { None } else { other.lang }),
            instrumental: self.instrumental,
            families: if self.families.is_empty() { other.families } else { self.families },
            source: self.source,
        }
    }

    /// "ko · pop, hip hop (tags)" for logs.
    pub fn label(&self) -> String {
        if !self.is_known() {
            return "unknown".into();
        }
        let lang = if self.instrumental { "instrumental" } else { self.lang.unwrap_or("?") };
        format!("{lang} · {} ({:?})", self.families.label(), self.source).to_lowercase()
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

/// Compares two scenes on what both know: the language (unless either is instrumental) and the genre
/// families. One dimension alone gives a weaker score.
pub fn compare(a: &Scene, b: &Scene) -> Fit {
    let mut parts: Vec<f32> = Vec::new();
    if let (Some(x), Some(y)) = (a.lang, b.lang)
        && !a.instrumental
        && !b.instrumental
    {
        if x != y {
            return Fit::Other("language");
        }
        parts.push(1.0);
    }
    if !a.families.is_empty() && !b.families.is_empty() {
        let o = a.families.overlap(b.families);
        if o == 0.0 {
            return Fit::Other("genre");
        }
        parts.push(o);
    }
    match parts.len() {
        0 => Fit::Unknown,
        1 => Fit::Same(0.6 * parts[0]),
        _ => Fit::Same(parts.iter().sum::<f32>() / parts.len() as f32),
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
            let scene = Scene { lang, instrumental, families: Families(families), source: Source::Artist };
            if scene.is_known() {
                artists.insert(artist.to_string(), scene);
            }
        }
        SceneIndex { songs, artists }
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
        let own = self.songs.get(song_id);
        let mut key = artist_key(artist);
        if key.is_empty() {
            key = own.map(|o| o.2.clone()).unwrap_or_default();
        }
        let tags = own.map(|o| o.0).unwrap_or_default();
        let by_artist = self.artists.get(&key).copied().unwrap_or_default();
        let scene = tags.or(Scene::from_genres(genres)).or(by_artist).or(Scene::from_script(title, artist));
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

/// How a candidate relates to one song: tags first, then sound, then the artist.
pub fn fit(a: &Profile, b: &Profile, neighbours: &Neighbours) -> Fit {
    if !a.artist.is_empty() && a.artist == b.artist {
        return Fit::Same(1.0);
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
    if !f.is_same()
        && f != Fit::Other("language")
        && f != Fit::Other("genre")
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
    /// Its scene is unknown while the songs' are known.
    UnknownScene,
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

    /// The candidate's scene score (0..1, higher is closer) or why it is rejected.
    pub fn judge(&self, c: &Profile) -> Result<f32, Reject> {
        if !self.active() {
            return Ok(0.0);
        }
        let (fo, fi) = (fit(c, &self.out, self.neighbours), fit(c, &self.inn, self.neighbours));
        let score = |f: Fit| if let Fit::Same(s) = f { s } else { 0.0 };
        let reject = || {
            if matches!(fo, Fit::Other(_)) || matches!(fi, Fit::Other(_)) {
                Reject::OtherScene
            } else {
                Reject::UnknownScene
            }
        };
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
                    Err(Reject::OtherScene)
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
}

impl Tally {
    pub fn rejected(&self) -> usize {
        self.other_scene + self.unknown_scene
    }

    /// "41 candidates, 33 rejected (30 other scene, 3 unknown scene)".
    pub fn describe(&self) -> String {
        if self.rejected() == 0 {
            return format!("{} candidates, none rejected by scene", self.candidates);
        }
        let mut parts = Vec::new();
        if self.other_scene > 0 {
            parts.push(format!("{} other scene", self.other_scene));
        }
        if self.unknown_scene > 0 {
            parts.push(format!("{} unknown scene", self.unknown_scene));
        }
        format!("{} candidates, {} rejected ({})", self.candidates, self.rejected(), parts.join(", "))
    }
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
        let mut c = Profile { artist: "someone".into(), ..Default::default() };
        let n =
            Neighbours(HashSet::from([("someone".into(), "bts".into()), ("bts".into(), "someone".into())]));
        assert_eq!(fit(&c, &a, &Neighbours::default()), Fit::Unknown);
        assert!(fit(&c, &a, &n).is_same());
        // A neighbour doesn't override a known other scene.
        c.scene = Scene::from_tags(USRAP);
        assert!(!fit(&c, &tags(KPOP), &n).is_same());
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
}
