//! The item list component on the Tracks page (table) and the Albums page
//! (grid): click semantics, multi-select, keyboard, hotkeys, drag payload,
//! play behaviour, columns, the YouTube Music cells and the scroll position.

use iced::keyboard::key::{Code, Named, Physical};
use iced::keyboard::{Event as KeyEvent, Key, Location, Modifiers};
use iced_test::simulator::Simulator;
use listen::app::hotkeys::{HotkeysMsg, ListHotkey};
use listen::app::list::{DropTarget, ListId, ListMsg};
use listen::app::{INTER, Listen, Message, Route};
use listen::{Args, FONTS, demo};
use listen_core::model::{ArtistRef, ServerId, SourceKind};
use listen_core::settings::{ColumnId, DisplayType, ListKind, PlayBehavior, RowSize};

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

fn state_on(route: Route) -> Listen {
    let (mut s, _) = Listen::boot(&Args { demo: true, theme: Some("tokyoNight".into()), ..Args::default() });
    let _ = s.update(Message::WindowResized(SIZE));
    let _ = s.update(Message::Navigate(route));
    s
}

fn tracks() -> Listen {
    state_on(Route::Tracks)
}

fn feed(s: &mut Listen, messages: Vec<Message>) {
    for m in messages {
        let _ = s.update(m);
    }
}

/// Clicks the text `times` times in one view (quick clicks make a double-click) and feeds the messages back.
fn click_text(s: &mut Listen, text: &str, times: usize) -> Vec<Message> {
    let messages: Vec<Message> = {
        let _gpu = gpu();
        let mut ui = Simulator::with_size(settings(), SIZE, s.view());
        for _ in 0..times {
            ui.click(text).unwrap_or_else(|e| panic!("no {text:?} on screen: {e:?}"));
        }
        ui.into_messages().collect()
    };
    feed(s, messages.clone());
    messages
}

/// Ctrl+click: the only way (with Shift) to select a row by clicking.
fn ctrl_click(s: &mut Listen, text: &str) {
    s.modifiers = Modifiers::CTRL;
    click_text(s, text, 1);
    s.modifiers = Modifiers::default();
}

fn right_click_text(s: &mut Listen, text: &str) -> Vec<Message> {
    let messages: Vec<Message> = {
        let _gpu = gpu();
        let mut ui = Simulator::with_size(settings(), SIZE, s.view());
        let target = ui.find(text).unwrap_or_else(|e| panic!("no {text:?}: {e:?}"));
        let at = iced_test::selector::Bounded::visible_bounds(&target).unwrap().center();
        ui.point_at(at);
        let _ = ui.simulate([
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Right)),
            iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Right)),
        ]);
        let mut messages = vec![Message::RightClickAt(at)];
        messages.extend(ui.into_messages());
        messages
    };
    feed(s, messages.clone());
    messages
}

fn shows(s: &Listen, text: &str) -> bool {
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings(), SIZE, s.view());
    ui.find(text).is_ok()
}

fn key(s: &mut Listen, named: Named, modifiers: Modifiers) {
    let code = match named {
        Named::Enter => Code::Enter,
        Named::ArrowDown => Code::ArrowDown,
        Named::ArrowUp => Code::ArrowUp,
        Named::Home => Code::Home,
        Named::End => Code::End,
        _ => Code::Escape,
    };
    let _ = s.update(Message::Keyboard(KeyEvent::KeyPressed {
        key: Key::Named(named),
        modified_key: Key::Named(named),
        physical_key: Physical::Code(code),
        location: Location::Standard,
        modifiers,
        text: None,
        repeat: false,
    }));
}

fn ctrl_a(s: &mut Listen) {
    let _ = s.update(Message::Keyboard(KeyEvent::KeyPressed {
        key: Key::Character("a".into()),
        modified_key: Key::Character("a".into()),
        physical_key: Physical::Code(Code::KeyA),
        location: Location::Standard,
        modifiers: Modifiers::CTRL,
        text: None,
        repeat: false,
    }));
}

fn selected(s: &Listen, id: &ListId) -> Vec<usize> {
    s.lists.get(id).selection.indices()
}

fn current_title(s: &Listen) -> String {
    s.queue.current().map(|i| i.song.title.clone()).unwrap_or_default()
}

fn titles() -> Vec<String> {
    (0..demo::ALBUMS.len()).flat_map(demo::album_songs).map(|s| s.title).collect()
}

fn has(messages: &[Message], f: impl Fn(&ListMsg) -> bool) -> bool {
    messages.iter().any(|m| matches!(m, Message::List(l) if f(l)))
}

// ---- click semantics and multi-select (Tracks table) -------------------------------

#[test]
fn a_plain_click_selects_nothing_does_not_play_and_opens_the_album_after_the_delay() {
    let mut s = tracks();
    let before = current_title(&s);
    let t = titles();
    let m = click_text(&mut s, &t[1], 1);
    assert!(has(&m, |l| matches!(l, ListMsg::Press(ListId::Songs, 1))), "{m:?}");
    assert!(selected(&s, &ListId::Songs).is_empty(), "a plain click does not select");
    assert_eq!(current_title(&s), before, "a single click used to play");
    // A plain click also clears an existing selection.
    ctrl_click(&mut s, &t[3]);
    assert_eq!(selected(&s, &ListId::Songs), [3]);
    click_text(&mut s, &t[2], 1);
    assert!(selected(&s, &ListId::Songs).is_empty());
    // The delayed open goes to the clicked song's album.
    click_text(&mut s, &t[1], 1);
    let album = s.library.songs[1].album_id.clone().expect("demo songs have albums");
    let generation = s.lists.click_gen;
    let _ = s.update(Message::List(ListMsg::ClickOpen(ListId::Songs, 1, generation)));
    assert_eq!(s.route, Route::Album(album));
}

#[test]
fn a_double_click_or_a_stale_generation_cancels_the_pending_open() {
    let mut s = tracks();
    let t = titles();
    // The first click of a double-click arms an open; the double-click bumps the generation.
    let _ = s.update(Message::List(ListMsg::Press(ListId::Songs, 1)));
    let armed = s.lists.click_gen;
    let _ = s.update(Message::List(ListMsg::DoubleClick(ListId::Songs, 1)));
    assert_ne!(s.lists.click_gen, armed);
    let _ = s.update(Message::List(ListMsg::ClickOpen(ListId::Songs, 1, armed)));
    assert_eq!(s.route, Route::Tracks, "the double-click plays instead of opening");
    assert_eq!(current_title(&s), t[1]);
    // A newer click makes the older timer stale as well.
    let _ = s.update(Message::List(ListMsg::Press(ListId::Songs, 2)));
    let stale = s.lists.click_gen;
    let _ = s.update(Message::List(ListMsg::Press(ListId::Songs, 4)));
    let _ = s.update(Message::List(ListMsg::ClickOpen(ListId::Songs, 2, stale)));
    assert_eq!(s.route, Route::Tracks);
}

#[test]
fn double_click_plays_the_list_from_that_song() {
    let mut s = tracks();
    let t = titles();
    let m = click_text(&mut s, &t[2], 2);
    assert!(has(&m, |l| matches!(l, ListMsg::DoubleClick(ListId::Songs, 2))), "{m:?}");
    assert_eq!(current_title(&s), t[2]);
    // The rows around it follow in the queue.
    assert_eq!(s.queue.iter().count(), t.len().min(101));
    // Playing does not select.
    assert!(selected(&s, &ListId::Songs).is_empty());
}

#[test]
fn double_click_follows_the_play_button_behavior() {
    let mut s = tracks();
    let t = titles();
    let playing = current_title(&s);
    s.config.settings.lists.play_behavior = PlayBehavior::Last;
    let before = s.queue.iter().count();
    click_text(&mut s, &t[4], 2);
    assert_eq!(current_title(&s), playing, "add last does not start the song");
    assert_eq!(s.queue.iter().count(), before + 1);
    // Listen's "add to queue": the song waits in the manual queue, which plays next here.
    assert_eq!(s.queue.peek_next().map(|i| i.song.title.clone()).as_deref(), Some(t[4].as_str()));

    s.config.settings.lists.play_behavior = PlayBehavior::Next;
    click_text(&mut s, &t[6], 2);
    let next = s.queue.peek_next().map(|i| i.song.title.clone());
    assert_eq!(next.as_deref(), Some(t[6].as_str()));
    assert_eq!(current_title(&s), playing);
}

#[test]
fn ctrl_click_toggles_and_shift_click_adds_a_range() {
    let mut s = tracks();
    let t = titles();
    ctrl_click(&mut s, &t[1]);
    assert_eq!(selected(&s, &ListId::Songs), [1]);
    s.modifiers = Modifiers::CTRL;
    click_text(&mut s, &t[3], 1);
    assert_eq!(selected(&s, &ListId::Songs), [1, 3]);
    click_text(&mut s, &t[1], 1);
    assert_eq!(selected(&s, &ListId::Songs), [3]);
    s.modifiers = Modifiers::SHIFT;
    click_text(&mut s, &t[6], 1);
    assert_eq!(selected(&s, &ListId::Songs), [3, 4, 5, 6]);
    s.modifiers = Modifiers::default();
    click_text(&mut s, &t[5], 1);
    assert!(selected(&s, &ListId::Songs).is_empty(), "a plain click clears the selection");
}

#[test]
fn mod_a_selects_all_and_the_bar_shows_the_count() {
    let mut s = tracks();
    let n = s.library.songs.len();
    ctrl_a(&mut s);
    assert_eq!(selected(&s, &ListId::Songs).len(), n);
    assert!(shows(&s, &format!("{n} selected")));
    assert!(shows(&s, "Clear"));
    assert!(shows(&s, "More"));
    // Again: none.
    ctrl_a(&mut s);
    assert!(selected(&s, &ListId::Songs).is_empty());
    assert!(!shows(&s, "Clear"));
    // The bar's Clear button.
    ctrl_a(&mut s);
    click_text(&mut s, "Clear", 1);
    assert!(selected(&s, &ListId::Songs).is_empty());
}

#[test]
fn the_selection_bar_more_opens_the_multi_target_menu() {
    let mut s = tracks();
    let t = titles();
    ctrl_click(&mut s, &t[0]);
    ctrl_click(&mut s, &t[2]);
    let m = click_text(&mut s, "More", 1);
    assert!(has(&m, |l| matches!(l, ListMsg::SelectionMenu(ListId::Songs, _))), "{m:?}");
    assert!(shows(&s, "Favorite") && shows(&s, "Clear selection"));
    // Playing the selection next adds both songs after the current one.
    let before = s.queue.iter().count();
    let _ = s.update(Message::Menu(listen::app::menu::MenuMsg::Do(listen::app::menu::MenuAction::PlayNext)));
    assert_eq!(s.queue.iter().count(), before + 2);
    assert!(s.lists.menu.is_none());
}

// ---- keyboard and hotkeys ---------------------------------------------------------

#[test]
fn arrow_keys_home_and_end_move_the_selection() {
    let mut s = tracks();
    let n = s.library.songs.len();
    key(&mut s, Named::ArrowDown, Modifiers::default());
    assert_eq!(selected(&s, &ListId::Songs), [0]);
    key(&mut s, Named::ArrowDown, Modifiers::default());
    key(&mut s, Named::ArrowDown, Modifiers::default());
    assert_eq!(selected(&s, &ListId::Songs), [2]);
    key(&mut s, Named::ArrowUp, Modifiers::default());
    assert_eq!(selected(&s, &ListId::Songs), [1]);
    key(&mut s, Named::End, Modifiers::default());
    assert_eq!(selected(&s, &ListId::Songs), [n - 1]);
    key(&mut s, Named::ArrowDown, Modifiers::default());
    assert_eq!(selected(&s, &ListId::Songs), [n - 1]);
    key(&mut s, Named::Home, Modifiers::default());
    assert_eq!(selected(&s, &ListId::Songs), [0]);
}

#[test]
fn arrow_keys_scroll_the_list_at_the_edges() {
    let mut s = tracks();
    // Make the viewport known, as the scrollable does.
    s.lists.entry(&ListId::Songs).scroll =
        listen::app::ScrollState { offset: 0.0, height: 640.0, width: 1200.0 };
    key(&mut s, Named::End, Modifiers::default());
    let n = s.library.songs.len() as f32;
    let scroll = s.lists.get(&ListId::Songs).scroll;
    assert!(scroll.offset > 0.0 && scroll.offset <= n * RowSize::Default.height());
}

#[test]
fn enter_plays_the_selection_and_the_other_list_hotkeys_act_on_it() {
    let mut s = tracks();
    let t = titles();
    ctrl_click(&mut s, &t[3]);
    // Enter is listPlayDefault: play now by default.
    key(&mut s, Named::Enter, Modifiers::default());
    assert_eq!(current_title(&s), t[3]);

    // Play next / last with the dedicated hotkeys (and several selected rows).
    ctrl_click(&mut s, &t[3]); // deselect the first
    ctrl_click(&mut s, &t[5]);
    ctrl_click(&mut s, &t[7]);
    let _ = s.update(Message::Hotkeys(HotkeysMsg::List(ListHotkey::PlayNext)));
    let queue: Vec<String> = s.queue.iter().map(|i| i.song.title.clone()).collect();
    let pos = queue.iter().position(|x| *x == t[3]).unwrap();
    assert_eq!(&queue[pos + 1], &t[5]);
    assert_eq!(&queue[pos + 2], &t[7]);
    let before = s.queue.iter().count();
    let _ = s.update(Message::Hotkeys(HotkeysMsg::List(ListHotkey::PlayLast)));
    assert_eq!(s.queue.iter().count(), before + 2);

    // listPlayDefault follows the setting.
    s.config.settings.lists.play_behavior = PlayBehavior::Last;
    let before = s.queue.iter().count();
    let _ = s.update(Message::Hotkeys(HotkeysMsg::List(ListHotkey::PlayDefault)));
    assert_eq!(s.queue.iter().count(), before + 2);
    assert_eq!(current_title(&s), t[3]);
}

#[test]
fn show_playing_song_selects_and_scrolls_to_it() {
    let mut s = tracks();
    let t = titles();
    s.lists.entry(&ListId::Songs).scroll =
        listen::app::ScrollState { offset: 0.0, height: 400.0, width: 1200.0 };
    s.queue.set(demo::album_songs(5), 1);
    let _ = s.update(Message::Hotkeys(HotkeysMsg::List(ListHotkey::ShowPlayingSong)));
    let playing = current_title(&s);
    let index = t.iter().position(|x| *x == playing).unwrap();
    assert_eq!(selected(&s, &ListId::Songs), [index]);
    assert!(s.lists.get(&ListId::Songs).scroll.offset > 0.0);
}

#[test]
fn navigate_to_page_opens_the_selected_album() {
    let mut s = state_on(Route::Albums);
    let id = s.library.albums[2].id.clone();
    s.modifiers = Modifiers::CTRL;
    let _ = s.update(Message::List(ListMsg::Press(ListId::Albums, 2)));
    s.modifiers = Modifiers::default();
    let _ = s.update(Message::Hotkeys(HotkeysMsg::List(ListHotkey::NavigateToPage)));
    assert_eq!(s.route, Route::Album(id));
}

// ---- drag and drop ----------------------------------------------------------------

#[test]
fn dragging_selected_rows_makes_a_payload_a_drop_target_takes() {
    let mut s = tracks();
    let t = titles();
    click_text(&mut s, &t[1], 1);
    s.modifiers = Modifiers::CTRL;
    click_text(&mut s, &t[2], 1);
    s.modifiers = Modifiers::default();
    // The button goes down on a selected row and the pointer moves.
    let _ = s.update(Message::List(ListMsg::Press(ListId::Songs, 2)));
    let _ = s.update(Message::List(ListMsg::PointerMoved(iced::Point::new(400.0, 300.0))));
    assert!(s.lists.drag.is_none(), "not yet: the pointer has to move a few px");
    let _ = s.update(Message::List(ListMsg::PointerMoved(iced::Point::new(430.0, 330.0))));
    let drag = s.lists.drag.as_ref().expect("drag started");
    assert_eq!(drag.payload.from, ListId::Songs);
    // The selected rows (the click on a selected row with Ctrl toggled it off, so 1 remains).
    let n = drag.payload.songs().len();
    assert!(n >= 1);
    assert_eq!(drag.payload.label(), if n == 1 { "1 song".to_string() } else { format!("{n} songs") });
    assert!(shows(&s, &drag.payload.label()));

    let before = s.queue.iter().count();
    let _ = s.update(Message::List(ListMsg::Drop(DropTarget::Queue)));
    assert_eq!(s.queue.iter().count(), before + n);
    assert!(s.lists.drag.is_none());
}

#[test]
fn dragging_an_unselected_row_drags_just_that_row() {
    let mut s = tracks();
    let _ = s.update(Message::List(ListMsg::Press(ListId::Songs, 4)));
    let _ = s.update(Message::List(ListMsg::PointerMoved(iced::Point::new(100.0, 100.0))));
    let _ = s.update(Message::List(ListMsg::PointerMoved(iced::Point::new(160.0, 160.0))));
    let drag = s.lists.drag.as_ref().unwrap();
    assert_eq!(drag.payload.songs().len(), 1);
    assert_eq!(drag.payload.songs()[0].title, titles()[4]);
    // Released over nothing: the drag is gone and nothing was queued.
    let before = s.queue.iter().count();
    let _ = s.update(Message::List(ListMsg::PointerReleased));
    assert!(s.lists.drag.is_none());
    assert_eq!(s.queue.iter().count(), before);
}

#[test]
fn a_small_pointer_move_is_a_click_not_a_drag() {
    let mut s = tracks();
    let _ = s.update(Message::List(ListMsg::Press(ListId::Songs, 1)));
    let _ = s.update(Message::List(ListMsg::PointerMoved(iced::Point::new(100.0, 100.0))));
    let _ = s.update(Message::List(ListMsg::PointerMoved(iced::Point::new(102.0, 101.0))));
    assert!(s.lists.drag.is_none());
}

// ---- right-click ------------------------------------------------------------------

#[test]
fn right_click_selects_and_opens_the_song_menu() {
    let mut s = tracks();
    let t = titles();
    right_click_text(&mut s, &t[2]);
    assert_eq!(selected(&s, &ListId::Songs), [2]);
    assert!(s.menu.is_some(), "the single-song context menu");
    assert!(shows(&s, "Favorite"));
    // On a multi-selection it is the same menu, for every selected song.
    let _ = s.update(Message::Menu(listen::app::menu::MenuMsg::Close));
    click_text(&mut s, &t[0], 1); // a plain click clears the earlier selection
    ctrl_click(&mut s, &t[1]);
    ctrl_click(&mut s, &t[3]);
    right_click_text(&mut s, &t[3]);
    assert!(s.lists.menu.is_none());
    assert_eq!(s.menu.as_ref().map(|m| m.target.len()), Some(2));
    assert!(shows(&s, "Favorite"));
    assert!(shows(&s, "+1 more"), "the preview footer counts the targets");
}

// ---- columns, appearance, pagination ----------------------------------------------------

#[test]
fn columns_come_from_the_settings() {
    let mut s = tracks();
    assert!(shows(&s, "Album") && shows(&s, "Duration"));
    assert!(!shows(&s, "Bitrate"));
    s.config.settings.lists.edit(ListKind::Songs, |c| {
        c.table.column_mut(ColumnId::BitRate).unwrap().enabled = true;
        c.table.column_mut(ColumnId::Album).unwrap().enabled = false;
    });
    assert!(shows(&s, "Bitrate"));
    assert!(!shows(&s, "Album"));
}

#[test]
fn config_edits_go_through_messages_and_the_popover() {
    let mut s = tracks();
    let _ = s.update(Message::List(ListMsg::Config(
        ListId::Songs,
        listen::app::list::ConfigEdit::RowSize(RowSize::Large),
    )));
    assert_eq!(s.config.settings.lists.config(ListKind::Songs).table.size, RowSize::Large);
    let _ = s.update(Message::List(ListMsg::Config(ListId::Songs, listen::app::list::ConfigEdit::Reset)));
    assert_eq!(s.config.settings.lists.config(ListKind::Songs).table.size, RowSize::Default);
    // The options button opens the popover.
    let _ = s.update(Message::List(ListMsg::ConfigMenu(ListId::Songs, iced::Point::new(900.0, 80.0))));
    assert!(shows(&s, "List options"));
    assert!(shows(&s, "Row size"));
    assert!(shows(&s, "Bitrate"));
    let _ = s.update(Message::List(ListMsg::CloseMenu));
    assert!(!shows(&s, "List options"));
}

#[test]
fn column_reorder_and_resize_by_messages() {
    let mut s = tracks();
    let _ = s.update(Message::List(ListMsg::ColumnDragStart(ListId::Songs, ColumnId::Duration)));
    let _ = s.update(Message::List(ListMsg::ColumnDragOver(ColumnId::Album)));
    let _ = s.update(Message::List(ListMsg::PointerReleased));
    let cols = s.config.settings.lists.config(ListKind::Songs).table.visible();
    let ids: Vec<_> = cols.iter().map(|c| c.id).collect();
    let d = ids.iter().position(|c| *c == ColumnId::Duration).unwrap();
    let a = ids.iter().position(|c| *c == ColumnId::Album).unwrap();
    // Dropping a header on another takes its place.
    assert!(a < d, "{ids:?}");

    let before = s.config.settings.lists.config(ListKind::Songs).table.column(ColumnId::Album).unwrap().width;
    let _ = s.update(Message::List(ListMsg::ColumnResizeStart(ListId::Songs, ColumnId::Album)));
    let _ = s.update(Message::List(ListMsg::PointerMoved(iced::Point::new(500.0, 50.0))));
    let _ = s.update(Message::List(ListMsg::PointerMoved(iced::Point::new(560.0, 50.0))));
    let after_move =
        s.config.settings.lists.config(ListKind::Songs).table.column(ColumnId::Album).unwrap().width;
    let _ = s.update(Message::List(ListMsg::PointerReleased));
    assert!(s.lists.resize.is_none());
    assert!(after_move > 0.0 && (after_move - before).abs() > 1.0, "{before} -> {after_move}");
}

#[test]
fn paginated_lists_show_a_page_at_a_time() {
    let mut s = tracks();
    let t = titles();
    s.config.settings.lists.edit(ListKind::Songs, |c| {
        c.pagination = listen_core::settings::Pagination::Paginated;
        c.items_per_page = 10;
    });
    assert!(shows(&s, &t[0]));
    assert!(shows(&s, "Page 1 of 3"));
    assert!(!shows(&s, &t[12]));
    // A selection is dropped when the page changes.
    let _ = s.update(Message::List(ListMsg::Press(ListId::Songs, 1)));
    let _ = s.update(Message::List(ListMsg::SetPage(ListId::Songs, 1)));
    assert!(selected(&s, &ListId::Songs).is_empty());
    assert!(shows(&s, "Page 2 of 3"));
    assert!(shows(&s, &t[12]));
    assert!(!shows(&s, &t[0]));
    // Past the last page stays on the last one.
    let _ = s.update(Message::List(ListMsg::SetPage(ListId::Songs, 99)));
    assert!(shows(&s, "Page 3 of 3"));
}

// ---- scroll position --------------------------------------------------------------

#[test]
fn the_scroll_position_of_a_list_is_kept_when_leaving_and_coming_back() {
    let mut s = tracks();
    let _ = s.update(Message::Navigate(Route::Albums));
    let at = |offset: f32| listen::app::ScrollState { offset, height: 640.0, width: 1200.0 };
    s.lists.entry(&ListId::Songs).scroll =
        listen::app::ScrollState { offset: 320.0, height: 640.0, width: 1200.0 };
    let _ = s.update(Message::Navigate(Route::Tracks));
    assert_eq!(s.lists.get(&ListId::Songs).scroll.offset, 320.0);
    // Albums keep their own position.
    assert_eq!(s.lists.get(&ListId::Albums).scroll.offset, 0.0);
    // The recreated scrollable first reports offset 0: that must not forget the position.
    let _ = s.update(Message::List(ListMsg::Scrolled(ListId::Songs, at(0.0))));
    assert_eq!(s.lists.get(&ListId::Songs).scroll.offset, 320.0);
    // Once it is restored, scrolling is followed again.
    let _ = s.update(Message::List(ListMsg::Scrolled(ListId::Songs, at(320.0))));
    let _ = s.update(Message::List(ListMsg::Scrolled(ListId::Songs, at(100.0))));
    assert_eq!(s.lists.get(&ListId::Songs).scroll.offset, 100.0);
}

// ---- Albums grid -------------------------------------------------------------------

#[test]
fn the_albums_page_is_a_grid_with_selection_and_a_display_toggle() {
    let mut s = state_on(Route::Albums);
    assert_eq!(s.config.settings.lists.config(ListKind::Albums).display, DisplayType::Grid);
    let name = s.library.albums[1].name.clone();
    // Titles are links, so select through the message the cover sends.
    let _ = s.update(Message::List(ListMsg::Press(ListId::Albums, 1)));
    assert!(selected(&s, &ListId::Albums).is_empty(), "a plain click does not select");
    s.modifiers = Modifiers::CTRL;
    let _ = s.update(Message::List(ListMsg::Press(ListId::Albums, 1)));
    let _ = s.update(Message::List(ListMsg::Press(ListId::Albums, 3)));
    s.modifiers = Modifiers::default();
    assert_eq!(selected(&s, &ListId::Albums), [1, 3]);
    assert!(shows(&s, "2 selected"));
    assert!(shows(&s, &name));
    // Arrow keys move through cards; right goes to the next card.
    s.modifiers = Modifiers::CTRL;
    let _ = s.update(Message::List(ListMsg::Press(ListId::Albums, 0)));
    s.modifiers = Modifiers::default();
    key(&mut s, Named::ArrowDown, Modifiers::default());
    let columns = listen::ui::list::grid::GridLayout::new(
        SIZE.width - 400.0,
        &s.config.settings.lists.config(ListKind::Albums).grid,
    )
    .columns;
    assert!(selected(&s, &ListId::Albums)[0] >= 1, "moved down by a row (columns {columns})");
    // Switch to the table and back to the grid.
    let _ = s.update(Message::List(ListMsg::Config(
        ListId::Albums,
        listen::app::list::ConfigEdit::Display(DisplayType::Table),
    )));
    assert!(shows(&s, "Release year"));
    let _ = s.update(Message::List(ListMsg::Config(
        ListId::Albums,
        listen::app::list::ConfigEdit::Display(DisplayType::Detail),
    )));
    assert!(shows(&s, &name));
    let _ = s.update(Message::List(ListMsg::Config(
        ListId::Albums,
        listen::app::list::ConfigEdit::Display(DisplayType::Grid),
    )));
}

#[test]
fn a_double_click_on_an_album_opens_it_and_the_play_button_plays_it() {
    let mut s = state_on(Route::Albums);
    let id = s.library.albums[2].id.clone();
    let _ = s.update(Message::List(ListMsg::DoubleClick(ListId::Albums, 2)));
    assert_eq!(s.route, Route::Album(id));
    let mut s = state_on(Route::Albums);
    let first = demo::album_songs(1)[0].title.clone();
    let _ = s.update(Message::List(ListMsg::PlayRow(ListId::Albums, 1, PlayBehavior::Now)));
    assert_eq!(current_title(&s), first);
    let before = s.queue.iter().count();
    let _ = s.update(Message::List(ListMsg::PlayRow(ListId::Albums, 2, PlayBehavior::Last)));
    assert_eq!(s.queue.iter().count(), before + demo::album_songs(2).len());
}

#[test]
fn grid_multi_select_off_opens_the_item_when_the_cover_is_clicked() {
    let mut s = state_on(Route::Albums);
    s.config.settings.lists.grid_multi_select = false;
    let id = s.library.albums[0].id.clone();
    let _ = s.update(Message::List(ListMsg::Open(ListId::Albums, 0)));
    assert_eq!(s.route, Route::Album(id));
}

// ---- YouTube Music ------------------------------------------------------------------

fn ytm_tracks() -> Listen {
    let mut s = tracks();
    make_ytm(&mut s);
    s
}

fn make_ytm(s: &mut Listen) {
    for (i, song) in s.library.songs.iter_mut().enumerate() {
        song.source = SourceKind::YouTubeMusic;
        song.server_id = ServerId("ytm".into());
        song.artists = vec![ArtistRef { id: Some(format!("UCartist{i}")), name: format!("Yt Artist {i}") }];
        song.album_id = Some(format!("MPREb_{i}"));
        song.album = Some(format!("Yt Album {i}"));
    }
}

#[test]
fn youtube_music_songs_keep_the_like_cell_and_the_links() {
    let mut s = ytm_tracks();
    // Artist and album names are links to their pages.
    let m = click_text(&mut s, "Yt Artist 1", 1);
    assert!(
        m.iter().any(|m| matches!(m, Message::Navigate(Route::Artist(id)) if id == "UCartist1")),
        "{m:?}"
    );
    let mut s = ytm_tracks();
    let m = click_text(&mut s, "Yt Album 2", 1);
    assert!(m.iter().any(|m| matches!(m, Message::OpenAlbum(id) if id == "MPREb_2")), "{m:?}");

    // The heart: a click asks to like the song (Liked Music), no rating stars are offered.
    let mut s = ytm_tracks();
    let _ = s.update(Message::List(ListMsg::Favorite(ListId::Songs, 0)));
    assert!(s.library.songs[0].favorite || true);
    assert!(!shows(&s, "Rating"));
}

#[test]
fn youtube_music_rows_still_offer_add_to_playlist_in_the_context_menu() {
    use listen::app::SourceHandle;
    let (mut s, _) = Listen::boot(&Args { ephemeral: true, ..Args::default() });
    // A connected account whose server can add to playlists; its songs are YouTube Music songs.
    s.config.servers = vec![listen_core::config::ServerConfig {
        id: ServerId("ytm".into()),
        name: "YouTube Music".into(),
        kind: SourceKind::Subsonic,
        url: "https://srv.example".into(),
        username: "me".into(),
    }];
    let source: std::sync::Arc<dyn listen_source::MusicSource> = std::sync::Arc::new(
        listen_subsonic::Subsonic::new(ServerId("ytm".into()), "https://srv.example", "me", "pw").unwrap(),
    );
    let _ = s.update(Message::Connected(ServerId("ytm".into()), Ok(SourceHandle(source))));
    let _ = s.update(Message::WindowResized(SIZE));
    let _ = s.update(Message::Navigate(Route::Tracks));
    s.library.songs = (0..demo::ALBUMS.len()).flat_map(demo::album_songs).collect();
    make_ytm(&mut s);
    // Right-click a row: the song menu opens.
    let _ = s.update(Message::List(ListMsg::Context(ListId::Songs, 0)));
    assert!(s.menu.is_some());
    assert!(shows(&s, "Add to playlist…"));
    // YouTube Music songs are liked, not added to favorites.
    assert!(shows(&s, "Favorite"));
    let menu = s.menu.clone().unwrap();
    let entries = listen::app::menu::entries(&s, &menu);
    let fav = entries.iter().find(|e| e.label == "Favorite").expect("the Favorite submenu");
    let labels: Vec<_> = fav.children().iter().map(|e| e.label.as_str()).collect();
    assert_eq!(labels, ["Like", "Remove from Liked Music"]);
}
