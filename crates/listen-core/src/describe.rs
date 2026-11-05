//! "Describe it": natural-language library search (item 14).
//!
//! `listen-embed` (a Python job on a PC with a GPU, `tools/listen-embed/`) writes one line per song into
//! `embeddings/v1/<model>/*.jsonl`, keyed by the analysis store's song ids: a CLaMP 3 embedding of a 30-s
//! clip of the audio, search tags Gemini wrote for the song (English and Korean), and the CLaMP 3 text
//! embedding of those tags. listen-server encodes the query with CLaMP 3's text tower and ranks with
//! [`Index::search`]: three channels, each z-scored over the library and summed —
//!
//! * audio: query ↔ audio clip (what the music sounds like),
//! * tags: query ↔ tag text in the same embedding space (what is known about the song: era, scene, singer),
//! * words: query words found in the tags and the title (BM25-like, character bigrams for Hangul),
//!
//! then hard filters from the analysis store (tempo, energy, vocal share, key). On the 30 test queries of
//! the spike (295 songs) audio alone reaches recall@10 0.45, all three 0.68.

use crate::analysis::{Camelot, Features, vocal_share};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Embedding model and folder name under `embeddings/v1/`.
pub const MODEL: &str = "clamp3";
/// Width of CLaMP 3's shared embedding space.
pub const DIM: usize = 768;

/// One line of `embeddings/v1/<model>/*.jsonl` (later lines for a song replace earlier ones).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EmbedRecord {
    /// The analysis store's song id (`ytm:<videoId>` or a Navidrome id).
    pub song_id: String,
    /// "Artist – Title", as in the analysis store.
    #[serde(default)]
    pub title_hint: String,
    #[serde(default)]
    pub duration_ms: u32,
    /// Host of the server the song is on (Navidrome songs), as in the analysis store.
    #[serde(default)]
    pub server_hint: String,
    /// Comma-separated search tags (genre, decade, country, singer, instruments, mood, scene labels).
    #[serde(default)]
    pub tags_en: String,
    #[serde(default)]
    pub tags_ko: String,
    /// CLaMP 3 audio embedding, little-endian f16, base64 (absent: no audio could be fetched).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<String>,
    /// CLaMP 3 text embedding of `tags_en` + `tags_ko`, same encoding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<String>,
    /// Why the audio is missing (download failed, too short), for the server status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_error: Option<String>,
    /// Unix seconds.
    #[serde(default)]
    pub at: i64,
}

/// Decodes a base64 little-endian f16 vector into unit length f32 (`None` when malformed or zero).
pub fn decode_vec(b64: &str) -> Option<Vec<f32>> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64.trim()).ok()?;
    if bytes.is_empty() || bytes.len() % 2 != 0 {
        return None;
    }
    let v: Vec<f32> = bytes.as_chunks::<2>().0.iter().map(|c| f16_to_f32(u16::from_le_bytes(*c))).collect();
    normalized(v)
}

/// Encodes a vector as base64 little-endian f16 (the store's format; for tests and tools).
pub fn encode_vec(v: &[f32]) -> String {
    let bytes: Vec<u8> = v.iter().flat_map(|x| f32_to_f16(*x).to_le_bytes()).collect();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Scales `v` to unit length (`None` for a zero or non-finite vector).
pub fn normalized(mut v: Vec<f32>) -> Option<Vec<f32>> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if !n.is_finite() || n == 0.0 {
        return None;
    }
    v.iter_mut().for_each(|x| *x /= n);
    Some(v)
}

fn f16_to_f32(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = i32::from((h >> 10) & 0x1f);
    let frac = f32::from(h & 0x3ff);
    match exp {
        0 => sign * frac * 2f32.powi(-24),
        31 => {
            if frac == 0.0 {
                sign * f32::INFINITY
            } else {
                f32::NAN
            }
        }
        _ => sign * (1.0 + frac / 1024.0) * 2f32.powi(exp - 15),
    }
}

fn f32_to_f16(x: f32) -> u16 {
    let sign: u16 = if x.is_sign_negative() { 0x8000 } else { 0 };
    let a = x.abs();
    if a.is_nan() {
        return 0x7e00;
    }
    if a >= 65520.0 {
        return sign | 0x7c00;
    }
    if a < 2f32.powi(-14) {
        return sign | (a / 2f32.powi(-24)).round() as u16;
    }
    let exp = a.log2().floor() as i32;
    let mut m = ((a / 2f32.powi(exp) - 1.0) * 1024.0).round() as u32;
    let mut e = exp + 15;
    if m == 1024 {
        m = 0;
        e += 1;
    }
    sign | ((e as u16) << 10) | m as u16
}

/// Search words of a text: lower-case words, plus character bigrams of Hangul words (Korean writes
/// "걸그룹" and "걸 그룹", "발라드곡" for "발라드").
pub fn grams(text: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    let lower = text.to_lowercase();
    for w in lower.split(|c: char| !(c.is_alphanumeric() || c == '&' || c == '-')) {
        let w = w.trim_matches('-');
        if w.is_empty() {
            continue;
        }
        out.insert(w.to_string());
        let chars: Vec<char> = w.chars().collect();
        if chars.iter().any(|c| ('\u{ac00}'..='\u{d7a3}').contains(c)) {
            out.extend(chars.windows(2).map(|p| p.iter().collect::<String>()));
        }
    }
    out
}

/// Store facts used as hard filters.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Facets {
    pub bpm: Option<f32>,
    /// 0..1.
    pub energy: Option<f32>,
    /// Share of sung bars, 0..1.
    pub vocal: Option<f32>,
    pub key: Option<Camelot>,
}

impl Facets {
    pub fn of(f: &Features) -> Self {
        Facets {
            bpm: f.grid.as_ref().map(|g| g.bpm),
            energy: Some(f.mood.energy),
            vocal: vocal_share(&f.bars),
            key: f.key.map(|(k, _)| k),
        }
    }
}

/// Hard filters (all optional). A song without the fact a filter needs is left out.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Filter {
    #[serde(default)]
    pub bpm_min: Option<f32>,
    #[serde(default)]
    pub bpm_max: Option<f32>,
    #[serde(default)]
    pub energy_min: Option<f32>,
    #[serde(default)]
    pub energy_max: Option<f32>,
    /// Upper bound of the sung-bar share (0.3 ≈ instrumental).
    #[serde(default)]
    pub vocal_max: Option<f32>,
    #[serde(default)]
    pub vocal_min: Option<f32>,
    /// Camelot code ("8A"); compatible keys (same, ±1, relative) pass.
    #[serde(default)]
    pub key: Option<String>,
}

impl Filter {
    pub fn is_empty(&self) -> bool {
        *self == Filter::default()
    }

    pub fn allows(&self, f: Option<&Facets>) -> bool {
        if self.is_empty() {
            return true;
        }
        let Some(f) = f else { return false };
        let within = |v: Option<f32>, lo: Option<f32>, hi: Option<f32>| {
            if lo.is_none() && hi.is_none() {
                return true;
            }
            v.is_some_and(|v| lo.is_none_or(|lo| v >= lo) && hi.is_none_or(|hi| v <= hi))
        };
        let key_ok = match self.key.as_deref().and_then(parse_camelot) {
            None => true,
            Some(want) => f.key.is_some_and(|k| k.distance(want) <= 0.15),
        };
        // Half / double tempo counts: a 70 bpm ballad is also a 140 bpm song to a dancer.
        let bpm_ok = within(f.bpm, self.bpm_min, self.bpm_max)
            || f.bpm.is_some_and(|b| {
                within(Some(b * 2.0), self.bpm_min, self.bpm_max)
                    || within(Some(b / 2.0), self.bpm_min, self.bpm_max)
            });
        bpm_ok
            && within(f.energy, self.energy_min, self.energy_max)
            && within(f.vocal, self.vocal_min, self.vocal_max)
            && key_ok
    }

    /// Filters written into the query ("120 bpm"); the rest of the query is returned.
    pub fn from_query(query: &str) -> (Filter, String) {
        let mut filter = Filter::default();
        let words: Vec<&str> = query.split_whitespace().collect();
        let mut rest = Vec::new();
        let mut i = 0;
        while i < words.len() {
            let w = words[i].to_lowercase();
            let num = w.trim_end_matches("bpm").parse::<f32>().ok().filter(|n| (40.0..=240.0).contains(n));
            if let Some(n) = num {
                let unit_here = w.ends_with("bpm");
                let unit_next = words.get(i + 1).is_some_and(|n| n.eq_ignore_ascii_case("bpm"));
                if unit_here || unit_next {
                    filter.bpm_min = Some(n - 6.0);
                    filter.bpm_max = Some(n + 6.0);
                    i += if unit_next { 2 } else { 1 };
                    continue;
                }
            }
            rest.push(words[i]);
            i += 1;
        }
        (filter, rest.join(" "))
    }
}

/// "8A" / "11b" → a Camelot key.
pub fn parse_camelot(s: &str) -> Option<Camelot> {
    let s = s.trim();
    let (num, ring) = s.split_at(s.len().checked_sub(1)?);
    let number: u8 = num.parse().ok().filter(|n| (1..=12).contains(n))?;
    match ring {
        "A" | "a" => Some(Camelot { number, minor: true }),
        "B" | "b" => Some(Camelot { number, minor: false }),
        _ => None,
    }
}

/// One song of the index.
#[derive(Debug, Clone)]
struct Entry {
    song_id: String,
    title_hint: String,
    duration_ms: u32,
    server_hint: String,
    audio: Option<Vec<f32>>,
    tags: Option<Vec<f32>>,
    grams: HashSet<String>,
}

/// One search result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hit {
    pub song_id: String,
    pub title: String,
    pub artist: String,
    #[serde(default)]
    pub duration_ms: u32,
    #[serde(default)]
    pub server_hint: String,
    pub score: f32,
}

/// `GET /v1/search` answer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SearchResponse {
    #[serde(default)]
    pub hits: Vec<Hit>,
    /// Songs in the index (with any embedding).
    #[serde(default)]
    pub indexed: usize,
    #[serde(default)]
    pub took_ms: u32,
}

/// Splits a title hint "Artist – Title" (the store's separator is an en dash).
pub fn split_hint(hint: &str) -> (String, String) {
    match hint.split_once(" – ").or_else(|| hint.split_once(" - ")) {
        Some((a, t)) => (t.trim().to_string(), a.trim().to_string()),
        None => (hint.trim().to_string(), String::new()),
    }
}

/// The searchable library: every song listen-embed wrote.
#[derive(Debug, Clone, Default)]
pub struct Index {
    songs: Vec<Entry>,
    df: HashMap<String, u32>,
}

impl Index {
    /// Builds the index; for a song id seen twice the later record wins.
    pub fn from_records(records: impl IntoIterator<Item = EmbedRecord>) -> Self {
        let mut by_id: HashMap<String, EmbedRecord> = HashMap::new();
        let mut order = Vec::new();
        for r in records {
            if r.song_id.is_empty() {
                continue;
            }
            if !by_id.contains_key(&r.song_id) {
                order.push(r.song_id.clone());
            }
            by_id.insert(r.song_id.clone(), r);
        }
        let mut songs = Vec::with_capacity(order.len());
        for id in order {
            let r = by_id.remove(&id).expect("id in map");
            let audio = r.audio.as_deref().and_then(decode_vec).filter(|v| v.len() == DIM);
            let tags = r.tags.as_deref().and_then(decode_vec).filter(|v| v.len() == DIM);
            let grams = grams(&format!("{} {} {}", r.tags_en, r.tags_ko, r.title_hint));
            if audio.is_none() && tags.is_none() && grams.is_empty() {
                continue;
            }
            songs.push(Entry {
                song_id: r.song_id,
                title_hint: r.title_hint,
                duration_ms: r.duration_ms,
                server_hint: r.server_hint,
                audio,
                tags,
                grams,
            });
        }
        let mut df = HashMap::new();
        for s in &songs {
            for g in &s.grams {
                *df.entry(g.clone()).or_insert(0u32) += 1;
            }
        }
        Index { songs, df }
    }

    /// Reads every `*.jsonl` file of `dir` (name order; unreadable lines are skipped).
    pub fn load(dir: &Path) -> Self {
        let mut files: Vec<_> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
            .collect();
        files.sort();
        let records = files.iter().flat_map(|p| {
            std::fs::read_to_string(p)
                .unwrap_or_default()
                .lines()
                .filter_map(|l| serde_json::from_str::<EmbedRecord>(l).ok())
                .collect::<Vec<_>>()
        });
        Self::from_records(records)
    }

    pub fn len(&self) -> usize {
        self.songs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.songs.is_empty()
    }

    /// Songs with an audio embedding.
    pub fn with_audio(&self) -> usize {
        self.songs.iter().filter(|s| s.audio.is_some()).count()
    }

    pub fn contains(&self, song_id: &str) -> bool {
        self.songs.iter().any(|s| s.song_id == song_id)
    }

    /// Song ids in the index.
    pub fn ids(&self) -> HashSet<&str> {
        self.songs.iter().map(|s| s.song_id.as_str()).collect()
    }

    /// Ranks the library for a query: `query_vec` is the query's CLaMP 3 text embedding (`None`: words
    /// only), `facets` gives the store's facts per song id for `filter`. Best first, at most `limit`.
    pub fn search(
        &self,
        query: &str,
        query_vec: Option<&[f32]>,
        filter: &Filter,
        facets: impl Fn(&str) -> Option<Facets>,
        limit: usize,
    ) -> Vec<Hit> {
        let n = self.songs.len();
        if n == 0 || limit == 0 {
            return Vec::new();
        }
        let q = query_vec.and_then(|v| normalized(v.to_vec()));
        let dot = |v: &Option<Vec<f32>>| -> Option<f32> {
            let (q, v) = (q.as_ref()?, v.as_ref()?);
            (q.len() == v.len()).then(|| q.iter().zip(v).map(|(a, b)| a * b).sum())
        };
        let audio: Vec<Option<f32>> = self.songs.iter().map(|s| dot(&s.audio)).collect();
        let tags: Vec<Option<f32>> = self.songs.iter().map(|s| dot(&s.tags)).collect();
        let qg = grams(query);
        let words: Vec<Option<f32>> = self
            .songs
            .iter()
            .map(|s| {
                Some(
                    qg.iter()
                        .filter(|g| s.grams.contains(*g))
                        .map(|g| (n as f32 / *self.df.get(g).unwrap_or(&1) as f32).ln())
                        .sum(),
                )
            })
            .collect();
        let (za, zt, zw) = (zscores(&audio), zscores(&tags), zscores(&words));
        let mut ranked: Vec<(f32, usize)> = (0..n)
            .filter(|&i| filter.allows(facets(&self.songs[i].song_id).as_ref()))
            .map(|i| (za[i] + zt[i] + zw[i], i))
            .collect();
        ranked.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        ranked
            .into_iter()
            .take(limit)
            .map(|(score, i)| {
                let s = &self.songs[i];
                let (title, artist) = split_hint(&s.title_hint);
                Hit {
                    song_id: s.song_id.clone(),
                    title,
                    artist,
                    duration_ms: s.duration_ms,
                    server_hint: s.server_hint.clone(),
                    score,
                }
            })
            .collect()
    }
}

/// z-scores of the values present; absent values (and a channel with no spread) count as average (0).
fn zscores(v: &[Option<f32>]) -> Vec<f32> {
    let present: Vec<f32> = v.iter().flatten().copied().collect();
    if present.len() < 2 {
        return vec![0.0; v.len()];
    }
    let mean = present.iter().sum::<f32>() / present.len() as f32;
    let var = present.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / present.len() as f32;
    let sd = var.sqrt();
    if sd < 1e-9 {
        return vec![0.0; v.len()];
    }
    v.iter().map(|x| x.map_or(0.0, |x| (x - mean) / sd)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(i: usize) -> Vec<f32> {
        let mut v = vec![0.0; DIM];
        v[i] = 1.0;
        v
    }

    fn rec(
        id: &str,
        hint: &str,
        en: &str,
        ko: &str,
        audio: Option<Vec<f32>>,
        tags: Option<Vec<f32>>,
    ) -> EmbedRecord {
        EmbedRecord {
            song_id: id.into(),
            title_hint: hint.into(),
            duration_ms: 200_000,
            tags_en: en.into(),
            tags_ko: ko.into(),
            audio: audio.map(|v| encode_vec(&v)),
            tags: tags.map(|v| encode_vec(&v)),
            ..Default::default()
        }
    }

    #[test]
    fn f16_round_trip_keeps_direction() {
        let v: Vec<f32> = (0..DIM).map(|i| ((i as f32) * 0.37).sin() * 0.05).collect();
        let back = decode_vec(&encode_vec(&v)).unwrap();
        let n = normalized(v).unwrap();
        let cos: f32 = n.iter().zip(&back).map(|(a, b)| a * b).sum();
        assert!(cos > 0.9999, "{cos}");
        assert!(decode_vec("!!").is_none());
        assert!(decode_vec(&encode_vec(&[0.0; 4])).is_none());
        for x in [0.0f32, 1.0, -2.5, 1e-6, 60000.0, 0.333] {
            let y = f16_to_f32(f32_to_f16(x));
            assert!((x - y).abs() <= x.abs() * 1e-3 + 1e-7, "{x} -> {y}");
        }
    }

    #[test]
    fn hangul_words_also_give_bigrams() {
        let g = grams("90년대 한국 발라드, Girl group, R&B");
        for w in ["90년대", "한국", "발라드", "발라", "라드", "girl", "group", "r&b"] {
            assert!(g.contains(w), "{w} in {g:?}");
        }
        assert!(grams("걸그룹 댄스곡").contains("걸그"));
    }

    #[test]
    fn three_channels_add_up() {
        let idx = Index::from_records(vec![
            rec(
                "ytm:a",
                "Artist A – Samba",
                "samba, Brazil, 1970s",
                "삼바, 브라질",
                Some(unit(0)),
                Some(unit(1)),
            ),
            rec(
                "ytm:b",
                "Artist B – Ballad",
                "ballad, Korea, 1990s",
                "발라드, 한국, 1990년대",
                Some(unit(2)),
                Some(unit(3)),
            ),
            rec("ytm:c", "Artist C – Trot", "trot, Korea", "트로트", None, Some(unit(4))),
            rec("ytm:d", "Artist D – Rock", "rock, band", "록, 밴드", Some(unit(5)), None),
        ]);
        assert_eq!((idx.len(), idx.with_audio()), (4, 3));
        // Query vector points at b's audio; words point at b too.
        let hits = idx.search("90년대 한국 발라드", Some(&unit(2)), &Filter::default(), |_| None, 10);
        assert_eq!(hits[0].song_id, "ytm:b");
        assert_eq!((hits[0].title.as_str(), hits[0].artist.as_str()), ("Ballad", "Artist B"));
        // Words alone (no encoder): the trot song comes first for 트로트.
        let hits = idx.search("트로트", None, &Filter::default(), |_| None, 2);
        assert_eq!(hits[0].song_id, "ytm:c");
        assert_eq!(hits.len(), 2);
        // The tag vector channel alone.
        let hits = idx.search("zzz", Some(&unit(1)), &Filter::default(), |_| None, 1);
        assert_eq!(hits[0].song_id, "ytm:a");
    }

    #[test]
    fn later_records_replace_earlier_ones_and_empty_ones_are_dropped() {
        let idx = Index::from_records(vec![
            rec("ytm:a", "A – Old", "old", "", None, None),
            rec("ytm:a", "A – New", "new", "", Some(unit(0)), None),
            rec("", "nobody", "x", "", None, None),
            rec("ytm:e", "", "", "", None, None),
        ]);
        assert_eq!(idx.len(), 1);
        let hits = idx.search("new", None, &Filter::default(), |_| None, 5);
        assert_eq!(hits[0].title, "New");
    }

    #[test]
    fn filters_use_store_facts() {
        let idx = Index::from_records(vec![
            rec("fast", "X – Fast", "dance", "", Some(unit(0)), None),
            rec("slow", "X – Slow", "dance", "", Some(unit(1)), None),
            rec("unknown", "X – Unknown", "dance", "", Some(unit(2)), None),
        ]);
        let facts = |id: &str| match id {
            "fast" => Some(Facets { bpm: Some(128.0), energy: Some(0.9), vocal: Some(0.8), key: None }),
            "slow" => Some(Facets { bpm: Some(70.0), energy: Some(0.2), vocal: Some(0.1), key: None }),
            _ => None,
        };
        let f = Filter { bpm_min: Some(120.0), bpm_max: Some(135.0), ..Default::default() };
        // 70 bpm doubles to 140: out; 128: in; unknown tempo: out.
        let ids: Vec<_> = idx.search("dance", None, &f, facts, 10).into_iter().map(|h| h.song_id).collect();
        assert_eq!(ids, vec!["fast"]);
        let f = Filter { vocal_max: Some(0.3), ..Default::default() };
        let ids: Vec<_> = idx.search("dance", None, &f, facts, 10).into_iter().map(|h| h.song_id).collect();
        assert_eq!(ids, vec!["slow"]);
        // No filter: songs without facts stay.
        assert_eq!(idx.search("dance", None, &Filter::default(), facts, 10).len(), 3);
    }

    #[test]
    fn bpm_in_the_query_becomes_a_filter() {
        let (f, rest) = Filter::from_query("dreamy pop 120 bpm");
        assert_eq!((f.bpm_min, f.bpm_max, rest.as_str()), (Some(114.0), Some(126.0), "dreamy pop"));
        let (f, rest) = Filter::from_query("90bpm hip hop");
        assert_eq!((f.bpm_min, rest.as_str()), (Some(84.0), "hip hop"));
        let (f, rest) = Filter::from_query("1990s ballads");
        assert!(f.is_empty());
        assert_eq!(rest, "1990s ballads");
    }

    #[test]
    fn key_filter_passes_compatible_keys() {
        assert_eq!(parse_camelot("8A"), Some(Camelot { number: 8, minor: true }));
        assert_eq!(parse_camelot("12b"), Some(Camelot { number: 12, minor: false }));
        assert!(
            parse_camelot("13A").is_none() && parse_camelot("A").is_none() && parse_camelot("").is_none()
        );
        let f = Filter { key: Some("8A".into()), ..Default::default() };
        let at = |number, minor| Facets { key: Some(Camelot { number, minor }), ..Default::default() };
        assert!(
            f.allows(Some(&at(8, true))) && f.allows(Some(&at(9, true))) && f.allows(Some(&at(8, false)))
        );
        assert!(!f.allows(Some(&at(2, true))) && !f.allows(Some(&Facets::default())));
    }

    #[test]
    fn title_hints_split_at_the_dash() {
        assert_eq!(split_hint("IU – 좋은 날"), ("좋은 날".into(), "IU".into()));
        assert_eq!(split_hint("Solo"), ("Solo".into(), String::new()));
    }
}
