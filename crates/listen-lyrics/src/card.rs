//! "About this song" cards: a few facts about the song from Gemini with Google Search grounding,
//! each with the web sources it rests on, plus the credits (writers, composers, producers) from
//! MusicBrainz.
//!
//! - Only grounded facts survive: a fact line without a citation is dropped, and a card without
//!   any sourced fact is no card ([`parse_facts`]). Gemini's citations are byte ranges of the
//!   reply text, so a fact owns every citation that overlaps its line.
//! - Credits come from the MusicBrainz recording (its MBID when the server knows it, else a search
//!   by title and artist) and its works; their source is the recording's MusicBrainz page.
//! - Cards are cached per song and language ([`CardCache`]), and refetched after [`MAX_AGE_SECS`].

use crate::gemini::{AiError, fnv, post_reply};
use listen_core::text::fold;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A cached card older than this is fetched again (citation links do not live forever).
pub const MAX_AGE_SECS: i64 = 30 * 86_400;
/// Facts asked for and kept.
pub const MAX_FACTS: usize = 5;
/// Sources kept per fact.
pub const MAX_SOURCES: usize = 3;

const MB: &str = "https://musicbrainz.org/ws/2";

/// What the card is about.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SongInfo {
    pub title: String,
    pub artists: Vec<String>,
    pub album: Option<String>,
    pub year: Option<u32>,
    /// MusicBrainz recording id, when the server knows it.
    pub recording_mbid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    /// The site ("wikipedia.org").
    pub title: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fact {
    pub text: String,
    /// Never empty: unsourced facts are dropped.
    pub sources: Vec<Source>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Credit {
    /// English role name ("Composer"); the apps translate it.
    pub role: String,
    pub names: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Credits {
    pub credits: Vec<Credit>,
    /// The recording's MusicBrainz page (the credits' source).
    pub url: Option<String>,
}

/// One song's card in one language.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Card {
    pub facts: Vec<Fact>,
    /// What Gemini searched for (shown as Google Search links).
    #[serde(default)]
    pub queries: Vec<String>,
    #[serde(default)]
    pub credits: Credits,
    /// Unix seconds when it was made.
    pub at: i64,
}

impl Card {
    /// Whether there is anything to show (sourced facts or sourced credits).
    pub fn is_empty(&self) -> bool {
        self.facts.is_empty() && (self.credits.credits.is_empty() || self.credits.url.is_none())
    }
}

// ---- Gemini -----------------------------------------------------------------------

/// The grounded request. `language` is the card language's English name ("German").
pub fn card_body(model: &str, language: &str, song: &SongInfo) -> Value {
    let mut about = format!("Song: \"{}\" by {}", song.title, song.artists.join(", "));
    if let Some(a) = song.album.as_deref().filter(|a| !a.is_empty()) {
        about.push_str(&format!(", from \"{a}\""));
    }
    if let Some(y) = song.year {
        about.push_str(&format!(" ({y})"));
    }
    let system = format!(
        "You write the \"About this song\" card of a music player. Search the web for this exact song. \
         Write 3 to {MAX_FACTS} short facts about it, each at most 220 characters: what the title and the lyrics \
         mean (for a Korean, Japanese or Chinese title also what the title says), how it was made, its release \
         and album or era, how it was received. State only what the search results support; no guesses, no \
         praise. One fact per line, each line starting with \"- \". No introduction, no closing line, no \
         markdown. Write in {language}. If you find nothing reliable about this exact song, answer only NONE."
    );
    json!({
        "model": model,
        "system_instruction": system,
        "input": about,
        "tools": [{ "type": "google_search" }],
        "generation_config": { "thinking_level": "minimal" },
        "store": false,
    })
}

/// The sourced facts and the search queries of a grounded reply.
pub fn parse_facts(reply: &Value) -> (Vec<Fact>, Vec<String>) {
    let steps = reply["steps"].as_array().cloned().unwrap_or_default();
    let mut queries: Vec<String> = Vec::new();
    for s in steps.iter().filter(|s| s["type"] == "google_search_call") {
        for q in s["arguments"]["queries"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            if !queries.iter().any(|x| x == q) {
                queries.push(q.to_string());
            }
        }
    }
    let mut facts = Vec::new();
    let texts = steps
        .iter()
        .filter(|s| s["type"].as_str().is_none_or(|t| t == "model_output"))
        .flat_map(|s| s["content"].as_array().cloned().unwrap_or_default())
        .filter(|c| c["type"] == "text");
    for c in texts {
        let text = c["text"].as_str().unwrap_or_default();
        let notes: Vec<(usize, usize, Source)> = c["annotations"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|a| a["type"] == "url_citation")
            .filter_map(|a| {
                let url =
                    a["url"].as_str().filter(|u| u.starts_with("https://") || u.starts_with("http://"))?;
                let title = a["title"].as_str().filter(|t| !t.is_empty()).unwrap_or(url);
                Some((
                    a["start_index"].as_u64().unwrap_or(0) as usize,
                    a["end_index"].as_u64()? as usize,
                    Source { title: title.to_string(), url: url.to_string() },
                ))
            })
            .collect();
        // Byte ranges of the lines.
        let mut start = 0;
        for line in text.split_inclusive('\n') {
            let end = start + line.len();
            if let Some(fact) = fact_text(line) {
                let mut sources: Vec<Source> = Vec::new();
                for (a, b, src) in &notes {
                    if *a < end && *b > start && !sources.iter().any(|s| s.url == src.url) {
                        sources.push(src.clone());
                    }
                }
                sources.truncate(MAX_SOURCES);
                if !sources.is_empty() && facts.len() < MAX_FACTS {
                    facts.push(Fact { text: fact, sources });
                }
            }
            start = end;
        }
    }
    (facts, queries)
}

/// A fact line without its bullet and markdown; `None` for empty lines, intros and "NONE".
fn fact_text(line: &str) -> Option<String> {
    let t = line.trim();
    let body =
        t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")).or_else(|| t.strip_prefix("• ")).or_else(
            || {
                let digits = t.find(|c: char| !c.is_ascii_digit())?;
                (digits > 0).then(|| t[digits..].strip_prefix(". ")).flatten()
            },
        )?;
    let clean = body.replace("**", "").replace('*', "").trim().to_string();
    (clean.chars().count() >= 12 && clean != "NONE").then_some(clean)
}

/// Attempts per card: Gemini leaves out the citations of a grounded reply about every second
/// time, and an unsourced reply shows nothing, so it is asked again.
pub const ATTEMPTS: usize = 3;

/// Asks Gemini for the song's facts (`language`: English name of the card language).
pub async fn facts(
    http: &reqwest::Client,
    key: &str,
    model: &str,
    language: &str,
    song: &SongInfo,
) -> Result<(Vec<Fact>, Vec<String>), AiError> {
    let started = std::time::Instant::now();
    let body = card_body(model, language, song);
    let mut out = (Vec::new(), Vec::new());
    for attempt in 1..=ATTEMPTS {
        let reply = post_reply(http, key, &body, Duration::from_secs(60)).await?;
        out = parse_facts(&reply);
        let none = crate::gemini::reply_text(&reply).trim() == "NONE";
        tracing::info!(
            "Gemini song card ({model}, {language}) attempt {attempt} after {:.1}s: {} sourced facts{}",
            started.elapsed().as_secs_f32(),
            out.0.len(),
            if none { " (nothing found)" } else { "" }
        );
        if !out.0.is_empty() || none {
            break;
        }
    }
    Ok(out)
}

// ---- MusicBrainz ------------------------------------------------------------------

fn lucene(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            let esc = "+-&|!(){}[]^\"~*?:\\/".contains(c);
            esc.then_some('\\').into_iter().chain(std::iter::once(c))
        })
        .collect()
}

/// The recording search for title and first artist.
pub fn search_url(song: &SongInfo) -> String {
    let artist = song.artists.first().map(String::as_str).unwrap_or_default();
    let q = format!("recording:\"{}\" AND artist:\"{}\"", lucene(&song.title), lucene(artist));
    let mut u = url::Url::parse(&format!("{MB}/recording")).expect("static url");
    u.query_pairs_mut().append_pair("query", &q).append_pair("fmt", "json").append_pair("limit", "5");
    u.into()
}

/// The matching search hits, best first: score ≥ 90 and the same title (letters and digits, any
/// case). MusicBrainz often has several copies of a recording, only some of them with credits.
pub fn pick_recordings(search: &Value, song: &SongInfo) -> Vec<String> {
    let want = fold(&song.title);
    search["recordings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["score"].as_u64().unwrap_or(0) >= 90)
        .filter(|r| r["title"].as_str().is_some_and(|t| fold(t) == want))
        .filter_map(|r| r["id"].as_str().map(str::to_string))
        .collect()
}

/// Recordings looked up per song at most (one request per second).
pub const MAX_LOOKUPS: usize = 3;

pub fn lookup_url(mbid: &str) -> String {
    format!("{MB}/recording/{mbid}?inc=artist-rels+work-rels+work-level-rels&fmt=json")
}

/// Roles shown, in this order: (MusicBrainz relation type, label).
const ROLES: [(&str, &str); 7] = [
    ("writer", "Writer"),
    ("composer", "Composer"),
    ("lyricist", "Lyricist"),
    ("arranger", "Arranger"),
    ("producer", "Producer"),
    ("mix", "Mixing"),
    ("engineer", "Engineer"),
];

/// The credits of a recording lookup (recording and work relations).
pub fn parse_credits(rec: &Value) -> Credits {
    let mut found: Vec<(&str, String)> = Vec::new();
    let mut add = |rels: &Value| {
        for r in rels.as_array().into_iter().flatten() {
            let (Some(t), Some(name)) = (r["type"].as_str(), r["artist"]["name"].as_str()) else { continue };
            if ROLES.iter().any(|(k, _)| *k == t) && !found.iter().any(|(k, n)| *k == t && n == name) {
                let t = ROLES.iter().find(|(k, _)| *k == t).map(|(k, _)| *k).unwrap_or_default();
                found.push((t, name.to_string()));
            }
        }
    };
    add(&rec["relations"]);
    for r in rec["relations"].as_array().into_iter().flatten() {
        if r["work"].is_object() {
            add(&r["work"]["relations"]);
        }
    }
    let credits = ROLES
        .iter()
        .filter_map(|(k, label)| {
            let names: Vec<String> = found.iter().filter(|(t, _)| t == k).map(|(_, n)| n.clone()).collect();
            (!names.is_empty()).then(|| Credit { role: (*label).to_string(), names })
        })
        .collect();
    let url = rec["id"].as_str().map(|id| format!("https://musicbrainz.org/recording/{id}"));
    Credits { credits, url }
}

async fn get_json(http: &reqwest::Client, url: &str) -> Result<Value, String> {
    let resp = http
        .get(url)
        .header("User-Agent", listen_core::MUSICBRAINZ_USER_AGENT)
        .header("Accept", "application/json")
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| e.without_url().to_string())?;
    if !resp.status().is_success() {
        return Err(format!("MusicBrainz HTTP {}", resp.status().as_u16()));
    }
    resp.json().await.map_err(|e| e.without_url().to_string())
}

/// The song's credits from MusicBrainz (empty when no recording with credits is found).
pub async fn credits(http: &reqwest::Client, song: &SongInfo) -> Result<Credits, String> {
    // `searched`: a search request just went out (an empty MBID counts as none).
    let (ids, searched) = match song.recording_mbid.clone().filter(|m| !m.is_empty()) {
        Some(id) => (vec![id], false),
        None => (pick_recordings(&get_json(http, &search_url(song)).await?, song), true),
    };
    let mut first = Credits::default();
    for (i, id) in ids.iter().take(MAX_LOOKUPS).enumerate() {
        if i > 0 || searched {
            // MusicBrainz allows one request per second: wait after every request of this card.
            tokio::time::sleep(Duration::from_millis(1100)).await;
        }
        let c = parse_credits(&get_json(http, &lookup_url(id)).await?);
        if !c.credits.is_empty() {
            return Ok(c);
        }
        if i == 0 {
            first = c;
        }
    }
    Ok(first)
}

// ---- cache ------------------------------------------------------------------------

/// Cards on disk: one file per song and language.
pub struct CardCache;

impl CardCache {
    pub fn path(dir: &Path, song_key: &str, lang: &str) -> PathBuf {
        dir.join(format!("{:016x}-{lang}.json", fnv(song_key)))
    }

    /// The cached card, unless missing or older than [`MAX_AGE_SECS`] at `now`.
    pub fn load(dir: &Path, song_key: &str, lang: &str, now: i64) -> Option<Card> {
        let card: Card =
            serde_json::from_slice(&std::fs::read(Self::path(dir, song_key, lang)).ok()?).ok()?;
        (now - card.at < MAX_AGE_SECS).then_some(card)
    }

    pub fn save(dir: &Path, song_key: &str, lang: &str, card: &Card) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let bytes = serde_json::to_vec(card).map_err(std::io::Error::other)?;
        listen_core::files::write_atomic(&Self::path(dir, song_key, lang), &bytes)
    }
}

/// Builds the whole card: facts (Gemini) and credits (MusicBrainz), side by side. A failing
/// half leaves its part empty; both failing is an error.
pub async fn card(
    http: &reqwest::Client,
    key: &str,
    model: &str,
    language: &str,
    song: &SongInfo,
    now: i64,
) -> Result<Card, String> {
    let (f, c) = tokio::join!(facts(http, key, model, language, song), credits(http, song));
    if let Err(e) = &c {
        tracing::warn!("MusicBrainz credits: {e}");
    }
    let (facts, queries) = match f {
        Ok(v) => v,
        Err(e) if c.as_ref().is_ok_and(|c| !c.credits.is_empty()) => {
            tracing::warn!("Gemini song card: {e}");
            Default::default()
        }
        Err(e) => return Err(e.to_string()),
    };
    Ok(Card { facts, queries, credits: c.unwrap_or_default(), at: now })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song() -> SongInfo {
        SongInfo {
            title: "Love wins all".into(),
            artists: vec!["IU".into()],
            album: Some("The Winning".into()),
            year: Some(2024),
            recording_mbid: None,
        }
    }

    #[test]
    fn the_request_is_grounded_and_names_the_song() {
        let b = card_body("m", "German", &song());
        assert_eq!(b["tools"][0]["type"], "google_search");
        let input = b["input"].as_str().unwrap();
        assert!(
            input.contains("\"Love wins all\" by IU")
                && input.contains("The Winning")
                && input.contains("2024")
        );
        assert!(b["system_instruction"].as_str().unwrap().contains("Write in German"));
    }

    #[test]
    fn facts_keep_the_citations_of_their_line_and_unsourced_lines_go() {
        // Byte offsets: "ü" is two bytes, so the second line starts at 26 bytes, not 25 chars.
        let text = "- Erster Fakt über Liebe.\n- Zweiter Fakt ohne Quelle.\n- **Dritter** Fakt mit Quelle.\n";
        let l1 = "- Erster Fakt über Liebe.\n".len();
        let l2 = l1 + "- Zweiter Fakt ohne Quelle.\n".len();
        let reply = json!({"steps": [
            {"type": "google_search_call", "arguments": {"queries": ["love wins all iu", "love wins all iu"]}},
            {"type": "model_output", "content": [{"type": "text", "text": text, "annotations": [
                {"type": "url_citation", "start_index": 2, "end_index": l1 - 2, "url": "https://a.example/1", "title": "a.example"},
                {"type": "url_citation", "start_index": 4, "end_index": 10, "url": "https://a.example/1", "title": "a.example"},
                {"type": "url_citation", "start_index": l2 + 3, "end_index": l2 + 12, "url": "https://b.example/2", "title": ""},
                {"type": "url_citation", "start_index": l2 + 3, "end_index": l2 + 12, "url": "javascript:x", "title": "bad"},
            ]}]}
        ]});
        let (facts, queries) = parse_facts(&reply);
        assert_eq!(queries, ["love wins all iu"]);
        assert_eq!(facts.len(), 2);
        assert_eq!(facts[0].text, "Erster Fakt über Liebe.");
        assert_eq!(
            facts[0].sources,
            [Source { title: "a.example".into(), url: "https://a.example/1".into() }]
        );
        assert_eq!(facts[1].text, "Dritter Fakt mit Quelle.");
        assert_eq!(facts[1].sources[0].title, "https://b.example/2", "no title: the url");
    }

    #[test]
    fn none_or_no_citations_is_no_card() {
        let reply =
            json!({"steps": [{"type": "model_output", "content": [{"type": "text", "text": "NONE"}]}]});
        assert!(parse_facts(&reply).0.is_empty());
        let reply = json!({"steps": [{"type": "model_output", "content": [
            {"type": "text", "text": "- A long fact without any source at all.\n"}]}]});
        assert!(parse_facts(&reply).0.is_empty());
        assert!(Card::default().is_empty());
        let credits_only = Card {
            credits: Credits {
                credits: vec![Credit { role: "Composer".into(), names: vec!["X".into()] }],
                url: Some("https://musicbrainz.org/recording/r".into()),
            },
            ..Card::default()
        };
        assert!(!credits_only.is_empty());
    }

    #[test]
    fn bullets_numbers_and_intros() {
        assert_eq!(
            fact_text("1. **Meaning:** love wins over hate").as_deref(),
            Some("Meaning: love wins over hate")
        );
        assert_eq!(fact_text("• Released in January 2024").as_deref(), Some("Released in January 2024"));
        assert_eq!(fact_text("Here are three facts about the song:"), None);
        assert_eq!(fact_text("- short"), None);
    }

    #[test]
    fn musicbrainz_search_pick_and_credits() {
        let u = search_url(&SongInfo {
            title: "Hype Boy (Remix)".into(),
            artists: vec!["NewJeans".into()],
            ..song()
        });
        assert!(u.starts_with("https://musicbrainz.org/ws/2/recording?query="));
        assert!(u.contains("fmt=json"));
        let hits = json!({"recordings": [
            {"id": "low", "score": 70, "title": "Love wins all"},
            {"id": "other", "score": 100, "title": "Love wins"},
            {"id": "good", "score": 95, "title": "Love Wins All"},
        ]});
        assert_eq!(pick_recordings(&hits, &song()), ["good"]);
        let rec = json!({"id": "good", "relations": [
            {"type": "producer", "artist": {"name": "Seo Dong-hwan"}},
            {"type": "vocal", "artist": {"name": "IU"}},
            {"type": "performance", "work": {"title": "Love wins all", "relations": [
                {"type": "lyricist", "artist": {"name": "IU"}},
                {"type": "composer", "artist": {"name": "Seo Dong-hwan"}},
                {"type": "composer", "artist": {"name": "Seo Dong-hwan"}},
            ]}},
        ]});
        let c = parse_credits(&rec);
        let roles: Vec<(&str, usize)> = c.credits.iter().map(|c| (c.role.as_str(), c.names.len())).collect();
        assert_eq!(roles, [("Composer", 1), ("Lyricist", 1), ("Producer", 1)]);
        assert_eq!(c.url.as_deref(), Some("https://musicbrainz.org/recording/good"));
        assert!(lookup_url("good").contains("work-level-rels"));
    }

    #[test]
    fn the_cache_expires() {
        let dir = std::env::temp_dir().join(format!("listen-cards-{}", std::process::id()));
        let card = Card {
            facts: vec![Fact {
                text: "x".into(),
                sources: vec![Source { title: "t".into(), url: "https://u".into() }],
            }],
            at: 1000,
            ..Card::default()
        };
        CardCache::save(&dir, "s:1", "de", &card).unwrap();
        assert_eq!(CardCache::load(&dir, "s:1", "de", 2000), Some(card));
        assert_eq!(CardCache::load(&dir, "s:1", "en", 2000), None);
        assert_eq!(CardCache::load(&dir, "s:1", "de", 1000 + MAX_AGE_SECS), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    #[ignore = "live: needs LISTEN_GEMINI_API_KEY and network"]
    async fn live_card() {
        let Ok(key) = std::env::var("LISTEN_GEMINI_API_KEY") else {
            eprintln!("SKIPPED live_card: LISTEN_GEMINI_API_KEY not set");
            return;
        };
        let model = std::env::var("LISTEN_GEMINI_MODEL").unwrap_or_else(|_| "gemini-3.5-flash-lite".into());
        let http = reqwest::Client::new();
        let c = card(&http, &key, &model, "German", &song(), 0).await.unwrap();
        println!("{}", serde_json::to_string_pretty(&c).unwrap());
        assert!(!c.facts.is_empty());
        assert!(c.facts.iter().all(|f| !f.sources.is_empty()));
    }
}
