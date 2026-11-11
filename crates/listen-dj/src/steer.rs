//! "Tell the DJ": the listener's requests for the AI DJ (chips such as Calmer or No ballads, or free
//! text). They go into every plan prompt as listener requests and narrow the candidate pool with the
//! measured energy and tempo before Gemini sees it, so "calmer" cannot be ignored by the model.
//! Requests are kept in a small JSON file until the listener removes them (they survive a restart).

use serde::{Deserialize, Serialize};
use std::path::Path;

/// At most this many requests are active; the oldest goes when another one is added.
pub const MAX_REQUESTS: usize = 6;
/// A typed request is cut to this many characters.
pub const MAX_TEXT: usize = 120;
/// The pool filters never leave fewer candidates than this (the DJ needs room to pick a flowing set).
pub const MIN_POOL: usize = 8;

/// Calmer / More energy keep only the songs on the requested side of the bar once at least this many are
/// there: one is enough (a short set beats one that breaks the request, bug 124). Only when no candidate is
/// on that side, the best-fitting [`MIN_POOL`] stay (the request can't be met from this pool).
pub const MIN_STEERED: usize = 1;

/// A quick request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Chip {
    Calmer,
    MoreEnergy,
    MoreNew,
    OnlyFavourites,
    Focus,
    KoreanOnly,
    NoBallads,
}

impl Chip {
    pub const ALL: [Chip; 7] = [
        Chip::Calmer,
        Chip::MoreEnergy,
        Chip::MoreNew,
        Chip::OnlyFavourites,
        Chip::Focus,
        Chip::KoreanOnly,
        Chip::NoBallads,
    ];

    /// The English label (both apps translate it).
    pub fn label(self) -> &'static str {
        match self {
            Chip::Calmer => "Calmer",
            Chip::MoreEnergy => "More energy",
            Chip::MoreNew => "More new songs",
            Chip::OnlyFavourites => "Only favourites",
            Chip::Focus => "Focus",
            Chip::KoreanOnly => "Korean only",
            Chip::NoBallads => "No ballads",
        }
    }

    /// Stable id for UIs, MCP and the CLI ("calmer", "more_energy", …).
    pub fn id(self) -> &'static str {
        match self {
            Chip::Calmer => "calmer",
            Chip::MoreEnergy => "more_energy",
            Chip::MoreNew => "more_new",
            Chip::OnlyFavourites => "only_favourites",
            Chip::Focus => "focus",
            Chip::KoreanOnly => "korean_only",
            Chip::NoBallads => "no_ballads",
        }
    }

    pub fn from_id(id: &str) -> Option<Chip> {
        let id = id.trim().to_lowercase().replace(['-', ' '], "_");
        Chip::ALL.into_iter().find(|c| c.id() == id || c.label().to_lowercase().replace(' ', "_") == id)
    }

    /// What the request asks of the DJ, for the plan prompt.
    fn prompt(self) -> &'static str {
        match self {
            Chip::Calmer => {
                "Calmer: softer, lower-energy songs than what played so far; ease the energy down."
            }
            Chip::MoreEnergy => "More energy: livelier, more energetic songs; build the energy up.",
            Chip::MoreNew => "More new songs: mostly songs tagged new to the listener.",
            Chip::OnlyFavourites => {
                "Only favourites: only songs the listener knows well (tagged favourite or with several plays)."
            }
            Chip::Focus => {
                "Focus: the listener is working — steady, unobtrusive songs without big jumps in energy, and keep \
                 your breaks very short."
            }
            Chip::KoreanOnly => "Korean only: only songs sung in Korean.",
            Chip::NoBallads => "No ballads: no slow, soft ballads.",
        }
    }

    /// Chips that ask for the opposite: adding one drops the other.
    fn opposite(self) -> Option<Chip> {
        match self {
            Chip::Calmer => Some(Chip::MoreEnergy),
            Chip::MoreEnergy => Some(Chip::Calmer),
            Chip::MoreNew => Some(Chip::OnlyFavourites),
            Chip::OnlyFavourites => Some(Chip::MoreNew),
            _ => None,
        }
    }
}

/// One active request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Steer {
    Chip(Chip),
    Text(String),
}

impl Steer {
    /// A typed request: trimmed and cut to [`MAX_TEXT`] characters; a typed chip label becomes the chip.
    pub fn text(s: &str) -> Option<Steer> {
        let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
        if s.is_empty() {
            return None;
        }
        if let Some(chip) = Chip::ALL.into_iter().find(|c| c.label().eq_ignore_ascii_case(&s)) {
            return Some(Steer::Chip(chip));
        }
        Some(Steer::Text(s.chars().take(MAX_TEXT).collect()))
    }

    /// The English label of a chip, or the typed text.
    pub fn label(&self) -> &str {
        match self {
            Steer::Chip(c) => c.label(),
            Steer::Text(t) => t,
        }
    }

    fn prompt(&self) -> String {
        match self {
            Steer::Chip(c) => c.prompt().to_string(),
            Steer::Text(t) => format!("In their own words: \"{t}\""),
        }
    }
}

/// The active requests, oldest first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Requests {
    #[serde(default)]
    pub items: Vec<Steer>,
}

impl Requests {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn has(&self, chip: Chip) -> bool {
        self.items.contains(&Steer::Chip(chip))
    }

    /// Adds a request; `false` when it was already active (nothing changed, no replan).
    pub fn add(&mut self, steer: Steer) -> bool {
        if self.items.contains(&steer) {
            return false;
        }
        if let Steer::Chip(c) = &steer
            && let Some(o) = c.opposite()
        {
            self.items.retain(|s| *s != Steer::Chip(o));
        }
        self.items.push(steer);
        while self.items.len() > MAX_REQUESTS {
            self.items.remove(0);
        }
        true
    }

    /// Adds `chip`, or removes it when active (a chip button toggles); always a change.
    pub fn toggle(&mut self, chip: Chip) {
        if !self.remove(&Steer::Chip(chip)) {
            self.add(Steer::Chip(chip));
        }
    }

    /// Removes a request; `false` when it wasn't active.
    pub fn remove(&mut self, steer: &Steer) -> bool {
        let before = self.items.len();
        self.items.retain(|s| s != steer);
        self.items.len() != before
    }

    /// Removes the request at `index`; `false` when there is none.
    pub fn remove_at(&mut self, index: usize) -> bool {
        if index < self.items.len() {
            self.items.remove(index);
            true
        } else {
            false
        }
    }

    pub fn clear(&mut self) -> bool {
        let had = !self.items.is_empty();
        self.items.clear();
        had
    }

    /// The section of the plan prompt (empty without requests). `fresh`: a request just changed, so the
    /// break acknowledges it.
    pub fn prompt(&self, fresh: bool) -> String {
        if self.items.is_empty() {
            return String::new();
        }
        let mut p = String::from(
            "\nThe listener's requests right now (they outrank the seed's mood where they conflict; every song \
             of this set must respect them):\n",
        );
        for s in &self.items {
            p.push_str("- ");
            p.push_str(&s.prompt());
            p.push('\n');
        }
        if fresh {
            p.push_str(
                "The listener just told you this: if you speak, acknowledge it in a few words (\"you wanted it \
                 calmer — …\") without quoting it back word for word.\n",
            );
        }
        p
    }

    /// Reads the requests; a missing or broken file is no requests.
    pub fn load(path: &Path) -> Requests {
        std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?)?;
        std::fs::rename(tmp, path)
    }

    /// Calls for more of the listener's favourites in the pool (the history bucket gets bigger).
    pub fn wants_favourites(&self) -> bool {
        self.has(Chip::OnlyFavourites)
    }
}

/// What the pool filters know of a candidate.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Facts {
    /// Measured energy 0..1 (audio analysis).
    pub energy: Option<f32>,
    /// Measured tempo (confident beat grids only).
    pub bpm: Option<f32>,
    pub new_to_you: bool,
    pub plays: u32,
}

/// Plays for a song to count as one the listener knows well ("Only favourites").
pub const KNOWN_PLAYS: u32 = 3;

/// The mean energy of `songs` that have one (the replaced part of a set: "calmer" means calmer than it).
pub fn mean_energy<T>(songs: &[T], facts: impl Fn(&T) -> Facts) -> Option<f32> {
    let e: Vec<f32> = songs.iter().filter_map(|s| facts(s).energy).collect();
    (!e.is_empty()).then(|| e.iter().sum::<f32>() / e.len() as f32)
}

fn median(mut v: Vec<f32>) -> Option<f32> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(f32::total_cmp);
    Some(v[v.len() / 2])
}

/// Keeps the candidates that fit the requests, in their order. Each filter applies only when it leaves
/// at least [`MIN_POOL`] songs; an energy request keeps every song on its side of the bar when at least
/// [`MIN_STEERED`] are, else the [`MIN_POOL`] that fit it best. `reference`: the energy to beat for Calmer / More energy (the replaced songs' mean, else the
/// song playing now); without one, the pool's median. Songs without analysis never pass an energy filter
/// when enough analysed ones do, and a pool without any analysed song is left alone (with or without a
/// reference: nothing in it is measured, so there is nothing to rank by).
pub fn filter_pool<T: Clone>(
    pool: Vec<T>,
    facts: impl Fn(&T) -> Facts,
    requests: &Requests,
    reference: Option<f32>,
) -> Vec<T> {
    let mut pool: Vec<(T, Facts)> = pool.into_iter().map(|s| (s.clone(), facts(&s))).collect();
    let narrow = |pool: &mut Vec<(T, Facts)>, keep: &dyn Fn(&Facts) -> bool| {
        let kept: Vec<(T, Facts)> = pool.iter().filter(|(_, f)| keep(f)).cloned().collect();
        if kept.len() >= MIN_POOL {
            *pool = kept;
        }
    };
    for steer in &requests.items {
        let Steer::Chip(chip) = steer else { continue };
        match chip {
            Chip::Calmer | Chip::MoreEnergy => {
                let calmer = *chip == Chip::Calmer;
                let known: Vec<f32> = pool.iter().filter_map(|(_, f)| f.energy).collect();
                // No candidate measured: the request can't narrow this pool, even when the reference is known.
                if known.is_empty() {
                    continue;
                }
                let Some(bar) = reference.or_else(|| median(known)) else { continue };
                let fits = |e: f32| if calmer { e < bar } else { e > bar };
                let kept: Vec<(T, Facts)> =
                    pool.iter().filter(|(_, f)| f.energy.is_some_and(fits)).cloned().collect();
                if kept.len() >= MIN_STEERED {
                    pool = kept;
                } else {
                    // Too few on the right side: the best-fitting analysed songs, then the rest in order.
                    let mut ranked: Vec<usize> =
                        (0..pool.len()).filter(|&i| pool[i].1.energy.is_some()).collect();
                    ranked.sort_by(|&a, &b| {
                        let (ea, eb) = (pool[a].1.energy.unwrap_or(0.5), pool[b].1.energy.unwrap_or(0.5));
                        if calmer { ea.total_cmp(&eb) } else { eb.total_cmp(&ea) }
                    });
                    ranked.extend((0..pool.len()).filter(|&i| pool[i].1.energy.is_none()));
                    ranked.truncate(MIN_POOL.max(kept.len()));
                    ranked.sort_unstable();
                    pool = ranked.into_iter().map(|i| pool[i].clone()).collect();
                }
            }
            Chip::NoBallads => narrow(&mut pool, &|f| !is_ballad(f)),
            Chip::Focus => narrow(&mut pool, &|f| f.energy.is_none_or(|e| e <= 0.75)),
            Chip::OnlyFavourites => narrow(&mut pool, &|f| f.plays >= KNOWN_PLAYS),
            Chip::MoreNew => {
                // Mostly new songs, with a few known ones for the flow.
                let new = pool.iter().filter(|(_, f)| f.new_to_you).count();
                if new >= MIN_POOL / 2 {
                    let mut known = 0;
                    pool.retain(|(_, f)| {
                        f.new_to_you || {
                            known += 1;
                            known <= 3
                        }
                    });
                }
            }
            Chip::KoreanOnly => {} // the prompt carries it: titles and artist names don't tell the language
        }
    }
    pool.into_iter().map(|(s, _)| s).collect()
}

/// A slow, soft song: measured energy below 0.35, or below 0.45 at under 90 bpm.
fn is_ballad(f: &Facts) -> bool {
    match f.energy {
        Some(e) => e < 0.35 || (e < 0.45 && f.bpm.is_some_and(|b| b < 90.0)),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool(energies: &[Option<f32>]) -> Vec<(usize, Facts)> {
        energies.iter().enumerate().map(|(i, e)| (i, Facts { energy: *e, ..Facts::default() })).collect()
    }

    fn ids(p: Vec<(usize, Facts)>) -> Vec<usize> {
        p.into_iter().map(|(i, _)| i).collect()
    }

    fn with(chips: &[Chip]) -> Requests {
        let mut r = Requests::default();
        for c in chips {
            r.add(Steer::Chip(*c));
        }
        r
    }

    #[test]
    fn calmer_keeps_songs_below_the_replaced_ones() {
        let energies: Vec<Option<f32>> = (0..20).map(|i| Some(i as f32 / 20.0)).collect();
        let kept = ids(filter_pool(pool(&energies), |s| s.1, &with(&[Chip::Calmer]), Some(0.6)));
        assert_eq!(kept, (0..12).collect::<Vec<_>>());
        let kept = ids(filter_pool(pool(&energies), |s| s.1, &with(&[Chip::MoreEnergy]), Some(0.6)));
        // Only 7 songs above 0.6: those 7, not the song at the bar.
        assert_eq!(kept, (13..20).collect::<Vec<_>>());
    }

    #[test]
    fn a_thin_pool_never_admits_songs_on_the_wrong_side_of_the_bar() {
        // The QA case: 16 candidates, only three below the replaced set's 0.32.
        let e: Vec<Option<f32>> =
            [0.5, 0.3, 0.5, 0.4, 0.6, 0.3, 0.7, 0.5, 0.3, 0.8, 0.6, 0.45, 0.9, 0.55, 0.65, 0.7]
                .into_iter()
                .map(Some)
                .collect();
        let kept = ids(filter_pool(pool(&e), |s| s.1, &with(&[Chip::Calmer]), Some(0.32)));
        assert_eq!(kept, vec![1, 5, 8]);
        // Every pool with enough songs on the requested side has its mean on that side.
        for bar in [0.2f32, 0.35, 0.5, 0.65, 0.8] {
            for (chip, below) in [(Chip::Calmer, true), (Chip::MoreEnergy, false)] {
                let kept = filter_pool(pool(&e), |s| s.1, &with(&[chip]), Some(bar));
                let side = e.iter().flatten().filter(|&&x| if below { x < bar } else { x > bar }).count();
                if side < MIN_STEERED {
                    continue;
                }
                let mean = kept.iter().map(|s| s.1.energy.unwrap()).sum::<f32>() / kept.len() as f32;
                assert!(if below { mean < bar } else { mean > bar }, "{chip:?} bar {bar}: mean {mean}");
            }
        }
    }

    #[test]
    fn one_or_two_songs_on_the_requested_side_are_the_whole_pool() {
        // Bug 124: the check's round 2 had only Holocene (0.25) below the replaced set's 0.30; the eight
        // best-fitting let 0.35 / 0.45 in and the set came out louder. Now only the songs below stay.
        let e: Vec<Option<f32>> =
            [0.35, 0.45, 0.25, 0.30, 0.40, 0.30, 0.50, 0.38, 0.30, 0.42, 0.33, 0.36, 0.31, 0.48, 0.30, 0.44]
                .into_iter()
                .map(Some)
                .collect();
        let kept = filter_pool(pool(&e), |s| s.1, &with(&[Chip::Calmer]), Some(0.30));
        assert_eq!(ids(kept.clone()), vec![2]);
        let mut two = e.clone();
        two[10] = Some(0.28);
        let kept = filter_pool(pool(&two), |s| s.1, &with(&[Chip::Calmer]), Some(0.30));
        assert_eq!(ids(kept.clone()), vec![2, 10]);
        // Whatever a planner picks from it, the set is calmer than the songs it replaces.
        assert!(kept.iter().all(|s| s.1.energy.unwrap() < 0.30));
        let up = filter_pool(pool(&e), |s| s.1, &with(&[Chip::MoreEnergy]), Some(0.49));
        assert_eq!(ids(up), vec![6]);
    }

    #[test]
    fn no_song_on_the_requested_side_keeps_the_calmest_and_the_order() {
        // Nothing below the bar: the eight calmest stay, in pool order; unknown energy last.
        let e = [
            Some(0.9),
            Some(0.2),
            None,
            Some(0.8),
            Some(0.7),
            Some(0.1),
            Some(0.6),
            Some(0.95),
            Some(0.65),
            Some(0.85),
        ];
        let kept = ids(filter_pool(pool(&e), |s| s.1, &with(&[Chip::Calmer]), Some(0.05)));
        assert_eq!(kept.len(), MIN_POOL);
        assert!(!kept.contains(&2), "unknown energy left out while analysed songs remain: {kept:?}");
        assert!(!kept.contains(&7), "the loudest song goes: {kept:?}");
        assert!(kept.windows(2).all(|w| w[0] < w[1]), "pool order kept: {kept:?}");
    }

    #[test]
    fn calmer_without_reference_uses_the_pool_median() {
        let energies: Vec<Option<f32>> = (0..20).map(|i| Some(i as f32 / 20.0)).collect();
        let kept = ids(filter_pool(pool(&energies), |s| s.1, &with(&[Chip::Calmer]), None));
        assert_eq!(kept, (0..10).collect::<Vec<_>>());
    }

    #[test]
    fn energy_requests_leave_an_unanalysed_pool_alone_even_with_a_reference() {
        // The replaced songs (or the song playing now) are analysed, the candidates are not.
        let e = vec![None; 12];
        for chip in [Chip::Calmer, Chip::MoreEnergy] {
            let kept = ids(filter_pool(pool(&e), |s| s.1, &with(&[chip]), Some(0.5)));
            assert_eq!(kept, (0..12).collect::<Vec<_>>(), "{chip:?}");
        }
    }

    #[test]
    fn no_analysis_leaves_the_pool_alone() {
        let e = vec![None; 12];
        for chip in Chip::ALL {
            assert_eq!(filter_pool(pool(&e), |s| s.1, &with(&[chip]), None).len(), 12, "{chip:?}");
        }
    }

    #[test]
    fn no_ballads_drops_slow_soft_songs() {
        let mut p: Vec<(usize, Facts)> =
            (0..10).map(|i| (i, Facts { energy: Some(0.7), bpm: Some(120.0), ..Facts::default() })).collect();
        p.push((10, Facts { energy: Some(0.3), bpm: Some(140.0), ..Facts::default() }));
        p.push((11, Facts { energy: Some(0.4), bpm: Some(72.0), ..Facts::default() }));
        p.push((12, Facts { energy: Some(0.4), bpm: Some(120.0), ..Facts::default() }));
        let kept = ids(filter_pool(p, |s| s.1, &with(&[Chip::NoBallads]), None));
        assert!(!kept.contains(&10) && !kept.contains(&11) && kept.contains(&12), "{kept:?}");
    }

    #[test]
    fn favourites_and_new_songs() {
        let p: Vec<(usize, Facts)> = (0..20)
            .map(|i| (i, Facts { plays: if i < 9 { 5 } else { 0 }, new_to_you: i >= 9, ..Facts::default() }))
            .collect();
        let fav = ids(filter_pool(p.clone(), |s| s.1, &with(&[Chip::OnlyFavourites]), None));
        assert_eq!(fav, (0..9).collect::<Vec<_>>());
        let new = ids(filter_pool(p, |s| s.1, &with(&[Chip::MoreNew]), None));
        assert_eq!(new, (0..3).chain(9..20).collect::<Vec<_>>());
    }

    #[test]
    fn requests_toggle_replace_opposites_and_dedupe() {
        let mut r = Requests::default();
        assert!(r.add(Steer::Chip(Chip::Calmer)));
        assert!(!r.add(Steer::Chip(Chip::Calmer)), "the same request again changes nothing");
        assert!(r.add(Steer::Chip(Chip::MoreEnergy)));
        assert_eq!(r.items, vec![Steer::Chip(Chip::MoreEnergy)]);
        r.toggle(Chip::MoreEnergy);
        assert!(r.is_empty());
        for i in 0..10 {
            r.add(Steer::Text(format!("request {i}")));
        }
        assert_eq!(r.items.len(), MAX_REQUESTS);
        assert_eq!(r.items[0], Steer::Text("request 4".into()));
        assert_eq!(Steer::text("  no   ballads "), Some(Steer::Chip(Chip::NoBallads)));
        assert_eq!(Steer::text("   "), None);
        assert_eq!(Steer::text(&"x".repeat(500)).unwrap().label().chars().count(), MAX_TEXT);
        assert_eq!(Chip::from_id("more-energy"), Some(Chip::MoreEnergy));
        assert_eq!(Chip::from_id("Only favourites"), Some(Chip::OnlyFavourites));
    }

    #[test]
    fn requests_survive_a_restart() {
        let dir = std::env::temp_dir().join(format!("listen-steer-{}", std::process::id()));
        let file = dir.join("requests.json");
        let r = with(&[Chip::Calmer, Chip::KoreanOnly]);
        let mut r2 = r.clone();
        r2.add(Steer::Text("more 2010s girl groups".into()));
        r2.save(&file).unwrap();
        assert_eq!(Requests::load(&file), r2);
        std::fs::write(&file, "not json").unwrap();
        assert!(Requests::load(&file).is_empty());
        let _ = std::fs::remove_dir_all(dir);
        assert_ne!(r, r2);
    }

    #[test]
    fn prompt_lists_requests_and_asks_for_an_acknowledgement_once() {
        assert_eq!(Requests::default().prompt(true), "");
        let mut r = with(&[Chip::Calmer]);
        r.add(Steer::Text("more 2010s girl groups".into()));
        let p = r.prompt(true);
        assert!(
            p.contains("Calmer:") && p.contains("\"more 2010s girl groups\"") && p.contains("acknowledge")
        );
        assert!(!r.prompt(false).contains("acknowledge"));
    }
}
