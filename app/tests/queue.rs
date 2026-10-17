//! The play queue surfaces (side panel, drawer, Now Playing page, toolbar, drag and drop, selection,
//! Up Next headers, confirmations, player filters, layout) and Auto-DJ / manual queue behaviour.

use iced::keyboard::key::{Code, Named, Physical};
use iced::keyboard::{Event as KeyEvent, Key, Location, Modifiers};
use iced_test::simulator::Simulator;
use listen::app::list::{Drag, DragPayload, DropTarget, ListId, ListMsg, Picked};
use listen::app::menu::{MenuAction, MenuMsg};
use listen::app::overlay::Modal;
use listen::app::queue_ui::{QueueMsg, QueueSetting};
use listen::app::{INTER, Listen, Message, Route};
use listen::ui::queue::{Panel, panels};
use listen::{Args, FONTS, demo};
use listen_core::settings::{FilterField, FilterOp, PlayerFilter, SideQueueLayout, SideQueueStyle};

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

/// Demo state: the first album queued (six songs, the third playing), the queue panel open.
fn boot() -> Listen {
    let (mut s, _) = Listen::boot(&Args {
        demo: true,
        show_queue: true,
        theme: Some("tokyoNight".into()),
        ..Args::default()
    });
    let _ = s.update(Message::WindowResized(SIZE));
    s
}

fn titles(s: &Listen) -> Vec<String> {
    s.queue.iter().map(|i| i.song.title.clone()).collect()
}

fn playing(s: &Listen) -> String {
    s.queue.current().map(|i| i.song.title.clone()).unwrap_or_default()
}

fn shows(s: &Listen, text: &str) -> bool {
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings(), SIZE, s.view());
    ui.find(text).is_ok()
}

fn click_text(s: &mut Listen, text: &str, times: usize) {
    let messages: Vec<Message> = {
        let _gpu = gpu();
        let mut ui = Simulator::with_size(settings(), SIZE, s.view());
        for _ in 0..times {
            ui.click(text).unwrap_or_else(|e| panic!("no {text:?} on screen: {e:?}"));
        }
        ui.into_messages().collect()
    };
    for m in messages {
        let _ = s.update(m);
    }
}

fn key(s: &mut Listen, named: Named, code: Code) {
    let _ = s.update(Message::Keyboard(KeyEvent::KeyPressed {
        key: Key::Named(named),
        modified_key: Key::Named(named),
        physical_key: Physical::Code(code),
        location: Location::Standard,
        modifiers: Modifiers::empty(),
        text: None,
        repeat: false,
    }));
}

fn songs(album: usize) -> Vec<listen_core::model::Song> {
    demo::album_songs(album)
}

fn queue_songs(s: &mut Listen, action: MenuAction, songs: Vec<listen_core::model::Song>) {
    let _ = s.update(Message::Menu(MenuMsg::Songs(action, Ok(songs))));
}

fn drag_from(s: &mut Listen, from: ListId, songs: Vec<listen_core::model::Song>) {
    s.lists.drag = Some(Drag {
        payload: DragPayload { from, items: songs.into_iter().map(Picked::Song).collect() },
        at: iced::Point::ORIGIN,
    });
}

// ---- the Now Playing page ---------------------------------------------------------

#[test]
fn the_now_playing_page_shows_the_queue_and_the_panel_steps_aside() {
    let mut s = boot();
    assert!(s.queue_attached(), "the panel is open on the home page");
    let _ = s.update(Message::Navigate(Route::NowPlaying));
    assert!(!s.queue_attached(), "the side queue hides on the Now Playing route");
    assert_eq!(s.active_list(), Some(ListId::Queue));
    assert!(shows(&s, "Now Playing"));
    assert!(shows(&s, "Midnight Signal") && shows(&s, "Last Exit"));
    // The page keeps the whole width.
    let wide = s.page_width();
    let _ = s.update(Message::Navigate(Route::Home));
    assert!(s.queue_attached() && s.page_width() < wide);
}

#[test]
fn the_page_and_the_panel_keep_separate_looks() {
    let s = boot();
    let page = s.config.settings.lists.config(ListId::Queue.kind());
    let panel = s.config.settings.lists.config(ListId::SideQueue.kind());
    assert_ne!(
        page.table.columns.iter().filter(|c| c.enabled).count(),
        panel.table.columns.iter().filter(|c| c.enabled).count()
    );
    assert!(!panel.table.header, "the narrow panel has no header row");
}

// ---- rows, headers, tags ------------------------------------------------------------

#[test]
fn up_next_songs_get_headers_and_without_them_a_tag() {
    let mut s = boot();
    assert!(!shows(&s, "Up Next"), "nothing queued yet");
    queue_songs(&mut s, MenuAction::PlayNext, songs(1)[..2].to_vec());
    assert_eq!(s.queue.up_next_range(), 3..5);
    assert!(shows(&s, "Up Next"), "the labelled block");
    assert!(!shows(&s, "Queued"));
    // Setting off: the old tag on the waiting songs instead of the headers.
    let _ = s.update(Message::Queue(QueueMsg::Setting(QueueSetting::UpNext(false))));
    assert!(!shows(&s, "Up Next"));
    assert!(shows(&s, "Queued"));
}

#[test]
fn auto_dj_songs_keep_their_tag_and_stay_behind_the_manual_queue() {
    let mut s = boot();
    let _ = s.update(Message::AutoDjLoaded(Ok(songs(2))));
    queue_songs(&mut s, MenuAction::AddToQueue, songs(1)[..1].to_vec());
    let order = titles(&s);
    let manual = order.iter().position(|t| t == "Paper Moons").unwrap();
    let first_auto = s.queue.iter().position(|i| i.auto).unwrap();
    assert!(manual < first_auto, "queued songs play before the Auto-DJ songs: {order:?}");
    assert!(shows(&s, "Auto"));
    assert!(!shows(&s, "Auto-DJ"));
    // Rows dragged around keep the flag.
    let uid = s.queue.iter().find(|i| i.auto).unwrap().uid;
    s.queue.move_uids(&[uid], 0);
    assert!(s.queue.iter().find(|i| i.uid == uid).unwrap().auto);
}

#[test]
fn the_mode_tag_can_be_hidden_in_the_sidebar() {
    let mut s = boot();
    let _ = s.update(Message::AutoDjLoaded(Ok(songs(2))));
    assert!(shows(&s, "Auto"), "shown by default");
    let _ = s.update(Message::Queue(QueueMsg::Setting(QueueSetting::ModeTag(false))));
    assert!(!s.config.settings.queue.show_mode_tag);
    assert!(!shows(&s, "Auto"), "no mode row and no tags in the panel");
}

#[test]
fn a_double_click_jumps_to_the_song_and_does_not_replace_the_queue() {
    let mut s = boot();
    assert_eq!(playing(&s), "Afterglow Avenue");
    let before = titles(&s);
    click_text(&mut s, "Static Hearts", 2);
    assert_eq!(playing(&s), "Static Hearts");
    assert_eq!(titles(&s), before, "the same songs in the same order");
}

// ---- toolbar ----------------------------------------------------------------------------

#[test]
fn the_toolbar_shuffles_clears_and_searches() {
    let mut s = boot();
    let _ = s.update(Message::Queue(QueueMsg::Shuffle));
    assert_eq!(s.queue.len(), 6);
    assert_eq!(playing(&s), "Afterglow Avenue", "the playing song stays");

    let _ = s.update(Message::Queue(QueueMsg::Search("coastl".into())));
    assert_eq!(s.queue_row_count(), 1);
    assert!(shows(&s, "Coastline") && !shows(&s, "Last Exit"));
    let _ = s.update(Message::Queue(QueueMsg::Search(String::new())));
    assert_eq!(s.queue_row_count(), 6);

    let _ = s.update(Message::Queue(QueueMsg::Clear));
    assert!(s.queue.is_empty());
    assert!(!s.playback.playing);
    assert!(shows(&s, "The queue is empty"));
}

#[test]
fn create_playlist_from_the_queue_opens_the_dialog_with_every_song() {
    let mut s = boot();
    let _ = s.update(Message::Queue(QueueMsg::CreatePlaylist));
    let form = s.playlist_page.form.as_ref().expect("the create dialog");
    match &form.kind {
        listen::app::playlists::FormKind::Create { songs, .. } => assert_eq!(songs.len(), 6),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_toolbar_folds_into_a_menu_when_narrow() {
    use listen::ui::queue::overflow_tier;
    assert_eq!([600.0, 450.0, 350.0, 250.0].map(overflow_tier), [0, 1, 2, 3]);
}

// ---- selection and removal -----------------------------------------------------------------

#[test]
fn delete_removes_the_selected_rows_but_not_the_playing_song() {
    let mut s = boot();
    s.lists.entry(&ListId::SideQueue).selection.set_many([0, 1, 2]);
    key(&mut s, Named::Delete, Code::Delete);
    assert_eq!(titles(&s), ["Afterglow Avenue", "Static Hearts", "Rearview", "Last Exit"]);
    assert_eq!(playing(&s), "Afterglow Avenue");
    assert!(s.lists.get(&ListId::SideQueue).selection.is_empty());
    // Backspace does the same.
    s.lists.entry(&ListId::SideQueue).selection.set_many([1]);
    key(&mut s, Named::Backspace, Code::Backspace);
    assert_eq!(titles(&s).len(), 3);
}

#[test]
fn the_selection_follows_its_songs_when_the_queue_changes() {
    let mut s = boot();
    s.lists.entry(&ListId::SideQueue).selection.set_many([4, 5]);
    // Two songs play next: Rearview and Last Exit move down two rows, still selected.
    queue_songs(&mut s, MenuAction::PlayNext, songs(1)[..2].to_vec());
    assert_eq!(s.lists.get(&ListId::SideQueue).selection.indices(), [6, 7]);
    // A search shows other rows: the selection is dropped, not left on the wrong songs.
    let _ = s.update(Message::Queue(QueueMsg::Search("rearview".into())));
    assert!(s.lists.get(&ListId::SideQueue).selection.is_empty());
}

// ---- drag and drop ------------------------------------------------------------------------------

fn pointer(s: &mut Listen, x: f32) {
    let _ = s.update(Message::List(ListMsg::PointerMoved(iced::Point::new(x, 100.0))));
}

#[test]
fn dragging_a_row_to_another_reorders_the_queue() {
    let mut s = boot();
    let _ = s.update(Message::List(ListMsg::Press(ListId::SideQueue, 4)));
    pointer(&mut s, 100.0);
    pointer(&mut s, 140.0);
    assert!(s.lists.drag.is_some(), "the drag started");
    let _ = s.update(Message::List(ListMsg::Hover(ListId::SideQueue, 1)));
    let _ = s.update(Message::List(ListMsg::Drop(DropTarget::QueueAt(1))));
    assert!(s.lists.drag.is_none());
    assert_eq!(
        titles(&s),
        ["Midnight Signal", "Rearview", "Coastline", "Afterglow Avenue", "Static Hearts", "Last Exit"]
    );
    assert_eq!(playing(&s), "Afterglow Avenue", "playback follows its song");
    assert_eq!(s.lists.get(&ListId::SideQueue).selection.indices(), [1], "the moved row stays selected");
}

#[test]
fn several_selected_rows_drag_together_and_the_end_is_a_drop_zone() {
    let mut s = boot();
    let _ = s.update(Message::Keyboard(KeyEvent::ModifiersChanged(Modifiers::CTRL)));
    let _ = s.update(Message::List(ListMsg::Press(ListId::SideQueue, 0)));
    let _ = s.update(Message::List(ListMsg::Press(ListId::SideQueue, 1)));
    pointer(&mut s, 100.0);
    pointer(&mut s, 140.0);
    let len = s.queue_row_count();
    let _ = s.update(Message::List(ListMsg::Drop(DropTarget::QueueAt(len))));
    assert_eq!(
        titles(&s),
        ["Afterglow Avenue", "Static Hearts", "Rearview", "Last Exit", "Midnight Signal", "Coastline"]
    );
    assert_eq!(s.lists.get(&ListId::SideQueue).selection.indices(), [4, 5]);
}

#[test]
fn rows_dropped_into_up_next_join_it() {
    let mut s = boot();
    queue_songs(&mut s, MenuAction::PlayNext, songs(1)[..2].to_vec());
    assert_eq!(s.queue.up_next_range(), 3..5);
    let _ = s.update(Message::List(ListMsg::Press(ListId::SideQueue, 7)));
    pointer(&mut s, 100.0);
    pointer(&mut s, 140.0);
    let _ = s.update(Message::List(ListMsg::Drop(DropTarget::QueueAt(4))));
    assert_eq!(s.queue.up_next_range(), 3..6, "it is part of the block now");
    assert_eq!(s.queue.manual_len(), 3);
}

#[test]
fn songs_from_other_lists_drop_between_rows() {
    let mut s = boot();
    drag_from(&mut s, ListId::Songs, songs(1)[..2].to_vec());
    let _ = s.update(Message::List(ListMsg::Drop(DropTarget::QueueAt(1))));
    assert_eq!(
        titles(&s),
        [
            "Midnight Signal",
            "Paper Moons",
            "Lanterns",
            "Coastline",
            "Afterglow Avenue",
            "Static Hearts",
            "Rearview",
            "Last Exit"
        ]
    );
    assert_eq!(playing(&s), "Afterglow Avenue");
    // At the end.
    drag_from(&mut s, ListId::Songs, songs(1)[2..3].to_vec());
    let len = s.queue_row_count();
    let _ = s.update(Message::List(ListMsg::Drop(DropTarget::QueueAt(len))));
    assert_eq!(titles(&s).last().unwrap(), "Soft Rain");
}

#[test]
fn an_empty_queue_is_a_drop_zone_that_plays() {
    let mut s = boot();
    let _ = s.update(Message::Queue(QueueMsg::Clear));
    drag_from(&mut s, ListId::Songs, songs(1)[..2].to_vec());
    let _ = s.update(Message::List(ListMsg::Drop(DropTarget::QueueEmpty)));
    assert_eq!(titles(&s), ["Paper Moons", "Lanterns"]);
    assert_eq!(playing(&s), "Paper Moons");
}

// ---- follow current song ----------------------------------------------------------------------------

#[test]
fn follow_current_song_is_a_setting_that_defaults_on() {
    let mut s = boot();
    assert!(s.config.settings.queue.follow_current_song && s.config.settings.queue.up_next_queue);
    let _ = s.update(Message::Queue(QueueMsg::Setting(QueueSetting::FollowCurrent(false))));
    assert!(!s.config.settings.queue.follow_current_song);
}

// ---- confirmations and player filters ------------------------------------------------------------------

#[test]
fn confirm_queue_changes_asks_before_the_queue_is_replaced() {
    let mut s = boot();
    // Off: the queue is replaced right away.
    queue_songs(&mut s, MenuAction::Play, songs(1));
    assert_eq!(playing(&s), "Paper Moons");

    let _ = s.update(Message::Queue(QueueMsg::Setting(QueueSetting::Confirm(true))));
    queue_songs(&mut s, MenuAction::Play, songs(2));
    assert_eq!(playing(&s), "Paper Moons", "waiting for the answer");
    let Some(Modal::Confirm(confirm)) = s.overlay.modal.clone() else { panic!("no confirmation") };
    let _ = s.update(*confirm.on_confirm);
    assert_ne!(playing(&s), "Paper Moons");

    // Adding to the queue never asks; the Auto-DJ neither.
    s.overlay.modal = None;
    queue_songs(&mut s, MenuAction::AddToQueue, songs(1)[..1].to_vec());
    assert!(s.overlay.modal.is_none());
    let _ = s.update(Message::AutoDjLoaded(Ok(songs(0))));
    assert!(s.overlay.modal.is_none());
}

#[test]
fn clearing_the_queue_asks_when_confirming_is_on() {
    let mut s = boot();
    let _ = s.update(Message::Queue(QueueMsg::Setting(QueueSetting::Confirm(true))));
    let _ = s.update(Message::Queue(QueueMsg::Clear));
    assert_eq!(s.queue.len(), 6);
    let Some(Modal::Confirm(confirm)) = s.overlay.modal.clone() else { panic!("no confirmation") };
    let _ = s.update(*confirm.on_confirm);
    assert!(s.queue.is_empty());
}

#[test]
fn player_filters_keep_songs_out_of_what_the_user_queues_but_not_out_of_the_auto_dj() {
    let mut s = boot();
    s.config.settings.queue.filters.push(PlayerFilter {
        field: FilterField::Name,
        op: FilterOp::Contains,
        value: "lantern".into(),
        enabled: true,
    });
    queue_songs(&mut s, MenuAction::AddToQueue, songs(1));
    assert!(!titles(&s).contains(&"Lanterns".to_string()));
    assert!(titles(&s).contains(&"Paper Moons".to_string()));
    // Dropped rows pass the filters too.
    drag_from(&mut s, ListId::Songs, songs(1));
    let _ = s.update(Message::List(ListMsg::Drop(DropTarget::Queue)));
    assert!(!titles(&s).contains(&"Lanterns".to_string()));
    // The Auto-DJ adds what it likes.
    let _ = s.update(Message::AutoDjLoaded(Ok(songs(1))));
    assert!(s.queue.iter().any(|i| i.auto && i.song.title == "Lanterns"));
}

#[test]
fn every_song_filtered_leaves_the_queue_alone() {
    let mut s = boot();
    s.config.settings.queue.filters.push(PlayerFilter {
        field: FilterField::Duration,
        op: FilterOp::Gt,
        value: "0".into(),
        enabled: true,
    });
    let before = titles(&s);
    queue_songs(&mut s, MenuAction::Play, songs(1));
    assert_eq!(titles(&s), before);
}

// ---- panels, drawer and layout --------------------------------------------------------------------------

#[test]
fn the_panel_order_decides_what_the_right_sidebar_shows() {
    let mut s = boot();
    assert_eq!(panels(&s), [Panel::Queue, Panel::Visualizer]);
    let _ = s.update(Message::Queue(QueueMsg::Setting(QueueSetting::LyricsInSidebar(true))));
    assert_eq!(panels(&s), [Panel::Queue, Panel::Lyrics, Panel::Visualizer]);
    let _ = s.update(Message::Queue(QueueMsg::Setting(QueueSetting::MovePanel("lyrics".into(), -1))));
    assert_eq!(panels(&s), [Panel::Lyrics, Panel::Queue, Panel::Visualizer]);
    let _ = s.update(Message::Queue(QueueMsg::Setting(QueueSetting::Combined(true))));
    assert_eq!(
        panels(&s),
        [Panel::Combined, Panel::Queue],
        "lyrics and visualizer share the panel of the first"
    );
    let _ = s.update(Message::Queue(QueueMsg::Setting(QueueSetting::QueuePanel(false))));
    assert_eq!(panels(&s), [Panel::Combined]);
    // The view builds with every combination.
    assert!(shows(&s, "No lyrics found for this song."));
}

#[test]
fn a_damaged_panel_order_is_repaired() {
    let mut s = boot();
    s.config.settings.sidebar.panel_order = vec!["visualizer".into(), "visualizer".into(), "bogus".into()];
    assert_eq!(panels(&s), [Panel::Visualizer, Panel::Queue]);
}

#[test]
fn the_drawer_slides_in_after_the_pointer_rests_on_the_edge() {
    let mut s = boot();
    s.show_queue = false;
    let _ = s.update(Message::Queue(QueueMsg::Setting(QueueSetting::Style(SideQueueStyle::Detached))));
    assert!(s.queue_edge_visible() && !s.queue_drawer_visible() && !s.queue_attached());

    let _ = s.update(Message::Queue(QueueMsg::EdgeEnter));
    let generation = s.queue_ui.drawer_gen;
    // The pointer left again before the delay ran out: nothing opens.
    let _ = s.update(Message::Queue(QueueMsg::EdgeExit));
    let _ = s.update(Message::Queue(QueueMsg::DrawerOpen(generation)));
    assert!(!s.queue_drawer_visible());

    let _ = s.update(Message::Queue(QueueMsg::EdgeEnter));
    let generation = s.queue_ui.drawer_gen;
    let _ = s.update(Message::Queue(QueueMsg::DrawerOpen(generation)));
    assert!(s.queue_drawer_visible() && !s.queue_edge_visible());
    assert!(shows(&s, "Midnight Signal"), "the drawer shows the queue");

    // Leaving closes it ...
    let _ = s.update(Message::Queue(QueueMsg::DrawerExit));
    assert!(!s.queue_drawer_visible());
    // ... unless rows are being dragged.
    let _ = s.update(Message::Queue(QueueMsg::EdgeEnter));
    let generation = s.queue_ui.drawer_gen;
    let _ = s.update(Message::Queue(QueueMsg::DrawerOpen(generation)));
    drag_from(&mut s, ListId::SideQueue, songs(0)[..1].to_vec());
    let _ = s.update(Message::Queue(QueueMsg::DrawerExit));
    assert!(s.queue_drawer_visible(), "stays open during a drag");
    s.lists.drag = None;
    let _ = s.update(Message::Noop);
    assert!(!s.queue_drawer_visible(), "closes once the drag is over");

    // Never on the Now Playing page.
    let _ = s.update(Message::Navigate(Route::NowPlaying));
    assert!(!s.queue_edge_visible() && !s.queue_drawer_visible());
}

#[test]
fn the_queue_button_pins_the_drawer_open() {
    let mut s = boot();
    let _ = s.update(Message::Queue(QueueMsg::Setting(QueueSetting::Style(SideQueueStyle::Detached))));
    assert!(s.show_queue && s.queue_drawer_visible());
    assert!(s.page_width() > 1000.0, "the drawer floats over the page");
    let _ = s.update(Message::ToggleQueue);
    assert!(!s.queue_drawer_visible());
}

#[test]
fn the_vertical_layout_puts_the_queue_under_the_page() {
    let mut s = boot();
    let wide = s.page_width();
    let _ = s.update(Message::Queue(QueueMsg::Setting(QueueSetting::Layout(SideQueueLayout::Vertical))));
    assert!(s.page_width() > wide, "the page gets the width back");
    assert!(shows(&s, "Midnight Signal"));
    // The divider drag changes the height.
    use listen::app::queue_ui::ResizeAxis;
    let h = s.config.settings.queue.height;
    let _ = s.update(Message::Queue(QueueMsg::ResizeStart(ResizeAxis::QueueHeight)));
    let _ = s.update(Message::Queue(QueueMsg::ResizeMove(iced::Point::new(0.0, 500.0))));
    let _ = s.update(Message::Queue(QueueMsg::ResizeMove(iced::Point::new(0.0, 450.0))));
    let _ = s.update(Message::Queue(QueueMsg::ResizeEnd));
    assert_eq!(s.config.settings.queue.height, h + 50.0);
}

// ---- the server's play queue --------------------------------------------------------------------------------

#[test]
fn saving_and_restoring_the_queue_on_a_server() {
    use listen_core::config::ServerConfig;
    use listen_core::model::{ServerId, SourceKind};
    use std::sync::Arc;
    let (mut s, _) = Listen::boot(&Args { ephemeral: true, ..Args::default() });
    assert!(!s.queue_server_sync(), "no server yet");
    s.config.servers = vec![ServerConfig {
        id: ServerId("nd".into()),
        name: "Navidrome".into(),
        kind: SourceKind::Subsonic,
        url: "https://nd.example".into(),
        username: "me".into(),
    }];
    let source: Arc<dyn listen_source::MusicSource> = Arc::new(
        listen_subsonic::Subsonic::new(ServerId("nd".into()), "https://nd.example", "me", "pw").unwrap(),
    );
    let _ = s.update(Message::Connected(ServerId("nd".into()), Ok(listen::app::SourceHandle(source))));
    assert!(s.queue_server_sync(), "Navidrome keeps a play queue");

    // What is saved: this server's songs in play order, the playing one and the position.
    let mut mine = songs(0);
    for song in &mut mine {
        song.server_id = ServerId("nd".into());
    }
    let mut foreign = songs(1);
    for song in &mut foreign {
        song.server_id = ServerId("other".into());
    }
    s.queue.set(mine.clone(), 2);
    s.queue.add_last(foreign, false);
    let q = s.queue_for_server(&ServerId("nd".into()));
    assert_eq!(q.songs.len(), 6, "other servers' songs are not sent");
    assert_eq!(q.current.as_deref(), Some(mine[2].id.as_str()));

    // Restoring replaces the queue and waits at the saved position.
    let saved = listen_source::PlayQueue {
        songs: mine[1..5].to_vec(),
        current: Some(mine[3].id.clone()),
        position: std::time::Duration::from_secs(42),
        changed_by: Some("Feishin".into()),
    };
    let _ = s.update(Message::Queue(QueueMsg::Restored(Ok(Some(saved)))));
    assert_eq!(s.queue.len(), 4);
    assert_eq!(playing(&s), mine[3].title);
    assert_eq!(s.playback.position, std::time::Duration::from_secs(42));
    assert!(!s.playback.playing, "restoring does not start playback");

    // Nothing saved on the server.
    let _ = s.update(Message::Queue(QueueMsg::Restored(Ok(None))));
    assert_eq!(s.queue.len(), 4);
}

#[test]
fn the_server_buttons_are_hidden_without_a_server_queue() {
    // Demo mode has no server: the same toolbar still builds.
    let s = boot();
    assert!(!s.queue_server_sync());
    assert!(shows(&s, "Search"));
}

// ---- YouTube Music songs in the queue ----------------------------------------------------------------

#[test]
fn youtube_music_rows_keep_their_menu_hearts_and_dj() {
    use listen::app::menu::MenuTarget;
    use listen_core::model::SourceKind;
    let mut s = boot();
    let mut yt = songs(0);
    for song in &mut yt {
        song.source = SourceKind::YouTubeMusic;
        song.server_id = listen_core::model::ServerId("yt".into());
    }
    s.queue.set(yt, 1);
    s.lists.entry(&ListId::SideQueue).selection.set_many([1, 3]);
    // The row menu knows the rows' uids (Remove, Move to top, Shuffle selected) and the songs
    // (Like, Add to playlist, radio).
    let Some(MenuTarget::Songs { songs, queue_uids, .. }) = s.selection_target(&ListId::SideQueue) else {
        panic!("a songs target")
    };
    assert_eq!(songs.len(), 2);
    assert!(songs.iter().all(|x| x.source == SourceKind::YouTubeMusic));
    assert_eq!(queue_uids, [s.queue.item_at(1).unwrap().uid, s.queue.item_at(3).unwrap().uid]);
    // Hearts and the DJ toggle are still on the rows / in the header.
    // The play-mode toggle is in the header (it names the current mode).
    assert!(shows(&s, s.play_mode().tag()));
    assert!(shows(&s, "Coastline"));
    // YouTube Music has no server queue: no save / restore.
    assert!(!s.queue_server_sync());
}
