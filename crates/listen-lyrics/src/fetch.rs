//! Lyrics lookup: automatic (providers in order), manual search (all providers), fetching a picked
//! result, and choosing between server and provider lyrics.

use crate::providers::{self, Query, SearchHit, Source};
use crate::{lrc, rank};
use listen_core::model::{Lyrics, Song};

/// How the lookups behave (the lyrics settings).
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Providers in the user's order.
    pub sources: Vec<Source>,
    /// Ask NetEase for its translation layer too.
    pub netease_translation: bool,
}

impl Query {
    pub fn for_song(song: &Song) -> Query {
        Query {
            name: song.title.clone(),
            artist: song.artists.first().map(|a| a.name.clone()).unwrap_or_default(),
            album: song.album.clone().unwrap_or_else(|| song.title.clone()),
            duration_secs: song.duration.as_secs_f64(),
        }
    }
}

/// Turns fetched provider text into lyrics, naming the provider and the match.
pub fn lyrics_from_text(text: &str, hit: &SearchHit) -> Lyrics {
    let mut l = lrc::parse_lrc(text);
    l.provider = hit.source.name().to_string();
    l.matched = Some(hit.matched());
    l
}

/// Fetches one candidate (a manual pick, or an override restored from disk).
pub async fn get(http: &reqwest::Client, hit: &SearchHit, opts: &Options) -> Option<Lyrics> {
    let text = providers::fetch_text(http, hit.source, &hit.id, opts.netease_translation).await?;
    Some(lyrics_from_text(&text, hit))
}

/// Manual search over every enabled provider at once, synchronized results first, then by match.
pub async fn search_all(http: &reqwest::Client, q: &Query, sources: &[Source]) -> Vec<SearchHit> {
    let tasks: Vec<_> = sources
        .iter()
        .map(|s| {
            let (http, q, s) = (http.clone(), q.clone(), *s);
            tokio::spawn(async move { providers::search(&http, s, &q).await })
        })
        .collect();
    let mut all = Vec::new();
    for t in tasks {
        if let Ok(hits) = t.await {
            all.extend(hits);
        }
    }
    all.sort_by(|a, b| {
        (b.synced == Some(true))
            .cmp(&(a.synced == Some(true)))
            .then(a.score.partial_cmp(&b.score).unwrap_or(std::cmp::Ordering::Equal))
    });
    all
}

/// Automatic lookup in the user's provider order: the first synchronized good match wins, else the
/// earliest provider's best plain match.
pub async fn auto(http: &reqwest::Client, song: &Song, opts: &Options) -> Option<Lyrics> {
    let q = Query::for_song(song);
    let mut fallback: Option<Lyrics> = None;
    for source in &opts.sources {
        let found = match source {
            Source::Lrclib => lrclib_lookup(http, song).await,
            other => {
                let hits = providers::search(http, *other, &q).await;
                match hits.into_iter().next() {
                    Some(best) if best.score <= rank::AUTO_THRESHOLD => get(http, &best, opts).await,
                    _ => None,
                }
            }
        };
        match found {
            Some(l) if l.synced && !l.lines.is_empty() => return Some(l),
            Some(l) if fallback.is_none() && !l.lines.is_empty() => fallback = Some(l),
            _ => {}
        }
    }
    fallback
}

async fn lrclib_lookup(http: &reqwest::Client, song: &Song) -> Option<Lyrics> {
    crate::lrclib(http, song).await
}

/// Picks between server and provider lyrics. With `prefer_local` synchronized server lyrics win, plain
/// ones lose to synchronized provider lyrics; without it any provider result wins (Feishin).
pub fn choose(server: Option<Lyrics>, remote: Option<Lyrics>, prefer_local: bool) -> Option<Lyrics> {
    let server = server.filter(|l| !l.lines.is_empty());
    let remote = remote.filter(|l| !l.lines.is_empty());
    if prefer_local {
        return match (server, remote) {
            (Some(s), _) if s.synced => Some(s),
            (s, Some(r)) if r.synced || s.is_none() => Some(r),
            (s, _) => s,
        };
    }
    remote.or(server)
}

/// Whether the server's lyrics alone settle it (no provider lookup needed).
pub fn local_settles(server: Option<&Lyrics>, prefer_local: bool) -> bool {
    prefer_local && server.is_some_and(|l| l.synced && !l.lines.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use listen_core::model::LyricLine;

    fn lyr(provider: &str, synced: bool) -> Lyrics {
        Lyrics {
            provider: provider.into(),
            synced,
            lines: vec![LyricLine::new(synced.then_some(std::time::Duration::ZERO), "x")],
            matched: None,
        }
    }

    fn pick(server: Option<Lyrics>, remote: Option<Lyrics>, prefer_local: bool) -> Option<String> {
        choose(server, remote, prefer_local).map(|l| l.provider)
    }

    #[test]
    fn prefer_local_keeps_synced_server_lyrics() {
        assert_eq!(
            pick(Some(lyr("server", true)), Some(lyr("lrclib", true)), true).as_deref(),
            Some("server")
        );
    }

    #[test]
    fn prefer_local_upgrades_plain_server_lyrics_to_synced_ones() {
        assert_eq!(pick(Some(lyr("ytm", false)), Some(lyr("lrclib", true)), true).as_deref(), Some("lrclib"));
        assert_eq!(pick(Some(lyr("ytm", false)), Some(lyr("genius", false)), true).as_deref(), Some("ytm"));
        assert_eq!(pick(None, Some(lyr("genius", false)), true).as_deref(), Some("genius"));
        assert_eq!(pick(Some(lyr("ytm", false)), None, true).as_deref(), Some("ytm"));
    }

    #[test]
    fn without_prefer_local_the_providers_win() {
        assert_eq!(
            pick(Some(lyr("server", true)), Some(lyr("lrclib", false)), false).as_deref(),
            Some("lrclib")
        );
        assert_eq!(pick(Some(lyr("server", true)), None, false).as_deref(), Some("server"));
    }

    #[test]
    fn empty_lyrics_count_as_none() {
        let empty = Lyrics { provider: "server".into(), synced: true, lines: vec![], matched: None };
        assert_eq!(pick(Some(empty), None, true), None);
    }

    #[test]
    fn local_settles_only_when_preferred_and_synced() {
        assert!(local_settles(Some(&lyr("s", true)), true));
        assert!(!local_settles(Some(&lyr("s", true)), false));
        assert!(!local_settles(Some(&lyr("s", false)), true));
        assert!(!local_settles(None, true));
    }

    #[test]
    fn provider_text_becomes_lyrics_with_its_match() {
        let hit = SearchHit {
            source: Source::NetEase,
            id: "1".into(),
            artist: "A".into(),
            name: "T".into(),
            synced: None,
            score: 0.1,
        };
        let l = lyrics_from_text("[00:01.00]Hi_BREAK_Hallo", &hit);
        assert_eq!(l.provider, "NetEase");
        assert_eq!(l.matched.as_deref(), Some("T — A"));
        assert_eq!(l.lines[0].translation.as_deref(), Some("Hallo"));
    }
}
