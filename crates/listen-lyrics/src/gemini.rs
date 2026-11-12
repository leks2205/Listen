//! Lyric translation and word glosses with Gemini (the AI DJ's key and text model).
//!
//! - Translation: one request per song and language. Only lines with non-Latin letters are sent
//!   (K-pop mixes in whole English lines; those stay as they are). The reply is keyed by line
//!   number, so it always lays back over the original line for line, timing included.
//! - Glosses: one request per song and language for every distinct word (Hangul run, the way Lute
//!   cuts Korean words): dictionary form, part of speech and a short meaning in context.
//! - Both are cached per song in a small JSON file ([`SongCache`]); a lyrics change (different
//!   lines) invalidates the entry.
//!
//! No romanisation. The key travels in the `x-goog-api-key` header and is never logged.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Shown in the provider list of the translation settings.
pub const PROVIDER: &str = "Gemini";

const ENDPOINT: &str = "https://generativelanguage.googleapis.com/v1beta/interactions";
/// Upper bound for distinct words in one gloss request (a long song has ~150).
pub const MAX_GLOSS_WORDS: usize = 400;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum AiError {
    #[error("no Gemini API key set")]
    NoKey,
    #[error("Gemini quota exhausted")]
    Quota,
    #[error("the Gemini API key was rejected")]
    Auth,
    #[error("Gemini: {0}")]
    Api(String),
    #[error("network: {0}")]
    Network(String),
}

/// One glossed word.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gloss {
    /// Dictionary form (기본형): 골랐다는 → 고르다, 사람들은 → 사람.
    pub lemma: String,
    /// Part of speech in the gloss language ("verb", "Nomen").
    #[serde(default)]
    pub pos: String,
    /// Short meaning in context.
    pub meaning: String,
}

// ---- words ------------------------------------------------------------------------

/// Hangul syllables and jamo (Lute's word characters for Korean): U+AC00-D7A3, U+1100-11C2 and
/// U+3131-318E, no jamo extensions, so words split where Lute splits them.
/// Its siblings have other ranges on purpose (pick by range, do not merge):
/// `listen_lyrics::furigana::is_kanji`, `listen_lyrics::vocal::is_syllabic`, `listen_lastfm::names::has_cjk`,
/// `listen_core::substitute::script_of`, the app's `ui::is_hangul`.
pub fn is_hangul(c: char) -> bool {
    ('가'..='힣').contains(&c) || ('ᄀ'..='ᇂ').contains(&c) || ('ㄱ'..='ㆎ').contains(&c)
}

/// A piece of a lyric line: a Hangul word or the text between words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece<'a> {
    pub text: &'a str,
    pub word: bool,
}

/// Cuts a line into Hangul words and the rest (spaces, punctuation, Latin), in order.
pub fn pieces(line: &str) -> Vec<Piece<'_>> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut in_word = None;
    for (i, c) in line.char_indices() {
        let w = is_hangul(c);
        match in_word {
            Some(prev) if prev != w => {
                out.push(Piece { text: &line[start..i], word: prev });
                start = i;
            }
            _ => {}
        }
        in_word = Some(w);
    }
    if let Some(w) = in_word {
        out.push(Piece { text: &line[start..], word: w });
    }
    out
}

/// The distinct Hangul words of the lines, in order of first appearance.
pub fn words(lines: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for l in lines {
        for p in pieces(l).into_iter().filter(|p| p.word) {
            if seen.insert(p.text) {
                out.push(p.text.to_string());
            }
        }
    }
    out
}

/// Whether a line needs translating: it has letters and at least one is not Latin script.
/// Pure English (or other Latin-script) lines stay untouched.
pub fn needs_translation(line: &str) -> bool {
    line.chars().any(|c| c.is_alphabetic() && !is_latin(c))
}

fn is_latin(c: char) -> bool {
    c.is_ascii_alphabetic() || matches!(c, '\u{00C0}'..='\u{024F}' | '\u{1E00}'..='\u{1EFF}')
}

/// Whether any line has Hangul (glosses are offered only then).
pub fn has_hangul(lines: &[String]) -> bool {
    lines.iter().any(|l| l.chars().any(is_hangul))
}

// ---- requests ---------------------------------------------------------------------

fn interaction(model: &str, system: String, input: String, schema: Value) -> Value {
    json!({
        "model": model,
        "system_instruction": system,
        "input": input,
        "response_format": { "type": "text", "mime_type": "application/json", "schema": schema },
        "generation_config": { "thinking_level": "minimal" },
        "store": false,
    })
}

/// The translation request for the lines that need it (`None`: nothing to translate).
/// `language` is the target language's English name ("German").
pub fn translation_body(model: &str, language: &str, lines: &[String]) -> Option<Value> {
    let numbered: Vec<String> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| needs_translation(l))
        .map(|(i, l)| format!("{i}\t{}", l.trim()))
        .collect();
    if numbered.is_empty() {
        return None;
    }
    let system = format!(
        "You translate song lyrics (mostly K-pop) into {language} for a listener who is learning the \
         language of the song. Translate each numbered line on its own, faithfully and naturally, keeping \
         its meaning and tone; use the other lines only as context. English words or phrases inside a \
         line stay in English. Never add romanisation, notes or explanations. Return every line number \
         you were given exactly once."
    );
    let input = format!("Lines (number, tab, text):\n{}", numbered.join("\n"));
    let schema = json!({
        "type": "object",
        "properties": {
            "lines": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": { "i": { "type": "integer" }, "t": { "type": "string" } },
                    "required": ["i", "t"],
                },
            },
        },
        "required": ["lines"],
    });
    Some(interaction(model, system, input, schema))
}

/// Lays the reply back over the lines: one entry per original line; `None` for lines that were
/// not sent (English), missing from the reply, empty, or unchanged.
pub fn parse_translation(text: &str, lines: &[String]) -> Result<Vec<Option<String>>, AiError> {
    #[derive(Deserialize)]
    struct Line {
        i: i64,
        t: String,
    }
    #[derive(Deserialize)]
    struct Reply {
        lines: Vec<Line>,
    }
    let reply: Reply =
        serde_json::from_str(text).map_err(|e| AiError::Api(format!("unreadable translation: {e}")))?;
    let mut out = vec![None; lines.len()];
    for l in reply.lines {
        let Some(orig) = usize::try_from(l.i).ok().and_then(|i| lines.get(i).map(|o| (i, o))) else {
            continue;
        };
        let t = l.t.trim();
        if needs_translation(orig.1) && !t.is_empty() && t != orig.1.trim() {
            out[orig.0] = Some(t.to_string());
        }
    }
    Ok(out)
}

/// The gloss request for the song's words (`None` when it has no Hangul words).
/// `language` is the gloss language's English name ("German", "English").
pub fn gloss_body(model: &str, language: &str, lines: &[String]) -> Option<Value> {
    let words = words(lines);
    if words.is_empty() {
        return None;
    }
    let words = &words[..words.len().min(MAX_GLOSS_WORDS)];
    let system = format!(
        "You are a Korean dictionary for a learner who reads {language}. For every Korean word form you \
         are given (exactly as it appears in the song lyrics below), return: w = the form unchanged; \
         lemma = its dictionary form in Hangul (verbs and adjectives ending in 다, nouns without \
         particles: 골랐다는 -> 고르다, 사람들은 -> 사람, 공부했어요 -> 공부하다); pos = the part of speech \
         in {language}, one word; meaning = a short {language} meaning that fits this song (at most 6 \
         words). Never add romanisation. Return every given form exactly once."
    );
    let input = format!(
        "Lyrics for context:\n{}\n\nWord forms, one per line:\n{}",
        lines.iter().map(|l| l.trim()).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n"),
        words.join("\n")
    );
    let schema = json!({
        "type": "object",
        "properties": {
            "words": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "w": { "type": "string" },
                        "lemma": { "type": "string" },
                        "pos": { "type": "string" },
                        "meaning": { "type": "string" },
                    },
                    "required": ["w", "lemma", "meaning"],
                },
            },
        },
        "required": ["words"],
    });
    Some(interaction(model, system, input, schema))
}

/// The glossary out of the reply: word form → gloss (forms that are not in the lyrics are dropped).
pub fn parse_glosses(text: &str, lines: &[String]) -> Result<BTreeMap<String, Gloss>, AiError> {
    #[derive(Deserialize)]
    struct Word {
        w: String,
        #[serde(flatten)]
        gloss: Gloss,
    }
    #[derive(Deserialize)]
    struct Reply {
        words: Vec<Word>,
    }
    let reply: Reply =
        serde_json::from_str(text).map_err(|e| AiError::Api(format!("unreadable glosses: {e}")))?;
    let known: std::collections::HashSet<String> = words(lines).into_iter().collect();
    Ok(reply
        .words
        .into_iter()
        .filter(|w| known.contains(w.w.trim()) && !w.gloss.meaning.trim().is_empty())
        .map(|w| {
            let mut g = w.gloss;
            g.lemma = g.lemma.trim().to_string();
            if g.lemma.is_empty() {
                g.lemma = w.w.trim().to_string();
            }
            g.meaning = g.meaning.trim().to_string();
            g.pos = g.pos.trim().to_string();
            (w.w.trim().to_string(), g)
        })
        .collect())
}

/// The model's text output of an Interactions reply.
pub fn reply_text(reply: &Value) -> String {
    reply["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| s["type"].as_str().is_none_or(|t| t == "model_output"))
        .flat_map(|s| s["content"].as_array().into_iter().flatten())
        .filter(|c| c["type"] == "text")
        .filter_map(|c| c["text"].as_str())
        .collect()
}

fn classify(status: u16, body: &str) -> AiError {
    if status == 429 || body.contains("RESOURCE_EXHAUSTED") {
        return AiError::Quota;
    }
    if status == 401 || status == 403 || body.contains("API_KEY_INVALID") {
        return AiError::Auth;
    }
    let msg: String = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
        .unwrap_or_else(|| body.to_string())
        .chars()
        .take(200)
        .collect();
    AiError::Api(format!("HTTP {status}: {msg}"))
}

async fn post(http: &reqwest::Client, key: &str, body: &Value, timeout: Duration) -> Result<String, AiError> {
    post_reply(http, key, body, timeout).await.map(|r| reply_text(&r))
}

/// Sends an Interactions request; the whole completed reply (steps with annotations).
pub(crate) async fn post_reply(
    http: &reqwest::Client,
    key: &str,
    body: &Value,
    timeout: Duration,
) -> Result<Value, AiError> {
    let key = key.trim();
    if key.is_empty() {
        return Err(AiError::NoKey);
    }
    let resp = http
        .post(ENDPOINT)
        .header("x-goog-api-key", key)
        .timeout(timeout)
        .json(body)
        .send()
        .await
        .map_err(|e| AiError::Network(e.without_url().to_string()))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| AiError::Network(e.without_url().to_string()))?;
    if !status.is_success() {
        return Err(classify(status.as_u16(), &text));
    }
    let reply: Value =
        serde_json::from_str(&text).map_err(|e| AiError::Api(format!("unreadable reply: {e}")))?;
    if let Some(s) = reply["status"].as_str().filter(|s| *s != "completed") {
        return Err(AiError::Api(format!("interaction {s}")));
    }
    Ok(reply)
}

/// Translates the lines into `language` (English name). One entry per line.
pub async fn translate(
    http: &reqwest::Client,
    key: &str,
    model: &str,
    language: &str,
    lines: &[String],
) -> Result<Vec<Option<String>>, AiError> {
    let Some(body) = translation_body(model, language, lines) else {
        return Ok(vec![None; lines.len()]);
    };
    let started = std::time::Instant::now();
    let text = post(http, key, &body, Duration::from_secs(45)).await?;
    let out = parse_translation(&text, lines)?;
    tracing::info!(
        "Gemini lyric translation ({model}, {language}) in {:.1}s: {} of {} lines",
        started.elapsed().as_secs_f32(),
        out.iter().filter(|l| l.is_some()).count(),
        lines.len()
    );
    Ok(out)
}

/// Glosses every Hangul word of the lines in `language` (English name).
pub async fn glosses(
    http: &reqwest::Client,
    key: &str,
    model: &str,
    language: &str,
    lines: &[String],
) -> Result<BTreeMap<String, Gloss>, AiError> {
    let Some(body) = gloss_body(model, language, lines) else {
        return Ok(BTreeMap::new());
    };
    let started = std::time::Instant::now();
    let text = post(http, key, &body, Duration::from_secs(60)).await?;
    let out = parse_glosses(&text, lines)?;
    tracing::info!(
        "Gemini lyric glosses ({model}, {language}) in {:.1}s: {} words",
        started.elapsed().as_secs_f32(),
        out.len()
    );
    Ok(out)
}

// ---- cache ------------------------------------------------------------------------

/// FNV-1a 64: a stable hash for cache keys and the lines fingerprint.
pub fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3))
}

/// Fingerprint of the lyric lines a cached result belongs to.
pub fn lines_key(lines: &[String]) -> String {
    format!("{:016x}", fnv(&lines.join("\n")))
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CachedTranslation {
    pub lines_key: String,
    pub lines: Vec<Option<String>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CachedGlosses {
    pub lines_key: String,
    pub words: BTreeMap<String, Gloss>,
}

/// Everything Gemini made for one song, by language code.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SongCache {
    #[serde(default)]
    pub translations: BTreeMap<String, CachedTranslation>,
    #[serde(default)]
    pub glosses: BTreeMap<String, CachedGlosses>,
}

impl SongCache {
    /// The cache file for a song (`song_key` = a stable song identity, e.g. "server:id").
    pub fn path(dir: &Path, song_key: &str) -> PathBuf {
        dir.join(format!("{:016x}.json", fnv(song_key)))
    }

    /// Loads the song's cache (empty when missing or unreadable).
    pub fn load(dir: &Path, song_key: &str) -> Self {
        std::fs::read(Self::path(dir, song_key))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    /// Writes the song's cache (atomically: temp file, then rename).
    pub fn save(&self, dir: &Path, song_key: &str) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let path = Self::path(dir, song_key);
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(self).map_err(std::io::Error::other)?)?;
        std::fs::rename(tmp, path)
    }

    /// The cached translation of these lines into `lang`, if any.
    pub fn translation(&self, lang: &str, lines: &[String]) -> Option<&Vec<Option<String>>> {
        self.translations
            .get(lang)
            .filter(|t| t.lines_key == lines_key(lines) && t.lines.len() == lines.len())
            .map(|t| &t.lines)
    }

    /// The cached glossary of these lines in `lang`, if any.
    pub fn glossary(&self, lang: &str, lines: &[String]) -> Option<&BTreeMap<String, Gloss>> {
        self.glosses.get(lang).filter(|g| g.lines_key == lines_key(lines)).map(|g| &g.words)
    }

    pub fn put_translation(&mut self, lang: &str, lines: &[String], out: Vec<Option<String>>) {
        self.translations.insert(lang.into(), CachedTranslation { lines_key: lines_key(lines), lines: out });
    }

    pub fn put_glosses(&mut self, lang: &str, lines: &[String], words: BTreeMap<String, Gloss>) {
        self.glosses.insert(lang.into(), CachedGlosses { lines_key: lines_key(lines), words });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn cuts_lines_into_hangul_words() {
        let p = pieces("너를 사랑해, baby 정말!");
        let words: Vec<_> = p.iter().filter(|p| p.word).map(|p| p.text).collect();
        assert_eq!(words, ["너를", "사랑해", "정말"]);
        assert_eq!(p.iter().map(|p| p.text).collect::<String>(), "너를 사랑해, baby 정말!");
        assert!(pieces("").is_empty());
        assert_eq!(words_of(&["보고 싶어 보고", "싶어 oh"]), ["보고", "싶어"]);
    }

    fn words_of(v: &[&str]) -> Vec<String> {
        words(&lines(v))
    }

    #[test]
    fn english_lines_are_not_sent() {
        assert!(needs_translation("너를 사랑해 baby"));
        assert!(needs_translation("君が好き"));
        assert!(!needs_translation("I got you, baby (oh-oh)"));
        assert!(!needs_translation("Café déjà vu"));
        assert!(!needs_translation("♪ 1, 2, 3"));
        assert!(!needs_translation(""));
    }

    #[test]
    fn translation_request_numbers_only_foreign_lines() {
        let l = lines(&["Yeah, yeah", "너를 사랑해", "", "Baby I love you", "보고 싶어 baby"]);
        let body = translation_body("m", "German", &l).unwrap();
        let input = body["input"].as_str().unwrap();
        assert_eq!(input.lines().skip(1).collect::<Vec<_>>(), ["1\t너를 사랑해", "4\t보고 싶어 baby"]);
        assert!(body["system_instruction"].as_str().unwrap().contains("German"));
        assert!(body["system_instruction"].as_str().unwrap().contains("Never add romanisation"));
        assert_eq!(body["response_format"]["mime_type"], "application/json");
        assert!(translation_body("m", "German", &lines(&["Only English", ""])).is_none());
    }

    #[test]
    fn translation_lays_back_line_for_line() {
        let l = lines(&["Yeah", "너를 사랑해", "", "보고 싶어 baby", "또"]);
        let reply = r#"{"lines":[{"i":3,"t":"Ich vermisse dich, baby"},{"i":1,"t":"Ich liebe dich"},
            {"i":0,"t":"Ja"},{"i":9,"t":"x"},{"i":-1,"t":"y"},{"i":4,"t":"  "}]}"#;
        let out = parse_translation(reply, &l).unwrap();
        assert_eq!(out.len(), l.len());
        assert_eq!(
            out,
            [None, Some("Ich liebe dich".into()), None, Some("Ich vermisse dich, baby".into()), None]
        );
        assert!(parse_translation("nope", &l).is_err());
    }

    #[test]
    fn gloss_request_and_reply() {
        let l = lines(&["내가 골랐다는 그 노래", "Yeah 노래"]);
        let body = gloss_body("m", "German", &l).unwrap();
        let input = body["input"].as_str().unwrap();
        assert!(input.ends_with("내가\n골랐다는\n그\n노래"), "{input}");
        assert!(gloss_body("m", "German", &lines(&["only english"])).is_none());
        let reply = r#"{"words":[
            {"w":"골랐다는","lemma":"고르다","pos":"Verb","meaning":"auswählen"},
            {"w":"노래","lemma":"","pos":"Nomen","meaning":" Lied "},
            {"w":"없는말","lemma":"x","pos":"","meaning":"erfunden"},
            {"w":"그","lemma":"그","pos":"","meaning":""}]}"#;
        let g = parse_glosses(reply, &l).unwrap();
        assert_eq!(g.len(), 2);
        assert_eq!(
            g["골랐다는"],
            Gloss { lemma: "고르다".into(), pos: "Verb".into(), meaning: "auswählen".into() }
        );
        assert_eq!(g["노래"].lemma, "노래");
        assert_eq!(g["노래"].meaning, "Lied");
    }

    #[test]
    fn reads_reply_text_and_errors() {
        let reply = json!({"status":"completed","steps":[
            {"type":"thought","content":[{"type":"text","text":"hmm"}]},
            {"type":"model_output","content":[{"type":"text","text":"{\"lines\":"},{"type":"text","text":"[]}"}]}]});
        assert_eq!(reply_text(&reply), "{\"lines\":[]}");
        assert_eq!(classify(429, "{}"), AiError::Quota);
        assert_eq!(classify(400, r#"{"error":{"details":[{"reason":"API_KEY_INVALID"}]}}"#), AiError::Auth);
        assert!(
            matches!(classify(500, r#"{"error":{"message":"boom"}}"#), AiError::Api(m) if m.contains("boom"))
        );
    }

    #[test]
    fn cache_round_trip_and_invalidation() {
        let dir = std::env::temp_dir().join(format!("listen-gemini-cache-{}", std::process::id()));
        let l = lines(&["너를 사랑해", "Yeah"]);
        let mut c = SongCache::load(&dir, "ytm:abc");
        assert_eq!(c, SongCache::default());
        c.put_translation("de", &l, vec![Some("Ich liebe dich".into()), None]);
        let mut g = BTreeMap::new();
        g.insert(
            "사랑해".into(),
            Gloss { lemma: "사랑하다".into(), pos: "Verb".into(), meaning: "lieben".into() },
        );
        c.put_glosses("de", &l, g);
        c.save(&dir, "ytm:abc").unwrap();
        let back = SongCache::load(&dir, "ytm:abc");
        assert_eq!(back.translation("de", &l).unwrap()[0].as_deref(), Some("Ich liebe dich"));
        assert!(back.translation("en", &l).is_none());
        assert_eq!(back.glossary("de", &l).unwrap()["사랑해"].lemma, "사랑하다");
        // Other lyrics for the same song: the cached results no longer apply.
        let changed = lines(&["너를 사랑해", "Yeah yeah"]);
        assert!(back.translation("de", &changed).is_none());
        assert!(back.glossary("de", &changed).is_none());
        assert!(SongCache::load(&dir, "ytm:other").translations.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Live check against Gemini: `LISTEN_GEMINI_API_KEY=… cargo test -p listen-lyrics live_gemini -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore = "live: Gemini, needs LISTEN_GEMINI_API_KEY"]
    async fn live_gemini() {
        let Ok(key) = std::env::var("LISTEN_GEMINI_API_KEY") else {
            eprintln!("SKIPPED live_gemini: LISTEN_GEMINI_API_KEY not set");
            return;
        };
        let model = std::env::var("LISTEN_GEMINI_MODEL").unwrap_or_else(|_| "gemini-3.5-flash-lite".into());
        let l = lines(&[
            "I got you, baby",
            "내가 골랐다는 그 노래를",
            "너와 함께 듣고 싶어",
            "",
            "Ooh, la-la-la",
            "사랑해 baby 매일 밤",
        ]);
        let http = reqwest::Client::new();
        let t = translate(&http, &key, &model, "German", &l).await.unwrap();
        println!("{t:#?}");
        assert_eq!(t.len(), l.len());
        assert!(t[0].is_none() && t[3].is_none() && t[4].is_none());
        assert!(t[1].is_some() && t[2].is_some() && t[5].is_some());
        let g = glosses(&http, &key, &model, "German", &l).await.unwrap();
        println!("{g:#?}");
        assert!(g.len() >= words(&l).len() - 1, "{} of {}", g.len(), words(&l).len());
    }
}
