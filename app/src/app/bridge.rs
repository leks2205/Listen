//! DJ bridges in the queue (`listen_mix::bridge`): when the DJ's next song is far from the current one, a
//! short instrumental stretch of an already analysed song goes between them and is mixed in and out.
//!
//! The bridge only exists while a mix into it is planned or running: a mix that is given up before its
//! handover removes the bridge again, and a bridge that becomes current any other way is skipped. If its
//! second mix doesn't happen, the next song starts when the stretch ends.

use super::{Listen, Message};
use iced::Task;
use listen_core::analysis::{Features, TrackAnalysis};
use listen_core::model::{Song, SourceKind};
use listen_core::queue::QueueItem;
use listen_core::versions::song_key;
use listen_mix::plan::PlanCtx;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

/// Songs remembered as bridge candidates (newest win).
const POOL_MAX: usize = 400;
/// Candidates checked per pair: the DJ pool and the history first, then the rest of the analysis store.
/// Few songs have a usable instrumental stretch at a tempo between both sides, so the whole store is
/// needed (a few ms per pair).
const MAX_CANDIDATES: usize = 6000;

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
            no_echo: self.came_in_by_echo(cur.uid),
        };
        let direct = listen_mix::plan::plan(fa, fb, &ctx);
        let Some(why) = listen_mix::bridge::gap(fa, fb, &direct, &settings) else { return false };
        // A pair that would echo out, cut or fade always tries a bridge; the chance thins out the others.
        let chance = f64::from(settings.bridge_chance_pct.min(100)) / 100.0;
        if !listen_mix::bridge::always_try(&direct) && !rand::random_bool(chance) {
            tracing::info!(
                "bridge: “{}” → “{}” are apart ({why}); no bridge this time",
                cur.song.title,
                next.song.title
            );
            return false;
        }
        let (songs, records) = self.bridge_candidates();
        // Only bridges from the scene the pair plays in (K-pop between K-pop songs, never US rap).
        let Some(analysis) = self.analysis.as_ref() else { return false };
        let neighbours = listen_core::scene::Neighbours::from_history(&self.history.entries);
        let scenes = analysis.scenes();
        let profile = |s: &Song| listen_analysis::scene_profile(&scenes, s, &listen_analysis::record_key(s));
        let gated = listen_analysis::bridge_scene_gate(
            &scenes,
            &neighbours,
            profile(&cur_song),
            profile(&next_song),
            songs,
            records,
        );
        let feats: Vec<&Features> = gated.records.iter().filter_map(|r| r.features()).collect();
        if feats.len() != gated.songs.len() {
            return false;
        }
        let tally = gated.tally.describe();
        let Some(choice) = listen_mix::bridge::choose_scored(fa, fb, &direct, &feats, &gated.scores, &ctx)
        else {
            tracing::info!(
                "bridge: “{}” → “{}” are apart ({why}); {tally} [{}]; none of the {} left fits",
                cur.song.title,
                next.song.title,
                gated.pair,
                gated.songs.len()
            );
            return false;
        };
        let song = gated.songs[choice.index].clone();
        tracing::info!(
            "bridge: {tally} [{}], chose “{}” by {} (scene {:.2}) between “{}” and “{}” ({why}): {}",
            gated.pair,
            song.title,
            song.artist_line(),
            gated.scores[choice.index],
            cur.song.title,
            next.song.title,
            choice.reason
        );
        let why = listen_dj::why::bridge(&cur.song.title, &next.song.title);
        match self.queue.insert_bridge(song, choice.cut, next.ai) {
            Some(uid) => self.queue.set_why(uid, Some(why)),
            None => false,
        }
    }

    /// Analysed songs that may bridge now: the DJ pool (newest first), then the listening history, then
    /// every other analysed YouTube Music song of the store. Only from a connected server, not in the
    /// queue and not played this session.
    fn bridge_candidates(&self) -> (Vec<Song>, Vec<Arc<TrackAnalysis>>) {
        let Some(analysis) = self.analysis.as_ref() else { return (vec![], vec![]) };
        let mut skip: HashSet<String> = self.queue.iter().map(|i| song_key(&i.song)).collect();
        skip.extend(self.queue.iter().map(|i| listen_analysis::record_key(&i.song)));
        skip.extend(self.session_played.iter().cloned());
        skip.extend(self.auto_blocked());
        let known = self
            .bridge
            .pool
            .iter()
            .rev()
            .cloned()
            .chain(self.history.entries.iter().rev().map(|e| e.to_song()))
            .filter(|s| self.sources.contains_key(&s.server_id));
        let ytm =
            self.sources.iter().find(|(_, s)| s.kind() == SourceKind::YouTubeMusic).map(|(id, _)| id.clone());
        let (songs, records) = listen_analysis::bridge_candidates(
            known,
            |s| analysis.lookup(&self.playable(s)),
            analysis.all_records(),
            ytm.as_ref(),
            &skip,
            MAX_CANDIDATES,
        );
        // Substitutes: keep only songs that play the analysed file.
        songs
            .into_iter()
            .zip(records)
            .filter(|(s, r)| listen_analysis::record_key(&self.playable(s)) == r.song_id)
            .unzip()
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
