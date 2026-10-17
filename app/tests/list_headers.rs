//! The header of the Tracks and Albums pages: sort dropdown and order toggle,
//! filters, search, refresh, play-all and the saved collections.

use iced_test::simulator::Simulator;
use listen::app::list::ListId;
use listen::app::list_query::{FilterEdit, ListQueryMsg};
use listen::app::{INTER, Listen, Message, Route};
use listen::{Args, FONTS};
use listen_core::settings::{CollectionList, PlayBehavior};
use listen_source::{AlbumSort, SongSort, SortOrder};

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

fn q(s: &mut Listen, msg: ListQueryMsg) {
    let _ = s.update(Message::ListQuery(msg));
}

fn click(s: &mut Listen, text: &str) {
    let messages: Vec<Message> = {
        let _gpu = gpu();
        let mut ui = Simulator::with_size(settings(), SIZE, s.view());
        ui.click(text).unwrap_or_else(|e| panic!("no {text:?} on screen: {e:?}"));
        ui.into_messages().collect()
    };
    for m in messages {
        let _ = s.update(m);
    }
}

fn shows(s: &Listen, text: &str) -> bool {
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings(), SIZE, s.view());
    ui.find(text).is_ok()
}

fn names(s: &Listen) -> Vec<String> {
    s.library.albums.iter().map(|a| a.name.clone()).collect()
}

#[test]
fn headers_show_the_controls_and_the_sort() {
    for (route, title) in [(Route::Albums, "Albums"), (Route::Tracks, "Tracks")] {
        let s = page(route);
        assert!(shows(&s, title));
        assert!(shows(&s, "Name"), "{title}: sort dropdown");
        assert!(shows(&s, "Play"), "{title}: play all");
    }
}

#[test]
fn the_sort_dropdown_lists_the_servers_options_and_sorts() {
    let mut s = page(Route::Albums);
    let before = names(&s);
    // The button opens the dropdown.
    click(&mut s, "Name");
    assert!(s.list_queries.popup.is_some());
    assert!(shows(&s, "Recently added") && shows(&s, "Song count"), "Navidrome's list");
    click(&mut s, "Recently added");
    assert!(s.list_queries.popup.is_none());
    assert_eq!(s.list_queries.albums.sort, AlbumSort::RecentlyAdded);
    // The option's own order (newest first) applies.
    assert_eq!(s.list_queries.albums.order, SortOrder::Descending);
    let mut reversed = before.clone();
    reversed.reverse();
    assert_eq!(names(&s), reversed);
    assert!(shows(&s, "Recently added"), "the button shows the sort");
}

#[test]
fn the_order_toggle_flips_the_list() {
    let mut s = page(Route::Albums);
    q(&mut s, ListQueryMsg::ToggleOrder(ListId::Albums));
    let desc = names(&s);
    let mut asc = desc.clone();
    asc.sort_by_key(|n| n.to_lowercase());
    asc.reverse();
    assert_eq!(desc, asc);
    q(&mut s, ListQueryMsg::ToggleOrder(ListId::Albums));
    let mut sorted = desc.clone();
    sorted.sort_by_key(|n| n.to_lowercase());
    assert_eq!(names(&s), sorted);
}

#[test]
fn search_filters_the_page_and_clears() {
    let mut s = page(Route::Albums);
    let all = s.library.albums.len();
    q(&mut s, ListQueryMsg::SearchInput(ListId::Albums, "night".into()));
    assert!(!s.library.albums.is_empty() && s.library.albums.len() < all);
    assert!(s.library.albums.iter().all(|a| a.name.to_lowercase().contains("night")));
    q(&mut s, ListQueryMsg::SearchInput(ListId::Albums, "zzzz".into()));
    assert!(s.library.albums.is_empty());
    assert!(shows(&s, "No albums match."));
    q(&mut s, ListQueryMsg::ClearSearch(ListId::Albums));
    assert_eq!(s.library.albums.len(), all);
    // The Tracks list has its own search.
    let mut t = page(Route::Tracks);
    let n = t.library.songs.len();
    q(&mut t, ListQueryMsg::SearchInput(ListId::Songs, "coastline".into()));
    assert!(t.library.songs.len() < n && !t.library.songs.is_empty());
}

#[test]
fn a_changed_query_resets_selection_and_page() {
    let mut s = page(Route::Albums);
    s.modifiers = iced::keyboard::Modifiers::CTRL;
    let _ = s.update(Message::List(listen::app::list::ListMsg::Press(ListId::Albums, 2)));
    assert!(!s.lists.get(&ListId::Albums).selection.is_empty());
    q(&mut s, ListQueryMsg::ToggleOrder(ListId::Albums));
    assert!(s.lists.get(&ListId::Albums).selection.is_empty());
}

#[test]
fn filters_open_in_a_modal_and_reset() {
    let mut s = page(Route::Albums);
    click(&mut s, "Name"); // open the dropdown, then close it
    q(&mut s, ListQueryMsg::ClosePopup);
    q(&mut s, ListQueryMsg::OpenFilters(ListId::Albums));
    assert!(shows(&s, "Album filters") && shows(&s, "Favorited") && shows(&s, "Compilation"));
    assert!(shows(&s, "Recently played") || shows(&s, "Genres"));
    let all = s.library.albums.len();
    q(&mut s, ListQueryMsg::Filter(ListId::Albums, FilterEdit::YearFrom("2025".into())));
    assert!(s.list_queries.albums.filters.active());
    assert!(s.library.albums.len() < all);
    assert!(s.library.albums.iter().all(|a| a.year.is_some_and(|y| y >= 2025)));
    // The button's highlight follows the filters; Reset clears them all.
    click(&mut s, "Reset");
    assert!(!s.list_queries.albums.filters.active());
    assert_eq!(s.library.albums.len(), all);
}

#[test]
fn genre_and_artist_filters_pick_from_the_lists() {
    let mut s = page(Route::Albums);
    q(&mut s, ListQueryMsg::OpenFilters(ListId::Albums));
    assert!(!s.list_queries.artist_options.is_empty());
    let (id, name) = s.list_queries.artist_options[0].clone();
    q(&mut s, ListQueryMsg::Filter(ListId::Albums, FilterEdit::Artist(id, name.clone())));
    assert!(!s.library.albums.is_empty());
    assert!(s.library.albums.iter().all(|a| a.artist_line() == name));
    q(&mut s, ListQueryMsg::Filter(ListId::Albums, FilterEdit::Genre("Nope".into())));
    assert!(s.library.albums.is_empty());
    q(&mut s, ListQueryMsg::Filter(ListId::Albums, FilterEdit::Reset));
    assert!(!s.library.albums.is_empty());
}

#[test]
fn track_filters_favorite_yes_and_no() {
    let mut s = page(Route::Tracks);
    let all = s.library.songs.len();
    q(&mut s, ListQueryMsg::Filter(ListId::Songs, FilterEdit::Favorite(Some(true))));
    let liked = s.library.songs.len();
    assert!(liked > 0 && liked < all);
    assert!(s.library.songs.iter().all(|x| x.favorite));
    q(&mut s, ListQueryMsg::Filter(ListId::Songs, FilterEdit::Favorite(Some(false))));
    assert_eq!(s.library.songs.len(), all - liked);
    q(&mut s, ListQueryMsg::OpenFilters(ListId::Songs));
    assert!(shows(&s, "Track filters"));
    // Compilation is an album filter.
    assert!(!shows(&s, "Compilation"));
}

#[test]
fn play_all_queues_everything_the_filter_matches() {
    let mut s = page(Route::Albums);
    q(&mut s, ListQueryMsg::SearchInput(ListId::Albums, "night".into()));
    let expected: usize = s.library.albums.iter().map(|a| a.songs.len()).sum();
    assert!(expected > 0);
    q(&mut s, ListQueryMsg::PlayAll(ListId::Albums, PlayBehavior::Now, false));
    assert_eq!(s.queue.len(), expected);
    // Tracks: the shuffle variant queues the same songs.
    let mut t = page(Route::Tracks);
    q(&mut t, ListQueryMsg::Filter(ListId::Songs, FilterEdit::Favorite(Some(true))));
    let n = t.library.songs.len();
    q(&mut t, ListQueryMsg::PlayAll(ListId::Songs, PlayBehavior::Now, true));
    assert_eq!(t.queue.len(), n);
}

#[test]
fn clicking_play_sends_play_all_with_the_behaviour() {
    let mut s = page(Route::Tracks);
    s.config.settings.lists.play_behavior = PlayBehavior::Last;
    let messages: Vec<Message> = {
        let _gpu = gpu();
        let mut ui = Simulator::with_size(settings(), SIZE, s.view());
        ui.click("Play last").expect("play button");
        ui.into_messages().collect()
    };
    assert!(messages.iter().any(|m| matches!(
        m,
        Message::ListQuery(ListQueryMsg::PlayAll(ListId::Songs, PlayBehavior::Last, false))
    )));
}

#[test]
fn collections_save_override_apply_and_delete() {
    let mut s = page(Route::Albums);
    q(&mut s, ListQueryMsg::Filter(ListId::Albums, FilterEdit::YearFrom("2025".into())));
    q(&mut s, ListQueryMsg::ToggleOrder(ListId::Albums));
    // Without a name nothing is stored.
    q(&mut s, ListQueryMsg::SaveCollection(ListId::Albums));
    assert!(s.config.settings.lists.collections.is_empty());
    q(&mut s, ListQueryMsg::CollectionName("New ones".into()));
    q(&mut s, ListQueryMsg::SaveCollection(ListId::Albums));
    let saved = s.config.settings.lists.collections.clone();
    assert_eq!(saved.len(), 1);
    assert_eq!(
        (saved[0].name.as_str(), saved[0].list, saved[0].descending),
        ("New ones", CollectionList::Albums, true)
    );
    assert_eq!(saved[0].year_min, Some(2025));
    // The same name overrides.
    q(&mut s, ListQueryMsg::CollectionName("New ones".into()));
    q(&mut s, ListQueryMsg::SaveCollection(ListId::Albums));
    assert_eq!(s.config.settings.lists.collections.len(), 1);
    // Change the list, then pick the collection: sort, order and filters come back.
    q(&mut s, ListQueryMsg::Filter(ListId::Albums, FilterEdit::Reset));
    q(&mut s, ListQueryMsg::ToggleOrder(ListId::Albums));
    let all = s.library.albums.len();
    q(&mut s, ListQueryMsg::ApplyCollection(0));
    assert_eq!(s.list_queries.albums.filters.year_min, Some(2025));
    assert_eq!(s.list_queries.albums.order, SortOrder::Descending);
    assert!(s.library.albums.len() < all);
    // The picker lists it.
    q(&mut s, ListQueryMsg::OpenCollections(ListId::Albums, iced::Point::new(300.0, 120.0)));
    assert!(shows(&s, "New ones"));
    q(&mut s, ListQueryMsg::DeleteCollection(0));
    assert!(s.config.settings.lists.collections.is_empty());
}

#[test]
fn a_collection_for_tracks_opens_the_tracks_page() {
    let mut s = page(Route::Albums);
    let mut tracks = page(Route::Tracks);
    q(&mut tracks, ListQueryMsg::Filter(ListId::Songs, FilterEdit::Favorite(Some(true))));
    q(&mut tracks, ListQueryMsg::CollectionName("Liked".into()));
    q(&mut tracks, ListQueryMsg::SaveCollection(ListId::Songs));
    s.config.settings.lists.collections = tracks.config.settings.lists.collections.clone();
    assert_eq!(s.list_queries.songs.sort, SongSort::Name);
    q(&mut s, ListQueryMsg::ApplyCollection(0));
    assert_eq!(s.route, Route::Tracks);
    assert!(s.library.songs.iter().all(|x| x.favorite));
}

#[test]
fn refresh_reloads_the_list() {
    let mut s = page(Route::Albums);
    s.library.albums.truncate(2);
    q(&mut s, ListQueryMsg::Refresh(ListId::Albums));
    assert!(s.library.albums.len() > 2);
}
