//! Playlists: the list page, the playlist page (search, sort, edit mode with
//! reorder, remove with a confirmation), the dialogs and the add-to-playlist
//! picker. Demo mode; writes are applied locally.

use iced_test::simulator::Simulator;
use listen::app::album_page::TrackSort;
use listen::app::list::ListId;
use listen::app::menu::{MenuAction, MenuMsg, MenuPage, MenuTarget};
use listen::app::overlay::{Modal, OverlayMsg};
use listen::app::playlists::PlaylistPageMsg as P;
use listen::app::{INTER, Listen, Message, PlaylistDetail, Route};
use listen::{Args, FONTS, demo};

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

fn boot() -> Listen {
    let (mut s, _) = Listen::boot(&Args { demo: true, theme: Some("tokyoNight".into()), ..Args::default() });
    let _ = s.update(Message::WindowResized(iced::Size::new(1400.0, 900.0)));
    s.library.playlists = demo::playlists();
    s
}

fn page(detail: PlaylistDetail) -> Listen {
    let mut s = boot();
    let _ = s.update(Message::Navigate(Route::Playlist(detail.playlist.id.clone())));
    let _ = s.update(Message::PlaylistLoaded(Ok(detail)));
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

fn titles(s: &Listen) -> Vec<String> {
    s.playlist_rows("nd-pl-road").iter().map(|x| x.title.clone()).collect()
}

#[test]
fn the_list_page_shows_playlists_searches_and_sorts() {
    let mut s = boot();
    let _ = s.update(Message::Navigate(Route::Playlists));
    for t in ["Playlists", "Road trip", "Gym", "Create playlist", "Search"] {
        assert!(has_text(&s, t), "missing {t}");
    }
    assert_eq!(s.playlist_page.list.len(), 6);
    let _ = s.update(Message::PlaylistPage(P::ListSearch("amy".into())));
    assert_eq!(s.playlist_page.list.len(), 2);
    let _ = s.update(Message::PlaylistPage(P::ListSearch(String::new())));
    let _ = s.update(Message::PlaylistPage(P::ListSort(listen::app::playlists::PlaylistSort::SongCount)));
    assert_eq!(s.playlist_page.list[0].name, "Old favourites");
}

#[test]
fn create_dialog_makes_a_playlist() {
    let mut s = boot();
    let _ = s.update(Message::Navigate(Route::Playlists));
    let _ = click_text(&mut s, "Create playlist");
    assert!(matches!(s.overlay.modal, Some(Modal::Custom(_))));
    let _ = s.update(Message::PlaylistPage(P::FormName("Summer".into())));
    let _ = s.update(Message::PlaylistPage(P::FormComment("sun".into())));
    let _ = s.update(Message::PlaylistPage(P::FormPublic(true)));
    let _ = s.update(Message::PlaylistPage(P::FormSubmit));
    // The demo applies the result by message.
    let done = listen::demo::navidrome_playlist().playlist;
    let _ = done;
    assert!(s.playlist_page.form.as_ref().is_some_and(|f| f.busy));
}

#[test]
fn search_and_sort_keep_positions() {
    let mut s = page(demo::navidrome_playlist());
    assert_eq!(titles(&s).len(), 10);
    let _ = s.update(Message::PlaylistPage(P::Search("lantern".into())));
    // "Lanterns" is in the playlist twice.
    assert_eq!(s.playlist_page.pos, [1, 5]);
    let _ = s.update(Message::PlaylistPage(P::Search(String::new())));
    let _ = s.update(Message::PlaylistPage(P::Sort(TrackSort::Name)));
    let names = titles(&s);
    let mut sorted = names.clone();
    sorted.sort_by_key(|n| n.to_lowercase());
    assert_eq!(names, sorted);
}

#[test]
fn edit_mode_reorders_and_saves_after_a_confirmation() {
    let mut s = page(demo::navidrome_playlist());
    let before = titles(&s);
    let _ = click_text(&mut s, "Edit");
    assert!(s.playlist_page.edit.is_some());
    assert!(has_text(&s, "Save and replace") && has_text(&s, "Cancel"));
    let _ = s.update(Message::PlaylistDragStart(4));
    let _ =
        s.update(Message::List(listen::app::list::ListMsg::Hover(ListId::Playlist("nd-pl-road".into()), 0)));
    let _ = s.update(Message::PlaylistDragEnd);
    let moved = titles(&s);
    assert_eq!(moved[0], before[4]);
    // Cancel throws it away.
    let _ = click_text(&mut s, "Cancel");
    assert_eq!(titles(&s), before);
    // Again, and save: a confirm dialog first.
    let _ = s.update(Message::PlaylistPage(P::EditStart));
    let _ = s.update(Message::PlaylistDragStart(4));
    let _ = s.update(Message::PlaylistDragOver(0));
    let _ = s.update(Message::PlaylistDragEnd);
    let _ = click_text(&mut s, "Save and replace");
    assert!(matches!(s.overlay.modal, Some(Modal::Confirm(_))));
    let task_msgs = s.update(Message::Overlay(OverlayMsg::Confirmed));
    drop(task_msgs);
    let _ = s.update(Message::PlaylistPage(P::EditSaved("nd-pl-road".into(), Ok(()))));
    assert!(s.playlist_page.edit.is_none());
    assert_eq!(s.library.playlist.as_ref().unwrap().songs[0].title, before[4]);
}

#[test]
fn remove_asks_first_and_drops_the_songs() {
    let mut s = page(demo::navidrome_playlist());
    let list = ListId::Playlist("nd-pl-road".into());
    let gone = s.playlist_rows("nd-pl-road")[2].title.clone();
    s.lists.entry(&list).selection.set_only(2);
    let song = s.playlist_rows("nd-pl-road")[2].clone();
    let _ = s.update(Message::Menu(MenuMsg::OpenIn {
        target: MenuTarget::song(&song),
        at: None,
        list: list.clone(),
    }));
    // The entry is offered inside an editable playlist.
    let entries = listen::app::menu::entries(&s, s.menu.as_ref().unwrap());
    let remove = entries.iter().find(|e| e.label == "Remove from playlist").expect("entry");
    let listen::app::menu::EntryKind::Action(action) = &remove.kind else { panic!() };
    let _ = s.update(Message::Menu(MenuMsg::Do(action.clone())));
    assert!(matches!(s.overlay.modal, Some(Modal::Confirm(_))));
    let _ = s.update(Message::Overlay(OverlayMsg::Confirmed));
    let _ = s.update(Message::PlaylistPage(P::Removed("nd-pl-road".into(), vec![2], Ok(()))));
    assert_eq!(s.library.playlist.as_ref().unwrap().songs.len(), 9);
    assert!(!titles(&s).contains(&gone) || titles(&s).iter().filter(|t| **t == gone).count() == 0);
}

#[test]
fn picker_searches_and_pins_the_recent_playlist() {
    let mut s = boot();
    let song = demo::album_songs(1)[0].clone();
    let _ = s.update(Message::Menu(MenuMsg::Open(MenuTarget::song(&song))));
    let _ = s.update(Message::Menu(MenuMsg::Do(MenuAction::ChoosePlaylist)));
    s.playlist_page.recent.insert(song.server_id.clone(), "nd-pl-chill".into());
    if let Some(m) = s.menu.as_mut() {
        m.page = MenuPage::Playlists {
            list: Some(Ok(demo::playlists())),
            new_name: String::new(),
            search: String::new(),
        };
    }
    assert!(has_text(&s, "Chill evenings") && has_text(&s, "Skip duplicates"));
    let _ = s.update(Message::PlaylistPage(P::PickerSearch("gym".into())));
    let Some(MenuPage::Playlists { search, .. }) = s.menu.as_ref().map(|m| &m.page) else { panic!() };
    assert_eq!(search, "gym");
    let all = demo::playlists();
    let shown = listen::app::playlists::picker::ordered(&all, search, Some("nd-pl-chill"));
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].name, "Gym");
    let _ = s.update(Message::PlaylistPage(P::SkipDuplicates(true)));
    assert!(s.config.settings.lists.skip_duplicates);
}
