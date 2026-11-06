//! "Order for smooth mixing": playing a playlist in flow order, reordering the queue's upcoming part,
//! and the playlist page's edit mode ordering its rows (saved with "Save and replace").

use listen::app::analysis::AnalysisHandle;
use listen::app::menu::{MenuAction, MenuMsg, MenuTarget};
use listen::app::playlists::PlaylistPageMsg as P;
use listen::app::{Listen, Message, Route};
use listen::{Args, demo};
use listen_core::model::Song;
use std::collections::HashMap;
use std::sync::Arc;

/// A record saying `song` runs at `bpm` (key and energy fixed, so tempo decides the flow).
fn analysed(song: &Song, bpm: f32) -> Arc<listen_core::analysis::TrackAnalysis> {
    use listen_core::analysis::*;
    let period = 60.0 / bpm;
    Arc::new(TrackAnalysis {
        version: ANALYZER_VERSION,
        analyzed_at: 1,
        machine: "test".into(),
        decoder: "test".into(),
        song_id: listen_analysis::record_key(song),
        server_hint: String::new(),
        file: FileId::of(song),
        title_hint: song.title.clone(),
        outcome: Outcome::Ok(Box::new(Features {
            audio_start: 0.1,
            audio_end: song.duration.as_secs_f32().max(200.0),
            grid: Some(BeatGrid {
                beats: (0..400).map(|i| 0.1 + i as f32 * period).collect(),
                downbeat_phase: 0,
                beats_per_bar: 4,
                bpm,
                confidence: 0.9,
                stability: 0.99,
            }),
            key: Some((Camelot { number: 8, minor: true }, 0.9)),
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

/// Tempos that zig-zag (100, 140, 104, 136, …) so the given order flows badly.
fn zigzag(n: usize) -> Vec<f32> {
    (0..n).map(|i| if i % 2 == 0 { 100.0 + 2.0 * i as f32 } else { 140.0 - 2.0 * i as f32 }).collect()
}

/// Analysis for `songs` with tempos from [`zigzag`]; returns the tempo by song id.
fn analyse(state: &mut Listen, songs: &[Song]) -> HashMap<String, f32> {
    let handle = AnalysisHandle::default();
    let tempos = zigzag(songs.len());
    let mut by_id = HashMap::new();
    for (s, &bpm) in songs.iter().zip(&tempos) {
        handle.insert([analysed(s, bpm)]);
        by_id.insert(s.id.clone(), bpm);
    }
    state.analysis = Some(handle);
    by_id
}

/// Sum of the log tempo steps along `songs`.
fn tempo_path(songs: &[Song], bpm: &HashMap<String, f32>) -> f32 {
    songs.windows(2).map(|w| (bpm[&w[0].id] / bpm[&w[1].id]).ln().abs()).sum()
}

fn unique_songs() -> Vec<Song> {
    let mut songs: Vec<Song> = (0..3).flat_map(demo::album_songs).collect();
    let mut seen = std::collections::HashSet::new();
    songs.retain(|s| seen.insert(s.id.clone()));
    songs
}

fn same_set(a: &[Song], b: &[Song]) -> bool {
    let ids = |v: &[Song]| {
        let mut x: Vec<String> = v.iter().map(|s| s.id.clone()).collect();
        x.sort();
        x
    };
    ids(a) == ids(b)
}

#[test]
fn play_in_smooth_order_keeps_the_first_song_and_flows_better() {
    let (mut s, _) = Listen::boot(&Args { demo: true, ..Args::default() });
    let songs = unique_songs();
    let bpm = analyse(&mut s, &songs);
    let _ = s.update(Message::Menu(MenuMsg::Songs(MenuAction::PlaySmooth, Ok(songs.clone()))));
    let queued: Vec<Song> = s.queue.iter().map(|i| i.song.clone()).collect();
    assert_eq!(queued.len(), songs.len());
    assert!(same_set(&queued, &songs), "every song once");
    assert_eq!(queued[0].id, songs[0].id, "first song unchanged");
    assert_eq!(s.queue.current().unwrap().song.id, songs[0].id, "plays from the first song");
    assert!(tempo_path(&queued, &bpm) < 0.5 * tempo_path(&songs, &bpm));
}

#[test]
fn queue_reorders_only_the_upcoming_part() {
    let (mut s, _) = Listen::boot(&Args { demo: true, ..Args::default() });
    let songs = unique_songs();
    let bpm = analyse(&mut s, &songs);
    s.queue.set(songs.clone(), 2);
    let before: Vec<u64> = s.queue.iter().map(|i| i.uid).collect();
    let current = s.queue.current().unwrap().uid;
    let menu_songs: Vec<Song> = s.queue.iter().map(|i| i.song.clone()).collect();
    let _ = s.update(Message::Menu(MenuMsg::Open(MenuTarget::Songs {
        songs: menu_songs,
        queue_uids: before.clone(),
        history_entries: vec![],
    })));
    let entries = listen::app::menu::entries(&s, s.menu.as_ref().unwrap());
    assert!(entries.iter().any(|e| e.label == "Order for smooth mixing"));
    let _ = s.update(Message::Menu(MenuMsg::Do(MenuAction::SmoothQueue)));
    let after: Vec<u64> = s.queue.iter().map(|i| i.uid).collect();
    assert_eq!(after[..3], before[..3], "played songs and the current one stay");
    assert_eq!(s.queue.current().unwrap().uid, current, "the current song keeps playing");
    let mut a = after.clone();
    let mut b = before.clone();
    a.sort_unstable();
    b.sort_unstable();
    assert_eq!(a, b, "every song once");
    let upcoming =
        |q: &listen_core::queue::Queue| -> Vec<Song> { q.iter().skip(2).map(|i| i.song.clone()).collect() };
    let after_songs = upcoming(&s.queue);
    let before_songs: Vec<Song> = songs[2..].to_vec();
    assert!(tempo_path(&after_songs, &bpm) < tempo_path(&before_songs, &bpm));
}

#[test]
fn queue_up_next_block_stays_first() {
    let (mut s, _) = Listen::boot(&Args { demo: true, ..Args::default() });
    let songs = unique_songs();
    let _ = analyse(&mut s, &songs);
    s.queue.set(songs[..6].to_vec(), 0);
    s.queue.enqueue_next(songs[6..9].to_vec());
    let up_next: Vec<String> = songs[6..9].iter().map(|s| s.id.clone()).collect();
    let uids: Vec<u64> = s.queue.iter().map(|i| i.uid).collect();
    let all: Vec<Song> = s.queue.iter().map(|i| i.song.clone()).collect();
    let _ = s.update(Message::Menu(MenuMsg::Open(MenuTarget::Songs {
        songs: all,
        queue_uids: uids,
        history_entries: vec![],
    })));
    let _ = s.update(Message::Menu(MenuMsg::Do(MenuAction::SmoothQueue)));
    let mut got: Vec<String> = s.queue.iter().skip(1).take(3).map(|i| i.song.id.clone()).collect();
    got.sort();
    let mut want = up_next.clone();
    want.sort();
    assert_eq!(got, want, "Up Next still plays right after the current song");
    assert!(s.queue.iter().skip(1).take(3).all(|i| i.queued));
    assert_eq!(s.queue.len(), 9);
}

#[test]
fn playlist_edit_mode_orders_rows_and_saves_like_a_manual_reorder() {
    let (mut s, _) = Listen::boot(&Args { demo: true, ..Args::default() });
    s.library.playlists = demo::playlists();
    let detail = demo::navidrome_playlist();
    let id = detail.playlist.id.clone();
    let _ = s.update(Message::Navigate(Route::Playlist(id.clone())));
    let _ = s.update(Message::PlaylistLoaded(Ok(detail)));
    let rows: Vec<Song> = s.playlist_rows(&id).to_vec();
    let bpm = analyse(&mut s, &rows);
    // Not in edit mode: nothing happens.
    let _ = s.update(Message::PlaylistPage(P::EditSmooth));
    assert!(s.playlist_page.edit.is_none());
    let _ = s.update(Message::PlaylistPage(P::EditStart));
    let _ = s.update(Message::PlaylistPage(P::EditSmooth));
    let edit = s.playlist_page.edit.as_ref().expect("still editing");
    assert!(edit.dirty(), "the order changed");
    assert_eq!(edit.rows[0].id, rows[0].id, "first song unchanged");
    assert_eq!(edit.rows.len(), rows.len());
    // `order[i]` is the playlist position of row i.
    for (row, &pos) in edit.rows.iter().zip(&edit.order) {
        assert_eq!(row.id, rows[pos].id);
    }
    let ordered: Vec<Song> = s.playlist_rows(&id).to_vec();
    assert!(tempo_path(&ordered, &bpm) < tempo_path(&rows, &bpm));
}
