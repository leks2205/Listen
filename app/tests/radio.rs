//! Internet radio, the Related list and the jukebox setting, in demo mode:
//! the Radio page's buttons, the create / edit / delete flow, the ICY title,
//! and that a playing stream stays out of the queue's business.

use iced_test::simulator::Simulator;
use listen::app::radio::RadioMsg;
use listen::app::{INTER, Listen, Message, Route};
use listen::{Args, FONTS, demo};
use listen_player::PlayerEvent;
use std::time::Duration;

static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn gpu() -> std::sync::MutexGuard<'static, ()> {
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

fn state() -> Listen {
    let (mut s, _) = Listen::boot(&Args { demo: true, theme: Some("tokyoNight".into()), ..Args::default() });
    s.queue.set(demo::album_songs(0), 0);
    let _ = s.update(Message::Navigate(Route::Radio));
    s
}

fn send(s: &mut Listen, m: RadioMsg) {
    let _ = s.update(Message::Radio(m));
}

fn has_text(s: &Listen, text: &str) -> bool {
    let settings = iced::Settings {
        fonts: FONTS.iter().map(|f| std::borrow::Cow::Borrowed(*f)).collect(),
        default_font: INTER,
        ..Default::default()
    };
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings, s.window_size, s.view());
    ui.find(text).is_ok()
}

#[test]
fn the_page_lists_the_stations_and_filters_them() {
    let mut s = state();
    assert_eq!(s.radio.stations.len(), 3);
    assert!(has_text(&s, "Jazz Cafe"));
    send(&mut s, RadioMsg::Search("night".into()));
    assert!(has_text(&s, "Night Drive FM"));
    assert!(!has_text(&s, "Jazz Cafe"));
}

#[test]
fn create_edit_and_delete() {
    let mut s = state();
    send(&mut s, RadioMsg::OpenCreate);
    assert!(s.radio.form.is_some());
    send(&mut s, RadioMsg::FormName("New One".into()));
    send(&mut s, RadioMsg::FormSubmit);
    assert!(s.radio.form.as_ref().is_some_and(|f| !f.busy), "a stream address is needed first");
    send(&mut s, RadioMsg::FormStream("https://radio.example/new".into()));
    send(&mut s, RadioMsg::FormSubmit);
    // The demo answers at once through a task; feed the answer back.
    let created = listen_source::InternetRadio {
        id: "d4".into(),
        name: "New One".into(),
        stream_url: "https://radio.example/new".into(),
        home_page_url: None,
        image_id: None,
    };
    send(&mut s, RadioMsg::Saved(Ok(Some(created))));
    assert!(s.radio.form.is_none());
    assert_eq!(s.radio.stations.len(), 4);

    send(&mut s, RadioMsg::OpenEdit("d4".into()));
    assert_eq!(s.radio.form.as_ref().unwrap().name, "New One");
    send(&mut s, RadioMsg::FormName("Renamed".into()));
    let edited = listen_source::InternetRadio {
        id: "d4".into(),
        name: "Renamed".into(),
        stream_url: "https://radio.example/new".into(),
        home_page_url: None,
        image_id: None,
    };
    send(&mut s, RadioMsg::Saved(Ok(Some(edited))));
    assert_eq!(s.radio.stations.iter().find(|x| x.id == "d4").unwrap().name, "Renamed");

    send(&mut s, RadioMsg::AskDelete("d4".into()));
    assert!(matches!(s.overlay.modal, Some(listen::app::overlay::Modal::Confirm(_))));
    send(&mut s, RadioMsg::Deleted("d4".into(), Ok(())));
    assert_eq!(s.radio.stations.len(), 3);
    // A failed save keeps the dialog open.
    send(&mut s, RadioMsg::OpenCreate);
    send(&mut s, RadioMsg::Saved(Err("no".into())));
    assert!(s.radio.form.is_some());
}

#[test]
fn a_station_plays_without_touching_the_queue_or_scrobbling() {
    let mut s = state();
    let queued = s.queue.len();
    let current = s.queue.current().map(|i| i.uid);
    send(&mut s, RadioMsg::Play("d1".into()));
    assert!(s.radio_active());
    assert!(s.playback.playing);

    // ICY titles show; positions move the clock but never scrobble or advance.
    let _ = s.update(Message::Deck(
        0,
        listen::playback::PlayerMsg::Event(PlayerEvent::StreamTitle(Some("Miles Davis - So What".into()))),
    ));
    assert_eq!(s.radio.active.as_ref().unwrap().title.as_deref(), Some("Miles Davis - So What"));
    assert!(has_text(&s, "Now playing: Miles Davis - So What"));
    for secs in [1, 100, 5000] {
        let _ = s.update(Message::Deck(
            0,
            listen::playback::PlayerMsg::Event(PlayerEvent::Position(Duration::from_secs(secs))),
        ));
    }
    assert_eq!(s.queue.len(), queued);
    assert_eq!(s.queue.current().map(|i| i.uid), current);

    // Next / previous do nothing; Stop ends the stream.
    let _ = s.update(Message::Next);
    assert_eq!(s.queue.current().map(|i| i.uid), current);
    assert!(s.radio_active());
    let _ = s.update(Message::Stop);
    assert!(!s.radio_active());
    assert!(!s.playback.playing);

    // A stream error is reported once and leaves the queue alone.
    send(&mut s, RadioMsg::Play("d2".into()));
    let _ = s.update(Message::Deck(0, listen::playback::PlayerMsg::Event(PlayerEvent::Error("boom".into()))));
    assert!(!s.radio_active());
    assert_eq!(s.queue.current().map(|i| i.uid), current);

    // Starting a song takes over from a station; clicking the playing station stops it.
    send(&mut s, RadioMsg::RelatedRefresh);
    send(&mut s, RadioMsg::Play("d1".into()));
    assert!(s.radio_active());
    let _ = s.update(Message::Fullscreen(listen::app::fullscreen_state::FullscreenMsg::PlayRelated(0)));
    assert!(!s.radio_active());
    send(&mut s, RadioMsg::Play("d1".into()));
    send(&mut s, RadioMsg::Play("d1".into()));
    assert!(!s.radio_active(), "clicking the playing station stops it");
}

#[test]
fn the_related_list_goes_to_the_fullscreen_tab() {
    use listen::app::FullscreenTab;
    let mut s = state();
    // Closed: nothing is fetched on a track change.
    send(&mut s, RadioMsg::RelatedRefresh);
    let first = s.queue.current().map(|i| i.song.id.clone());
    assert_eq!(s.fs.related_for, first);
    assert!(!s.fs.related.is_empty());
    assert!(s.fs.related.iter().all(|x| Some(&x.id) != first.as_ref()));
    // Open: it follows the playing song.
    let _ = s.update(Message::ToggleFullscreen);
    let _ = s.update(Message::Fullscreen(listen::app::fullscreen_state::FullscreenMsg::TogglePanel(
        FullscreenTab::Related,
    )));
    let _ = s.update(Message::Next);
    let now = s.queue.current().map(|i| i.song.id.clone());
    assert_ne!(now, first);
    assert_eq!(s.fs.related_for, now);
}
