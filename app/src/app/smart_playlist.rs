//! Smart playlists (Navidrome `.nsp` rules): the create/edit modal (`ui::smart_playlist`), saving through
//! Navidrome's native API, and the ids behind the "smart" badge. Rules and JSON live in `listen_source::nsp`.
//!
//! For the playlist pages (they live in other files):
//! - `Message::Smart(SmartMsg::Create)` opens the editor for a new smart playlist,
//!   `SmartMsg::Edit(playlist_id)` loads the rules of an existing one and edits them.
//! - `Listen::smart_supported()` tells whether to show those buttons (Navidrome only).
//! - `Listen::is_smart(id)` and `ui::smart_playlist::badge` draw the "smart" badge.

use super::*;
use listen_source::nsp::GroupKind;
use listen_source::nsp::fields::OpDef;
use listen_source::nsp::query::{FieldOpt, LimitMode, Query, field_options, sort_options};
use listen_source::{Capabilities, SmartPlaylistDef};
use std::collections::HashSet;

/// Key of the editor modal (`ui::overlay` draws it through `ui::smart_playlist::panel`).
pub const MODAL_KEY: &str = "smart-playlist";

/// A playlist in the "Is in / Is not in" picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistChoice {
    pub id: String,
    pub name: String,
}

impl std::fmt::Display for PlaylistChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

/// The smart playlist being created or edited.
#[derive(Debug)]
pub struct Editor {
    /// `Some`: editing this playlist.
    pub id: Option<String>,
    pub name: String,
    pub comment: String,
    pub public: bool,
    pub sync: bool,
    pub query: Query,
    /// The fields rules can use (custom tags first) and the sort choices.
    pub fields: Arc<[FieldOpt]>,
    pub sorts: Arc<[FieldOpt]>,
    /// The rules are on their way from the server.
    pub loading: bool,
    pub saving: bool,
    /// Why the last save was refused.
    pub error: Option<String>,
}

impl Editor {
    pub fn new(id: Option<String>, tags: &[listen_core::settings::QueryTag]) -> Self {
        let fields: Vec<FieldOpt> =
            field_options(tags.iter().map(|t| (t.label.as_str(), t.kind.as_str(), t.tag.as_str())));
        Self {
            id,
            name: String::new(),
            comment: String::new(),
            public: false,
            sync: false,
            query: Query::default(),
            sorts: sort_options(&fields).into(),
            fields: fields.into(),
            loading: false,
            saving: false,
            error: None,
        }
    }

    /// The definition to save, or why it cannot be saved yet.
    pub fn def(&self) -> Result<SmartPlaylistDef, String> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err("Enter a name for the playlist.".into());
        }
        Ok(SmartPlaylistDef {
            name: name.to_string(),
            comment: self.comment.trim().to_string(),
            public: self.public,
            sync: self.sync,
            rules: self.query.to_nsp(&self.fields)?,
        })
    }

    /// Fills the editor from a saved definition.
    pub fn fill(&mut self, def: SmartPlaylistDef) {
        self.name = def.name;
        self.comment = def.comment;
        self.public = def.public;
        self.sync = def.sync;
        self.query = Query::from_nsp(&def.rules, &self.fields);
        self.loading = false;
    }
}

#[derive(Debug, Default)]
pub struct SmartState {
    pub editor: Option<Editor>,
    /// Ids of the smart playlists of the active server (for the badge).
    pub ids: HashSet<String>,
}

#[derive(Debug, Clone)]
pub enum GroupEdit {
    Kind(GroupKind),
    AddRule,
    AddGroup,
    Remove,
}

#[derive(Debug, Clone)]
pub enum RuleEdit {
    Field(FieldOpt),
    Op(OpDef),
    /// The value, or the lower end of a range.
    A(String),
    B(String),
    Remove,
}

#[derive(Debug, Clone)]
pub enum SortEdit {
    Add,
    Remove(usize),
    Field(usize, FieldOpt),
    Desc(usize, bool),
    Mode(LimitMode),
    Limit(String),
}

#[derive(Debug, Clone)]
pub enum SmartMsg {
    /// Open the editor for a new smart playlist.
    Create,
    /// Open the editor on an existing smart playlist (its rules are fetched).
    Edit(String),
    Loaded(String, Result<Option<SmartPlaylistDef>, String>),
    Name(String),
    Comment(String),
    Public(bool),
    Sync(bool),
    /// An edit of the group at this path (indices of sub-groups, root = empty).
    Group(Vec<usize>, GroupEdit),
    /// An edit of rule `.1` of the group at `.0`.
    Rule(Vec<usize>, usize, RuleEdit),
    Sort(SortEdit),
    /// "Reset to default" / "Clear filters" of the root group.
    Reset,
    Clear,
    Save,
    /// The id of the saved playlist.
    Saved(Result<String, String>),
    Ids(Result<Vec<String>, String>),
}

impl Listen {
    /// The active server can make smart playlists (Navidrome through its native API).
    pub fn smart_supported(&self) -> bool {
        self.source.as_ref().is_some_and(|s| s.capabilities().contains(Capabilities::SMART_PLAYLISTS))
    }

    pub fn is_smart(&self, playlist_id: &str) -> bool {
        self.smart.ids.contains(playlist_id)
    }

    /// Looks up which playlists are smart (after the playlists loaded).
    pub(super) fn smart_ids_task(&self) -> Task<Message> {
        let Some(source) = self.source.clone().filter(|_| self.smart_supported()) else {
            return Task::none();
        };
        Task::perform(
            async move { source.smart_playlist_ids().await.map_err(err) },
            self.scoped(|r| Message::Smart(SmartMsg::Ids(r))),
        )
    }

    fn open_editor(&mut self, editor: Editor) {
        self.smart.editor = Some(editor);
        self.open_modal(MODAL_KEY);
    }

    pub(super) fn on_smart(&mut self, msg: SmartMsg) -> Task<Message> {
        match msg {
            SmartMsg::Create => {
                if !self.smart_supported() {
                    self.toast_info("Smart playlists need a Navidrome server.".into());
                    return Task::none();
                }
                let editor = Editor::new(None, &self.config.settings.query_builder.tags);
                self.open_editor(editor);
                // The playlist picker of "Is in" rules wants the list.
                if self.library.playlists.is_empty() {
                    return self.load_playlists();
                }
            }
            SmartMsg::Edit(id) => {
                let Some(source) = self.source.clone().filter(|_| self.smart_supported()) else {
                    self.toast_info("Smart playlists need a Navidrome server.".into());
                    return Task::none();
                };
                let mut editor = Editor::new(Some(id.clone()), &self.config.settings.query_builder.tags);
                editor.loading = true;
                self.open_editor(editor);
                let mut tasks = vec![Task::perform(
                    {
                        let id = id.clone();
                        async move { source.smart_playlist(&id).await.map_err(err) }
                    },
                    self.scoped(move |r| Message::Smart(SmartMsg::Loaded(id, r))),
                )];
                if self.library.playlists.is_empty() {
                    tasks.push(self.load_playlists());
                }
                return Task::batch(tasks);
            }
            SmartMsg::Loaded(id, result) => {
                let open = self.smart.editor.as_ref().is_some_and(|e| e.id.as_deref() == Some(&id));
                if !open {
                    return Task::none();
                }
                match result {
                    Ok(Some(def)) => {
                        self.smart.ids.insert(id);
                        if let Some(e) = self.smart.editor.as_mut() {
                            e.fill(def);
                        }
                    }
                    Ok(None) => {
                        self.smart.editor = None;
                        self.close_modal();
                        self.toast_info("That playlist has no rules, it is not a smart playlist.".into());
                    }
                    Err(e) => {
                        self.smart.editor = None;
                        self.close_modal();
                        self.toast_error(format!("Could not load the rules: {e}"));
                    }
                }
            }
            SmartMsg::Name(v) => self.edit(|e| e.name = v),
            SmartMsg::Comment(v) => self.edit(|e| e.comment = v),
            SmartMsg::Public(v) => self.edit(|e| e.public = v),
            SmartMsg::Sync(v) => self.edit(|e| e.sync = v),
            SmartMsg::Group(path, edit) => self.edit(|e| match edit {
                GroupEdit::Kind(k) => e.query.set_kind(&path, k),
                GroupEdit::AddRule => e.query.add_rule(&path),
                GroupEdit::AddGroup => e.query.add_group(&path),
                GroupEdit::Remove => e.query.remove_group(&path),
            }),
            SmartMsg::Rule(path, i, edit) => self.edit(|e| match edit {
                RuleEdit::Field(f) => e.query.set_field(&path, i, &f),
                RuleEdit::Op(op) => e.query.set_op(&path, i, op.id),
                RuleEdit::A(t) => e.query.set_a(&path, i, t),
                RuleEdit::B(t) => e.query.set_b(&path, i, t),
                RuleEdit::Remove => e.query.remove_rule(&path, i),
            }),
            SmartMsg::Sort(edit) => self.edit(|e| match edit {
                SortEdit::Add => e.query.add_sort(),
                SortEdit::Remove(i) => e.query.remove_sort(i),
                SortEdit::Field(i, f) => {
                    if let Some(s) = e.query.sort.get_mut(i) {
                        s.field = f.key;
                    }
                }
                SortEdit::Desc(i, desc) => {
                    if let Some(s) = e.query.sort.get_mut(i) {
                        s.desc = desc;
                    }
                }
                SortEdit::Mode(m) => e.query.limit_mode = m,
                SortEdit::Limit(t) => match e.query.limit_mode {
                    LimitMode::Count => e.query.limit = t,
                    LimitMode::Percent => e.query.limit_percent = t,
                },
            }),
            SmartMsg::Reset => self.edit(|e| e.query.reset()),
            SmartMsg::Clear => self.edit(|e| e.query.clear()),
            SmartMsg::Save => return self.smart_save(),
            SmartMsg::Saved(Ok(id)) => {
                let editing = self.smart.editor.take().is_some_and(|e| e.id.is_some());
                self.close_modal();
                self.smart.ids.insert(id.clone());
                self.toast_success(
                    if editing { "Smart playlist saved." } else { "Smart playlist created." }.into(),
                );
                // The playlist page lists the songs the rules select.
                return Task::batch([self.load_playlists(), self.navigate(Route::Playlist(id))]);
            }
            SmartMsg::Saved(Err(e)) => {
                self.edit(|ed| {
                    ed.saving = false;
                    ed.error = Some(format!("Could not save: {e}"));
                });
            }
            SmartMsg::Ids(Ok(ids)) => self.smart.ids = ids.into_iter().collect(),
            SmartMsg::Ids(Err(e)) => tracing::warn!("smart playlist ids: {e}"),
        }
        Task::none()
    }

    fn edit(&mut self, f: impl FnOnce(&mut Editor)) {
        if let Some(e) = self.smart.editor.as_mut().filter(|e| !e.saving) {
            f(e);
            e.error = None;
        }
    }

    fn smart_save(&mut self) -> Task<Message> {
        let Some(source) = self.source.clone() else { return Task::none() };
        let Some(editor) = self.smart.editor.as_mut().filter(|e| !e.saving && !e.loading) else {
            return Task::none();
        };
        let def = match editor.def() {
            Ok(def) => def,
            Err(e) => {
                editor.error = Some(e);
                return Task::none();
            }
        };
        editor.saving = true;
        editor.error = None;
        let id = editor.id.clone();
        Task::perform(
            async move {
                match id {
                    Some(id) => source.update_smart_playlist(&id, &def).await.map(|()| id),
                    None => source.create_smart_playlist(&def).await.map(|p| p.id),
                }
                .map_err(err)
            },
            self.scoped(|r| Message::Smart(SmartMsg::Saved(r))),
        )
    }
}

/// A change of the custom fields on the settings page.
#[derive(Debug, Clone)]
pub enum QueryTagSetting {
    Add,
    Remove(usize),
    Label(usize, String),
    Kind(usize, String),
    Tag(usize, String),
}

impl QueryTagSetting {
    pub fn apply(self, s: &mut listen_core::settings::QueryBuilder) {
        use listen_core::settings::QueryTag;
        match self {
            QueryTagSetting::Add => s.tags.push(QueryTag { kind: "string".into(), ..Default::default() }),
            QueryTagSetting::Remove(i) if i < s.tags.len() => {
                s.tags.remove(i);
            }
            QueryTagSetting::Label(i, v) => {
                if let Some(t) = s.tags.get_mut(i) {
                    t.label = v;
                }
            }
            QueryTagSetting::Kind(i, v) => {
                if let Some(t) = s.tags.get_mut(i) {
                    t.kind = v;
                }
            }
            QueryTagSetting::Tag(i, v) => {
                if let Some(t) = s.tags.get_mut(i) {
                    t.tag = v;
                }
            }
            QueryTagSetting::Remove(_) => {}
        }
    }
}
