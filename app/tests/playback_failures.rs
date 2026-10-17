//! A track that fails to play: three silent retries of the same track
//! (backup server, fresh URL, its backup), then a skip to exactly the next
//! one. The shown track always matches what's queued.

use listen::app::{Listen, Message};
use listen::playback::PlayerMsg;
use listen::{Args, demo};
use listen_player::PlayerEvent;

fn state() -> Listen {
    let (mut state, _) = Listen::boot(&Args { ephemeral: true, ..Args::default() });
    state.queue.set(demo::album_songs(0), 0);
    state
}

fn title(state: &Listen) -> String {
    state.queue.current().map(|i| i.song.title.clone()).unwrap_or_default()
}

fn fail(state: &mut Listen) {
    let _ = state.update(Message::Deck(0, PlayerMsg::Event(PlayerEvent::Error("loading failed".into()))));
}

#[test]
fn retries_then_skips_exactly_one() {
    let mut s = state();
    let songs = demo::album_songs(0);
    assert_eq!(title(&s), songs[0].title);

    for attempt in 1..=3 {
        fail(&mut s);
        assert_eq!(title(&s), songs[0].title, "retry {attempt} keeps the same track");
        assert!(s.overlay.toasts.is_empty(), "retries are silent");
    }
    fail(&mut s);
    assert_eq!(title(&s), songs[1].title, "then it skips to the very next track");
    assert!(s.overlay.toasts.iter().any(|t| t.message.contains(&songs[0].title)));

    // The next track gets its own retries.
    for _ in 1..=3 {
        fail(&mut s);
        assert_eq!(title(&s), songs[1].title);
    }
    fail(&mut s);
    assert_eq!(title(&s), songs[2].title);
}
