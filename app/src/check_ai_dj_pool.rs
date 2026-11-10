//! `listen --check-ai-dj-pool <analysis dir> <embeddings dir> <history.jsonl> [--at TITLE]…`: replays the
//! AI DJ's candidate pool at moments of the listening history and prints it before and after the scene
//! gate (`listen_mix::pool::gate_pool`), with each song's scene and era.
//!
//! Offline, so the pool is rebuilt from what is on disk: the songs that sound closest to the current one
//! (CLaMP 3 embeddings of the describe-it index) stand in for the source's similar songs (30, as the DJ
//! asks for), the history's heavy-rotation and forgotten favourites as the DJ takes them (5), and 8 random
//! analysed store songs (fixed per moment). The session is the current song and the plays before it.
//! Default moments: the last "To a Laday" and "Don't know" plays.
//!
//! The analysis dir is opened as a store (its own machine shard is created): point it at a copy.

use listen_analysis::{bridge_candidates, record_key, scene_profile, song_of_record};
use listen_core::analysis::{FileId, FlowFeatures};
use listen_core::history::Entry;
use listen_core::model::{ServerId, Song, SourceKind};
use listen_core::scene::Neighbours;
use listen_core::versions::song_key;
use std::collections::HashSet;
use std::path::Path;

/// Songs that sound closest, standing in for the source's similar songs.
const SIMILAR: usize = 30;
const RANDOM: usize = 8;
const HISTORY_SLOTS: usize = 5;
const HISTORY_LOOKUP: usize = 30;
/// Plays before the current one that tell the session (as the app's queue look-back).
const SESSION_LOOK: usize = 8;

pub fn run(args: &[String]) -> i32 {
    let (Some(store_dir), Some(emb_dir), Some(history)) = (args.first(), args.get(1), args.get(2)) else {
        eprintln!(
            "usage: listen --check-ai-dj-pool <analysis dir (a copy)> <embeddings/v1/clamp3> <history.jsonl> [--at TITLE]…"
        );
        return 2;
    };
    let mut at: Vec<String> = args.windows(2).filter(|w| w[0] == "--at").map(|w| w[1].clone()).collect();
    if at.is_empty() {
        at = vec!["To a Laday".into(), "Don't know".into()];
    }
    match check(Path::new(store_dir), Path::new(emb_dir), Path::new(history), &at) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("✗ {e}");
            1
        }
    }
}

fn check(store_dir: &Path, emb_dir: &Path, history: &Path, at: &[String]) -> Result<(), String> {
    let (store, index, entries) =
        crate::check_bridges::replay_inputs(store_dir, emb_dir, history, "check-ai-dj-pool")?;
    let neighbours = Neighbours::from_history(&entries);
    println!(
        "store {} songs; scene index {} songs ({} tagged); history {} plays",
        store.len(),
        index.len(),
        index.tagged(),
        entries.len()
    );
    let ytm: ServerId = entries
        .iter()
        .find(|e| e.source == SourceKind::YouTubeMusic)
        .map(|e| e.server_id.clone())
        .ok_or("no YouTube Music plays in the history")?;
    let lookup = |s: &Song| store.lookup(&record_key(s), &FileId::of(s)).cloned();
    let mut records: Vec<_> = store.best_records().cloned().collect();
    records.sort_by(|a, b| a.song_id.cmp(&b.song_id));
    let store_songs: Vec<Song> = records.iter().filter_map(|r| song_of_record(r, &ytm)).collect();
    let label = |s: &Song| {
        let p = scene_profile(&index, s, &record_key(s));
        listen_dj::scene::label(&p.scene).unwrap_or_else(|| "unknown".into())
    };
    for title in at {
        let Some(i) = entries.iter().rposition(|e| e.title.contains(title.as_str())) else {
            println!("\n“{title}”: not in the history");
            continue;
        };
        let current = entries[i].to_song();
        let now = entries[i].ts;
        let before = &entries[..i];
        println!(
            "\n=== {} – {} [{}] ({} plays before it)",
            current.artist_line(),
            current.title,
            label(&current),
            before.len()
        );
        // The session: the current song, then the plays before it (newest first).
        let around: Vec<Song> = std::iter::once(current.clone())
            .chain(before.iter().rev().take(SESSION_LOOK).map(Entry::to_song))
            .collect();
        println!(
            "    before it: {}",
            around[1..].iter().map(|s| format!("{} [{}]", s.title, label(s))).collect::<Vec<_>>().join(", ")
        );
        let exclude: HashSet<String> = around.iter().flat_map(|s| [song_key(s), record_key(s)]).collect();
        let usable = |s: &Song| !exclude.contains(&song_key(s)) && !exclude.contains(&record_key(s));
        // The pool as the DJ builds it (offline stand-ins, see the module docs).
        let here = scene_profile(&index, &current, &record_key(&current));
        let mut similar: Vec<(f32, Song)> = match &here.audio {
            Some(q) => store_songs
                .iter()
                .filter(|s| usable(s))
                .filter_map(|s| {
                    let p = scene_profile(&index, s, &record_key(s));
                    p.audio.map(|a| (a.cos(q), s.clone()))
                })
                .collect(),
            None => Vec::new(),
        };
        similar.sort_by(|a, b| b.0.total_cmp(&a.0));
        let (heavy, rediscover) = listen_dj::familiarity::history_candidates(before, now, HISTORY_LOOKUP);
        let history = listen_dj::familiarity::mix_buckets(
            heavy.into_iter().filter(|s| usable(s)).collect(),
            rediscover.into_iter().filter(|s| usable(s)).collect(),
            HISTORY_SLOTS,
        );
        let mut random: Vec<Song> = Vec::new();
        let mut x = now as u64 | 1;
        while random.len() < RANDOM && !store_songs.is_empty() {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let s = &store_songs[(x % store_songs.len() as u64) as usize];
            if usable(s) && !random.iter().any(|r| r.id == s.id) {
                random.push(s.clone());
            }
        }
        let mut seen = HashSet::new();
        let pool: Vec<Song> = similar
            .into_iter()
            .take(SIMILAR)
            .map(|(_, s)| s)
            .chain(history)
            .chain(random)
            .filter(|s| seen.insert(song_key(s)))
            .collect();
        // The gate, as the app runs it.
        let session = listen_analysis::tempo_session(&index, &neighbours, around.iter());
        let heard = listen_analysis::heard_keys(before);
        let filter = listen_analysis::tempo_scene_filter(&index, &session, &heard);
        let known: Vec<Song> = before.iter().rev().map(Entry::to_song).collect();
        let candidates = || -> Vec<(Song, FlowFeatures)> {
            let (songs, recs) = bridge_candidates(
                known.iter().cloned(),
                lookup,
                records.iter().cloned(),
                Some(&ytm),
                &exclude,
                usize::MAX,
            );
            songs
                .into_iter()
                .zip(recs)
                .filter_map(|(s, r)| r.features().map(|f| (s, FlowFeatures::of(f))))
                .collect()
        };
        let here_flow = lookup(&current).and_then(|r| r.features().map(FlowFeatures::of));
        println!("    {}", session.describe());
        println!("    before the gate: {} candidates; {}", pool.len(), composition(&pool, &label));
        for s in &pool {
            let verdict = match (filter.judge)(s) {
                Ok(_) => "kept".to_string(),
                Err(r) => format!("✗ {r:?}"),
            };
            println!("      {:<16} {} – {} [{}]", verdict, s.artist_line(), s.title, label(s));
        }
        let g = listen_mix::pool::gate_pool(
            pool,
            Some(&filter),
            candidates,
            &exclude,
            listen_mix::pool::MIN_POOL,
            listen_mix::pool::TOP_UP_TO,
            here_flow.as_ref(),
        );
        println!("    {}", g.note("AI DJ").unwrap_or_else(|| "no gate (scene unknown)".into()));
        println!("    after the gate: {} candidates; {}", g.songs.len(), composition(&g.songs, &label));
        for s in &g.songs {
            println!("      {} – {} [{}]", s.artist_line(), s.title, label(s));
        }
        println!(
            "    prompt rule: {}",
            listen_dj::scene::rule("", filter.scene.as_ref()).chars().take(160).collect::<String>()
        );
    }
    Ok(())
}

/// "12 Korean · pop · 1990s, 3 Korean · pop · 2020s, …" (most common first).
fn composition(songs: &[Song], label: &dyn Fn(&Song) -> String) -> String {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for s in songs {
        let l = label(s);
        match counts.iter_mut().find(|(x, _)| *x == l) {
            Some(c) => c.1 += 1,
            None => counts.push((l, 1)),
        }
    }
    counts.sort_by_key(|c| std::cmp::Reverse(c.1));
    counts.iter().map(|(l, n)| format!("{n} {l}")).collect::<Vec<_>>().join(", ")
}
