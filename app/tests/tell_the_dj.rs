//! "Tell the DJ" in the queue panel (item 8): the request box and chips show while the AI DJ is on, a
//! click on a chip adds the request and a second click removes it, and the panel renders in German.
//! One test in its own binary: the UI language is global.

use iced_test::simulator::Simulator;
use listen::app::{INTER, Listen, Message};
use listen::{Args, FONTS, demo};
use listen_dj::Chip;

fn settings() -> iced::Settings {
    iced::Settings {
        fonts: FONTS.iter().map(|f| std::borrow::Cow::Borrowed(*f)).collect(),
        default_font: INTER,
        ..Default::default()
    }
}

const SIZE: iced::Size = iced::Size::new(1600.0, 950.0);

fn shows(s: &Listen, text: &str) -> bool {
    Simulator::with_size(settings(), SIZE, s.view()).find(text).is_ok()
}

fn click(s: &mut Listen, text: &str) {
    let messages: Vec<Message> = {
        let mut ui = Simulator::with_size(settings(), SIZE, s.view());
        ui.click(text).unwrap_or_else(|e| panic!("no {text:?} on screen: {e:?}"));
        ui.into_messages().collect()
    };
    for m in messages {
        let _ = s.update(m);
    }
}

#[test]
fn the_queue_panel_tells_the_dj_by_chip_in_english_and_german() {
    let (mut s, _) = Listen::boot(&Args { demo: true, ephemeral: true, show_queue: true, ..Args::default() });
    let _ = s.update(Message::WindowResized(SIZE));
    let songs = demo::album_songs(0);
    s.queue.set(songs[..3].to_vec(), 0);
    // Off: no request row.
    assert!(!shows(&s, "Calmer"), "no chips while the AI DJ is off");
    // The AI DJ is on (demo mode keeps it off: emulate it; no paths, so nothing is written; `connecting`
    // stands in for a server so the login page stays away).
    s.demo = false;
    s.paths = None;
    s.connecting = true;
    s.config.settings.ai_dj.enabled = true;
    assert!(shows(&s, &songs[1].title), "the queue panel is on screen");
    for chip in Chip::ALL {
        assert!(shows(&s, chip.label()), "chip {:?} in the queue panel", chip.label());
    }
    click(&mut s, "Calmer");
    assert!(s.dj.requests.has(Chip::Calmer), "a click adds the request");
    assert!(shows(&s, "Calmer"), "the active chip stays on screen");
    click(&mut s, "Calmer");
    assert!(!s.dj.requests.has(Chip::Calmer), "a second click removes it");

    // German (a boot reads the language; set it directly as the i18n test does).
    listen::i18n::set_language(listen::i18n::resolve("de"));
    for (en, de) in
        [("Calmer", "Ruhiger"), ("More energy", "Mehr Energie"), ("Only favourites", "Nur Lieblingssongs")]
    {
        assert!(shows(&s, de), "{en} in German: {de}");
        assert!(!shows(&s, en), "{en} not left in English");
    }
    click(&mut s, "Mehr Energie");
    assert!(s.dj.requests.has(Chip::MoreEnergy));
    listen::i18n::set_language("en");
}
