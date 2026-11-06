//! `listen --check-tempo <analysis dir> <embeddings dir> <history.jsonl> [--bpm N] [--want N]
//! [--current "Artist – Title"] [--before "Artist – Title"]…`: replays the heart-rate tempo mode's store
//! top-up (`listen_mix::tempo::tempo_fill` with an empty DJ pool, as when none of the DJ's songs fit) for a
//! session and prints what it takes without and with the scene gate (`listen_core::scene::Session`), plus
//! the gate's verdict on each `--probe "Artist – Title"`. Without `--before` the session is the current
//! song and the newest Korean pop plays of the history (a K-pop set).
//!
//! The analysis dir is opened as a store (its own machine shard is created): point it at a copy.

use listen_analysis::store::ShardStore;
use listen_analysis::{record_key, scene_profile, song_of_record, tempo_scene_filter, tempo_session};
use listen_core::analysis::{FileId, FlowFeatures};
use listen_core::history::Entry;
use listen_core::model::{ServerId, Song, SourceKind};
use listen_core::scene::{Families, Neighbours, SceneIndex};
use listen_core::versions::song_key;
use std::collections::HashSet;
use std::path::Path;

fn arg<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(String::as_str)
}

fn args_of<'a>(args: &'a [String], name: &str) -> Vec<&'a str> {
    args.windows(2).filter(|w| w[0] == name).map(|w| w[1].as_str()).collect()
}

pub fn run(args: &[String]) -> i32 {
    let (Some(store_dir), Some(emb_dir), Some(history)) = (args.first(), args.get(1), args.get(2)) else {
        eprintln!(
            "usage: listen --check-tempo <analysis dir (a copy)> <embeddings/v1/clamp3> <history.jsonl> [--bpm N] [--want N] [--current \"Artist – Title\"] [--before \"Artist – Title\"]… [--probe \"Artist – Title\"]…"
        );
        return 2;
    };
    match check(Path::new(store_dir), Path::new(emb_dir), Path::new(history), args) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("✗ {e}");
            1
        }
    }
}

fn check(store_dir: &Path, emb_dir: &Path, history: &Path, args: &[String]) -> Result<(), String> {
    let bpm: f32 = arg(args, "--bpm").and_then(|b| b.parse().ok()).unwrap_or(146.0);
    let want: usize = arg(args, "--want").and_then(|b| b.parse().ok()).unwrap_or(12);
    let store = ShardStore::open(store_dir, "check-tempo").map_err(|e| e.to_string())?;
    let index = SceneIndex::load_embeddings(emb_dir);
    let text = std::fs::read_to_string(history).map_err(|e| format!("{}: {e}", history.display()))?;
    let entries: Vec<Entry> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    let neighbours = Neighbours::from_history(&entries);
    let ytm: ServerId = entries
        .iter()
        .find(|e| e.source == SourceKind::YouTubeMusic)
        .map(|e| e.server_id.clone())
        .unwrap_or_else(|| ServerId("ytm".into()));
    let records: Vec<_> = store.best_records().cloned().collect();
    let by_hint = |hint: &str| -> Option<Song> {
        let h = hint.to_lowercase();
        records.iter().find(|r| r.title_hint.to_lowercase() == h).and_then(|r| song_of_record(r, &ytm))
    };
    let label = |s: &Song| {
        let p = scene_profile(&index, s, &record_key(s));
        p.scene.label()
    };
    // The session: the current song, then the songs before it.
    let current = arg(args, "--current").unwrap_or("TRI.BE – WE ARE YOUNG");
    let mut around: Vec<Song> = by_hint(current).into_iter().collect();
    let before = args_of(args, "--before");
    if before.is_empty() {
        let korean_pop = |s: &Song| {
            let p = scene_profile(&index, s, &record_key(s));
            p.scene.lang == Some("ko") && p.scene.families.0 & Families::POP != 0
        };
        let mut seen = HashSet::new();
        around.extend(
            entries
                .iter()
                .rev()
                .map(Entry::to_song)
                .filter(|s| korean_pop(s) && seen.insert(song_key(s)))
                .take(4),
        );
    } else {
        around.extend(before.iter().filter_map(|h| by_hint(h)));
    }
    let session = tempo_session(&index, &neighbours, around.iter());
    println!(
        "store {} songs; scene index {} songs ({} tagged); history {} plays; target {bpm} BPM, want {want}",
        store.len(),
        index.len(),
        index.tagged(),
        entries.len()
    );
    println!("session ({}):", session.describe());
    for s in &around {
        println!("  {} – {} [{}]", s.artist_line(), s.title, label(s));
    }
    let heard: HashSet<String> = entries
        .iter()
        .filter(|e| e.played())
        .flat_map(|e| {
            let s = e.to_song();
            [song_key(&s), record_key(&s)]
        })
        .collect();
    let target = listen_mix::tempo::target(bpm);
    let lookup = |s: &Song| store.lookup(&record_key(s), &FileId::of(s)).cloned();
    let skip: HashSet<String> = around.iter().flat_map(|s| [song_key(s), record_key(s)]).collect();
    let known: Vec<Song> = entries.iter().rev().map(Entry::to_song).collect();
    let candidates = || {
        let (songs, recs) = listen_analysis::bridge_candidates(
            known.iter().cloned(),
            lookup,
            records.iter().cloned(),
            Some(&ytm),
            &HashSet::new(),
            usize::MAX,
        );
        songs
            .into_iter()
            .zip(recs)
            .filter_map(|(s, r)| r.features().map(|f| (s, FlowFeatures::of(f))))
            .collect::<Vec<_>>()
    };
    let here = around.first().and_then(lookup).and_then(|r| r.features().map(FlowFeatures::of));
    let bpm_of = |s: &Song| {
        lookup(s)
            .and_then(|r| r.features().map(FlowFeatures::of))
            .and_then(|f| listen_mix::tempo::store_bpm(s, &f))
    };
    let show = |title: &str, fill: &listen_mix::tempo::TempoFill| {
        println!(
            "\n{title}: {} songs ({} from the store, {} heard)",
            fill.songs.len(),
            fill.from_store,
            fill.heard
        );
        let session_era = session.describe().contains("2020s") || session.describe().contains("2010s");
        for (i, s) in fill.songs.iter().enumerate() {
            let l = label(s);
            let old = l.contains("1980s") || l.contains("1990s") || l.contains("before 1980");
            let flag = if session_era && old { "  ⚠ old era next to modern K-pop" } else { "" };
            let tempo = bpm_of(s).map(|b| format!("{b:.0}")).unwrap_or_else(|| "?".into());
            let h =
                if heard.contains(&song_key(s)) || heard.contains(&record_key(s)) { " heard" } else { "" };
            println!("  {:2}. {} – {} [{l}] {tempo} BPM{h}{flag}", i + 1, s.artist_line(), s.title);
        }
    };
    let old =
        listen_mix::tempo::tempo_fill(&[], bpm_of, candidates, &skip, &target, want, here.as_ref(), None);
    show("before (sound and tempo only)", &old);
    let filter = tempo_scene_filter(&index, &session, &heard);
    let new = listen_mix::tempo::tempo_fill(
        &[],
        bpm_of,
        candidates,
        &skip,
        &target,
        want,
        here.as_ref(),
        Some(&filter),
    );
    show("after (scene gate)", &new);
    if let Some(note) = new.scene_note(&target) {
        println!("  log: {note}");
    }
    let probes = args_of(args, "--probe");
    if !probes.is_empty() {
        println!("\nprobes:");
    }
    for p in probes {
        match by_hint(p) {
            Some(s) => {
                let verdict = match (filter.judge)(&s) {
                    Ok(score) => format!("passes (scene {score:.2})"),
                    Err(r) => format!("rejected ({r:?})"),
                };
                let tempo = bpm_of(&s).map(|b| format!("{b:.0}")).unwrap_or_else(|| "?".into());
                println!("  {p} [{}] {tempo} BPM: {verdict}", label(&s));
            }
            None => println!("  {p}: not in the store"),
        }
    }
    Ok(())
}
