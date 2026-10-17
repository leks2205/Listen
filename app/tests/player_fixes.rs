//! Seek-on-release, scrobbling on real play time, album jumps and the
//! queue behaviours Auto-DJ / shuffle depend on.

use listen::app::{Listen, Message};
use listen::playback::PlayerMsg;
use listen::{Args, demo};
use listen_core::queue::Repeat;
use listen_player::PlayerEvent;
use std::time::Duration;

fn state() -> Listen {
    let (mut state, _) = Listen::boot(&Args { ephemeral: true, ..Args::default() });
    state.queue.set(demo::album_songs(0), 0);
    state.config.settings.history.enabled = true;
    state
}

fn pos(s: &mut Listen, secs: f64) {
    let ev = PlayerEvent::Position(Duration::from_secs_f64(secs));
    let _ = s.update(Message::Deck(0, PlayerMsg::Event(ev)));
}

#[test]
fn dragging_the_slider_seeks_once_on_release() {
    let mut s = state();
    pos(&mut s, 5.0);
    for t in [10.0, 20.0, 30.0] {
        let _ = s.update(Message::SeekPreview(t));
        assert_eq!(s.seek_preview, Some(t));
        assert_eq!(s.playback.position, Duration::from_secs(5), "no seek while dragging");
    }
    let _ = s.update(Message::SeekRelease);
    assert_eq!(s.playback.position, Duration::from_secs(30), "one seek, to the last dragged time");
    assert_eq!(s.seek_preview, None);
    // A stray release without a drag does nothing.
    pos(&mut s, 31.0);
    let _ = s.update(Message::SeekRelease);
    assert_eq!(s.playback.position, Duration::from_secs(31));
}

#[test]
fn a_forward_seek_does_not_scrobble_until_enough_is_played() {
    let mut s = state();
    let total = s.duration();
    let p = &s.config.settings.playback;
    let threshold = Duration::from_secs(u64::from(p.scrobble_at_secs))
        .min(total.mul_f32(f32::from(p.scrobble_at_percent) / 100.0));
    assert!(threshold > Duration::from_secs(4), "test needs a threshold to cross");

    pos(&mut s, 1.0);
    pos(&mut s, 2.0);
    // Seek far past the threshold position (and past 75 % of the song).
    let far = total.as_secs_f64() * 0.9;
    pos(&mut s, far);
    assert!(s.history.entries.is_empty(), "a seek is not listening");
    // Real play from there on counts: tick one second at a time.
    let mut t = far;
    let mut ticks = 0;
    while s.history.entries.is_empty() && ticks < 600 {
        t += 1.0;
        ticks += 1;
        pos(&mut s, t);
    }
    assert_eq!(s.history.entries.len(), 1);
    let played_needed = threshold.as_secs_f64() - 2.0; // two seconds were played before the seek
    assert!(
        (f64::from(ticks) - played_needed).abs() <= 1.5,
        "scrobbled after {ticks} ticks, expected about {played_needed}"
    );
    // Not again for the same track.
    for _ in 0..10 {
        t += 1.0;
        pos(&mut s, t);
    }
    assert_eq!(s.history.entries.len(), 1);
}

#[test]
fn a_new_track_starts_counting_afresh() {
    let mut s = state();
    let threshold_ticks = 3;
    for t in 0..threshold_ticks {
        pos(&mut s, f64::from(t));
    }
    let _ = s.update(Message::Next);
    pos(&mut s, 100.0);
    assert!(s.history.entries.is_empty(), "the old track's played time is gone");
}

#[test]
fn alt_click_jumps_by_album_and_plain_click_by_track() {
    let mut s = state();
    let mut second = demo::album_songs(1);
    let first_of_second = second[0].id.clone();
    let mut songs = demo::album_songs(0);
    songs.append(&mut second);
    s.queue.set(songs, 0);
    let _ = s.update(Message::NextClick);
    assert_eq!(s.queue.current_position(), Some(1), "no modifier: next track");
    s.modifiers = iced::keyboard::Modifiers::ALT;
    let _ = s.update(Message::NextClick);
    assert_eq!(s.queue.current().unwrap().song.id, first_of_second, "alt: first track of the next album");
    let _ = s.update(Message::PreviousClick);
    assert_eq!(s.queue.current_position(), Some(0), "alt: back to the start of the previous album");
}

#[test]
fn auto_dj_appends_and_manual_queue_survive_repeat_and_skips() {
    let mut s = state();
    s.queue.enqueue(demo::album_songs(1).into_iter().take(1).collect());
    let before = s.queue.len();
    let _ = s.update(Message::AutoDjLoaded(Ok(demo::album_songs(2))));
    let added = s.queue.len() - before;
    assert!(added > 0);
    assert!(s.queue.iter().skip(before).all(|i| i.auto), "Auto-DJ songs still go to the end");
    assert_eq!(s.queue.manual_len(), 1, "the manual queue is untouched");
    s.queue.repeat = Repeat::One;
    let _ = s.update(Message::Previous);
    assert_eq!(s.queue.current_position(), Some(0));
    assert_eq!(s.queue.manual_len(), 1);
    s.queue.toggle_shuffle();
    assert_eq!(s.queue.manual_len(), 1, "shuffle keeps the manual queue next");
    assert!(s.queue.iter().filter(|i| i.auto).count() == added);
}
