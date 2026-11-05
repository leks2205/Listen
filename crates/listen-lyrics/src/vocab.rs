//! A vocab-hub client: which Korean words the listener knows (Lute is the source of truth behind
//! the hub) and "Add to Lute" for a word from the lyrics.
//!
//! Hub routes: `GET /api/words` → `{version, terms: {word: status}}` (single-token Lute terms,
//! status 1-5 learning, 99 well known, 98 ignored) and `POST /api/lute/term {text, translation,
//! status}` → 201 created / 409 exists (with its status) / 404 when the hub has no such route yet.

use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::time::Duration;

/// Lute's statuses as the lyrics show them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Knowledge {
    /// Not a Lute term yet (or status 0).
    New,
    /// Status 1 to 4 (with the default "known from 5").
    Learning(u8),
    /// Status 5 or 99.
    Known,
    /// Status 98.
    Ignored,
}

impl Knowledge {
    /// From a Lute status; `known_min` = lowest learning status that counts as known (5 by default).
    pub fn from_status(status: Option<i32>, known_min: i32) -> Self {
        match status {
            Some(99) => Self::Known,
            Some(98) => Self::Ignored,
            Some(s @ 1..=5) if s >= known_min => Self::Known,
            Some(s @ 1..=5) => Self::Learning(s as u8),
            _ => Self::New,
        }
    }
}

/// Words and their Lute status, as the hub last answered.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Words {
    pub version: String,
    pub terms: HashMap<String, i32>,
}

impl Words {
    /// The status of a word form, else of its dictionary form (from a gloss).
    pub fn status(&self, form: &str, lemma: Option<&str>) -> Option<i32> {
        self.terms.get(form).or_else(|| lemma.and_then(|l| self.terms.get(l))).copied()
    }
}

/// What "Add to Lute" did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Added {
    Created,
    /// Lute already has the term (with this status); nothing was changed.
    Exists(i32),
    /// The hub has no term route (older hub).
    Unsupported,
}

fn url(base: &str, path: &str) -> Option<String> {
    let base = base.trim().trim_end_matches('/');
    (base.starts_with("http://") || base.starts_with("https://")).then(|| format!("{base}{path}"))
}

/// Parses `GET /api/words`.
pub fn parse_words(body: &str) -> Option<Words> {
    #[derive(Deserialize)]
    struct Reply {
        #[serde(default)]
        version: String,
        terms: HashMap<String, i32>,
    }
    let r: Reply = serde_json::from_str(body).ok()?;
    Some(Words { version: r.version, terms: r.terms })
}

/// Loads the word list from the hub at `base` (e.g. `https://vocab.example`).
pub async fn words(http: &reqwest::Client, base: &str) -> Result<Words, String> {
    let url = url(base, "/api/words").ok_or("no vocab-hub address set")?;
    let resp = http
        .get(url)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| e.without_url().to_string())?;
    if !resp.status().is_success() {
        return Err(format!("vocab-hub answered {}", resp.status()));
    }
    parse_words(&resp.text().await.map_err(|e| e.without_url().to_string())?)
        .ok_or_else(|| "unreadable word list".to_string())
}

/// The body of an "Add to Lute" request: new terms start at status 1 (learning).
pub fn add_body(text: &str, translation: &str) -> serde_json::Value {
    json!({ "text": text.trim(), "translation": translation.trim(), "status": 1, "source": "listen" })
}

/// Maps the hub's answer to [`Added`].
pub fn parse_added(status: u16, body: &str) -> Result<Added, String> {
    match status {
        200 | 201 => Ok(Added::Created),
        409 => {
            let v: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
            Ok(Added::Exists(v["status"].as_i64().unwrap_or(0) as i32))
        }
        404 | 405 => Ok(Added::Unsupported),
        s => Err(format!("vocab-hub answered {s}")),
    }
}

/// Adds a word (dictionary form) with its meaning to Lute through the hub.
pub async fn add_term(
    http: &reqwest::Client,
    base: &str,
    text: &str,
    translation: &str,
) -> Result<Added, String> {
    let url = url(base, "/api/lute/term").ok_or("no vocab-hub address set")?;
    let resp = http
        .post(url)
        .timeout(Duration::from_secs(15))
        .json(&add_body(text, translation))
        .send()
        .await
        .map_err(|e| e.without_url().to_string())?;
    let status = resp.status().as_u16();
    let body = resp.text().await.unwrap_or_default();
    let out = parse_added(status, &body);
    tracing::info!("Add to Lute “{}”: {out:?}", text.trim());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_map_to_knowledge() {
        assert_eq!(Knowledge::from_status(Some(99), 5), Knowledge::Known);
        assert_eq!(Knowledge::from_status(Some(5), 5), Knowledge::Known);
        assert_eq!(Knowledge::from_status(Some(4), 5), Knowledge::Learning(4));
        assert_eq!(Knowledge::from_status(Some(4), 4), Knowledge::Known);
        assert_eq!(Knowledge::from_status(Some(98), 5), Knowledge::Ignored);
        assert_eq!(Knowledge::from_status(Some(0), 5), Knowledge::New);
        assert_eq!(Knowledge::from_status(None, 5), Knowledge::New);
    }

    #[test]
    fn word_list_and_lemma_fallback() {
        let w = parse_words(r#"{"version":"f9b6","count":2,"terms":{"고르다":3,"노래":99}}"#).unwrap();
        assert_eq!(w.version, "f9b6");
        assert_eq!(w.status("노래", None), Some(99));
        assert_eq!(w.status("골랐다는", Some("고르다")), Some(3));
        assert_eq!(w.status("골랐다는", None), None);
        assert!(parse_words("<html>").is_none());
    }

    #[test]
    fn add_requests_and_answers() {
        assert_eq!(add_body(" 고르다 ", "auswählen")["text"], "고르다");
        assert_eq!(add_body("고르다", "x")["status"], 1);
        assert_eq!(parse_added(201, "{}"), Ok(Added::Created));
        assert_eq!(parse_added(409, r#"{"exists":true,"status":5}"#), Ok(Added::Exists(5)));
        assert_eq!(parse_added(404, ""), Ok(Added::Unsupported));
        assert!(parse_added(500, "").is_err());
    }

    #[test]
    fn needs_an_http_address() {
        assert_eq!(
            url(" https://hub.example/ ", "/api/words").as_deref(),
            Some("https://hub.example/api/words")
        );
        assert!(url("", "/api/words").is_none());
        assert!(url("hub.example", "/api/words").is_none());
    }
}
