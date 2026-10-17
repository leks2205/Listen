//! The smart-playlist editor: capability gating, the rule builder (add / remove rules,
//! nested groups, field types), loading an existing playlist and saving.

use async_trait::async_trait;
use iced::Task;
use iced_test::simulator::Simulator;
use listen::app::smart_playlist::{Editor, GroupEdit, MODAL_KEY, RuleEdit, SmartMsg, SortEdit};
use listen::app::{INTER, Listen, Message, Route, SourceHandle};
use listen::{Args, FONTS};
use listen_core::config::ServerConfig;
use listen_core::model::{
    Album, Artist, ImageRef, Page, Playlist, SearchResults, ServerId, Song, SourceKind,
};
use listen_source::nsp::fields::operators;
use listen_source::nsp::query::{Input, field_options};
use listen_source::nsp::{GroupKind, Nsp};
use listen_source::{
    AlbumQuery, Capabilities, ListQuery, MusicSource, Result, SmartPlaylistDef, SongQuery, SourceError,
    StreamSource,
};
use std::sync::{Arc, Mutex};
use std::time::Duration;

static GPU: Mutex<()> = Mutex::new(());

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

#[derive(Default, Debug)]
struct Calls {
    created: Vec<SmartPlaylistDef>,
    updated: Vec<(String, SmartPlaylistDef)>,
}

/// A Navidrome-like source: smart playlists work when `caps` says so.
struct Fake(Capabilities, Arc<Mutex<Calls>>, Option<SmartPlaylistDef>);

#[async_trait]
impl MusicSource for Fake {
    fn kind(&self) -> SourceKind {
        SourceKind::Subsonic
    }
    fn capabilities(&self) -> Capabilities {
        self.0
    }
    async fn ping(&self) -> Result<()> {
        Ok(())
    }
    async fn albums(&self, _: &AlbumQuery) -> Result<Page<Album>> {
        Err(SourceError::Unsupported)
    }
    async fn album(&self, _: &str) -> Result<Album> {
        Err(SourceError::Unsupported)
    }
    async fn artists(&self, _: &ListQuery) -> Result<Page<Artist>> {
        Err(SourceError::Unsupported)
    }
    async fn artist(&self, _: &str) -> Result<Artist> {
        Err(SourceError::Unsupported)
    }
    async fn artist_albums(&self, _: &str) -> Result<Vec<Album>> {
        Ok(vec![])
    }
    async fn top_songs(&self, _: &Artist, _: usize) -> Result<Vec<Song>> {
        Ok(vec![])
    }
    async fn songs(&self, _: &SongQuery) -> Result<Page<Song>> {
        Err(SourceError::Unsupported)
    }
    async fn random_songs(&self, _: usize, _: Option<&str>) -> Result<Vec<Song>> {
        Ok(vec![])
    }
    async fn search(&self, _: &str, _: usize) -> Result<SearchResults> {
        Ok(SearchResults::default())
    }
    async fn playlists(&self) -> Result<Vec<Playlist>> {
        Ok(vec![])
    }
    async fn playlist_songs(&self, _: &str) -> Result<Vec<Song>> {
        Ok(vec![])
    }
    async fn smart_playlist(&self, _: &str) -> Result<Option<SmartPlaylistDef>> {
        Ok(self.2.clone())
    }
    async fn smart_playlist_ids(&self) -> Result<Vec<String>> {
        Ok(vec!["smart-1".into(), "pl-7".into()])
    }
    async fn create_smart_playlist(&self, def: &SmartPlaylistDef) -> Result<Playlist> {
        self.1.lock().unwrap().created.push(def.clone());
        Ok(Playlist {
            id: "smart-1".into(),
            server_id: ServerId("srv".into()),
            source: SourceKind::Subsonic,
            name: def.name.clone(),
            description: None,
            owner: None,
            image: None,
            song_count: None,
            duration: Duration::ZERO,
            public: def.public,
            editable: true,
        })
    }
    async fn update_smart_playlist(&self, id: &str, def: &SmartPlaylistDef) -> Result<()> {
        self.1.lock().unwrap().updated.push((id.into(), def.clone()));
        Ok(())
    }
    async fn set_favorite(&self, _: &[String], _: bool) -> Result<()> {
        Ok(())
    }
    async fn stream(&self, _: &Song) -> Result<StreamSource> {
        Err(SourceError::Unsupported)
    }
    fn image_url(&self, _: &ImageRef, _: u32) -> Option<String> {
        None
    }
}

fn app(caps: Capabilities, existing: Option<SmartPlaylistDef>) -> (Listen, Arc<Mutex<Calls>>) {
    let (mut s, _) =
        Listen::boot(&Args { ephemeral: true, theme: Some("tokyoNight".into()), ..Args::default() });
    let id = ServerId("srv".into());
    s.config.servers = vec![ServerConfig {
        id: id.clone(),
        name: "srv".into(),
        kind: SourceKind::Subsonic,
        url: "https://x.example".into(),
        username: "me".into(),
    }];
    let calls = Arc::new(Mutex::new(Calls::default()));
    let _ = s.update(Message::Connected(id, Ok(SourceHandle(Arc::new(Fake(caps, calls.clone(), existing))))));
    let _ = s.update(Message::WindowResized(iced::Size::new(1200.0, 900.0)));
    (s, calls)
}

fn navidrome() -> Capabilities {
    listen_subsonic::capabilities() | Capabilities::SMART_PLAYLISTS
}

/// Runs a task's futures to completion and returns the messages it produced.
fn run(task: Task<Message>) -> Vec<Message> {
    use iced::futures::StreamExt;
    let Some(mut stream) = iced_runtime::task::into_stream(task) else { return vec![] };
    tokio::runtime::Runtime::new().unwrap().block_on(async move {
        let mut out = Vec::new();
        while let Some(action) = stream.next().await {
            if let iced_runtime::Action::Output(m) = action {
                out.push(m);
            }
        }
        out
    })
}

/// Sends a message and everything its tasks answer (a few rounds).
fn send(s: &mut Listen, msg: SmartMsg) {
    let mut queue = vec![Message::Smart(msg)];
    for _ in 0..4 {
        let mut next = vec![];
        for m in queue {
            let m = match m {
                Message::ForServer(_, inner) => *inner,
                other => other,
            };
            next.extend(run(s.update(m)));
        }
        queue = next;
    }
}

fn editor(s: &Listen) -> &Editor {
    s.smart.editor.as_ref().expect("editor open")
}

fn shows(s: &Listen, text: &str) -> bool {
    let _gpu = gpu();
    let mut ui = Simulator::with_size(settings(), s.window_size, s.view());
    ui.find(text).is_ok()
}

fn click(s: &mut Listen, text: &str) -> Vec<Message> {
    let messages: Vec<Message> = {
        let _gpu = gpu();
        let mut ui = Simulator::with_size(settings(), s.window_size, s.view());
        ui.click(text).unwrap_or_else(|e| panic!("no {text:?} on screen: {e:?}"));
        ui.into_messages().collect()
    };
    for m in messages.clone() {
        let _ = run(s.update(m));
    }
    messages
}

fn year() -> listen_source::nsp::query::FieldOpt {
    field_options([]).into_iter().find(|f| f.key == "year").unwrap()
}

fn field(key: &str) -> listen_source::nsp::query::FieldOpt {
    field_options([]).into_iter().find(|f| f.key == key).unwrap()
}

// ---- capability -----------------------------------------------------------------

#[test]
fn only_navidrome_with_the_native_api_offers_smart_playlists() {
    // YouTube Music, Jellyfin, plain Subsonic (no native API): nothing opens.
    for caps in [
        listen_ytmusic::capabilities(false),
        listen_jellyfin::capabilities(),
        listen_subsonic::capabilities(),
    ] {
        assert!(!caps.contains(Capabilities::SMART_PLAYLISTS));
        let (mut s, _) = app(caps, None);
        assert!(!s.smart_supported());
        send(&mut s, SmartMsg::Create);
        assert!(s.smart.editor.is_none() && s.overlay.modal.is_none());
        send(&mut s, SmartMsg::Edit("p".into()));
        assert!(s.smart.editor.is_none());
        // And the settings page has no "Query builder" section.
        assert!(!shows(&s, "Custom fields"));
    }
    let (mut s, _) = app(navidrome(), None);
    assert!(s.smart_supported());
    send(&mut s, SmartMsg::Create);
    assert!(s.smart.editor.is_some() && s.overlay.modal.is_some());
}

// ---- the builder ----------------------------------------------------------------

#[test]
fn add_and_remove_rules_and_nested_groups_with_clicks() {
    let (mut s, _) = app(navidrome(), None);
    send(&mut s, SmartMsg::Create);
    assert!(shows(&s, "Create smart playlist") && shows(&s, "Query editor"));
    assert_eq!(editor(&s).query.root.rules.len(), 1);

    // The header buttons of the root group.
    let m = click(&mut s, "Add rule");
    assert!(
        m.iter().any(|m| matches!(m, Message::Smart(SmartMsg::Group(p, GroupEdit::AddRule)) if p.is_empty()))
    );
    assert_eq!(editor(&s).query.root.rules.len(), 2);
    click(&mut s, "Add rule group");
    assert_eq!(editor(&s).query.root.groups.len(), 1);
    assert_eq!(editor(&s).query.root.groups[0].kind, GroupKind::Any);
    // A group inside the group: the sub-group's own "Add rule group" is the second button.
    send(&mut s, SmartMsg::Group(vec![0], GroupEdit::AddGroup));
    send(&mut s, SmartMsg::Group(vec![0, 0], GroupEdit::AddRule));
    assert_eq!(editor(&s).query.root.get(&[0, 0]).unwrap().rules.len(), 2);
    assert_eq!(editor(&s).query.root.count(), 2 + 1 + 2);
    // Only nested groups can be removed ("Remove rule group"), the root cannot.
    assert!(shows(&s, "Remove rule group") && shows(&s, "Reset to default") && shows(&s, "Clear filters"));
    click(&mut s, "Clear filters");
    assert_eq!(editor(&s).query.root.count(), 1);

    send(&mut s, SmartMsg::Group(vec![], GroupEdit::AddGroup));
    send(&mut s, SmartMsg::Group(vec![0], GroupEdit::Remove));
    assert!(editor(&s).query.root.groups.is_empty());
    send(&mut s, SmartMsg::Group(vec![], GroupEdit::Kind(GroupKind::Any)));
    assert_eq!(editor(&s).query.root.kind, GroupKind::Any);
    send(&mut s, SmartMsg::Rule(vec![], 0, RuleEdit::Remove));
    assert!(editor(&s).query.root.rules.is_empty());
}

#[test]
fn the_field_type_decides_the_value_input() {
    let (mut s, _) = app(navidrome(), None);
    send(&mut s, SmartMsg::Create);
    let pick = |s: &mut Listen, key: &str, op: &str| {
        send(s, SmartMsg::Rule(vec![], 0, RuleEdit::Field(field(key))));
        if !op.is_empty() {
            let def = operators(field(key).kind).iter().find(|o| o.id == op).copied().unwrap();
            send(s, SmartMsg::Rule(vec![], 0, RuleEdit::Op(def)));
        }
    };
    // No field yet: no value placeholder of any type.
    assert!(!shows(&s, "Number") && !shows(&s, "Value"));
    pick(&mut s, "year", "gt");
    assert_eq!(editor(&s).query.root.rules[0].input(), Input::Number);
    assert!(shows(&s, "Number"));
    pick(&mut s, "year", "inTheRange");
    assert!(shows(&s, "From") && shows(&s, "To"));
    pick(&mut s, "title", "contains");
    assert_eq!(editor(&s).query.root.rules[0].input(), Input::Text);
    assert!(shows(&s, "Value") && !shows(&s, "Number"));
    pick(&mut s, "dateadded", "afterDate");
    assert!(shows(&s, "YYYY-MM-DD"));
    pick(&mut s, "dateadded", "inTheRangeDate");
    assert!(shows(&s, "From (YYYY-MM-DD)"));
    pick(&mut s, "loved", "is");
    assert_eq!(editor(&s).query.root.rules[0].input(), Input::Bool);
    // A drop-down, no text box.
    assert!(!shows(&s, "Value") && !shows(&s, "Number") && !shows(&s, "YYYY-MM-DD"));
    pick(&mut s, "id", "inPlaylist");
    assert_eq!(editor(&s).query.root.rules[0].input(), Input::Playlist);
    assert!(!shows(&s, "Value") && !shows(&s, "Number"));
    // Typing into the input reaches the rule.
    pick(&mut s, "title", "contains");
    send(&mut s, SmartMsg::Rule(vec![], 0, RuleEdit::A("love".into())));
    assert!(shows(&s, "love"));
}

#[test]
fn custom_fields_from_the_settings_are_offered_first() {
    let (mut s, _) = app(navidrome(), None);
    let _ = s.update(Message::Setting(listen::app::Setting::QueryBuilder(
        listen::app::smart_playlist::QueryTagSetting::Add,
    )));
    let _ = s.update(Message::Setting(listen::app::Setting::QueryBuilder(
        listen::app::smart_playlist::QueryTagSetting::Tag(0, "mood2".into()),
    )));
    let _ = s.update(Message::Setting(listen::app::Setting::QueryBuilder(
        listen::app::smart_playlist::QueryTagSetting::Label(0, "Moods".into()),
    )));
    assert_eq!(s.config.settings.query_builder.tags.len(), 1);
    send(&mut s, SmartMsg::Create);
    let first = &editor(&s).fields[0];
    assert_eq!((first.label.as_str(), first.key.as_str(), first.custom), ("Moods", "mood2", true));
    assert!(editor(&s).sorts.iter().all(|f| !f.custom), "custom tags are not sortable");
}

// ---- saving and editing -----------------------------------------------------------

#[test]
fn saving_creates_the_playlist_and_shows_its_songs() {
    let (mut s, calls) = app(navidrome(), None);
    send(&mut s, SmartMsg::Create);
    // A name is required, and a rule value that does not fit is reported.
    send(&mut s, SmartMsg::Save);
    assert!(editor(&s).error.as_deref().unwrap().contains("name"));
    send(&mut s, SmartMsg::Name("Nineties".into()));
    send(&mut s, SmartMsg::Rule(vec![], 0, RuleEdit::Field(year())));
    let gt = operators(year().kind).iter().find(|o| o.id == "gt").copied().unwrap();
    send(&mut s, SmartMsg::Rule(vec![], 0, RuleEdit::Op(gt)));
    send(&mut s, SmartMsg::Rule(vec![], 0, RuleEdit::A("nineteen".into())));
    send(&mut s, SmartMsg::Save);
    assert!(editor(&s).error.as_deref().unwrap().contains("not a number"));
    assert!(shows(&s, "Year: \"nineteen\" is not a number."));
    assert!(calls.lock().unwrap().created.is_empty());
    send(&mut s, SmartMsg::Rule(vec![], 0, RuleEdit::A("1990".into())));
    send(&mut s, SmartMsg::Sort(SortEdit::Add));
    send(&mut s, SmartMsg::Sort(SortEdit::Field(1, field("title"))));
    send(&mut s, SmartMsg::Sort(SortEdit::Desc(1, true)));
    send(&mut s, SmartMsg::Sort(SortEdit::Limit("40".into())));
    send(&mut s, SmartMsg::Public(true));
    send(&mut s, SmartMsg::Save);

    let created = calls.lock().unwrap().created.clone();
    assert_eq!(created.len(), 1, "{:?}", editor_state(&s));
    assert_eq!((created[0].name.as_str(), created[0].public), ("Nineties", true));
    assert_eq!(
        created[0].rules.to_value(),
        serde_json::json!({"all": [{"gt": {"year": 1990}}], "sort": "+album,-title", "limit": 40})
    );
    // Saved: the modal closes, the badge knows the playlist and its page opens.
    assert!(s.smart.editor.is_none() && s.overlay.modal.is_none());
    assert!(s.is_smart("smart-1"));
    assert_eq!(s.route, Route::Playlist("smart-1".into()));
}

fn editor_state(s: &Listen) -> String {
    format!("{:?}", s.smart.editor.as_ref().map(|e| (&e.error, e.saving)))
}

#[test]
fn editing_loads_the_rules_and_saves_an_update() {
    let rules = Nsp::from_json(
        r#"{"any": [{"is": {"loved": true}}, {"all": [{"gt": {"rating": 3}}, {"contains": {"title": "x"}}]},
            {"inTheLast": {"lastplayed": 30}}], "sort": "-year", "limit": 10}"#,
    )
    .unwrap();
    let def = SmartPlaylistDef {
        name: "Loved".into(),
        comment: "my loved".into(),
        public: true,
        sync: true,
        rules,
    };
    let (mut s, calls) = app(navidrome(), Some(def));
    send(&mut s, SmartMsg::Edit("pl-7".into()));
    let e = editor(&s);
    assert_eq!((e.name.as_str(), e.comment.as_str(), e.public, e.sync), ("Loved", "my loved", true, true));
    assert!(!e.loading);
    assert_eq!(e.query.root.kind, GroupKind::Any);
    assert_eq!(e.query.root.rules.len(), 2);
    assert_eq!(e.query.root.groups.len(), 1);
    assert_eq!(e.query.root.groups[0].kind, GroupKind::All);
    assert_eq!((e.query.sort[0].field.as_str(), e.query.sort[0].desc), ("year", true));
    assert_eq!(e.query.limit, "10");
    assert!(shows(&s, "Edit smart playlist") && shows(&s, "Loved"));
    assert!(s.is_smart("pl-7"));

    send(&mut s, SmartMsg::Name("Loved songs".into()));
    send(&mut s, SmartMsg::Save);
    let updated = calls.lock().unwrap().updated.clone();
    assert_eq!(updated.len(), 1);
    assert_eq!(updated[0].0, "pl-7");
    assert_eq!(updated[0].1.name, "Loved songs");
    // The same rules go back, in the order rules-then-groups.
    assert_eq!(
        updated[0].1.rules.to_value(),
        serde_json::json!({"any": [{"is": {"loved": true}}, {"inTheLast": {"lastplayed": 30}},
            {"all": [{"gt": {"rating": 3}}, {"contains": {"title": "x"}}]}], "sort": "-year", "limit": 10})
    );
    assert_eq!(s.route, Route::Playlist("pl-7".into()));
}

#[test]
fn a_plain_playlist_is_not_edited_as_a_smart_one() {
    let (mut s, _) = app(navidrome(), None);
    send(&mut s, SmartMsg::Edit("plain".into()));
    assert!(s.smart.editor.is_none() && s.overlay.modal.is_none());
}

#[test]
fn the_modal_key_is_drawn_by_the_overlay() {
    let (mut s, _) = app(navidrome(), None);
    s.smart.editor = Some(Editor::new(None, &[]));
    s.open_modal(MODAL_KEY);
    assert!(shows(&s, "Create smart playlist"));
    let _ = s.update(Message::Overlay(listen::app::overlay::OverlayMsg::CloseModal));
    assert!(!shows(&s, "Create smart playlist"));
}

#[test]
fn the_settings_page_lists_custom_fields_only_with_smart_playlists() {
    let (mut yes, _) = app(navidrome(), None);
    let _ = yes.update(Message::Navigate(Route::Settings));
    assert!(shows(&yes, "Custom fields") && shows(&yes, "Query builder"));
    let _ = yes.update(Message::Setting(listen::app::Setting::QueryBuilder(
        listen::app::smart_playlist::QueryTagSetting::Add,
    )));
    assert!(shows(&yes, "Tag") && shows(&yes, "Label"));
    let (mut no, _) = app(listen_subsonic::capabilities(), None);
    let _ = no.update(Message::Navigate(Route::Settings));
    assert!(!shows(&no, "Custom fields"));
}
