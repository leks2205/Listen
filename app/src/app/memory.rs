//! The memory model in the app ([`listen_core::memory`]): the Auto-DJ's forgotten-favourite slot (one
//! pick in eight) and resting worn-out songs from every automatic pick.

use super::Listen;
use listen_core::history::{Picker, now_secs};
use listen_core::memory::Memory;
use listen_core::model::Song;
use listen_core::queue::{Why, WhyCode};
use listen_core::versions::song_key;
use std::collections::HashSet;

/// Forgotten favourites the flow scorer chooses the slot's song from.
const FORGOTTEN_CANDIDATES: usize = 20;

impl Listen {
    /// The memory of the listening history now.
    pub(super) fn memory(&self) -> Memory {
        Memory::new(&self.history.entries, now_secs())
    }

    /// Songs skipped again and again lately: rested from automatic picks (never from the user's own).
    pub(super) fn worn_out(&self) -> HashSet<String> {
        self.memory().worn_out()
    }

    /// Puts a forgotten favourite into every [`FAMILIAR_EVERY`](listen_core::settings::FAMILIAR_EVERY)th
    /// Auto-DJ pick (counted over all fills), replacing the ordinary pick there. `skip` holds the song
    /// keys that may not come (queued, played this session, blocked). The favourite with the best flow
    /// from the current song wins.
    pub(super) fn with_forgotten(
        &mut self,
        picks: Vec<(Song, Why)>,
        skip: &HashSet<String>,
    ) -> Vec<(Song, Why)> {
        let slot = listen_mix::pick::Targets {
            familiar_slot: self.config.settings.auto_dj.familiar_slot(),
            ..Default::default()
        };
        let start = self.dj_picks_counted;
        self.dj_picks_counted += picks.len();
        if !(start..start + picks.len()).any(|i| slot.familiar_at(i)) {
            return picks;
        }
        let memory = self.memory();
        let mut taken: HashSet<String> = picks.iter().map(|(s, _)| song_key(s)).collect();
        let current = self.queue.current().map(|i| i.song.clone());
        let mut out = Vec::with_capacity(picks.len());
        for (i, pick) in picks.into_iter().enumerate() {
            if !slot.familiar_at(start + i) {
                out.push(pick);
                continue;
            }
            let candidates: Vec<Song> = memory
                .forgotten(FORGOTTEN_CANDIDATES + taken.len() + skip.len())
                .into_iter()
                .filter(|m| !skip.contains(&m.key) && !taken.contains(&m.key))
                .map(|m| m.song.clone())
                .filter(|s| self.demo || self.sources.contains_key(&s.server_id))
                .take(FORGOTTEN_CANDIDATES)
                .collect();
            let best = match &current {
                Some(c) => self.flow_order(c, candidates, 1).into_iter().next(),
                None => candidates.into_iter().next(),
            };
            match best.and_then(|s| memory.get(&s).map(|m| (s, m.last_played))) {
                Some((song, last_played)) => {
                    tracing::info!("Auto-DJ: forgotten favourite “{}”", song.title);
                    taken.insert(song_key(&song));
                    out.push((
                        song,
                        Why::new(Picker::Rediscover, WhyCode::ForgottenFavourite { last_played }),
                    ));
                }
                None => out.push(pick),
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{Args, Message};
    use crate::demo;
    use listen_core::history::{Entry, PlayCtx};

    const DAY: i64 = 86_400;

    fn rig() -> Listen {
        let (mut s, _) = Listen::boot(&Args { demo: true, ..Args::default() });
        s.history.entries.clear();
        s
    }

    #[test]
    fn every_eighth_auto_dj_pick_is_a_forgotten_favourite() {
        let mut s = rig();
        let now = now_secs();
        let old = demo::album_songs(0)[1..].to_vec();
        // Two loved songs from half a year ago, eight plays each.
        for song in &old[..2] {
            for d in 0..8 {
                s.history.entries.push(Entry::new(song, now - (180 + d) * DAY));
            }
        }
        s.queue.set(demo::album_songs(0)[..1].to_vec(), 0);
        let mut fills = Vec::new();
        for album in 1..=8 {
            let before = s.queue.len();
            let _ = s.update(Message::AutoDjLoaded(Ok(demo::album_songs(album))));
            fills.push(s.queue.len() - before);
        }
        let items: Vec<_> = s.queue.iter().skip(1).collect();
        let forgotten: Vec<usize> = items
            .iter()
            .enumerate()
            .filter(|(_, i)| {
                i.why.as_ref().is_some_and(|w| matches!(w.code, WhyCode::ForgottenFavourite { .. }))
            })
            .map(|(n, _)| n)
            .collect();
        assert!(items.len() >= 16, "fills {fills:?}");
        assert_eq!(forgotten, [7, 15], "fills {fills:?}");
        let keys: Vec<String> = forgotten.iter().map(|&n| song_key(&items[n].song)).collect();
        assert!(keys.iter().all(|k| old[..2].iter().any(|o| &song_key(o) == k)));
        assert_ne!(keys[0], keys[1], "a favourite comes once");
        // Off in the settings: ordinary picks only.
        let mut s2 = rig();
        s2.history.entries = s.history.entries.clone();
        s2.config.settings.auto_dj.forgotten_favourites = false;
        s2.queue.set(demo::album_songs(0)[..1].to_vec(), 0);
        for album in 1..=4 {
            let _ = s2.update(Message::AutoDjLoaded(Ok(demo::album_songs(album))));
        }
        assert!(s2.queue.iter().all(|i| !i.why.as_ref().is_some_and(|w| w.picker == Picker::Rediscover)));
    }

    #[test]
    fn a_song_skipped_three_times_this_week_rests_from_automatic_picks() {
        let mut s = rig();
        let now = now_secs();
        let songs = demo::album_songs(3);
        for d in 1..=3 {
            s.history.entries.push(Entry::skipped(&songs[0], now - d * DAY, 5.0, PlayCtx::default()));
        }
        assert!(s.worn_out().contains(&song_key(&songs[0])));
        assert!(s.auto_blocked().contains(&song_key(&songs[0])));
        s.queue.set(demo::album_songs(0)[..1].to_vec(), 0);
        let _ = s.update(Message::AutoDjLoaded(Ok(songs.clone())));
        let keys: Vec<String> = s.queue.iter().map(|i| song_key(&i.song)).collect();
        assert!(!keys.contains(&song_key(&songs[0])), "{keys:?}");
        assert!(keys.len() > 1);
        // The user can still start it.
        s.queue.set(vec![songs[0].clone()], 0);
        assert_eq!(s.queue.current().map(|i| song_key(&i.song)), Some(song_key(&songs[0])));
    }
}
