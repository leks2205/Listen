//! The tag editor (Feishin `features/tag-editor`) for local-folder songs: reads tags with `lofty`, edits
//! the common ones and the cover, previews, writes file by file, then rescans the folder.

use super::super::menu::MenuTarget;
use super::super::{Listen, Message};
use super::ItemMsg;
use iced::Task;
use iced::widget::text_editor;
use listen_core::model::{ServerId, Song};
use listen_local::tags::{CoverEdit, FileTags, TagEdit, TagField};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const MODAL: &str = "item-tags";

/// One field of the editor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FieldEdit {
    pub value: String,
    /// The songs have different values (the field shows a hint and is kept
    /// unless something is typed).
    pub mixed: bool,
    pub edited: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Edit,
    Preview,
    /// Writing file `done` of the list.
    Writing {
        done: usize,
    },
}

/// A line of the preview: a field changing in `files` songs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewRow {
    pub field: String,
    /// The old value, or "(N different values)".
    pub old: String,
    pub new: String,
    pub files: usize,
}

#[derive(Debug)]
pub struct TagsState {
    pub server: ServerId,
    pub songs: Vec<Song>,
    pub paths: Vec<PathBuf>,
    /// `None` while the files are being read.
    pub before: Option<Vec<FileTags>>,
    pub fields: BTreeMap<TagField, FieldEdit>,
    pub lyrics: text_editor::Content,
    pub cover_path: String,
    pub cover: CoverEdit,
    pub step: Step,
    pub error: Option<String>,
    /// Files that could not be written: (file name, reason).
    pub failed: Vec<(String, String)>,
}

impl TagsState {
    pub fn edit(&self) -> TagEdit {
        TagEdit {
            fields: self
                .fields
                .iter()
                .filter(|(_, f)| f.edited)
                .map(|(k, f)| (*k, f.value.clone()))
                .collect(),
            cover: self.cover.clone(),
        }
    }

    /// What the preview shows: each changed field once, with how many files it touches.
    pub fn preview(&self) -> Vec<PreviewRow> {
        preview(self.before.as_deref().unwrap_or(&[]), &self.edit())
    }
}

/// The editor's fields for `files`: a value when all agree, else "mixed".
pub fn merge_fields(files: &[FileTags]) -> BTreeMap<TagField, FieldEdit> {
    TagField::ALL
        .into_iter()
        .map(|f| {
            let first = files.first().map(|t| t.get(f)).unwrap_or("");
            let mixed = files.iter().any(|t| t.get(f) != first);
            let value = if mixed { String::new() } else { first.to_string() };
            (f, FieldEdit { value, mixed, edited: false })
        })
        .collect()
}

pub fn preview(files: &[FileTags], edit: &TagEdit) -> Vec<PreviewRow> {
    let mut rows: Vec<PreviewRow> = Vec::new();
    let per_file: Vec<_> = files.iter().map(|f| listen_local::tags::changes(f, edit)).collect();
    let mut order: Vec<String> = Vec::new();
    for changes in &per_file {
        for c in changes {
            if !order.contains(&c.field) {
                order.push(c.field.clone());
            }
        }
    }
    for field in order {
        let touched: Vec<_> = per_file.iter().filter_map(|cs| cs.iter().find(|c| c.field == field)).collect();
        let mut olds: Vec<&str> = touched.iter().map(|c| c.old.as_str()).collect();
        olds.sort_unstable();
        olds.dedup();
        let old =
            if olds.len() == 1 { olds[0].to_string() } else { format!("({} different values)", olds.len()) };
        rows.push(PreviewRow { field, old, new: touched[0].new.clone(), files: touched.len() });
    }
    rows
}

#[derive(Debug, Clone)]
pub enum TagsMsg {
    Loaded(Result<Vec<FileTags>, String>),
    Field(TagField, String),
    Lyrics(text_editor::Action),
    CoverPath(String),
    /// Use the image at the typed path as the cover.
    UseCover,
    RemoveCover,
    KeepCover,
    Preview,
    Back,
    Write,
    Wrote(usize, Result<(), String>),
}

impl Listen {
    /// Opens the editor for local songs; anything else is ignored.
    pub(in crate::app) fn open_tags(&mut self, target: &MenuTarget) -> Task<Message> {
        let Some(songs) = target.song_list().filter(|s| super::can_edit_tags(s)) else {
            return Task::none();
        };
        let Some(source) = self.sources.get(&songs[0].server_id).cloned() else { return Task::none() };
        let paths: Vec<PathBuf> = songs.iter().filter_map(|s| source.local_path(s)).collect();
        if paths.len() != songs.len() {
            self.toast_error("Some of these files are no longer in the folder. Rescan it first.".into());
            return Task::none();
        }
        self.item.tags = Some(TagsState {
            server: songs[0].server_id.clone(),
            songs: songs.to_vec(),
            paths: paths.clone(),
            before: None,
            fields: BTreeMap::new(),
            lyrics: text_editor::Content::new(),
            cover_path: String::new(),
            cover: CoverEdit::Keep,
            step: Step::Edit,
            error: None,
            failed: Vec::new(),
        });
        self.open_modal(MODAL);
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    paths.iter().map(|p| listen_local::tags::read(p)).collect::<Result<Vec<_>, _>>()
                })
                .await
                .map_err(|e| e.to_string())?
            },
            |r| Message::Item(ItemMsg::Tags(TagsMsg::Loaded(r))),
        )
    }

    pub(super) fn on_tags(&mut self, msg: TagsMsg) -> Task<Message> {
        let Some(state) = self.item.tags.as_mut() else { return Task::none() };
        match msg {
            TagsMsg::Loaded(Ok(files)) => {
                state.fields = merge_fields(&files);
                if let Some(l) = state.fields.get(&TagField::Lyrics).filter(|l| !l.mixed) {
                    state.lyrics = text_editor::Content::with_text(&l.value);
                }
                state.before = Some(files);
            }
            TagsMsg::Loaded(Err(e)) => {
                self.item.tags = None;
                self.close_modal();
                self.toast_error(format!("Could not read the tags: {e}"));
            }
            TagsMsg::Field(f, value) => {
                if let Some(field) = state.fields.get_mut(&f) {
                    field.value = if f.numeric() {
                        value.chars().filter(|c| c.is_ascii_digit() || *c == '/').collect()
                    } else {
                        value
                    };
                    field.edited = true;
                    field.mixed = false;
                }
                state.error = None;
            }
            TagsMsg::Lyrics(action) => {
                let edit = action.is_edit();
                state.lyrics.perform(action);
                if edit && let Some(field) = state.fields.get_mut(&TagField::Lyrics) {
                    field.value = state.lyrics.text().trim_end_matches('\n').to_string();
                    field.edited = true;
                    field.mixed = false;
                }
            }
            TagsMsg::CoverPath(p) => state.cover_path = p,
            TagsMsg::UseCover => match std::fs::read(state.cover_path.trim()) {
                Ok(data) => {
                    let edit = TagEdit { cover: CoverEdit::Set(data.clone()), ..Default::default() };
                    match listen_local::tags::validate(&edit) {
                        Ok(()) => {
                            state.cover = CoverEdit::Set(data);
                            state.error = None;
                        }
                        Err(e) => state.error = Some(e),
                    }
                }
                Err(e) => state.error = Some(format!("Could not read the image: {e}")),
            },
            TagsMsg::RemoveCover => state.cover = CoverEdit::Remove,
            TagsMsg::KeepCover => state.cover = CoverEdit::Keep,
            TagsMsg::Preview => {
                if let Err(e) = listen_local::tags::validate(&state.edit()) {
                    state.error = Some(e);
                } else if state.preview().is_empty() {
                    state.error = Some("Nothing has changed.".into());
                } else {
                    state.error = None;
                    state.step = Step::Preview;
                }
            }
            TagsMsg::Back => state.step = Step::Edit,
            TagsMsg::Write => {
                state.step = Step::Writing { done: 0 };
                state.failed.clear();
                return write_file(state, 0);
            }
            TagsMsg::Wrote(i, result) => {
                if let Err(e) = result {
                    let name = state.paths[i]
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    state.failed.push((name, e));
                }
                if i + 1 < state.paths.len() {
                    state.step = Step::Writing { done: i + 1 };
                    return write_file(state, i + 1);
                }
                let (server, failed, total) = (state.server.clone(), state.failed.clone(), state.paths.len());
                self.item.tags = None;
                self.close_modal();
                if failed.is_empty() {
                    self.toast_success(if total == 1 {
                        "Tags written.".into()
                    } else {
                        format!("Tags written to {total} files.")
                    });
                } else {
                    let (name, why) = &failed[0];
                    self.toast_error(format!(
                        "{} of {total} files could not be written ({name}: {why}).",
                        failed.len()
                    ));
                }
                // Re-read every tag, not just files whose mtime/size changed: the
                // editor must not depend on the filesystem's mtime granularity.
                return self.update(Message::RescanServer(server, true));
            }
        }
        Task::none()
    }
}

fn write_file(state: &TagsState, index: usize) -> Task<Message> {
    let (path, edit) = (state.paths[index].clone(), state.edit());
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || listen_local::tags::write(&path, &edit))
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r)
        },
        move |r| Message::Item(ItemMsg::Tags(TagsMsg::Wrote(index, r))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(pairs: &[(TagField, &str)]) -> FileTags {
        FileTags { fields: pairs.iter().map(|(f, v)| (*f, v.to_string())).collect(), cover: None }
    }

    #[test]
    fn fields_agree_or_are_mixed() {
        let files = [
            tags(&[(TagField::Album, "A"), (TagField::Title, "One")]),
            tags(&[(TagField::Album, "A"), (TagField::Title, "Two")]),
        ];
        let f = merge_fields(&files);
        assert_eq!(f[&TagField::Album], FieldEdit { value: "A".into(), mixed: false, edited: false });
        assert!(f[&TagField::Title].mixed && f[&TagField::Title].value.is_empty());
        assert!(!f[&TagField::Year].mixed);
    }

    #[test]
    fn the_preview_groups_a_change_over_many_files() {
        let files = [
            tags(&[(TagField::Album, "A"), (TagField::Year, "2000")]),
            tags(&[(TagField::Album, "B"), (TagField::Year, "2000")]),
            tags(&[(TagField::Album, "New"), (TagField::Year, "2000")]),
        ];
        let edit = TagEdit {
            fields: [(TagField::Album, "New".to_string()), (TagField::Year, "2001".to_string())]
                .into_iter()
                .collect(),
            cover: CoverEdit::Keep,
        };
        let rows = preview(&files, &edit);
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0],
            PreviewRow {
                field: "Album".into(),
                old: "(2 different values)".into(),
                new: "New".into(),
                files: 2
            }
        );
        assert_eq!(
            rows[1],
            PreviewRow { field: "Year".into(), old: "2000".into(), new: "2001".into(), files: 3 }
        );
        assert!(preview(&files, &TagEdit::default()).is_empty());
    }
}
