//! The full-screen About tab: sourced facts, credits, and nothing without a source.

mod common;

use iced_test::simulator::Simulator;
use listen::Args;
use listen::app::about::AboutMsg;
use listen::app::{FullscreenTab, Listen, Message};
use listen_lyrics::card::{Card, Credit, Credits, Fact, Source};

fn has_text(s: &Listen, text: &str) -> bool {
    let _gpu = common::gpu();
    let mut ui = Simulator::with_size(common::iced_settings(), iced::Size::new(1400.0, 900.0), s.view());
    ui.find(text).is_ok()
}

fn boot() -> Listen {
    let (mut s, _) = Listen::boot(&Args { demo: true, theme: Some("tokyoNight".into()), ..Args::default() });
    let _ = s.update(Message::OpenFullscreenTab(FullscreenTab::About));
    s
}

fn card() -> Card {
    Card {
        facts: vec![Fact {
            text: "Released in January 2024 as the lead single of The Winning.".into(),
            sources: vec![Source {
                title: "wikipedia.org".into(),
                url: "https://en.wikipedia.org/wiki/x".into(),
            }],
        }],
        queries: vec!["love wins all iu".into()],
        credits: Credits {
            credits: vec![Credit { role: "Composer".into(), names: vec!["Seo Dong-hwan".into()] }],
            url: Some("https://musicbrainz.org/recording/r".into()),
        },
        at: 1,
    }
}

#[test]
fn facts_show_with_their_sources_and_the_credits() {
    let mut s = boot();
    assert!(!s.about.key.is_empty(), "opening the tab asks for the playing song");
    assert!(has_text(&s, "About this song needs the AI DJ's Gemini key (Settings → AI DJ)."));
    let key = s.about.key.clone();
    let _ = s.update(Message::About(AboutMsg::Loaded(key, Ok(card()))));
    assert!(has_text(&s, "Released in January 2024 as the lead single of The Winning."));
    assert!(has_text(&s, "wikipedia.org"));
    assert!(has_text(&s, "Seo Dong-hwan") && has_text(&s, "musicbrainz.org"));
    assert!(has_text(&s, "love wins all iu"));
    // Another song's answer is ignored.
    let _ = s.update(Message::About(AboutMsg::Loaded("other|en".into(), Ok(Card::default()))));
    assert!(has_text(&s, "wikipedia.org"));
}

#[test]
fn a_card_without_sources_is_not_shown() {
    let mut s = boot();
    let key = s.about.key.clone();
    let mut c = card();
    c.facts.clear();
    c.credits.url = None;
    let _ = s.update(Message::About(AboutMsg::Loaded(key, Ok(c))));
    assert!(!has_text(&s, "Seo Dong-hwan"));
    assert!(!has_text(&s, "About this song"));
}
