//! The Statistics page: the rolling 4-week view and the switch to calendar years.

use iced_test::simulator::Simulator;
use listen::app::history::HistoryMsg;
use listen::app::{INTER, Listen, Message, Route};
use listen::{Args, FONTS};
use listen_core::history::{self, Entry};
use listen_core::model::ArtistRef;

fn settings() -> iced::Settings {
    iced::Settings {
        fonts: FONTS.iter().map(|f| std::borrow::Cow::Borrowed(*f)).collect(),
        default_font: INTER,
        ..Default::default()
    }
}

fn has_text(s: &Listen, text: &str) -> bool {
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
