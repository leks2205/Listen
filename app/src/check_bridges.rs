//! `listen --check-bridges <analysis dir> <embeddings dir> <history.jsonl> [--pairs N] [--all]`: runs the DJ
//! bridge chooser over consecutive plays of the listening history as the player would (candidates from the
//! history and the whole analysis store) and prints the bridge chosen without and with the scene gate
//! (`listen_core::scene`). By default only pairs of two Korean pop songs are checked (`--all`: every pair).
//! Each bridge shows how its legs mix (kinds and flow costs against the direct join) and how long it
//! plays; the summary counts instrumental electronic records and era misses. `--gain X` overrides the
//! margin each leg must beat a direct join that can't be beat-matched by (to measure it).
//!
//! The analysis dir is opened as a store (its own machine shard is created): point it at a copy of
//! `Listen/analysis`, not the synced folder.

use listen_analysis::store::ShardStore;
use listen_analysis::{bridge_candidates, bridge_scene_gate, record_key, scene_profile};
use listen_core::analysis::{Features, FileId};
use listen_core::history::Entry;
use listen_core::model::{ServerId, Song, SourceKind};
use listen_core::scene::{Families, Neighbours, SceneIndex};
use listen_mix::plan::PlanCtx;
use std::collections::HashSet;
use std::path::Path;

pub fn run(args: &[String]) -> i32 {
    let (Some(store_dir), Some(emb_dir), Some(history)) = (args.first(), args.get(1), args.get(2)) else {
        eprintln!(
            "usage: listen --check-bridges <analysis dir (a copy)> <embeddings/v1/clamp3> <history.jsonl> [--pairs N] [--all]"
        );
        return 2;
    };
    let pairs: usize = args
        .iter()
        .position(|a| a == "--pairs")
        .and_then(|i| args.get(i + 1))
        .and_then(|n| n.parse().ok())
        .unwrap_or(30);
    let all = args.iter().any(|a| a == "--all");
    let gain: Option<f32> =
        args.iter().position(|a| a == "--gain").and_then(|i| args.get(i + 1)).and_then(|n| n.parse().ok());
    match check(Path::new(store_dir), Path::new(emb_dir), Path::new(history), pairs, all, gain) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("✗ {e}");
            1
        }
    }
}

/// What the summary counts over the chosen bridges.
#[derive(Default)]
struct Counts {
    bridged: usize,
    instrumental_electronic: usize,
    era_miss: usize,
    heard: usize,
    both_beat_matched: usize,
    long: usize,
    gains: Vec<f32>,
}

fn check(
    store_dir: &Path,
    emb_dir: &Path,
    history: &Path,
    pairs: usize,
    all: bool,
    gain: Option<f32>,
) -> Result<(), String> {
    let store = ShardStore::open(store_dir, "check-bridges").map_err(|e| e.to_string())?;
    let index = SceneIndex::load_embeddings(emb_dir);
    let text = std::fs::read_to_string(history).map_err(|e| format!("{}: {e}", history.display()))?;
    let entries: Vec<Entry> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    let neighbours = Neighbours::from_history(&entries);
    let heard = listen_analysis::heard_keys(&entries);
    println!(
        "store {} songs; scene index {} songs ({} tagged); history {} plays, {} artist neighbours",
        store.len(),
        index.len(),
        index.tagged(),
        entries.len(),
        neighbours.len()
    );
    let ytm: Option<ServerId> =
        entries.iter().find(|e| e.source == SourceKind::YouTubeMusic).map(|e| e.server_id.clone());
    let lookup = |s: &Song| store.lookup(&record_key(s), &FileId::of(s)).cloned();
    let settings = listen_core::settings::Mixing::default();
    let known: Vec<Song> = entries.iter().rev().map(Entry::to_song).collect();
    // In a fixed order, so runs compare (the candidate cap takes the first ones).
    let mut all_records: Vec<_> = store.best_records().cloned().collect();
    all_records.sort_by(|a, b| a.song_id.cmp(&b.song_id));
    let korean_pop = |s: &Song| {
        let p = scene_profile(&index, s, &record_key(s));
        p.scene.lang == Some("ko") && p.scene.families.0 & Families::POP != 0
    };
    let mut seen = HashSet::new();
    let (mut checked, mut changed, mut before_bridged, mut after_bridged) = (0, 0, 0, 0);
    let mut counts = Counts::default();
    for w in entries.windows(2) {
        if checked >= pairs {
            break;
        }
        let (a, b) = (w[0].to_song(), w[1].to_song());
        if w[1].ts - w[0].ts > 20 * 60 || record_key(&a) == record_key(&b) {
            continue;
        }
        if !all && !(korean_pop(&a) && korean_pop(&b)) {
            continue;
        }
        if !seen.insert((record_key(&a), record_key(&b))) {
            continue;
        }
        let (Some(ra), Some(rb)) = (lookup(&a), lookup(&b)) else { continue };
        let (Some(fa), Some(fb)) = (ra.features(), rb.features()) else { continue };
        // As the player plans: the outgoing song is well into its last minute and a half.
        let ctx = PlanCtx {
            settings: &settings,
            out_position_s: (f64::from(fa.audio_end) - 90.0).max(0.0),
            voice_break: None,
            out_genres: &a.genres,
            in_genres: &b.genres,
            no_echo: false,
        };
        let direct = listen_mix::plan::plan(fa, fb, &ctx);
        let why = listen_mix::bridge::gap(fa, fb, &direct, &settings);
        let skip: HashSet<String> =
            [&a, &b].iter().flat_map(|s| [listen_core::versions::song_key(s), record_key(s)]).collect();
        let (songs, records) = bridge_candidates(
            known.iter().cloned(),
            lookup,
            all_records.iter().cloned(),
            ytm.as_ref(),
            &skip,
            6000,
        );
        checked += 1;
        let feats: Vec<&Features> = records.iter().filter_map(|r| r.features()).collect();
        let before = listen_mix::bridge::choose(fa, fb, &direct, &feats, &ctx).map(|c| {
            let s = &songs[c.index];
            let p = scene_profile(&index, s, &records[c.index].song_id);
            format!("{} – {} [{}]", s.artist_line(), s.title, p.scene.label())
        });
        let (pa, pb) =
            (scene_profile(&index, &a, &record_key(&a)), scene_profile(&index, &b, &record_key(&b)));
        let gated = bridge_scene_gate(&index, &neighbours, pa.clone(), pb.clone(), songs, records, &heard);
        let feats: Vec<&Features> = gated.records.iter().filter_map(|r| r.features()).collect();
        let chosen = match gain {
            Some(g) => {
                let g = if listen_mix::bridge::always_try(&direct) { g } else { g.max(0.25) };
                listen_mix::bridge::choose_with_gain(fa, fb, &direct, &feats, &gated.scores, &ctx, g)
            }
            None => listen_mix::bridge::choose_scored(fa, fb, &direct, &feats, &gated.scores, &ctx),
        };
        let after = chosen.map(|c| {
            let s = &gated.songs[c.index];
            let p = scene_profile(&index, s, &gated.records[c.index].song_id);
            let l = &c.legs;
            let electronic = p.scene.lang.is_none() && p.scene.families.0 & Families::ELECTRONIC != 0;
            let eras: Vec<u16> = [pa.scene.era, pb.scene.era].into_iter().flatten().collect();
            let era_miss = p.scene.era.is_some_and(|e| eras.iter().any(|x| x.abs_diff(e) > 10));
            counts.bridged += 1;
            counts.instrumental_electronic += usize::from(electronic || p.scene.instrumental && p.scene.lang.is_none());
            counts.era_miss += usize::from(era_miss);
            counts.heard += usize::from(gated.heard[c.index]);
            let matched = |k: &str| k == "BeatBlend" || k == "ShortBlend";
            counts.both_beat_matched += usize::from(matched(&l.kind_in) && matched(&l.kind_out));
            counts.long += usize::from(l.cut_s > 60.0 || l.alone_s > 30.0);
            counts.gains.push(l.direct - l.cost_in.max(l.cost_out));
            format!(
                "{} – {} [{}, scene {:.2}{}]{}{}\n            legs: in {} {:.2}, out {} {:.2}; direct {:.2}{} (gain {:.2}); {} bars, {:.0} s, {:.0} s alone",
                s.artist_line(),
                s.title,
                p.scene.label(),
                gated.scores[c.index],
                if gated.heard[c.index] { ", heard" } else { "" },
                if electronic { "  ⚠ instrumental electronic" } else { "" },
                if era_miss { "  ⚠ era" } else { "" },
                l.kind_in,
                l.cost_in,
                l.kind_out,
                l.cost_out,
                l.direct,
                if l.direct_matched { " beat-matched" } else { " not beat-matched" },
                l.direct - l.cost_in.max(l.cost_out),
                c.cut.bars,
                l.cut_s,
                l.alone_s
            )
        });
        before_bridged += usize::from(before.is_some());
        after_bridged += usize::from(after.is_some());
        changed += usize::from(before != after);
        println!(
            "\n{checked:2}. {} – {}  →  {} – {}\n    gap: {}\n    scenes: {}; {}\n    before: {}\n    after:  {}",
            a.artist_line(),
            a.title,
            b.artist_line(),
            b.title,
            why.as_deref().unwrap_or("none (would not want a bridge; chooser run anyway)"),
            gated.pair,
            gated.tally.describe(),
            before.as_deref().unwrap_or("no bridge"),
            after.as_deref().unwrap_or("no bridge"),
        );
    }
    println!(
        "\n{checked} pairs: bridged before {before_bridged}, after {after_bridged}; choice changed in {changed}"
    );
    let mut g = counts.gains.clone();
    g.sort_by(f32::total_cmp);
    let q = |p: f32| g.get(((g.len().saturating_sub(1)) as f32 * p).round() as usize).copied().unwrap_or(0.0);
    println!(
        "after: {} bridged; {} instrumental electronic; {} era misses; {} heard; {} with both legs beat-matched; {} longer than 60 s or 30 s alone; leg gain over the direct join p0 {:.2} p25 {:.2} p50 {:.2} p75 {:.2}",
        counts.bridged,
        counts.instrumental_electronic,
        counts.era_miss,
        counts.heard,
        counts.both_beat_matched,
        counts.long,
        q(0.0),
        q(0.25),
        q(0.5),
        q(0.75)
    );
    Ok(())
}
