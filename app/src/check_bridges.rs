//! `listen --check-bridges <analysis dir> <embeddings dir> <history.jsonl> [--pairs N] [--all]`: runs the DJ
//! bridge chooser over consecutive plays of the listening history as the player would (candidates from the
//! history and the whole analysis store) and prints the bridge chosen without and with the scene gate
//! (`listen_core::scene`). By default only pairs of two Korean pop songs are checked (`--all`: every pair).
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
    match check(Path::new(store_dir), Path::new(emb_dir), Path::new(history), pairs, all) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("✗ {e}");
            1
        }
    }
}

fn check(store_dir: &Path, emb_dir: &Path, history: &Path, pairs: usize, all: bool) -> Result<(), String> {
    let store = ShardStore::open(store_dir, "check-bridges").map_err(|e| e.to_string())?;
    let index = SceneIndex::load_embeddings(emb_dir);
    let text = std::fs::read_to_string(history).map_err(|e| format!("{}: {e}", history.display()))?;
    let entries: Vec<Entry> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    let neighbours = Neighbours::from_history(&entries);
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
    let all_records: Vec<_> = store.best_records().cloned().collect();
    let korean_pop = |s: &Song| {
        let p = scene_profile(&index, s, &record_key(s));
        p.scene.lang == Some("ko") && p.scene.families.0 & Families::POP != 0
    };
    let mut seen = HashSet::new();
    let (mut checked, mut changed, mut before_bridged, mut after_bridged) = (0, 0, 0, 0);
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
        let gated = bridge_scene_gate(
            &index,
            &neighbours,
            scene_profile(&index, &a, &record_key(&a)),
            scene_profile(&index, &b, &record_key(&b)),
            songs,
            records,
        );
        let feats: Vec<&Features> = gated.records.iter().filter_map(|r| r.features()).collect();
        let after =
            listen_mix::bridge::choose_scored(fa, fb, &direct, &feats, &gated.scores, &ctx).map(|c| {
                let s = &gated.songs[c.index];
                let p = scene_profile(&index, s, &gated.records[c.index].song_id);
                format!(
                    "{} – {} [{}, scene {:.2}]",
                    s.artist_line(),
                    s.title,
                    p.scene.label(),
                    gated.scores[c.index]
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
    Ok(())
}
