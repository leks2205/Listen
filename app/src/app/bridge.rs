//! DJ bridges in the queue (`listen_mix::bridge`): when the DJ's next song is far from the current one, a
//! short instrumental stretch of an already analysed song goes between them and is mixed in and out.
//!
//! The bridge only exists while a mix into it is planned or running: a mix that is given up before its
//! handover removes the bridge again, and a bridge that becomes current any other way is skipped. If its
//! second mix doesn't happen, the next song starts when the stretch ends.

use super::{Listen, Message};
use iced::Task;
use listen_core::analysis::{Features, TrackAnalysis};
use listen_core::model::Song;
use listen_core::queue::QueueItem;
use listen_core::versions::song_key;
use listen_mix::plan::PlanCtx;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

/// Songs remembered as bridge candidates (newest win).
const POOL_MAX: usize = 400;
/// Candidates checked per pair (newest first).
const MAX_CANDIDATES: usize = 150;

#[derive(Debug, Default)]
pub struct BridgeState {
    /// Songs the DJs were offered (fills, radio pools): the bridges are chosen from the analysed ones.
    pool: Vec<Song>,
    keys: HashSet<String>,
    /// Pairs (outgoing uid, incoming uid) already considered: one decision per pair.
    tried: HashSet<(u64, u64)>,
}

impl Listen {
    /// Remembers songs a DJ was offered as possible bridges.
    pub(super) fn bridge_pool_add(&mut self, songs: &[Song]) {
        let b = &mut self.bridge;
        for song in songs {
            if !listen_mix::plan::mixable_source(song.source) {
                continue;
            }
            if b.keys.insert(song_key(song)) {
                b.pool.push(song.clone());
            }
        }
        if b.pool.len() > POOL_MAX {
            let drop = b.pool.len() - POOL_MAX;
            for s in b.pool.drain(..drop) {
                b.keys.remove(&song_key(&s));
            }
        }
    }

    /// Before `cur` → `next` is planned: puts a bridge between them when they are far apart, the setting
    /// allows it and an analysed song fits. Returns whether one was inserted (then plan again).
    pub(super) fn bridge_maybe_insert(
        &mut self,
        cur: &QueueItem,
        next: &QueueItem,
        fa: &Features,
        fb: &Features,
        out_now: f64,
    ) -> bool {
        let settings = self.config.settings.mixing.clone();
        if !settings.bridges || !next.auto || cur.bridge.is_some() || next.bridge.is_some() {
            return false;
        }
        if !self.bridge.tried.insert((cur.uid, next.uid)) {
            return false;
        }
        let cur_song = self.substitutes.used(&cur.song).unwrap_or_else(|| cur.song.clone());
        let next_song = self.playable(&next.song);
        let ctx = PlanCtx {
            settings: &settings,
            out_position_s: out_now,
            voice_break: None,
            out_genres: &cur_song.genres,
            in_genres: &next_song.genres,
        };
        let direct = listen_mix::plan::plan(fa, fb, &ctx);
        let Some(why) = listen_mix::bridge::gap(fa, fb, &direct, &settings) else { return false };
        let chance = f64::from(settings.bridge_chance_pct.min(100)) / 100.0;
        if !rand::random_bool(chance) {
            tracing::info!(
                "bridge: “{}” → “{}” are apart ({why}); no bridge this time",
                cur.song.title,
                next.song.title
            );
            return false;
        }
        let (songs, records) = self.bridge_candidates();
        let feats: Vec<&Features> = records.iter().filter_map(|r| r.features()).collect();
        if feats.len() != songs.len() {
            return false;
        }
        let Some(choice) = listen_mix::bridge::choose(fa, fb, &feats, &ctx) else {
            tracing::info!(
                "bridge: “{}” → “{}” are apart ({why}); none of {} analysed candidates fits",
                cur.song.title,
                next.song.title,
                songs.len()
            );
            return false;
        };
        let song = songs[choice.index].clone();
        tracing::info!(
            "bridge: “{}” between “{}” and “{}” ({why}): {}",
            song.title,
            cur.song.title,
            next.song.title,
            choice.reason
        );
        self.queue.insert_bridge(song, choice.cut, next.ai).is_some()
    }

    /// Analysed pool songs that may bridge now: from a connected server, not in the queue and not played
    /// this session.
    fn bridge_candidates(&self) -> (Vec<Song>, Vec<Arc<TrackAnalysis>>) {
        let Some(analysis) = self.analysis.as_ref() else { return (vec![], vec![]) };
        let queued: HashSet<String> = self.queue.iter().map(|i| song_key(&i.song)).collect();
        let (mut songs, mut records) = (Vec::new(), Vec::new());
        for song in self.bridge.pool.iter().rev() {
            if songs.len() >= MAX_CANDIDATES {
                break;
            }
            let key = song_key(song);
            if queued.contains(&key)
                || self.session_played.contains(&key)
                || !self.sources.contains_key(&song.server_id)
            {
                continue;
            }
            let Some(record) = analysis.lookup(&self.playable(song)) else { continue };
            if record.features().is_some() {
                songs.push(song.clone());
                records.push(record);
            }
        }
        (songs, records)
    }

    /// A mix into a bridge was given up before its handover: the bridge goes (the pair is not bridged again).
    pub(super) fn bridge_drop(&mut self, uid: u64) {
        if self.queue.iter().any(|i| i.uid == uid && i.bridge.is_some())
            && self.queue.current().map(|i| i.uid) != Some(uid)
        {
            tracing::info!("bridge: the mix into it was given up; removed");
            self.queue.remove(uid);
        }
    }

    /// The current song is a bridge.
    pub(super) fn bridge_playing(&self) -> bool {
        self.queue.current().is_some_and(|i| i.bridge.is_some())
    }

    /// A bridge became current without being mixed in (it would play from its start): skip it.
    pub(super) fn bridge_unentered(&mut self) -> Option<Task<Message>> {
        let item = self.queue.current()?;
        item.bridge?;
        let uid = item.uid;
        if self.mix.entered_uid == Some(uid) {
            return None;
        }
        tracing::info!("bridge “{}” came up without its mix; skipped", item.song.title);
        self.queue.remove(uid);
        if !self.playback.playing {
            return None;
        }
        Some(self.start_current())
    }

    /// The bridge's stretch is over and no mix took it out: the next song starts.
    pub(super) fn bridge_watch(&mut self, pos: Duration) -> Option<Task<Message>> {
        let item = self.queue.current()?;
        let cut = item.bridge?;
        if self.mix.owns_next() || pos.as_secs_f64() < cut.end_s || !self.playback.playing {
            return None;
        }
        tracing::info!("bridge “{}” reached the end of its stretch unmixed; next song", item.song.title);
        self.queue.advance();
        Some(self.start_current())
    }
}
