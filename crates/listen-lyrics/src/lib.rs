//! Lyrics: LRC parsing (plain, synced, word-synced, translations), providers (lrclib.net, NetEase,
//! Genius, SimpMusic) with ranking and rate limits, translation, romaji, RTL detection, export.

pub mod export;
pub mod fetch;
pub mod furigana;
mod gate;
mod lrc;
pub mod providers;
pub mod rank;
pub mod romaji;
pub mod rtl;
pub mod translate;

pub use lrc::{BREAK, active_line, has_translation, has_word_sync, merge_translation, parse_lrc};

use listen_core::model::{LyricLine, Lyrics, Song};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LrclibResponse {
    synced_lyrics: Option<String>,
    plain_lyrics: Option<String>,
    instrumental: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LrclibSearchHit {
    duration: Option<f64>,
    synced_lyrics: Option<String>,
}

/// Looks the song up on lrclib.net, preferring synced lyrics; falls back to search when the exact
/// match is missing or junk (lrclib is user-edited).
pub async fn lrclib(http: &reqwest::Client, song: &Song) -> Option<Lyrics> {
    let exact = lrclib_get(http, song).await;
    if exact.as_ref().is_some_and(|l| l.lines.len() >= 3) {
        return exact;
    }
    lrclib_search(http, song).await.or(exact)
}

async fn lrclib_get(http: &reqwest::Client, song: &Song) -> Option<Lyrics> {
    let artist = song.artists.first().map(|a| a.name.as_str()).unwrap_or_default();
    let mut url = url::Url::parse("https://lrclib.net/api/get").ok()?;
    url.query_pairs_mut()
        .append_pair("artist_name", artist)
        .append_pair("track_name", &song.title)
        .append_pair("album_name", song.album.as_deref().unwrap_or_default())
        .append_pair("duration", &song.duration.as_secs().to_string());
    let resp = http.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body: LrclibResponse = resp.json().await.ok()?;
    if body.instrumental == Some(true) {
        return Some(Lyrics {
            provider: "lrclib.net".into(),
            synced: false,
            lines: vec![LyricLine::new(None, "♪ Instrumental ♪")],
            matched: None,
        });
    }
    let mut lyrics = match (body.synced_lyrics, body.plain_lyrics) {
        (Some(lrc), _) if !lrc.trim().is_empty() => parse_lrc(&lrc),
        (_, Some(plain)) if !plain.trim().is_empty() => parse_lrc(&plain),
        _ => return None,
    };
    lyrics.provider = "lrclib.net".into();
    Some(lyrics)
}

async fn lrclib_search(http: &reqwest::Client, song: &Song) -> Option<Lyrics> {
    let artist = song.artists.first().map(|a| a.name.as_str()).unwrap_or_default();
    let mut url = url::Url::parse("https://lrclib.net/api/search").ok()?;
    url.query_pairs_mut().append_pair("track_name", &song.title).append_pair("artist_name", artist);
    let hits: Vec<LrclibSearchHit> = http.get(url).send().await.ok()?.json().await.ok()?;
    let target = song.duration.as_secs_f64();
    hits.into_iter()
        .filter(|h| target == 0.0 || h.duration.is_some_and(|d| (d - target).abs() <= 3.0))
        .filter_map(|h| h.synced_lyrics.map(|lrc| parse_lrc(&lrc)))
        .filter(|l| l.lines.len() >= 3)
        .max_by_key(|l| l.lines.len())
        .map(|mut l| {
            l.provider = "lrclib.net".into();
            l
        })
}
