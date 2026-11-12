//! Known words in Korean lyrics: each word's Lute status (through the vocab-hub word list), the
//! share of a song's words the listener knows, and the "Learnable songs" mix (liked songs at
//! 90–98 % known words: enough new words to learn, few enough to follow the song).
//!
//! A word's status is looked up as written, then by its dictionary form (from the Gemini gloss
//! or the hub's `/api/resolve` when it has one), then with a trailing particle cut off
//! (사람들은 → 사람들 → 사람), since Lute's Korean parser keeps particles as their own terms.
//!
//! [`Store`] keeps every song's words (form, dictionary form, count) once its lyrics were seen,
//! so coverage is recomputed from the store when Lute changes, without fetching lyrics again.

use crate::gemini::{Gloss, pieces};
use crate::vocab::{Knowledge, Words};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Lowest share of known words for a learnable song.
pub const LEARNABLE_MIN: f32 = 0.90;
/// Highest share: above it there is next to nothing new to learn.
pub const LEARNABLE_MAX: f32 = 0.98;
/// Fewer Korean words than this make no meaningful percentage (a hook in an English song).
pub const MIN_WORDS: u32 = 20;
/// Songs without Korean lyrics are looked at again after this long (lyrics may turn up).
pub const RETRY_NONE_SECS: i64 = 30 * 86_400;

/// Particles and endings Lute's parser splits off a noun or a word, longest first.
const PARTICLES: &[&str] = &[
    "에서부터",
    "에게서",
    "한테서",
    "으로써",
    "으로서",
    "이라고",
    "에서는",
    "에서도",
    "으로는",
    "에게는",
    "까지",
    "부터",
    "처럼",
    "보다",
    "에서",
    "에게",
    "한테",
    "으로",
    "하고",
    "이랑",
    "이나",
    "라고",
    "마저",
    "조차",
    "밖에",
    "들",
    "은",
    "는",
    "이",
    "가",
    "을",
    "를",
    "에",
    "의",
    "도",
    "로",
    "와",
    "과",
    "랑",
    "만",
    "요",
    "야",
];

/// The status of a word: as written, its dictionary form, or the word with particles cut off.
pub fn status_of(words: &Words, form: &str, lemma: Option<&str>) -> Option<i32> {
    if let Some(s) = words.status(form, lemma) {
        return Some(s);
    }
    let mut stem = form;
    for _ in 0..3 {
        let Some(p) = PARTICLES.iter().find(|p| stem.len() > p.len() && stem.ends_with(**p)) else {
            break;
        };
        stem = &stem[..stem.len() - p.len()];
        if let Some(s) = words.terms.get(stem) {
            return Some(*s);
        }
    }
    None
}

/// A word of a song with how often it is sung.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Token {
    #[serde(rename = "f")]
    pub form: String,
    #[serde(rename = "l", default, skip_serializing_if = "Option::is_none")]
    pub lemma: Option<String>,
    #[serde(rename = "n")]
    pub count: u32,
}

/// The Korean words of the lines with their counts (in order of first appearance); the
/// dictionary form comes from `lemmas` (gloss or hub) when known.
pub fn tokens(lines: &[String], lemmas: &HashMap<String, String>) -> Vec<Token> {
    let mut index: HashMap<&str, usize> = HashMap::new();
    let mut out: Vec<Token> = Vec::new();
    for l in lines {
        for p in pieces(l).into_iter().filter(|p| p.word) {
            match index.get(p.text) {
                Some(&i) => out[i].count += 1,
                None => {
                    index.insert(p.text, out.len());
                    out.push(Token {
                        form: p.text.to_string(),
                        lemma: lemmas.get(p.text).filter(|l| *l != p.text).cloned(),
                        count: 1,
                    });
                }
            }
        }
    }
    out
}

/// The dictionary forms of a Gemini glossary.
pub fn gloss_lemmas(glosses: &BTreeMap<String, Gloss>) -> HashMap<String, String> {
    glosses
        .iter()
        .filter(|(_, g)| !g.lemma.trim().is_empty())
        .map(|(w, g)| (w.clone(), g.lemma.trim().to_string()))
        .collect()
}

/// How many of a song's sung words (every occurrence) the listener knows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Coverage {
    pub words: u32,
    pub known: u32,
    pub learning: u32,
    pub new: u32,
    /// Ignored terms (names, interjections marked 98): left out of the share.
    pub ignored: u32,
}

impl Coverage {
    /// Known words as a share of the counted ones (`None`: no Korean words).
    pub fn share(&self) -> Option<f32> {
        let counted = self.words.saturating_sub(self.ignored);
        (counted > 0).then(|| self.known as f32 / counted as f32)
    }

    /// The share in whole percent, rounded down (95.9 % shows as 95 %, never as a too-good 96 %).
    pub fn percent(&self) -> Option<u32> {
        self.share().map(|s| (s * 100.0 + 1e-4).floor() as u32)
    }

    pub fn learnable(&self) -> bool {
        self.words.saturating_sub(self.ignored) >= MIN_WORDS
            && self.share().is_some_and(|s| (LEARNABLE_MIN..=LEARNABLE_MAX).contains(&s))
    }
}

/// Coverage of a song's words against the word list (`known_min`: lowest status counted as known).
pub fn coverage(words: &Words, tokens: &[Token], known_min: i32) -> Coverage {
    let mut c = Coverage::default();
    for t in tokens {
        c.words += t.count;
        match Knowledge::from_status(status_of(words, &t.form, t.lemma.as_deref()), known_min) {
            Knowledge::Known => c.known += t.count,
            Knowledge::Learning(_) => c.learning += t.count,
            Knowledge::New => c.new += t.count,
            Knowledge::Ignored => c.ignored += t.count,
        }
    }
    c
}

// ---- store ------------------------------------------------------------------------

/// One song's words and its last coverage.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SongWords {
    /// Fingerprint of the lyric lines the words come from.
    pub lines_key: String,
    pub tokens: Vec<Token>,
    /// Coverage for the word list version `cov_version` (recomputed when Lute changed).
    #[serde(default)]
    pub cov: Option<Coverage>,
    #[serde(default)]
    pub cov_version: String,
    /// `cov` came from the hub's analysis (Kiwi dictionary forms, as Lute counts).
    #[serde(default)]
    pub hub: bool,
}

impl SongWords {
    /// The words as text for the hub's analysis (each one as often as it is sung).
    pub fn text(&self) -> String {
        let mut out = String::new();
        for t in &self.tokens {
            for _ in 0..t.count {
                out.push_str(&t.form);
                out.push('\n');
            }
        }
        out
    }
}

/// Every song's Korean words, by song key ("server:id"). Songs whose lyrics have no Korean
/// (or that have no lyrics) are remembered with the time they were looked at.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Store {
    #[serde(default)]
    pub songs: BTreeMap<String, SongWords>,
    #[serde(default)]
    pub none: BTreeMap<String, i64>,
}

impl Store {
    pub fn path(cache_dir: &Path) -> PathBuf {
        cache_dir.join("lyrics-words.json")
    }

    pub fn load(cache_dir: &Path) -> Self {
        std::fs::read(Self::path(cache_dir))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    /// Writes the store atomically (`listen_core::files::write_atomic`).
    pub fn save(&self, cache_dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(cache_dir)?;
        let bytes = serde_json::to_vec(self).map_err(std::io::Error::other)?;
        listen_core::files::write_atomic(&Self::path(cache_dir), &bytes)
    }

    /// Records a song's lyric lines. Returns whether anything changed. Lines without Korean
    /// mark the song as "none". Known dictionary forms stay when the new lemmas lack them.
    pub fn record(
        &mut self,
        key: &str,
        lines: &[String],
        lemmas: &HashMap<String, String>,
        now: i64,
    ) -> bool {
        let lk = crate::gemini::lines_key(lines);
        let mut toks = tokens(lines, lemmas);
        if toks.is_empty() {
            self.songs.remove(key);
            return self.none.insert(key.to_string(), now).is_none();
        }
        self.none.remove(key);
        if let Some(old) = self.songs.get(key).filter(|o| o.lines_key == lk) {
            let old_lemmas: HashMap<&str, &str> =
                old.tokens.iter().filter_map(|t| Some((t.form.as_str(), t.lemma.as_deref()?))).collect();
            for t in &mut toks {
                if t.lemma.is_none() {
                    t.lemma = old_lemmas.get(t.form.as_str()).map(|l| l.to_string());
                }
            }
            if old.tokens == toks {
                return false;
            }
        }
        self.songs.insert(key.to_string(), SongWords { lines_key: lk, tokens: toks, ..Default::default() });
        true
    }

    /// Whether the song still needs its lyrics looked at.
    pub fn needs(&self, key: &str, now: i64) -> bool {
        !self.songs.contains_key(key) && self.none.get(key).is_none_or(|at| now - at >= RETRY_NONE_SECS)
    }

    /// The song's coverage for this word list, from the cache when the list did not change.
    /// Returns whether a coverage was recomputed (the store should then be saved).
    pub fn refresh(&mut self, words: &Words, known_min: i32) -> bool {
        let version = Self::version(words, known_min);
        let mut changed = false;
        for s in self.songs.values_mut() {
            if s.cov.is_none() || s.cov_version != version || words.version.is_empty() {
                let c = coverage(words, &s.tokens, known_min);
                changed |= s.cov != Some(c) || s.hub;
                s.cov = Some(c);
                s.cov_version = version.clone();
                s.hub = false;
            }
        }
        changed
    }

    /// The version a coverage belongs to (word list and known-from status).
    pub fn version(words: &Words, known_min: i32) -> String {
        format!("{}/{known_min}", words.version)
    }

    /// Songs whose coverage is still the local estimate (the hub can count them better), with
    /// the text to send.
    pub fn hub_pending(&self, limit: usize) -> Vec<(String, String)> {
        self.songs.iter().filter(|(_, s)| !s.hub).take(limit).map(|(k, s)| (k.clone(), s.text())).collect()
    }

    /// Puts the hub's count for a song, if it still belongs to the current word list `version`.
    pub fn set_hub(&mut self, key: &str, version: &str, cov: Coverage) {
        if let Some(s) = self.songs.get_mut(key).filter(|s| s.cov_version == version) {
            s.cov = Some(cov);
            s.hub = true;
        }
    }

    pub fn coverage(&self, key: &str) -> Option<Coverage> {
        self.songs.get(key).and_then(|s| s.cov)
    }

    /// The learnable songs among `keys` (e.g. the liked songs), easiest first. Call
    /// [`Store::refresh`] first.
    pub fn learnable<'a>(&self, keys: impl IntoIterator<Item = &'a str>) -> Vec<(&'a str, Coverage)> {
        let mut out: Vec<(&str, Coverage)> = keys
            .into_iter()
            .filter_map(|k| Some((k, self.coverage(k).filter(Coverage::learnable)?)))
            .collect();
        out.sort_by(|a, b| {
            b.1.share()
                .partial_cmp(&a.1.share())
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(b.1.words.cmp(&a.1.words))
        });
        out
    }
}

// ---- hub analysis -------------------------------------------------------------------

/// Parses the hub's `POST /api/analyze` answer (tokens, known, learning, unknown; ignored words
/// are not counted as tokens).
pub fn parse_analyze(body: &str) -> Option<Coverage> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let n = |k: &str| v.get(k).and_then(|x| x.as_u64()).map(|x| x as u32);
    Some(Coverage {
        words: n("tokens")?,
        known: n("known")?,
        learning: n("learning")?,
        new: n("unknown")?,
        ignored: 0,
    })
}

/// Counts known words in `text` the way Lute does (the hub's Kiwi dictionary forms).
pub async fn hub_coverage(
    http: &reqwest::Client,
    base: &str,
    text: &str,
    known_min: i32,
) -> Result<Coverage, String> {
    let base = base.trim().trim_end_matches('/');
    if !(base.starts_with("http://") || base.starts_with("https://")) {
        return Err("no vocab-hub address set".into());
    }
    let resp = http
        .post(format!("{base}/api/analyze"))
        .timeout(Duration::from_secs(15))
        .json(&serde_json::json!({ "text": text, "knownMin": known_min }))
        .send()
        .await
        .map_err(|e| e.without_url().to_string())?;
    if !resp.status().is_success() {
        return Err(format!("vocab-hub answered {}", resp.status()));
    }
    let body = resp.text().await.map_err(|e| e.without_url().to_string())?;
    parse_analyze(&body).ok_or_else(|| "unreadable analysis".into())
}

/// One word's class from the hub's count of just that word.
pub fn class_of(c: &Coverage) -> Knowledge {
    if c.known > 0 {
        Knowledge::Known
    } else if c.learning > 0 {
        Knowledge::Learning(0)
    } else if c.new > 0 {
        Knowledge::New
    } else {
        Knowledge::Ignored
    }
}

// ---- hub dictionary forms -----------------------------------------------------------

/// Parses the hub's `POST /api/resolve` answer into dictionary forms by word.
pub fn parse_resolve(body: &str) -> Option<HashMap<String, String>> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let mut out = HashMap::new();
    for line in v.get("lines")?.as_array()? {
        for w in line.as_array().into_iter().flatten() {
            if let (Some(word), Some(lemma)) = (w["w"].as_str(), w["lemma"].as_str())
                && !lemma.is_empty()
                && word != lemma
            {
                out.insert(word.to_string(), lemma.to_string());
            }
        }
    }
    Some(out)
}

/// Dictionary forms of the lines' words from the hub (Kiwi, as Lute splits them).
/// `Ok(None)`: the hub has no `/api/resolve` yet (the gloss and particle rules stand in).
pub async fn hub_lemmas(
    http: &reqwest::Client,
    base: &str,
    lines: &[String],
) -> Result<Option<HashMap<String, String>>, String> {
    let base = base.trim().trim_end_matches('/');
    if !(base.starts_with("http://") || base.starts_with("https://")) {
        return Err("no vocab-hub address set".into());
    }
    let resp = http
        .post(format!("{base}/api/resolve"))
        .timeout(Duration::from_secs(15))
        .json(&serde_json::json!({ "lines": lines }))
        .send()
        .await
        .map_err(|e| e.without_url().to_string())?;
    match resp.status().as_u16() {
        404 | 405 => Ok(None),
        s if (200..300).contains(&s) => {
            let body = resp.text().await.map_err(|e| e.without_url().to_string())?;
            parse_resolve(&body).map(Some).ok_or_else(|| "unreadable resolve answer".into())
        }
        s => Err(format!("vocab-hub answered {s}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(pairs: &[(&str, i32)]) -> Words {
        Words { version: "v1".into(), terms: pairs.iter().map(|(w, s)| (w.to_string(), *s)).collect() }
    }

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn looks_up_form_then_lemma_then_stem() {
        let w = words(&[("사람", 99), ("고르다", 3), ("너", 5), ("오늘", 2)]);
        assert_eq!(status_of(&w, "사람", None), Some(99));
        assert_eq!(status_of(&w, "골랐다는", Some("고르다")), Some(3));
        assert_eq!(status_of(&w, "사람들은", None), Some(99), "two particles off");
        assert_eq!(status_of(&w, "너를", None), Some(5));
        assert_eq!(status_of(&w, "오늘도", None), Some(2));
        assert_eq!(status_of(&w, "는", None), None, "a particle alone is not stripped to nothing");
        assert_eq!(status_of(&w, "별", None), None);
    }

    #[test]
    fn tokens_count_every_occurrence_and_keep_lemmas() {
        let lemmas: HashMap<String, String> =
            [("사랑해".to_string(), "사랑하다".to_string()), ("너".to_string(), "너".to_string())].into();
        let t = tokens(&lines(&["너를 사랑해 baby", "사랑해 사랑해", "oh yeah"]), &lemmas);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0], Token { form: "너를".into(), lemma: None, count: 1 });
        assert_eq!(t[1], Token { form: "사랑해".into(), lemma: Some("사랑하다".into()), count: 3 });
    }

    #[test]
    fn coverage_counts_occurrences_and_leaves_ignored_out() {
        let w = words(&[("사랑하다", 99), ("너", 4), ("오", 98)]);
        let t = vec![
            Token { form: "사랑해".into(), lemma: Some("사랑하다".into()), count: 8 },
            Token { form: "너를".into(), lemma: None, count: 1 },
            Token { form: "별".into(), lemma: None, count: 1 },
            Token { form: "오".into(), lemma: None, count: 5 },
        ];
        let c = coverage(&w, &t, 5);
        assert_eq!((c.words, c.known, c.learning, c.new, c.ignored), (15, 8, 1, 1, 5));
        assert_eq!(c.percent(), Some(80));
        assert_eq!(coverage(&w, &t, 4).known, 9, "known from status 4");
        assert_eq!(Coverage::default().share(), None);
    }

    #[test]
    fn learnable_band_and_minimum_words() {
        let c = |words, known| Coverage { words, known, new: words - known, ..Default::default() };
        assert!(c(100, 90).learnable());
        assert!(c(100, 98).learnable());
        assert!(!c(100, 89).learnable());
        assert!(!c(100, 99).learnable(), "nothing left to learn");
        assert!(!c(10, 9).learnable(), "too few words");
        assert_eq!(c(1000, 959).percent(), Some(95));
    }

    #[test]
    fn store_recomputes_when_lute_changes() {
        let mut s = Store::default();
        let song: Vec<String> = (0..20).map(|i| if i < 19 { "사랑".into() } else { "별".into() }).collect();
        assert!(s.record("a:1", &song, &HashMap::new(), 0));
        assert!(!s.record("a:1", &song, &HashMap::new(), 5), "same lines, nothing new");
        assert!(s.record("a:2", &lines(&["hello", "world"]), &HashMap::new(), 0));
        assert!(!s.needs("a:2", 10) && s.needs("a:2", RETRY_NONE_SECS) && s.needs("a:3", 0));

        let mut w = words(&[("사랑", 99)]);
        assert!(s.refresh(&w, 5));
        assert!(!s.refresh(&w, 5), "cached for this version");
        assert_eq!(s.coverage("a:1").and_then(|c| c.percent()), Some(95));
        assert_eq!(s.learnable(["a:1", "a:2", "a:3"]).len(), 1);

        w.terms.insert("별".into(), 5);
        w.version = "v2".into();
        assert!(s.refresh(&w, 5));
        assert_eq!(s.coverage("a:1").and_then(|c| c.percent()), Some(100));
        assert!(s.learnable(["a:1"]).is_empty(), "all known now");
    }

    #[test]
    fn store_keeps_lemmas_and_round_trips() {
        let mut s = Store::default();
        let l = lines(&["사랑해"]);
        let lem: HashMap<String, String> = [("사랑해".to_string(), "사랑하다".to_string())].into();
        s.record("k", &l, &lem, 0);
        assert!(!s.record("k", &l, &HashMap::new(), 1), "lemma from before kept");
        assert_eq!(s.songs["k"].tokens[0].lemma.as_deref(), Some("사랑하다"));
        let dir = std::env::temp_dir().join(format!("listen-known-{}", std::process::id()));
        s.save(&dir).unwrap();
        assert_eq!(Store::load(&dir), s);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn learnable_sorted_easiest_first() {
        let mut s = Store::default();
        for (k, known) in [("a", 90u32), ("b", 97), ("c", 93)] {
            s.songs.insert(
                k.into(),
                SongWords {
                    cov: Some(Coverage { words: 100, known, new: 100 - known, ..Default::default() }),
                    ..Default::default()
                },
            );
        }
        let keys: Vec<_> = s.learnable(["a", "b", "c"]).into_iter().map(|(k, _)| k).collect();
        assert_eq!(keys, ["b", "c", "a"]);
    }

    #[test]
    fn hub_analysis_and_pending() {
        let c = parse_analyze(
            r#"{"tokens":271,"known":200,"knownViaDictForm":88,"learning":27,"unknown":44,"coverage":0.738}"#,
        )
        .unwrap();
        assert_eq!(c.percent(), Some(73));
        assert_eq!(class_of(&c), Knowledge::Known);
        assert_eq!(class_of(&Coverage { words: 1, new: 1, ..Default::default() }), Knowledge::New);
        assert_eq!(class_of(&Coverage::default()), Knowledge::Ignored);
        assert!(parse_analyze("{}").is_none());

        let mut s = Store::default();
        s.record("k", &lines(&["별 별", "사랑"]), &HashMap::new(), 0);
        assert_eq!(s.songs["k"].text(), "별\n별\n사랑\n");
        let w = words(&[("사랑", 99)]);
        s.refresh(&w, 5);
        assert_eq!(s.hub_pending(10).len(), 1);
        let v = Store::version(&w, 5);
        s.set_hub("k", "old/5", c);
        assert!(!s.songs["k"].hub, "a count for an older word list is dropped");
        s.set_hub("k", &v, c);
        assert!(s.hub_pending(10).is_empty());
        assert_eq!(s.coverage("k"), Some(c));
        assert!(!s.refresh(&w, 5), "hub count kept while Lute is unchanged");
        let w2 = Words { version: "v2".into(), ..w };
        assert!(s.refresh(&w2, 5));
        assert_eq!(s.hub_pending(10).len(), 1, "counted again after Lute changed");
    }

    #[test]
    fn resolve_answer_to_lemmas() {
        let body = r#"{"version":"x","lines":[[{"s":0,"e":3,"w":"골랐다는","status":3,"lemma":"고르다","how":"lemma"},
            {"s":4,"e":6,"w":"노래","status":99,"lemma":null,"how":"exact"}]],"coverage":0.5}"#;
        let m = parse_resolve(body).unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!(m["골랐다는"], "고르다");
        assert!(parse_resolve("<html>").is_none());
    }
}
