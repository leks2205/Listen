//! The Statistics page: the rolling 4-week view and the switch to calendar years.

use iced_test::simulator::Simulator;
use listen::app::history::HistoryMsg;
use listen::app::{INTER, Listen, Message, Route};
use listen::{Args, FONTS};
use listen_core::history::{self, Entry};
use listen_core::model::ArtistRef;

static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn settings() -> iced::Settings {
    iced::Settings {
        fonts: FONTS.iter().map(|f| std::borrow::Cow::Borrowed(*f)).collect(),
        default_font: INTER,
        ..Default::default()
    }
}

fn has_text(s: &Listen, text: &str) -> bool {
    let _gpu = GPU.lock().unwrap_or_else(|e| e.into_inner());
    let mut ui = Simulator::with_size(settings(), iced::Size::new(1400.0, 1000.0), s.view());
    ui.find(text).is_ok()
}

#[test]
fn the_week_view_shows_new_artists_skips_and_the_change() {
    let (mut s, _) = Listen::boot(&Args { demo: true, ephemeral: true, ..Args::default() });
    s.history.entries.clear();
    let song = s.queue.current().unwrap().song.clone();
    let now = history::now_secs();
    let at = |days: i64, artist: &str| {
        let mut e = Entry::new(&song, now - days * 86_400);
        e.artists = vec![ArtistRef { id: None, name: artist.into() }];
        e.title = format!("Song by {artist}");
        e
    };
    s.history.entries =
        vec![at(10, "Old Friend"), at(2, "Old Friend"), at(1, "Brand New"), at(1, "Brand New")];
    let mut skip = at(1, "Old Friend");
    skip.status = history::PlayStatus::Skipped;
    s.history.entries.push(skip);
    let _ = s.update(Message::History(HistoryMsg::Weeks));
    let _ = s.update(Message::Navigate(Route::Stats));
    assert!(has_text(&s, "New artists this week"));
    assert!(has_text(&s, "Brand New"), "first heard this week");
    assert!(has_text(&s, "+200 % vs last week"), "3 plays against 1");
    assert!(has_text(&s, "Skipped most (4 weeks)"));
    let w = &s.history_view.weekly;
    assert_eq!(w.new_artists.len(), 1);
    assert_eq!(w.most_skipped[0].name, "Song by Old Friend");
    // A year leaves the rolling view.
    let _ = s.update(Message::History(HistoryMsg::Year(None)));
    assert!(!has_text(&s, "New artists this week"));
    assert!(has_text(&s, "Your listening, all time"));
}

/// QA bug 45: at a 960 px window the tile values ("8 Std. 48 Min.") and long song names ran into the
/// next tile or column. Values and names now wrap inside their box.
#[test]
fn tiles_and_lists_keep_their_text_inside_at_960_px() {
    let (mut s, _) = Listen::boot(&Args { demo: true, ephemeral: true, ..Args::default() });
    s.history.entries.clear();
    let song = s.queue.current().unwrap().song.clone();
    let now = history::now_secs();
    let long = "I'm gonna sing a very long title (Narration by somebody) [special medley edition]";
    for i in 0..400 {
        let mut e = Entry::new(&song, now - 3_600 - i * 60);
        e.duration_secs = 11_000;
        e.title = long.into();
        e.artists = vec![ArtistRef { id: None, name: "Artist".into() }];
        s.history.entries.push(e);
    }
    let _ = s.update(Message::History(HistoryMsg::Weeks));
    let _ = s.update(Message::Navigate(Route::Stats));
    let _ = s.update(Message::WindowResized(iced::Size::new(960.0, 1048.0)));
    let _gpu = GPU.lock().unwrap_or_else(|e| e.into_inner());
    let mut ui = Simulator::with_size(settings(), iced::Size::new(960.0, 1048.0), s.view());
    use iced_test::selector::Bounded;
    let value = ui.find("1222 h 13 min").expect("the week's listening time");
    let next = ui.find("different songs").expect("the third tile");
    let (v, n) = (value.bounds(), next.bounds());
    assert!(v.x + v.width < n.x, "the time {v:?} ends before the next tile's text {n:?}");
    assert!(v.height > 40.0, "the time wraps to two lines at 960 px: {v:?}");
    let name = ui.find(long).expect("the long song name");
    let artists = ui.find("Top artists").expect("the artists column");
    let (a, b) = (name.bounds(), artists.bounds());
    assert!(a.x + a.width < b.x, "the song name {a:?} stays left of the artists column {b:?}");
    assert!(a.height > 30.0, "the long song name wraps inside its column: {a:?}");
}
