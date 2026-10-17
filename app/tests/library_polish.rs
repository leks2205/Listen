//! Library polish: hover play popover, columns and pinning, album groups,
//! play-button behaviour, filters, refresh, links, carousels, palette.

use iced_test::simulator::Simulator;
use listen::app::list::{ConfigEdit, ListId, ListMsg};
use listen::app::{INTER, Listen, Message, Route};
use listen::{Args, FONTS};
use listen_core::settings::{ColumnId, PlayBehavior};

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

const SIZE: iced::Size = iced::Size::new(1600.0, 950.0);

fn page(route: Route) -> Listen {
    let (mut s, _) = Listen::boot(&Args {
        demo: true,
        ephemeral: true,
        theme: Some("tokyoNight".into()),
        ..Args::default()
    });
    let _ = s.update(Message::WindowResized(SIZE));
    let _ = s.update(Message::Navigate(route));
    s
}

fn shows(s: &Listen, text: &str) -> bool {
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings(), SIZE, s.view());
    ui.find(text).is_ok()
}

fn list(s: &mut Listen, msg: ListMsg) {
    let _ = s.update(Message::List(msg));
}

/// A timer of the play popover: `older` ones belong to an earlier hover.
fn tick(s: &mut Listen, older: bool, open: bool) {
    let generation = s.lists.hover_gen - u64::from(older);
    list(s, ListMsg::PlayHoverTimer(generation, open));
}

#[test]
fn the_play_popover_opens_after_the_hover_delay_and_closes_after_leaving() {
    let mut s = page(Route::Tracks);
    let rect = iced::Rectangle::new(iced::Point::new(600.0, 300.0), iced::Size::new(28.0, 28.0));
    list(&mut s, ListMsg::PlayHover(ListId::Songs, 2, rect));
    let pop = s.lists.hover_pop.clone().expect("pending");
    assert!(!pop.open, "not yet: the pointer has to rest");
    // A timer of an older hover does nothing.
    tick(&mut s, true, true);
    assert!(!s.lists.hover_pop.as_ref().unwrap().open);
    tick(&mut s, false, true);
    assert!(s.lists.hover_pop.as_ref().unwrap().open);
    // Leaving the button starts the close timer; entering the popover cancels it.
    list(&mut s, ListMsg::PlayHoverLeave);
    list(&mut s, ListMsg::PlayHoverKeep);
    tick(&mut s, true, false);
    assert!(s.lists.hover_pop.is_some(), "the popover was kept");
    list(&mut s, ListMsg::PlayHoverLeave);
    tick(&mut s, false, false);
    assert!(s.lists.hover_pop.is_none());
}

#[test]
fn the_popover_offers_now_next_and_last_and_a_choice_closes_it() {
    let mut s = page(Route::Tracks);
    let rect = iced::Rectangle::new(iced::Point::new(600.0, 300.0), iced::Size::new(28.0, 28.0));
    list(&mut s, ListMsg::PlayHover(ListId::Songs, 1, rect));
    tick(&mut s, false, true);
    assert!(s.lists.hover_pop.as_ref().unwrap().open);
    list(&mut s, ListMsg::PlayRow(ListId::Songs, 1, PlayBehavior::Next));
    assert!(s.lists.hover_pop.is_none(), "choosing closes it");
}

#[test]
fn new_columns_have_data_in_the_demo_rows() {
    let mut s = page(Route::Tracks);
    for c in [ColumnId::Composer, ColumnId::Date, ColumnId::ReleaseDate, ColumnId::LastPlayed] {
        list(&mut s, ListMsg::Config(ListId::Songs, ConfigEdit::ColumnEnabled(c, true)));
    }
    assert!(shows(&s, "Composer") && shows(&s, "Release date") && shows(&s, "Last played"));
    assert!(shows(&s, "Lena Voss"));
}

#[test]
fn refresh_reloads_the_list_in_demo_without_a_source() {
    use listen::app::list_query::ListQueryMsg;
    let mut s = page(Route::Albums);
    s.library.albums.truncate(1);
    let _ = s.update(Message::ListQuery(ListQueryMsg::Refresh(ListId::Albums)));
    assert!(s.library.albums.len() > 1, "reloaded");
}

#[test]
fn the_play_row_of_the_context_menu_follows_the_play_button_behavior() {
    use listen::app::menu::{MenuAction, MenuMsg, MenuTarget};
    for behavior in [PlayBehavior::Now, PlayBehavior::Next, PlayBehavior::Last] {
        let mut s = page(Route::Home);
        list(&mut s, ListMsg::PlayBehavior(behavior));
        // Something is playing already, so next and last differ from now.
        let songs = listen::demo::album_songs(0);
        let _ = s.update(Message::Menu(MenuMsg::Open(MenuTarget::song(&songs[0]))));
        let _ = s.update(Message::Menu(MenuMsg::Do(MenuAction::PlayDefault)));
        assert!(s.menu.is_none());
        let queued: Vec<String> = s.queue.iter().map(|i| i.song.id.clone()).collect();
        assert!(queued.contains(&songs[0].id), "{behavior:?}: {queued:?}");
    }
}

#[test]
fn the_play_row_runs_the_default_when_clicked() {
    use listen::app::menu::{MenuMsg, MenuTarget};
    let mut s = page(Route::Home);
    let songs = listen::demo::album_songs(0);
    let _ = s.update(Message::Menu(MenuMsg::Open(MenuTarget::song(&songs[0]))));
    let entries = listen::app::menu::entries(&s, s.menu.as_ref().unwrap());
    let play = &entries[0];
    assert!(play.is_submenu() && play.primary.is_some(), "the Play row has a default action");
}

// ---- genre detail, saved queries, album filters -------------------------------------

use listen::app::list_query::{FilterEdit, ListQueryMsg};
use listen_core::settings::GenreTarget;

fn q(s: &mut Listen, msg: ListQueryMsg) {
    let _ = s.update(Message::ListQuery(msg));
}

fn genre_page(target: GenreTarget) -> Listen {
    let mut s = page(Route::Home);
    s.config.settings.lists.genre_target = target;
    let _ = s.update(Message::Navigate(Route::Genre("Synthwave".into())));
    s
}

fn genre_names(s: &Listen) -> Vec<String> {
    s.extra.genre.as_ref().unwrap().albums.iter().map(|a| a.name.clone()).collect()
}

#[test]
fn the_genre_page_has_the_list_header_with_sort_search_and_filters() {
    let mut s = genre_page(GenreTarget::Albums);
    for text in ["Synthwave", "Name", "Play", "Albums", "Tracks"] {
        assert!(shows(&s, text), "{text}");
    }
    let id = ListId::Genre("Synthwave".into());
    let all = genre_names(&s);
    assert!(all.len() > 3);
    // Sort: descending flips the order.
    q(&mut s, ListQueryMsg::ToggleOrder(id.clone()));
    let mut reversed = all.clone();
    reversed.sort_by_key(|n| n.to_lowercase());
    reversed.reverse();
    assert_eq!(genre_names(&s), reversed);
    // Search narrows the page, and clearing brings everything back.
    q(&mut s, ListQueryMsg::SearchInput(id.clone(), "night".into()));
    assert!(
        genre_names(&s).len() < all.len()
            && genre_names(&s).iter().all(|n| n.to_lowercase().contains("night"))
    );
    assert!(!s.list_queries.albums.search.contains("night"), "the Albums page keeps its own search");
    q(&mut s, ListQueryMsg::ClearSearch(id.clone()));
    assert_eq!(genre_names(&s).len(), all.len());
    // Filters: favourites only.
    q(&mut s, ListQueryMsg::Filter(id.clone(), FilterEdit::Favorite(Some(true))));
    assert!(genre_names(&s).len() < all.len());
    assert!(s.extra.genre.as_ref().unwrap().albums.iter().all(|a| a.favorite));
    // The filter modal has no genre list (the genre is fixed) but has the others.
    q(&mut s, ListQueryMsg::OpenFilters(id));
    assert!(shows(&s, "Favorited") && shows(&s, "Album filters"));
    assert!(
        !s.query_filters(&ListId::Genre("Synthwave".into())).contains(listen_source::QueryFilters::GENRES)
    );
    assert!(s.query_filters(&ListId::Albums).contains(listen_source::QueryFilters::GENRES));
}

#[test]
fn the_genre_tracks_have_their_own_sort_and_search() {
    let mut s = genre_page(GenreTarget::Tracks);
    let id = ListId::GenreSongs("Synthwave".into());
    let all = s.extra.genre.as_ref().unwrap().songs.len();
    q(&mut s, ListQueryMsg::SearchInput(id.clone(), "signal".into()));
    let songs = &s.extra.genre.as_ref().unwrap().songs;
    assert!(
        !songs.is_empty()
            && songs.len() < all
            && songs.iter().all(|x| x.title.to_lowercase().contains("signal"))
    );
    assert!(shows(&s, "Tracks"));
    // The sort dropdown lists the song sorts.
    q(&mut s, ListQueryMsg::OpenSort(id, iced::Point::new(600.0, 150.0)));
    assert!(shows(&s, "BPM"));
}

#[test]
fn sort_and_filters_are_saved_in_the_settings_per_list() {
    let mut s = page(Route::Albums);
    assert!(s.config.settings.lists.saved_queries.is_empty());
    q(&mut s, ListQueryMsg::Sort(ListId::Albums, 5));
    q(&mut s, ListQueryMsg::Filter(ListId::Albums, FilterEdit::Favorite(Some(true))));
    q(&mut s, ListQueryMsg::SearchInput(ListId::Albums, "x".into()));
    let saved = &s.config.settings.lists.saved_queries;
    assert!(saved.contains_key("albums") && !saved.contains_key("songs"));
    assert_eq!(saved["albums"].favorite, Some(true));
    assert!(saved["albums"].search.is_empty(), "the search is not saved");
    // The next start (same settings) opens with them.
    let restored = listen::app::list_query::ListQueries::from_settings(&s.config.settings.lists);
    assert_eq!(restored.albums.filters.favorite, Some(true));
    assert_eq!(restored.albums.sort, s.list_queries.albums.sort);
}

#[test]
fn the_album_filters_offer_custom_tags() {
    let mut s = page(Route::Albums);
    q(&mut s, ListQueryMsg::OpenFilters(ListId::Albums));
    assert!(shows(&s, "Tags"), "the tag section");
    assert!(shows(&s, "Dreamy"), "the values of the first tag are listed");
    // Picking a value filters the request.
    q(&mut s, ListQueryMsg::Filter(ListId::Albums, FilterEdit::Tag("mood".into(), "mood0".into())));
    assert!(s.list_queries.albums.filters.active());
    let query = s.list_queries.albums.to_source(listen_source::QueryFilters::TAGS, 0, 10);
    assert_eq!(query.tags, vec![("mood".to_string(), vec!["mood0".to_string()])]);
}

// ---- expanded album panel ------------------------------------------------------------

#[test]
fn the_expand_control_opens_one_album_panel_under_its_row() {
    let mut s = page(Route::Albums);
    list(&mut s, ListMsg::Expand(ListId::Albums, 1));
    assert_eq!(s.lists.expanded, Some((ListId::Albums, 1)));
    let name = s.library.albums[1].name.clone();
    // The panel shows the album's tracks and the play buttons.
    assert!(shows(&s, "Add next") && shows(&s, "Add last"), "panel buttons");
    assert!(shows(&s, &s.library.albums[1].songs[0].title.clone()), "its first track");
    // Another card replaces it; the same one closes it.
    list(&mut s, ListMsg::Expand(ListId::Albums, 3));
    assert_eq!(s.lists.expanded, Some((ListId::Albums, 3)));
    list(&mut s, ListMsg::Expand(ListId::Albums, 3));
    assert_eq!(s.lists.expanded, None);
    // Closing with the panel's own button.
    list(&mut s, ListMsg::Expand(ListId::Albums, 1));
    list(&mut s, ListMsg::Collapse);
    assert_eq!(s.lists.expanded, None);
    let _ = name;
}

#[test]
fn a_new_query_closes_the_expanded_panel() {
    let mut s = page(Route::Albums);
    list(&mut s, ListMsg::Expand(ListId::Albums, 0));
    q(&mut s, ListQueryMsg::ToggleOrder(ListId::Albums));
    assert_eq!(s.lists.expanded, None, "the rows were replaced");
}

#[test]
fn the_panel_play_buttons_follow_their_behavior() {
    let mut s = page(Route::Albums);
    list(&mut s, ListMsg::Expand(ListId::Albums, 0));
    let before = s.queue.iter().count();
    list(&mut s, ListMsg::PlayRow(ListId::Albums, 0, PlayBehavior::Last));
    assert!(s.queue.iter().count() > before, "the album was added");
}

// ---- carousels ------------------------------------------------------------------------

#[test]
fn dragging_a_carousel_pages_it_and_does_not_open_the_card_under_the_pointer() {
    use iced::mouse::{Button, Event as MouseEvent};
    use listen::app::home::HomeMsg;
    let s = page(Route::Home);
    // A card of the "Most Played" row.
    let name =
        s.library.home_state.row(listen::app::home::Section::MostPlayed).unwrap().albums[1].name.clone();
    let messages: Vec<Message> = {
        let _gpu = gpu();
        let mut ui = Simulator::with_size(settings(), SIZE, s.view());
        let target = ui.find(name.as_str()).expect("the card");
        let mut at = iced_test::selector::Bounded::visible_bounds(&target).unwrap().center();
        at.y -= 90.0; // the cover above the name
        ui.point_at(at);
        let _ = ui.simulate([iced::Event::Mouse(MouseEvent::ButtonPressed(Button::Left))]);
        ui.point_at(iced::Point::new(at.x - 120.0, at.y));
        let _ = ui.simulate([iced::Event::Mouse(MouseEvent::ButtonReleased(Button::Left))]);
        ui.into_messages().collect()
    };
    assert!(
        messages.iter().any(|m| matches!(m, Message::Home(HomeMsg::Drag(_, dx, _, _)) if *dx < -100.0)),
        "{messages:?}"
    );
    assert!(
        !messages.iter().any(|m| matches!(m, Message::OpenAlbum(_))),
        "a drag is not a click: {messages:?}"
    );
}

#[test]
fn a_plain_click_on_a_carousel_card_still_opens_it() {
    use iced::mouse::{Button, Event as MouseEvent};
    let s = page(Route::Home);
    let name =
        s.library.home_state.row(listen::app::home::Section::MostPlayed).unwrap().albums[1].name.clone();
    let messages: Vec<Message> = {
        let _gpu = gpu();
        let mut ui = Simulator::with_size(settings(), SIZE, s.view());
        let target = ui.find(name.as_str()).expect("the card");
        let mut at = iced_test::selector::Bounded::visible_bounds(&target).unwrap().center();
        at.y -= 90.0;
        ui.point_at(at);
        let _ = ui.simulate([
            iced::Event::Mouse(MouseEvent::ButtonPressed(Button::Left)),
            iced::Event::Mouse(MouseEvent::ButtonReleased(Button::Left)),
        ]);
        ui.into_messages().collect()
    };
    assert!(messages.iter().any(|m| matches!(m, Message::OpenAlbum(_))), "{messages:?}");
}

// ---- long press on the main Play button -------------------------------------------------

fn press_play(s: &Listen, hold: std::time::Duration) -> Vec<Message> {
    use iced::mouse::{Button, Event as MouseEvent};
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings(), SIZE, s.view());
    let target = ui.find("Play").expect("the Play button");
    let at = iced_test::selector::Bounded::visible_bounds(&target).unwrap().center();
    ui.point_at(at);
    let _ = ui.simulate([iced::Event::Mouse(MouseEvent::ButtonPressed(Button::Left))]);
    std::thread::sleep(hold);
    let _ = ui.simulate([iced::Event::Mouse(MouseEvent::ButtonReleased(Button::Left))]);
    ui.into_messages().collect()
}

#[test]
fn a_short_press_on_play_plays_and_a_long_press_plays_the_shuffled_version() {
    let s = page(Route::Albums);
    let short = press_play(&s, std::time::Duration::from_millis(0));
    assert!(
        short
            .iter()
            .any(|m| matches!(m, Message::ListQuery(ListQueryMsg::PlayAll(_, PlayBehavior::Now, false)))),
        "{short:?}"
    );
    let long = press_play(&s, std::time::Duration::from_millis(650));
    assert!(
        long.iter()
            .any(|m| matches!(m, Message::ListQuery(ListQueryMsg::PlayAll(_, PlayBehavior::Now, true)))),
        "{long:?}"
    );
    assert!(
        !long.iter().any(|m| matches!(m, Message::ListQuery(ListQueryMsg::PlayAll(_, _, false)))),
        "the long press does not also play normally: {long:?}"
    );
}

#[test]
fn the_long_press_shuffles_what_the_setting_does() {
    use listen::app::album_page::PagePlay;
    assert_eq!(PagePlay::long_press_of(PlayBehavior::Now), PagePlay::Shuffle);
    assert_eq!(PagePlay::long_press_of(PlayBehavior::Next), PagePlay::NextShuffled);
    assert_eq!(PagePlay::long_press_of(PlayBehavior::Last), PagePlay::LastShuffled);
}
