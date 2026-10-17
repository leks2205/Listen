//! Home carousels, the command palette and the search page, driven through the Simulator.

use iced::mouse::{Button, Event as MouseEvent};
use iced_test::simulator::Simulator;
use listen::app::home::{HomeMsg, RowId, Section};
use listen::app::palette::PaletteMsg;
use listen::app::{INTER, Listen, Message, Route};
use listen::{Args, FONTS};

static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn gpu() -> std::sync::MutexGuard<'static, ()> {
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

fn state() -> Listen {
    let (mut s, _) = Listen::boot(&Args {
        demo: true,
        ephemeral: true,
        theme: Some("tokyoNight".into()),
        ..Args::default()
    });
    let _ = s.update(Message::WindowResized(iced::Size::new(1600.0, 950.0)));
    s
}

fn settings() -> iced::Settings {
    iced::Settings {
        fonts: FONTS.iter().map(|f| std::borrow::Cow::Borrowed(*f)).collect(),
        default_font: INTER,
        ..Default::default()
    }
}

fn messages(s: &Listen, find: &str) -> Vec<Message> {
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings(), s.window_size, s.view());
    let target = ui.find(find).expect("text on screen");
    ui.point_at(target.visible_bounds().expect("visible").center());
    let _ = ui.simulate([
        iced::Event::Mouse(MouseEvent::ButtonPressed(Button::Left)),
        iced::Event::Mouse(MouseEvent::ButtonReleased(Button::Left)),
    ]);
    ui.into_messages().collect()
}

fn has(s: &Listen, text: &str) -> bool {
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings(), s.window_size, s.view());
    ui.find(text).is_ok()
}

#[test]
fn home_shows_the_feature_genres_and_rows() {
    let s = state();
    assert!(has(&s, "Night Drive"));
    assert!(has(&s, "Genres"));
    assert!(has(&s, "Most Played"));
}

#[test]
fn the_play_button_of_the_feature_plays_the_album() {
    let s = state();
    let msgs = messages(&s, "Play");
    assert!(msgs.iter().any(|m| matches!(m, Message::PlayAlbum(_))), "{msgs:?}");
}

#[test]
fn row_arrows_page_the_carousel() {
    let mut s = state();
    let row = RowId::Section(Section::MostPlayed);
    let _ = s.update(Message::Home(HomeMsg::Page(row, 1, 2)));
    let first = s.library.home_state.row(Section::MostPlayed).unwrap().first;
    assert_eq!(first, 2);
    let _ = s.update(Message::Home(HomeMsg::Page(row, -1, 2)));
    assert_eq!(s.library.home_state.row(Section::MostPlayed).unwrap().first, 0);
}

#[test]
fn the_sidebar_search_opens_the_palette_and_it_shows_commands() {
    let mut s = state();
    let msgs = messages(&s, "Search");
    assert!(msgs.iter().any(|m| matches!(m, Message::Palette(PaletteMsg::Open))), "{msgs:?}");
    for m in msgs {
        let _ = s.update(m);
    }
    assert!(s.palette_open());
    assert!(has(&s, "Go to page..."));
    // Clicking it goes one page deeper.
    for m in messages(&s, "Go to page...") {
        let _ = s.update(m);
    }
    assert!(has(&s, "Home"));
    let at = s.palette_items().iter().position(|i| i.label == "Tracks").expect("Tracks command");
    let _ = s.update(Message::Palette(PaletteMsg::Choose(at)));
    assert!(!s.palette_open());
    assert_eq!(s.route, Route::Tracks);
}

#[test]
fn the_search_page_has_tabs() {
    let mut s = state();
    let _ = s.update(Message::Navigate(Route::Search));
    assert!(has(&s, "Tracks"));
    assert!(has(&s, "Albums"));
    assert!(has(&s, "Artists"));
}
