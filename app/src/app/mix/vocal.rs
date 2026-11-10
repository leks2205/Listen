//! Synced lyrics as vocal maps for the transition planner (`listen_mix::lyric`): looked up once per
//! song while it is playing or next (the server's lyrics, then the lyric providers when lyric fetching
//! is on) and merged into the analysis before planning. Planning never waits for them: a plan made
//! without is made again when they arrive before the incoming deck is loaded.

use super::super::{Listen, Message};
use super::{MixMsg, PRELOAD_LEAD_S, Phase};
use iced::Task;
use listen_core::analysis::{Features, VocalMap};
use listen_core::model::{Lyrics, Song};
use listen_lyrics::fetch::{self, Options};
use listen_lyrics::providers::Source;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

/// A lyric lookup for mixing gives up after this long.
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
/// Maps kept (song keys); the cache is cleared beyond this.
const KEEP: usize = 64;
/// A re-plan for new lyrics needs this much time before the incoming deck is loaded.
const REPLAN_MARGIN_S: f64 = 2.0;

/// Vocal maps per song key (`None`: no usable synced lyrics) and lookups under way.
#[derive(Debug, Default)]
pub(super) struct LyricMaps {
    maps: HashMap<String, Option<Arc<VocalMap>>>,
    busy: HashSet<String>,
}

impl LyricMaps {
    pub(super) fn insert(&mut self, key: String, map: Option<VocalMap>) {
        self.busy.remove(&key);
        if self.maps.len() >= KEEP {
            self.maps.clear();
        }
        self.maps.insert(key, map.map(Arc::new));
    }

    #[cfg(test)]
    pub(super) fn get(&self, key: &str) -> Option<&Option<Arc<VocalMap>>> {
        self.maps.get(key)
    }
}

/// The vocal map of `lyrics` for a song whose audio ends at `audio_end`.
fn map_of(lyrics: Option<&Lyrics>, audio_end: f32) -> Option<VocalMap> {
    lyrics.and_then(|l| listen_lyrics::vocal_map(l, audio_end))
}

impl Listen {
    /// The analyses of the outgoing and incoming song with their lyrics merged in (`None`: no usable
    /// lyrics yet, plan with the analysis as is).
    pub(super) fn mix_with_lyrics(&self, songs: [&Song; 2], feats: [&Features; 2]) -> [Option<Features>; 2] {
        let merged = |song: &Song, f: &Features| {
            let map = self.mix.fetch.vocal.maps.get(&Self::server_song_key(song))?.as_deref()?;
            let m = listen_mix::lyric::with_lyrics(f, map);
            if m.is_none() {
                tracing::debug!("mix: the lyrics of “{}” don't fit its audio; not used", song.title);
            }
            m
        };
        [merged(songs[0], feats[0]), merged(songs[1], feats[1])]
    }

    /// Starts the lyric lookups the playing and the next song still need (analysed, mixable songs only).
    pub(super) fn mix_lyrics_prefetch(&mut self) -> Task<Message> {
        if !self.mixing_possible() {
            return Task::none();
        }
        let Some(analysis) = self.analysis.clone() else { return Task::none() };
        let cur =
            self.queue.current().map(|i| self.substitutes.used(&i.song).unwrap_or_else(|| i.song.clone()));
        let next = self.queue.peek_next().map(|i| self.playable(&i.song));
        let mut tasks = Vec::new();
        for song in [cur, next].into_iter().flatten() {
            if !listen_mix::mixable_source(song.source) {
                continue;
            }
            let key = Self::server_song_key(&song);
            let vocal = &self.mix.fetch.vocal;
            if vocal.maps.contains_key(&key) || vocal.busy.contains(&key) {
                continue;
            }
            let Some(audio_end) = analysis.lookup(&song).and_then(|a| a.features().map(|f| f.audio_end))
            else {
                continue;
            };
            // The playing song's lyrics are usually loaded already.
            if self.lyrics_song.as_deref() == Some(song.id.as_str()) && !self.lyrics_loading {
                let map = map_of(self.lyrics.as_ref(), audio_end);
                self.mix.fetch.vocal.insert(key, map);
            } else if let Some(t) = self.mix_lyrics_fetch(song, key, audio_end) {
                tasks.push(t);
            }
        }
        Task::batch(tasks)
    }

    /// A vocal map arrived: plans the pair (again, if the plan is still only on paper).
    pub(super) fn mix_lyrics_arrived(&mut self, key: String, map: Option<VocalMap>) -> Task<Message> {
        let useful = map.is_some();
        self.mix.fetch.vocal.insert(key.clone(), map);
        if self.mix.phase == Phase::Idle {
            return self.mix_consider();
        }
        if !useful || self.mix.phase != Phase::Planned || self.mix.loaded {
            return Task::none();
        }
        let item = |uid: u64| self.queue.iter().find(|i| i.uid == uid);
        let (Some(out), Some(inn)) = (item(self.mix.out_uid), item(self.mix.in_uid)) else {
            return Task::none();
        };
        let concerns = [&out.song, &inn.song]
            .iter()
            .any(|s| Self::server_song_key(s) == key || Self::server_song_key(&self.playable(s)) == key);
        let bridge = out.bridge.is_some() || inn.bridge.is_some();
        let Some(plan) = self.mix.plan() else { return Task::none() };
        let preload_at = plan.out_mix_start_s - PRELOAD_LEAD_S;
        let early = self.deck_now(self.mix.out_deck) + REPLAN_MARGIN_S < preload_at;
        if !concerns || bridge || !early {
            return Task::none();
        }
        let cancel = self.mix_cancel("lyrics arrived; planning again");
        Task::batch([cancel, self.mix_consider()])
    }

    /// Looks up a song's lyrics for mixing (`None` when nothing would be looked up).
    fn mix_lyrics_fetch(&mut self, song: Song, key: String, audio_end: f32) -> Option<Task<Message>> {
        let s = &self.config.settings.lyrics;
        let source = self.sources.get(&song.server_id).cloned();
        // "Fetch lyrics" covers the server's lyrics too (as in the lyrics view).
        if !s.fetch || self.demo {
            self.mix.fetch.vocal.insert(key, None);
            return None;
        }
        let opts = Options { sources: Source::parse_list(&s.sources), netease_translation: false };
        let prefer_local = s.prefer_local;
        let http = self.http.clone();
        self.mix.fetch.vocal.busy.insert(key.clone());
        Some(Task::perform(
            async move {
                let look = async {
                    let server = match source {
                        Some(s) => s.lyrics(&song).await.ok().flatten().filter(|l| !l.lines.is_empty()),
                        None => None,
                    };
                    if opts.sources.is_empty() || fetch::local_settles(server.as_ref(), prefer_local) {
                        return server;
                    }
                    let remote = fetch::auto(&http, &song, &opts).await;
                    fetch::choose(server, remote, prefer_local)
                };
                let lyrics = tokio::time::timeout(FETCH_TIMEOUT, look).await.ok().flatten();
                map_of(lyrics.as_ref(), audio_end)
            },
            move |map| Message::Mix(MixMsg::Lyrics(key.clone(), map)),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use listen_core::model::LyricLine;

    #[test]
    fn maps_come_from_synced_lyrics_only() {
        let line = |t: u64, s: &str| LyricLine::new(Some(Duration::from_secs(t)), s);
        let mut l = Lyrics {
            provider: "t".into(),
            synced: true,
            lines: vec![line(10, "one two"), line(13, "three four"), line(16, "five six"), line(19, "seven")],
            matched: None,
        };
        assert!(map_of(Some(&l), 200.0).is_some());
        l.synced = false;
        assert!(map_of(Some(&l), 200.0).is_none());
        assert!(map_of(None, 200.0).is_none());
    }

    #[test]
    fn the_cache_is_bounded() {
        let mut m = LyricMaps::default();
        m.busy.insert("a".into());
        m.insert("a".into(), None);
        assert!(m.busy.is_empty());
        for k in 0..KEEP + 3 {
            m.insert(k.to_string(), None);
        }
        assert!(m.maps.len() <= KEEP);
    }
}
