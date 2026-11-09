//! The scene gate for the songs the DJs choose from (the AI DJ's candidate pool, the Auto-DJ's fill), on
//! the phone and the desktop: every candidate (similar songs, history favourites, random songs) must
//! belong to the scene of what is playing (`listen_core::scene::Session::led`: the current song first,
//! then the songs played before it), so a 1980s ballad isn't followed by a 2020s idol song. When the gate
//! leaves too few, the pool is topped up with same-scene songs from the history and the analysis store
//! (songs the listener played first); it is never padded with another scene.

use crate::score::{TimbreStats, Weights, flow_terms};
use listen_core::analysis::FlowFeatures;
use listen_core::model::Song;
use listen_core::scene::{Scene, SceneFilter, Tally};
use listen_core::versions::song_key;
use std::collections::HashSet;

/// Below this many kept candidates the pool is topped up from the store.
pub const MIN_POOL: usize = 8;
/// A top-up brings the pool up to this many songs (when the store has them).
pub const TOP_UP_TO: usize = 12;
/// How much a closer scene (score 0..1) lowers a store song's sound cost (as for tempo picks).
const SCENE_WEIGHT: f32 = 0.3;

/// What [`gate_pool`] kept.
#[derive(Debug, Clone, Default)]
pub struct PoolGate {
    /// The candidates in the scene (in their order), then the store's top-up.
    pub songs: Vec<Song>,
    /// How many of the given candidates passed.
    pub kept: usize,
    /// The given candidates the gate turned away.
    pub rejected: Tally,
    /// Songs added from the store (all in the scene).
    pub topped_up: usize,
    /// How many of those the listener played or liked.
    pub heard: usize,
    /// Store songs the gate turned away while topping up.
    pub store_rejected: Tally,
    /// The session's scene ([`SceneFilter::session`]) when the gate was active.
    pub session: Option<String>,
    /// The session's scene when one leads ([`SceneFilter::scene`]), for the DJ's prompt.
    pub scene: Option<Scene>,
    /// The scene of each song of `songs` (song id, scene), for the DJ's notes.
    pub scenes: Vec<(String, Scene)>,
    /// Nothing of the scene was left anywhere, so the candidates whose scene is unknown were kept (never
    /// one of a known other scene): the DJ doesn't run dry where little is known (the phone without the
    /// server's scene data).
    pub kept_unknown: usize,
}

impl PoolGate {
    /// The log line: "AI DJ pool: 23 kept, 17 rejected (other scene 9, other era 6, unknown 2), 4 from
    /// the store [session scene ko · pop · 1980s (tags), 2 of 2 songs]". `None` without an active gate.
    pub fn note(&self, who: &str) -> Option<String> {
        let session = self.session.as_ref()?;
        let r = &self.rejected;
        let mut line = format!(
            "{who} pool: {} kept, {} rejected (other scene {}, other era {}, unknown {}, no language {})",
            self.kept,
            r.rejected(),
            r.other_scene + r.weak,
            r.other_era,
            r.unknown_scene,
            r.no_language
        );
        if self.topped_up > 0 {
            line.push_str(&format!(", {} from the store ({} heard)", self.topped_up, self.heard));
        }
        if self.kept_unknown > 0 {
            line.push_str(&format!(
                ", nothing of the scene left: kept {} of unknown scene",
                self.kept_unknown
            ));
        }
        line.push_str(&format!(" [{session}]"));
        Some(line)
    }

    /// The counts as JSON for logs and labs.
    pub fn stats(&self) -> serde_json::Value {
        let r = &self.rejected;
        serde_json::json!({
            "kept": self.kept, "rejected": r.rejected(), "other_scene": r.other_scene + r.weak,
            "other_era": r.other_era, "unknown_scene": r.unknown_scene, "no_language": r.no_language,
            "topped_up": self.topped_up, "heard": self.heard, "session": self.session,
            "store_judged": self.store_rejected.candidates, "kept_unknown": self.kept_unknown,
        })
    }
}

/// Keeps the candidates of `pool` that belong to the session's scene (`scene`, from
/// `listen_analysis::tempo_scene_filter`; `None` or inactive: all pass). When fewer than `min` are left,
/// tops the pool up to `top_up_to` with songs of the scene from `store` (analysed history and store songs;
/// only called when needed): those whose key (or `ytm:` id, or plain id) is in `skip` or already in the
/// pool are never taken; songs the listener played or liked come first, then the closest to the current
/// song's sound (`here`) and scene. Nothing from another scene is ever added.
pub fn gate_pool(
    pool: Vec<Song>,
    scene: Option<&SceneFilter>,
    store: impl FnOnce() -> Vec<(Song, FlowFeatures)>,
    skip: &HashSet<String>,
    min: usize,
    top_up_to: usize,
    here: Option<&FlowFeatures>,
) -> PoolGate {
    let Some(g) = scene.filter(|g| g.active) else {
        return PoolGate { kept: pool.len(), songs: pool, ..Default::default() };
    };
    let mut out = PoolGate { session: Some(g.session.clone()), scene: g.scene, ..Default::default() };
    out.rejected.candidates = pool.len();
    let mut seen: HashSet<String> = HashSet::new();
    let mut unknown: Vec<Song> = Vec::new();
    for song in pool {
        match (g.judge)(&song) {
            Ok(_) => {
                seen.insert(song_key(&song));
                out.songs.push(song);
            }
            Err(r) => {
                out.rejected.count(r);
                if r == listen_core::scene::Reject::UnknownScene {
                    unknown.push(song);
                }
            }
        }
    }
    out.kept = out.songs.len();
    if out.songs.len() >= min {
        out.scenes = out.songs.iter().map(|s| (s.id.clone(), (g.scene_of)(s))).collect();
        return out;
    }
    // (song, analysis, scene score, heard)
    let mut extra: Vec<(Song, FlowFeatures, f32, bool)> = Vec::new();
    for (song, f) in store() {
        let key = song_key(&song);
        let ytm_id = format!("ytm:{}", song.id);
        if skip.contains(&key) || skip.contains(&ytm_id) || skip.contains(&song.id) || seen.contains(&key) {
            continue;
        }
        out.store_rejected.candidates += 1;
        match (g.judge)(&song) {
            Ok(score) => {
                seen.insert(key);
                let heard = (g.heard)(&song);
                extra.push((song, f, score, heard));
            }
            Err(r) => out.store_rejected.count(r),
        }
    }
    let order = rank_store(extra.iter().map(|e| (&e.1, e.2, e.3, 0.0)), here);
    let room = top_up_to.saturating_sub(out.songs.len());
    for &i in order.iter().take(room) {
        out.heard += usize::from(extra[i].3);
        out.songs.push(extra[i].0.clone());
    }
    out.topped_up = room.min(order.len());
    if out.songs.is_empty() {
        unknown.truncate(top_up_to);
        out.kept_unknown = unknown.len();
        out.songs = unknown;
    }
    out.scenes = out.songs.iter().map(|s| (s.id.clone(), (g.scene_of)(s))).collect();
    out
}

/// Store songs in the order a top-up takes them: the ones the listener played or liked first, then the
/// closest to the current song's sound (`here`: energy and timbre, as the flow scorer weighs them) and
/// scene (0.3 × the score off the cost), then the lower `tie` (the tempo distance of a tempo pick). Items:
/// (analysis, scene score, heard, tie).
pub fn rank_store<'f>(
    items: impl Iterator<Item = (&'f FlowFeatures, f32, bool, f32)>,
    here: Option<&FlowFeatures>,
) -> Vec<usize> {
    let items: Vec<(&FlowFeatures, f32, bool, f32)> = items.collect();
    let w = Weights::default();
    let stats = TimbreStats::from_vectors(
        here.into_iter().chain(items.iter().map(|e| e.0)).map(|f| f.timbre.as_slice()),
    );
    let mut scored: Vec<(bool, f32, f32, usize)> = items
        .iter()
        .enumerate()
        .map(|(i, (f, score, heard, tie))| {
            let sound = here.map_or(0.0, |h| {
                let t = flow_terms(h, f, &[], stats.as_ref());
                w.energy * t[2] + w.timbre * t[3]
            });
            (!heard, sound - SCENE_WEIGHT * score, *tie, i)
        })
        .collect();
    scored.sort_by(|a, b| {
        a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.total_cmp(&b.2)).then(a.3.cmp(&b.3))
    });
    scored.into_iter().map(|s| s.3).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{flow, song};
    use listen_core::scene::{Neighbours, Profile, Scene, Session};

    const BALLAD80: &str = "Korean pop, ballad, 1980s, South Korea, Korean, male solo, emotional, 80s ballad";
    const BALLAD90: &str = "K-pop, ballad, 1990s, South Korea, Korean, male solo, piano";
    const IDOL20: &str = "K-pop, dance-pop, 2020s, South Korea, Korean, girl group, 4th-generation K-pop";
    const HOUSE: &str = "tech house, dance, electronic, 2020s, instrumental, club";
    const KRNB: &str = "R&B, soul, 2020s, South Korea, Korean, female solo";
    const TROT: &str = "Trot, 1990s, South Korea, Korean, male solo";

    fn tagged(title: &str, artist: &str, tags: &str) -> (Song, Profile) {
        let s = song(title, artist, None, None);
        let p = Profile {
            scene: Scene::from_tags(tags),
            artist: listen_core::scene::artist_key(artist),
            audio: None,
        };
        (s, p)
    }

    /// A filter over fixed profiles (by song id); `heard` ids count as played.
    fn filter<'a>(
        session: &'a Session<'a>,
        profiles: &'a [(Song, Profile)],
        heard: &'a [&'a str],
    ) -> SceneFilter<'a> {
        let profile = move |s: &Song| {
            profiles.iter().find(|(x, _)| x.id == s.id).map(|(_, p)| p.clone()).unwrap_or_default()
        };
        SceneFilter {
            judge: Box::new(move |s: &Song| session.judge_with(&profile(s), heard.contains(&s.id.as_str()))),
            heard: Box::new(move |s: &Song| heard.contains(&s.id.as_str())),
            active: session.active(),
            session: session.describe(),
            scene: session.scene().copied(),
            scene_of: Box::new(move |s: &Song| profile(s).scene),
        }
    }

    #[test]
    fn a_1980s_ballad_pool_has_no_2020s_idol_songs() {
        let n = Neighbours::default();
        let current = tagged("To a Lady", "Byun Jinsub", BALLAD80);
        // The four songs before it were 2020s idol songs: the listener's pick leads, they don't outvote it.
        let before: Vec<(Song, Profile)> =
            (0..4).map(|i| tagged(&format!("idol {i}"), &format!("group {i}"), IDOL20)).collect();
        let session =
            Session::led(std::iter::once(current.1.clone()).chain(before.iter().map(|b| b.1.clone())), &n);
        let mut cands: Vec<(Song, Profile)> = vec![
            tagged("New Jeans", "NewJeans", IDOL20),
            tagged("STYLE", "Hearts2Hearts", IDOL20),
            tagged("Supersonic", "fromis_9", IDOL20),
            tagged("내게 다시", "더더", BALLAD90),
            tagged("Ballad 2", "Lee Moon-se", BALLAD80),
            tagged("Freak", "Mesto", HOUSE),
            tagged("I Don't Lie", "Heize", KRNB),
            tagged("Trot song", "Trot singer", TROT),
            tagged("Unknown", "Nobody", ""),
        ];
        cands.push(current.clone());
        let pool: Vec<Song> = cands.iter().take(9).map(|c| c.0.clone()).collect();
        // The store: a 2020s idol song (heard) and two 1980s/1990s ballads.
        let store_songs = vec![
            tagged("Hype Boy", "NewJeans", IDOL20),
            tagged("Ballad 3", "Lee Sun-hee", BALLAD80),
            tagged("Ballad 4", "Shin Seung-hun", BALLAD90),
        ];
        let all: Vec<(Song, Profile)> = cands.iter().chain(&store_songs).cloned().collect();
        let heard = ["Hype Boy", "Ballad 4"];
        let f = filter(&session, &all, &heard);
        let store = || store_songs.iter().map(|(s, _)| (s.clone(), flow(Some(80.0), None, 0.4))).collect();
        let g = gate_pool(pool, Some(&f), store, &HashSet::new(), MIN_POOL, TOP_UP_TO, None);
        let titles: Vec<&str> = g.songs.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(titles, ["내게 다시", "Ballad 2", "Ballad 4", "Ballad 3"], "{:?}", g.note("AI DJ"));
        assert_eq!((g.kept, g.topped_up, g.heard), (2, 2, 1));
        let r = g.rejected;
        assert_eq!((r.other_era, r.unknown_scene), (3, 1), "{r:?}");
        assert!(r.other_scene + r.no_language >= 3, "house, R&B and trot: {r:?}");
        let note = g.note("AI DJ").unwrap();
        assert!(note.starts_with("AI DJ pool: 2 kept, 7 rejected (other scene"), "{note}");
        assert!(note.contains("1980s"), "{note}");
    }

    #[test]
    fn a_2020s_kpop_pool_keeps_its_scene_and_no_instrumental_house() {
        let n = Neighbours::default();
        let session = Session::led([tagged("Magnetic", "ILLIT", IDOL20).1], &n);
        let cands = vec![
            tagged("Supersonic", "fromis_9", IDOL20),
            tagged("Freak", "Mesto", HOUSE),
            tagged("Ballad", "Byun Jinsub", BALLAD80),
        ];
        let f = filter(&session, &cands, &["Freak"]);
        let pool = cands.iter().map(|c| c.0.clone()).collect();
        let g = gate_pool(pool, Some(&f), Vec::new, &HashSet::new(), 1, TOP_UP_TO, None);
        let titles: Vec<&str> = g.songs.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(
            titles,
            ["Supersonic"],
            "house is turned away even when heard (only pop/electronic shared)"
        );
    }

    #[test]
    fn with_nothing_of_the_scene_left_unknown_songs_keep_the_dj_going() {
        let n = Neighbours::default();
        // Known only by a Hangul title (the phone without the server's scene data).
        let current =
            Profile { scene: Scene::from_script("너만을 느끼며", "더 블루"), ..Default::default() };
        let session = Session::led([current], &n);
        let cands =
            vec![tagged("Supersonic", "fromis_9", ""), tagged("Rap", "MC", "hip hop, english, 2020s")];
        let f = filter(&session, &cands, &[]);
        let pool = cands.iter().map(|c| c.0.clone()).collect();
        let g = gate_pool(pool, Some(&f), Vec::new, &HashSet::new(), MIN_POOL, TOP_UP_TO, None);
        let titles: Vec<&str> = g.songs.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(titles, ["Supersonic"], "the unknown one, never the English rap song");
        assert_eq!(g.kept_unknown, 1);
        assert!(g.note("AI DJ").unwrap().contains("kept 1 of unknown scene"));
    }

    #[test]
    fn no_gate_when_nothing_is_known() {
        let pool = vec![song("a", "x", None, None), song("b", "y", None, None)];
        let g = gate_pool(pool.clone(), None, Vec::new, &HashSet::new(), MIN_POOL, TOP_UP_TO, None);
        assert_eq!(g.songs, pool);
        assert!(g.note("AI DJ").is_none());
    }
}
