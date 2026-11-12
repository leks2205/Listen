//! A vocab-hub client: which Korean words the listener knows (Lute is the source of truth behind
//! the hub). Read only: Listen never writes to Lute.
//!
//! Hub route: `GET /api/words` → `{version, terms: {word: status}}` (single-token Lute terms,
//! status 1-5 learning, 99 well known, 98 ignored).

use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;

/// The `known_min` every Listen view passes: Lute status 5 ("learned") and up counts as known. One
/// value for all of them, or the Learnable mix, the coverage line and the word colours disagree.
pub const DEFAULT_KNOWN_MIN: i32 = 5;

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
    /// From a Lute status; `known_min` = lowest learning status that counts as known
    /// ([`DEFAULT_KNOWN_MIN`] in the app).
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
    fn needs_an_http_address() {
        assert_eq!(
            url(" https://hub.example/ ", "/api/words").as_deref(),
            Some("https://hub.example/api/words")
        );
        assert!(url("", "/api/words").is_none());
        assert!(url("hub.example", "/api/words").is_none());
    }
}
