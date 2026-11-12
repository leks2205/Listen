//! Lyrics: LRC parsing (plain, synced, word-synced, translations), providers (lrclib.net, NetEase,
//! Genius, SimpMusic) with ranking and rate limits, translation, romaji, RTL detection, export.

pub mod card;
pub mod export;
pub mod fetch;
pub mod furigana;
mod gate;
pub mod gemini;
pub mod known;
mod lrc;
pub mod providers;
pub mod rank;
pub mod romaji;
pub mod rtl;
pub mod translate;
pub mod vocab;
mod vocal;

pub use lrc::{BREAK, active_line, has_translation, has_word_sync, merge_translation, parse_lrc};
pub use vocal::vocal_map;

use listen_core::model::{LyricLine, Lyrics, Song};
use providers::lrclib::{Record, parse_record, parse_records};
use providers::{LRCLIB, Source, get_text};

/// Looks the song up on lrclib.net, preferring synced lyrics; falls back to search when the exact
/// match is missing or junk (lrclib is user-edited). The answers are read by `providers::lrclib`,
/// the crate's one lrclib parser (the manual lyrics search uses it too); add fields there.
pub async fn lrclib(http: &reqwest::Client, song: &Song) -> Option<Lyrics> {
    let exact = lrclib_get(http, song).await;
    if exact.as_ref().is_some_and(|l| l.lines.len() >= 3) {
        return exact;
    }
    lrclib_search(http, song).await.or(exact)
}

async fn lrclib_get(http: &reqwest::Client, song: &Song) -> Option<Lyrics> {
    let artist = song.artists.first().map(|a| a.name.as_str()).unwrap_or_default();
    let mut url = url::Url::parse(&format!("{LRCLIB}/get")).ok()?;
    url.query_pairs_mut()
        .append_pair("artist_name", artist)
        .append_pair("track_name", &song.title)
        .append_pair("album_name", song.album.as_deref().unwrap_or_default())
        .append_pair("duration", &song.duration.as_secs().to_string());
    exact_lyrics(&parse_record(&get_text(http, Source::Lrclib, url).await?)?)
}

/// The lyrics of an `/get` answer: a marker line for an instrumental, else the synced lyrics, else
/// the plain ones.
fn exact_lyrics(record: &Record) -> Option<Lyrics> {
    if record.instrumental {
        return Some(Lyrics {
            provider: "lrclib.net".into(),
            synced: false,
            lines: vec![LyricLine::new(None, "♪ Instrumental ♪")],
            matched: None,
        });
    }
    let mut lyrics = parse_lrc(record.text()?);
    lyrics.provider = "lrclib.net".into();
    Some(lyrics)
}

async fn lrclib_search(http: &reqwest::Client, song: &Song) -> Option<Lyrics> {
    let artist = song.artists.first().map(|a| a.name.as_str()).unwrap_or_default();
    let mut url = url::Url::parse(&format!("{LRCLIB}/search")).ok()?;
    url.query_pairs_mut().append_pair("track_name", &song.title).append_pair("artist_name", artist);
    best_search_hit(parse_records(&get_text(http, Source::Lrclib, url).await?), song.duration.as_secs_f64())
}

/// The longest synced lyrics (3 lines or more) among search hits within 3 s of `target` seconds
/// (any length when `target` is 0).
fn best_search_hit(hits: Vec<Record>, target: f64) -> Option<Lyrics> {
    hits.into_iter()
        .filter(|h| target == 0.0 || h.duration.is_some_and(|d| (d - target).abs() <= 3.0))
        .filter_map(|h| h.synced.map(|lrc| parse_lrc(&lrc)))
        .filter(|l| l.lines.len() >= 3)
        .max_by_key(|l| l.lines.len())
        .map(|mut l| {
            l.provider = "lrclib.net".into();
            l
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SYNCED: &str = "[00:01.00]one\\n[00:02.00]two\\n[00:03.00]three";

    #[test]
    fn an_exact_lrclib_answer_prefers_synced_lyrics_and_marks_instrumentals() {
        let synced =
            format!(r#"{{"id":1,"syncedLyrics":"{SYNCED}","plainLyrics":"one","instrumental":false}}"#);
        let l = exact_lyrics(&parse_record(&synced).unwrap()).unwrap();
        assert_eq!((l.provider.as_str(), l.synced, l.lines.len()), ("lrclib.net", true, 3));

        let plain = r#"{"syncedLyrics":"  ","plainLyrics":"just words","instrumental":null}"#;
        let l = exact_lyrics(&parse_record(plain).unwrap()).unwrap();
        assert_eq!((l.synced, l.lines[0].text.as_str()), (false, "just words"));

        let inst = r#"{"syncedLyrics":null,"plainLyrics":null,"instrumental":true}"#;
        let l = exact_lyrics(&parse_record(inst).unwrap()).unwrap();
        assert_eq!(l.lines[0].text, "♪ Instrumental ♪");

        let missing = r#"{"code":404,"name":"TrackNotFound","message":"Failed to find specified track"}"#;
        assert!(exact_lyrics(&parse_record(missing).unwrap()).is_none());
        assert!(parse_record("not json").is_none());
    }

    #[test]
    fn an_lrclib_search_keeps_the_longest_synced_hit_near_the_duration() {
        let body = format!(
            r#"[{{"id":1,"duration":200.0,"syncedLyrics":"{SYNCED}\n[00:04.00]four"}},
                {{"id":2,"duration":181.5,"syncedLyrics":"{SYNCED}"}},
                {{"id":3,"duration":180,"syncedLyrics":"[00:01.00]short"}},
                {{"id":4,"duration":180,"syncedLyrics":null,"plainLyrics":"x"}}]"#
        );
        let l = best_search_hit(parse_records(&body), 180.0).unwrap();
        assert_eq!((l.provider.as_str(), l.lines.len()), ("lrclib.net", 3));
        assert_eq!(best_search_hit(parse_records(&body), 0.0).unwrap().lines.len(), 4);
        assert!(best_search_hit(parse_records(&body), 100.0).is_none());
        assert!(parse_records("{}").is_empty());
    }
}
