//! The album page: header, metadata, play buttons, disc rows, search and sort,
//! favourite / rating / radio, and the YouTube Music variant.

use iced_test::simulator::Simulator;
use listen::app::album_page::{AlbumPageMsg, PagePlay, TrackSort};
use listen::app::list::ListId;
use listen::app::{INTER, Listen, Message, Route};
use listen::{Args, FONTS, demo};
use listen_core::model::Album;

static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn gpu() -> std::sync::MutexGuard<'static, ()> {
    GPU.lock().unwrap_or_else(|e| e.into_inner())
}

fn settings() -> iced::Settings {
    iced::Settings {
        fonts: FONTS.iter().map(|f| std::borrow::Cow::Borrowed(*f)).collect(),
        default_font: INTER,
        ..Default::default()
    }
}

fn page(album: Album) -> Listen {
    let (mut s, _) = Listen::boot(&Args { demo: true, theme: Some("tokyoNight".into()), ..Args::default() });
    let _ = s.update(Message::WindowResized(iced::Size::new(1400.0, 900.0)));
    let id = album.id.clone();
    let _ = s.update(Message::Navigate(Route::Album(id)));
    let _ = s.update(Message::AlbumLoaded(Ok(album)));
    s
}

fn has_text(s: &Listen, text: &str) -> bool {
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings(), s.window_size, s.view());
    ui.find(text).is_ok()
}

fn click_text(s: &mut Listen, text: &str) -> Vec<Message> {
    let messages: Vec<Message> = {
        let _gpu = gpu();
        let mut ui = Simulator::with_size(settings(), s.window_size, s.view());
        ui.click(text).unwrap_or_else(|_| panic!("no {text}"));
        ui.into_messages().collect()
    };
    for m in messages.clone() {
        let _ = s.update(m);
    }
    messages
}

fn list_id(s: &Listen) -> ListId {
    ListId::Album(s.library.album.as_ref().unwrap().id.clone())
}

#[test]
fn navidrome_header_shows_metadata_and_every_action() {
    let s = page(demo::navidrome_album());
    for text in [
        "ALBUM",
        "Deluxe Edition",
        "Neon Horizons",
        "Night Drive",
        "Violet Skies",
        "Released Mar 12, 2021",
        "8 tracks",
        "Explicit",
        "37 plays",
        "Add next",
        "Add last",
        "Search",
    ] {
        assert!(has_text(&s, text), "missing {text}");
    }
    // Every album artist is its own link; the original date comes first.
    assert!(has_text(&s, "Nov 8, 2019"));
    // Disc rows with their titles.
    assert!(has_text(&s, "Disc 1 - Originals") && has_text(&s, "Disc 2 - Reworks"));
    // Pills and links below the tracks.
    for text in ["Synthwave", "Midnight Tapes", "Dreamy", "Night Drive Series"] {
        assert!(has_text(&s, text), "missing {text}");
    }
    assert!(has_text(&s, "Show more"));
}

#[test]
fn play_next_and_last_queue_the_whole_album() {
    let mut s = page(demo::navidrome_album());
    let before = s.queue.len();
    let sent = click_text(&mut s, "Add last");
    assert!(sent.iter().any(|m| matches!(m, Message::AlbumPage(AlbumPageMsg::Play(PagePlay::Last)))));
    assert_eq!(s.queue.len(), before + 8);
    let _ = click_text(&mut s, "Add next");
    assert_eq!(s.queue.len(), before + 16);
}

#[test]
fn shuffled_variants_hide_behind_the_chevron() {
    let mut s = page(demo::navidrome_album());
    assert!(!has_text(&s, "Add next shuffled"));
    let _ = s.update(Message::AlbumPage(AlbumPageMsg::TogglePlayMenu));
    for text in ["Shuffle", "Add next shuffled", "Add last shuffled"] {
        assert!(has_text(&s, text), "missing {text}");
    }
    let before = s.queue.len();
    let _ = click_text(&mut s, "Add last shuffled");
    assert_eq!(s.queue.len(), before + 8);
    // Choosing closes the menu.
    assert!(!has_text(&s, "Add next shuffled"));
}

#[test]
fn disc_checkbox_selects_that_discs_tracks() {
    let mut s = page(demo::navidrome_album());
    let id = list_id(&s);
    let _ = s.update(Message::AlbumPage(AlbumPageMsg::Disc(2)));
    assert_eq!(s.lists.get(&id).selection.indices(), vec![4, 5, 6, 7]);
    // Disc 1 adds to it; disc 2 again takes only its own away.
    let _ = s.update(Message::AlbumPage(AlbumPageMsg::Disc(1)));
    assert_eq!(s.lists.get(&id).selection.len(), 8);
    let _ = s.update(Message::AlbumPage(AlbumPageMsg::Disc(2)));
    assert_eq!(s.lists.get(&id).selection.indices(), vec![0, 1, 2, 3]);
}

#[test]
fn search_and_sort_change_the_rows_and_drop_the_disc_rows() {
    let mut s = page(demo::navidrome_album());
    assert!(s.album_page.grouped());
    let _ = s.update(Message::AlbumPage(AlbumPageMsg::Search("static".into())));
    let titles: Vec<_> = s.album_rows("nd-album-neon").iter().map(|r| r.title.clone()).collect();
    assert_eq!(titles, ["Static Hearts", "Static Hearts (Live)"]);
    assert!(!s.album_page.grouped());
    assert!(!has_text(&s, "Disc 1 - Originals"));
    let _ = s.update(Message::AlbumPage(AlbumPageMsg::Search(String::new())));
    let _ = s.update(Message::AlbumPage(AlbumPageMsg::Sort(TrackSort::Name)));
    assert_eq!(s.album_rows("nd-album-neon")[0].title, "Afterglow (Rework)");
    let _ = s.update(Message::AlbumPage(AlbumPageMsg::ToggleOrder));
    assert_eq!(s.album_rows("nd-album-neon")[0].title, "Static Hearts (Live)");
}

#[test]
fn double_click_plays_the_album_from_the_searched_row() {
    let mut s = page(demo::navidrome_album());
    let _ = s.update(Message::AlbumPage(AlbumPageMsg::Search("midnight".into())));
    let id = list_id(&s);
    let _ = s.update(Message::List(listen::app::list::ListMsg::DoubleClick(id, 1)));
    assert_eq!(s.queue.current().unwrap().song.title, "Midnight Mile (Live)");
}

#[test]
fn favorite_rating_and_more() {
    let mut s = page(demo::navidrome_album());
    assert!(s.library.album.as_ref().unwrap().favorite);
    let _ = s.update(Message::AlbumPage(AlbumPageMsg::Favorite));
    assert!(!s.library.album.as_ref().unwrap().favorite);
    let _ = s.update(Message::AlbumPage(AlbumPageMsg::Rate(2)));
    assert_eq!(s.library.album.as_ref().unwrap().rating, Some(2));
    // The same stars clear it.
    let _ = s.update(Message::AlbumPage(AlbumPageMsg::Rate(2)));
    assert_eq!(s.library.album.as_ref().unwrap().rating, None);
    let _ = s.update(Message::AlbumPage(AlbumPageMsg::More(iced::Point::new(400.0, 300.0))));
    assert!(matches!(s.menu.as_ref().map(|m| &m.target), Some(listen::app::menu::MenuTarget::Albums(_))));
}

#[test]
fn radio_queues_the_loaded_songs() {
    let mut s = page(demo::navidrome_album());
    let songs = demo::album_songs(1);
    let n = songs.len();
    let _ = s.update(Message::AlbumPage(AlbumPageMsg::RadioLoaded(
        listen_core::settings::PlayBehavior::Last,
        Ok(songs),
    )));
    assert!(s.queue.len() >= n);
    // Nothing found: a toast, no change.
    let before = s.queue.len();
    let _ = s.update(Message::AlbumPage(AlbumPageMsg::RadioLoaded(
        listen_core::settings::PlayBehavior::Last,
        Ok(vec![]),
    )));
    assert_eq!(s.queue.len(), before);
}

#[test]
fn external_links_follow_the_setting() {
    let mut s = page(demo::navidrome_album());
    assert!(has_text(&s, "EXTERNAL LINKS"));
    // The links are icon buttons (the site name is the tooltip); sites are toggled one by one.
    let album = s.library.album.clone().unwrap();
    let names = |s: &Listen| -> Vec<&'static str> {
        listen::ui::album_detail::format::external_links(&album, &s.config.settings.artist.external_links)
            .iter()
            .map(|l| l.label)
            .collect()
    };
    assert!(names(&s).contains(&"Last.fm") && names(&s).contains(&"Qobuz"));
    s.config.settings.artist.external_links.last_fm = false;
    assert!(!names(&s).contains(&"Last.fm") && names(&s).contains(&"Qobuz"));
    assert!(has_text(&s, "EXTERNAL LINKS"));
    s.config.settings.artist.external_links.enabled = false;
    assert!(!has_text(&s, "EXTERNAL LINKS") && names(&s).is_empty());
}

#[test]
fn ytm_album_renders_and_hides_what_youtube_cannot_do() {
    let mut s = page(demo::ytm_album());
    for text in ["SINGLE", "Glass Cities", "Paper Satellites", "5 tracks", "Add next", "Add last"] {
        assert!(has_text(&s, text), "missing {text}");
    }
    // One disc: no disc rows; no OpenSubsonic metadata.
    assert!(!has_text(&s, "Disc 1"));
    let album = s.library.album.clone().unwrap();
    let caps = s.album_caps(&album);
    assert!(caps.contains(listen_source::Capabilities::RADIO));
    assert!(!caps.contains(listen_source::Capabilities::RATINGS));
    // A rating does nothing.
    let _ = s.update(Message::AlbumPage(AlbumPageMsg::Rate(5)));
    assert_eq!(s.library.album.as_ref().unwrap().rating, None);
    // Play, add to the queue and like a track.
    let before = s.queue.len();
    let _ = click_text(&mut s, "Add last");
    assert_eq!(s.queue.len(), before + 5);
    let _ = s.update(Message::PlayAlbumTrack(0));
    assert_eq!(s.queue.current().unwrap().song.id, "ytDemoVid00");
    // The context menu opens for the album (add to playlist lives there).
    let _ = s.update(Message::AlbumPage(AlbumPageMsg::More(iced::Point::new(400.0, 300.0))));
    assert!(s.menu.is_some());
}

#[test]
fn opening_another_album_resets_search_and_sort() {
    let mut s = page(demo::navidrome_album());
    let _ = s.update(Message::AlbumPage(AlbumPageMsg::Search("x".into())));
    let _ = s.update(Message::AlbumLoaded(Ok(demo::ytm_album())));
    assert!(s.album_page.search.is_empty());
    assert_eq!(s.album_rows("MPREb_demoAlbum01").len(), 5);
    assert!(s.album_rows("nd-album-neon").is_empty());
}

#[test]
fn expanded_album_widget_lists_tracks_and_sends_its_messages() {
    let album = demo::navidrome_album();
    let s = page(album.clone());
    let element = listen::ui::album_detail::expanded::expanded(
        &s,
        &album,
        &album.songs,
        listen::ui::album_detail::expanded::Actions {
            close: Message::Noop,
            play: Box::new(|b| Message::List(listen::app::list::ListMsg::PlayBehavior(b))),
            track: Box::new(Message::PlayAlbumTrack),
        },
    );
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings(), s.window_size, element);
    assert!(ui.find("Neon Horizons").is_ok() && ui.find("Coastal Lights").is_ok());
    ui.click("Add last").unwrap();
    let sent: Vec<Message> = ui.into_messages().collect();
    assert!(matches!(
        sent.as_slice(),
        [Message::List(listen::app::list::ListMsg::PlayBehavior(listen_core::settings::PlayBehavior::Last))]
    ));
}
