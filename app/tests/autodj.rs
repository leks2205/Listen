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
    ids.sort();
    ids.dedup(); // `dedup` only drops adjacent repeats: sort first
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
            voiced: None,
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

/// "Why this song?": every song the Auto-DJ adds carries a reason, songs from the similar list name the
/// song they are similar to, the rest are random picks; the songs the user started have none.
#[test]
fn autodj_picks_carry_a_reason() {
    use listen::app::AutoDjFill;
    use listen_core::queue::{Picker, WhyCode};
    let (mut state, _) = Listen::boot(&Args { demo: true, ..Args::default() });
    state.history.entries.clear();
    let before = state.queue.len();
    let songs = demo::album_songs(2);
    let similar = songs.iter().take(2).map(|s| s.id.clone()).collect();
    let fill = AutoDjFill { songs: songs.clone(), similar, seed: "Seed Song".into() };
    let _ = state.update(Message::AutoDjFilled(state.autodj_generation(), Ok(fill)));
    assert!(state.queue.iter().take(before).all(|i| i.why.is_none()), "user songs have no reason");
    let added: Vec<_> = state.queue.iter().skip(before).collect();
    assert!(!added.is_empty());
    for it in &added {
        let why = it.why.as_ref().expect("every Auto-DJ song has a reason");
        assert_eq!(why.picker, Picker::AutoDj);
        let from_similar = songs.iter().take(2).any(|s| s.id == it.song.id);
        let want =
            if from_similar { WhyCode::SimilarTo { seed: "Seed Song".into() } } else { WhyCode::Random };
        assert_eq!(why.code, want, "{}", it.song.title);
        assert!(!listen::i18n::why(why).is_empty());
    }
}

/// The scene of what plays now gates the fill: a 1980s Korean ballad playing (picked after a run of 2020s
/// idol songs) is followed by songs of its scene, never by the 2020s idol songs the source also offered;
/// with too few left, same-scene songs the listener played come from the history.
#[test]
fn autodj_fills_stay_in_the_scene_and_decade_of_the_current_song() {
    use listen_core::scene::{SceneIndex, SceneRecord};
    let (mut s, _) = Listen::boot(&Args { demo: true, ..Args::default() });
    s.history.entries.clear();
    s.analysis = Some(listen::app::analysis::AnalysisHandle::default());
    let mk = |i: usize, artist: &str| {
        let mut x = demo::album_songs(i / 8)[i % 8 % demo::album_songs(i / 8).len()].clone();
        x.id = format!("scene-song-{i}");
        x.title = format!("Scene song {i}");
        x.artists[0].name = artist.into();
        x.genres.clear();
        x.year = None;
        x
    };
    const BALLAD80: &str = "Korean pop, ballad, 1980s, South Korea, Korean, male solo, 80s ballad";
    const BALLAD90: &str = "K-pop, ballad, 1990s, South Korea, Korean, female solo";
    const IDOL20: &str = "K-pop, dance-pop, 2020s, South Korea, Korean, girl group, 4th-generation K-pop";
    let idols_before: Vec<_> = (0..4).map(|i| mk(i, &format!("Idol group {i}"))).collect();
    let ballad = mk(10, "Ballad singer");
    let mut queue = idols_before.clone();
    queue.push(ballad.clone());
    s.queue.set(queue, 4);
    let offered: Vec<_> = (20..26).map(|i| mk(i, &format!("Offered idol {i}"))).collect();
    let old: Vec<_> = (30..32).map(|i| mk(i, &format!("Old singer {i}"))).collect();
    let history: Vec<_> = (40..46).map(|i| mk(i, &format!("History singer {i}"))).collect();
    let rec = |x: &listen_core::model::Song, t: &str| SceneRecord {
        song_id: listen_analysis::record_key(x),
        title_hint: format!("{} – {}", x.artist_line(), x.title),
        tags_en: t.into(),
        audio: None,
        at: 0,
    };
    let records: Vec<SceneRecord> = idols_before
        .iter()
        .chain(&offered)
        .map(|x| rec(x, IDOL20))
        .chain([rec(&ballad, BALLAD80)])
        .chain(old.iter().map(|x| rec(x, BALLAD80)))
        .chain(history.iter().map(|x| rec(x, BALLAD90)))
        .collect();
    let a = s.analysis.as_ref().unwrap();
    a.set_scenes(SceneIndex::from_records(records));
    for (i, x) in history.iter().enumerate() {
        a.insert([analysed(x, 80.0, 8)]);
        s.history.entries.push(listen_core::history::Entry::new(x, 1_000 + i as i64));
    }
    let mut fetched = offered.clone();
    fetched.extend(old.clone());
    let before = s.queue.len();
    let _ = s.update(Message::AutoDjLoaded(Ok(fetched)));
    let added: Vec<_> = s.queue.iter().skip(before).map(|i| i.song.clone()).collect();
    let n = s.config.settings.auto_dj.item_count as usize;
    assert!(!added.is_empty());
    assert!(added.len() <= n);
    assert!(
        added.iter().all(|x| !offered.iter().any(|o| o.id == x.id)),
        "no 2020s idol song after the 1980s ballad: {:?}",
        added.iter().map(|x| &x.title).collect::<Vec<_>>()
    );
    assert!(old.iter().all(|o| added.iter().any(|x| x.id == o.id)), "the 1980s songs offered are kept");
    if n > old.len() {
        assert!(added.iter().any(|x| history.iter().any(|h| h.id == x.id)), "topped up from the history");
    }
}
