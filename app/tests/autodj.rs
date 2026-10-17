//! Auto-DJ picks: respects item count, skips songs already queued, marks them auto.

use listen::app::{Listen, Message};
use listen::{Args, demo};

#[test]
fn autodj_appends_unique_songs() {
    let (mut state, _) = Listen::boot(&Args { demo: true, ..Args::default() });
    let before = state.queue.len();
    let queued_id = state.queue.iter().next().unwrap().song.id.clone();
    // Candidates: one already queued, a duplicate pair, and plenty of new ones.
    let mut candidates = demo::album_songs(1);
    candidates.extend(demo::album_songs(2));
    candidates.insert(0, state.queue.iter().next().unwrap().song.clone());
    candidates.insert(1, candidates[2].clone());
    let _ = state.update(Message::AutoDjLoaded(Ok(candidates)));

    let count = state.config.settings.auto_dj.item_count as usize;
    assert_eq!(state.queue.len(), before + count);
    let added: Vec<_> = state.queue.iter().skip(before).collect();
    assert!(added.iter().all(|i| i.auto));
    assert!(added.iter().all(|i| i.song.id != queued_id));
    let mut ids: Vec<_> = added.iter().map(|i| i.song.id.clone()).collect();
    ids.dedup();
    assert_eq!(ids.len(), count, "no duplicates");
}

/// A record saying `song` runs at `bpm` in Camelot key `number``A`.
fn analysed(
    song: &listen_core::model::Song,
    bpm: f32,
    number: u8,
) -> std::sync::Arc<listen_core::analysis::TrackAnalysis> {
    use listen_core::analysis::*;
    let period = 60.0 / bpm;
    let beats: Vec<f32> = (0..400).map(|i| 0.1 + i as f32 * period).collect();
    std::sync::Arc::new(TrackAnalysis {
        version: ANALYZER_VERSION,
        analyzed_at: 1,
        machine: "test".into(),
        decoder: "test".into(),
        // Namespaced like the worker's records (`ytm:` for YouTube Music).
        song_id: listen_analysis::record_key(song),
        server_hint: String::new(),
        file: FileId::of(song),
        title_hint: song.title.clone(),
        outcome: Outcome::Ok(Box::new(Features {
            audio_start: 0.1,
            audio_end: song.duration.as_secs_f32().max(200.0),
            grid: Some(BeatGrid {
                beats,
                downbeat_phase: 0,
                beats_per_bar: 4,
                bpm,
                confidence: 0.9,
                stability: 0.99,
            }),
            key: Some((Camelot { number, minor: true }, 0.9)),
            lufs: -9.0,
            lra: 5.0,
            bars: BarCurves::default(),
            phrase_bars: 8,
            phrase_offset: 0,
            sections: vec![],
            intro_instrumental_bars: 8,
            outro_instrumental_bars: 8,
            timbre: vec![],
            mood: Mood { energy: 0.6, danceability: 0.7, brightness: 0.5 },
        })),
    })
}

#[test]
fn autodj_orders_picks_by_flow_when_analysed() {
    let (mut state, _) = Listen::boot(&Args { demo: true, ..Args::default() });
    state.queue.set(demo::album_songs(0), 0);
    let current = state.queue.current().unwrap().song.clone();
    let candidates = demo::album_songs(3);
    let handle = listen::app::analysis::AnalysisHandle::default();
    handle.insert([analysed(&current, 120.0, 8)]);
    // The candidates drift away in tempo and key; the last one matches best.
    let n = candidates.len();
    for (i, song) in candidates.iter().enumerate() {
        let off = (n - 1 - i) as f32;
        handle.insert([analysed(song, 120.0 + 9.0 * off, 8 + (off as u8 * 3) % 12)]);
    }
    state.analysis = Some(handle);
    let before = state.queue.len();
    let _ = state.update(Message::AutoDjLoaded(Ok(candidates.clone())));
    let first_added = state.queue.iter().nth(before).unwrap();
    assert_eq!(first_added.song.id, candidates[n - 1].id, "the best-flowing song comes first");
}

/// YouTube Music songs without durations are found by videoId and ordered by flow.
#[test]
fn autodj_orders_youtube_music_picks_by_flow() {
    use listen_core::model::SourceKind;
    let ytm = |songs: Vec<listen_core::model::Song>| -> Vec<listen_core::model::Song> {
        songs
            .into_iter()
            .map(|mut s| {
                s.source = SourceKind::YouTubeMusic;
                s.duration = std::time::Duration::ZERO;
                s.codec = None;
                s
            })
            .collect()
    };
    let (mut state, _) = Listen::boot(&Args { demo: true, ..Args::default() });
    state.queue.set(ytm(demo::album_songs(0)), 0);
    let current = state.queue.current().unwrap().song.clone();
    let candidates = ytm(demo::album_songs(3));
    let handle = listen::app::analysis::AnalysisHandle::default();
    handle.insert([analysed(&current, 120.0, 8)]);
    let n = candidates.len();
    for (i, song) in candidates.iter().enumerate() {
        let off = (n - 1 - i) as f32;
        handle.insert([analysed(song, 120.0 + 9.0 * off, 8 + (off as u8 * 3) % 12)]);
    }
    assert!(handle.lookup(&current).is_some_and(|r| r.song_id.starts_with("ytm:")));
    state.analysis = Some(handle);
    let before = state.queue.len();
    let _ = state.update(Message::AutoDjLoaded(Ok(candidates.clone())));
    let first_added = state.queue.iter().nth(before).unwrap();
    assert_eq!(first_added.song.id, candidates[n - 1].id, "the best-flowing song comes first");
}
