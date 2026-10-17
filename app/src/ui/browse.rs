//! Genres (list + a page per genre) and the folder browser (tree panel and the
//! songs of the open folder), on the item list component.

use super::item_controls::tabs;
use super::list::ListView;
use super::list::columns::{Cx, ListItem, actions_cell, index_cell, link, text_cell};
use super::{ellipsize, icon, label, label_w, plural};
use crate::app::library_extra::{ExtraMsg, GenreSort, breadcrumbs};
use crate::app::list::ListId;
use crate::app::menu::MenuTarget;
use crate::app::{Listen, Message, Route};
use crate::icons::Icon;
use crate::style::{self, metrics};
use iced::font::Weight;
use iced::widget::{Column, Row, button, column, container, pick_list, row, scrollable, space, text_input};
use iced::{Alignment, Element, Length, Padding};
use listen_core::model::Genre;
use listen_core::settings::{ColumnId, GenreTarget, PlayBehavior};

// ---- genres ------------------------------------------------------------------------

impl std::fmt::Display for GenreSort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

impl ListItem for Genre {
    fn text(&self, col: ColumnId, index: usize) -> Option<String> {
        Some(match col {
            ColumnId::RowIndex => (index + 1).to_string(),
            ColumnId::Title => self.name.clone(),
            ColumnId::AlbumCount => self.album_count?.to_string(),
            ColumnId::SongCount => self.song_count?.to_string(),
            _ => return None,
        })
    }

    fn cell<'a>(&'a self, cx: &Cx<'a>, col: ColumnId) -> Element<'a, Message> {
        let t = cx.state.tokens;
        match col {
            ColumnId::RowIndex => index_cell(cx, cx.index + 1, true, false),
            ColumnId::Title => link(
                t,
                super::ellipsize_px(&self.name, cx.width, metrics::font_md()),
                metrics::font_md(),
                t.foreground,
                Some(Message::Navigate(Route::Genre(self.name.clone()))),
            ),
            ColumnId::Actions => actions_cell(cx, self.menu_target()),
            other => match self.text(other, cx.index) {
                Some(s) => text_cell(cx, s, false),
                None => space().width(0).into(),
            },
        }
    }

    fn cover<'a>(&self, _state: &Listen, size: f32, radius: f32) -> Element<'a, Message> {
        super::art((60, 60, 60), size, radius, Icon::RiFlag2Line)
    }

    fn menu_target(&self) -> Option<MenuTarget> {
        Some(MenuTarget::genre(self))
    }

    fn title(&self) -> String {
        self.name.clone()
    }
}

pub fn genres(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let x = &state.extra;
    let controls = row![
        text_input(crate::i18n::tr("Search genres"), &x.genre_search)
            .font(crate::ui::font_for(&x.genre_search, crate::appearance::current_font()))
            .on_input(|v| Message::Extra(ExtraMsg::GenreSearch(v)))
            .width(200)
            .style(style::input(t)),
        pick_list(GenreSort::ALL, Some(x.genre_sort), |s| Message::Extra(ExtraMsg::GenreSort(s))),
        crate::ui::tip::tip(
            state,
            button(icon(
                if x.genre_descending { Icon::LuArrowDown } else { Icon::LuArrowUp },
                metrics::font_lg(),
                t.foreground
            ))
            .padding(6)
            .style(style::icon_button(t, false))
            .on_press(Message::Extra(ExtraMsg::GenreOrder)),
            if x.genre_descending { "Sort order: descending" } else { "Sort order: ascending" },
            None,
            crate::ui::tip::Position::Bottom,
        ),
    ]
    .spacing(metrics::space_sm())
    .align_y(Alignment::Center);
    ListView::new(state, ListId::Genres, &x.genres_view)
        .title("Genres")
        .subtitle(plural(x.genres_view.len(), "genre"))
        .loading(!state.library.genres_loaded)
        .extra(controls)
        .empty("No genres")
        .view()
}

static NO_ALBUMS: Vec<listen_core::model::Album> = Vec::new();
static NO_SONGS: Vec<listen_core::model::Song> = Vec::new();

pub fn genre(state: &Listen) -> Element<'_, Message> {
    let Route::Genre(name) = &state.route else { return space().into() };
    let target = state.config.settings.lists.genre_target;
    let toggle = tabs(
        state,
        vec![
            (
                "Albums".into(),
                target == GenreTarget::Albums,
                Message::Extra(ExtraMsg::GenreTarget(GenreTarget::Albums)),
            ),
            (
                "Tracks".into(),
                target == GenreTarget::Tracks,
                Message::Extra(ExtraMsg::GenreTarget(GenreTarget::Tracks)),
            ),
        ],
    );
    let view = state.extra.genre.as_ref().filter(|g| &g.name == name);
    let q = &state.list_queries;
    match target {
        GenreTarget::Albums => {
            let id = ListId::Genre(name.clone());
            let albums = view.map_or(&NO_ALBUMS, |g| &g.albums);
            let loaded = view.is_some_and(|g| g.albums_load != crate::app::library_extra::Load::Idle);
            let done = view.is_some_and(|g| g.albums_load == crate::app::library_extra::Load::Done);
            let count =
                if done { plural(albums.len(), "album") } else { format!("{}+ albums", albums.len()) };
            let narrowed = !q.genre_albums.search.trim().is_empty() || q.genre_albums.filters.active();
            column![
                super::list::header::page_header_with(state, &id, name.clone(), count, Some(toggle)),
                ListView::new(state, id, albums)
                    .no_header()
                    .loading(!loaded)
                    .done(done)
                    .empty(if narrowed { "No albums match." } else { "No albums in this genre" })
                    .view()
            ]
            .into()
        }
        GenreTarget::Tracks => {
            let id = ListId::GenreSongs(name.clone());
            let songs = view.map_or(&NO_SONGS, |g| &g.songs);
            let loaded = view.is_some_and(|g| g.songs_load != crate::app::library_extra::Load::Idle);
            let done = view.is_some_and(|g| g.songs_load == crate::app::library_extra::Load::Done);
            let count = if done { plural(songs.len(), "track") } else { format!("{}+ tracks", songs.len()) };
            let narrowed = !q.genre_songs.search.trim().is_empty() || q.genre_songs.filters.active();
            column![
                super::list::header::page_header_with(state, &id, name.clone(), count, Some(toggle)),
                ListView::new(state, id, songs)
                    .no_header()
                    .loading(!loaded)
                    .done(done)
                    .empty(if narrowed { "No tracks match." } else { "No tracks in this genre" })
                    .view()
            ]
            .into()
        }
    }
}

// ---- folders -----------------------------------------------------------------------

const TREE_W: f32 = 260.0;

fn tree_rows<'a>(
    state: &'a Listen,
    out: &mut Vec<Element<'a, Message>>,
    parent: &str,
    depth: usize,
    current: &str,
) {
    let t = state.tokens;
    let tree = &state.extra.tree;
    let Some(entries) = tree.children.get(parent) else { return };
    for e in entries {
        let open = tree.expanded.contains(&e.id);
        let known_empty = tree.children.get(&e.id).is_some_and(|c| c.is_empty());
        let chevron: Element<'a, Message> = if known_empty {
            space().width(24).into()
        } else {
            crate::ui::tip::tip(
                state,
                button(icon(
                    if open { Icon::LuChevronDown } else { Icon::LuChevronRight },
                    metrics::font_md(),
                    t.muted,
                ))
                .padding(4)
                .style(style::icon_button(t, false))
                .on_press(Message::Extra(ExtraMsg::TreeToggle(e.id.clone()))),
                if open { "Collapse" } else { "Expand" },
                None,
                crate::ui::tip::Position::Right,
            )
        };
        let name = button(label(
            ellipsize(&e.name, 28usize.saturating_sub(depth * 2).max(10)),
            metrics::font_md(),
            t.foreground,
        ))
        .width(Length::Fill)
        .padding(Padding::from([4.0, 6.0]))
        .style(style::row_button(t, e.id == current))
        .on_press(Message::Navigate(Route::Folder(Some(e.id.clone()))));
        out.push(super::with_menu(
            row![space().width(depth as f32 * 14.0), chevron, name].align_y(Alignment::Center),
            MenuTarget::folder(e),
        ));
        if open {
            if tree.loading.contains(&e.id) {
                out.push(
                    row![
                        space().width((depth + 1) as f32 * 14.0 + 24.0),
                        label("Loading…", metrics::font_sm(), t.muted)
                    ]
                    .into(),
                );
            } else {
                tree_rows(state, out, &e.id, depth + 1, current);
            }
        }
    }
}

fn tree_panel<'a>(state: &'a Listen, current: &str) -> Element<'a, Message> {
    let t = state.tokens;
    let mut col = Column::new().spacing(2).push(
        button(label_w("All folders", metrics::font_md(), t.foreground, Weight::Medium))
            .width(Length::Fill)
            .padding(Padding::from([6.0, 8.0]))
            .style(style::row_button(t, current.is_empty()))
            .on_press(Message::Navigate(Route::Folder(None))),
    );
    if state.extra.tree.loading.contains("") && state.extra.tree.children.is_empty() {
        col = col.push(label("Loading…", metrics::font_sm(), t.muted));
    }
    let mut rows = Vec::new();
    tree_rows(state, &mut rows, "", 0, current);
    col = col.extend(rows);
    container(scrollable(col.padding(8)).style(style::scrollbar(t)).height(Length::Fill))
        .width(TREE_W)
        .height(Length::Fill)
        .into()
}

fn crumbs<'a>(state: &'a Listen, current: &str) -> Element<'a, Message> {
    let t = state.tokens;
    let mut out = Row::new().spacing(metrics::space_xs()).align_y(Alignment::Center);
    out = out.push(link(
        t,
        "Folders".into(),
        metrics::font_sm(),
        t.muted,
        (!current.is_empty()).then(|| Message::Navigate(Route::Folder(None))),
    ));
    for crumb in breadcrumbs(&state.extra.tree, current, 5) {
        out = out.push(icon(Icon::LuChevronRight, metrics::font_sm(), t.muted));
        out = match crumb {
            Some((name, id)) => {
                let last = id == current;
                out.push(link(
                    t,
                    ellipsize(&name, 28),
                    metrics::font_sm(),
                    if last { t.foreground } else { t.muted },
                    (!last).then(|| Message::Navigate(Route::Folder(Some(id)))),
                ))
            }
            None => out.push(label("…", metrics::font_sm(), t.muted)),
        };
    }
    container(out)
        .padding(Padding {
            top: 16.0,
            right: metrics::PAGE_PADDING_X,
            bottom: 0.0,
            left: metrics::PAGE_PADDING_X,
        })
        .into()
}

fn subfolders<'a>(state: &'a Listen, folder: &'a listen_core::model::Folder) -> Option<Element<'a, Message>> {
    if folder.folders.is_empty() {
        return None;
    }
    let t = state.tokens;
    let mut col = Column::new();
    for entry in &folder.folders {
        let play = button(icon(Icon::LuPlay, metrics::font_md(), t.muted))
            .padding(4)
            .style(style::icon_button(t, false))
            .on_press(Message::Extra(ExtraMsg::FolderPlay(entry.id.clone(), state.play_behavior())));
        let play = crate::ui::tip::tip(state, play, "Play folder", None, crate::ui::tip::Position::Right);
        col = col.push(super::with_menu(
            row![
                button(
                    row![
                        icon(Icon::RiFolder3Line, metrics::font_lg(), t.muted),
                        label_w(ellipsize(&entry.name, 80), metrics::font_md(), t.foreground, Weight::Medium),
                    ]
                    .spacing(metrics::space_md())
                    .align_y(Alignment::Center),
                )
                .width(Length::Fill)
                .height(metrics::ROW_HEIGHT_COMPACT)
                .padding(Padding::from([0.0, metrics::space_md()]))
                .style(style::row_button(t, false))
                .on_press(Message::Navigate(Route::Folder(Some(entry.id.clone())))),
                play,
            ]
            .align_y(Alignment::Center),
            MenuTarget::folder(entry),
        ));
    }
    Some(
        container(scrollable(col).style(style::scrollbar(t)))
            .max_height(6.0 * metrics::ROW_HEIGHT_COMPACT)
            .padding(Padding::from([0.0, metrics::PAGE_PADDING_X]))
            .into(),
    )
}

pub fn folder(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let current = match &state.route {
        Route::Folder(Some(id)) => id.as_str(),
        _ => "",
    };
    let main: Element<'_, Message> = match &state.extra.folder {
        None => container(label("Loading…", metrics::font_md(), t.muted)).padding(48).into(),
        Some(folder) => {
            let id = folder.id.clone();
            let button_for = |text: &'static str, msg: Message| {
                button(label(text, metrics::font_md(), t.foreground))
                    .padding(Padding::from([6.0, 14.0]))
                    .style(style::default_button(t))
                    .on_press(msg)
            };
            let extra = row![
                button(
                    row![
                        icon(Icon::LuPlay, metrics::font_md(), t.foreground),
                        label(state.play_behavior().button_label(), metrics::font_md(), t.foreground)
                    ]
                    .spacing(metrics::space_sm())
                    .align_y(Alignment::Center)
                )
                .padding(Padding::from([6.0, 14.0]))
                .style(style::primary_button(t))
                .on_press(Message::Extra(ExtraMsg::FolderPlay(id.clone(), state.play_behavior()))),
                button_for("Play next", Message::Extra(ExtraMsg::FolderPlay(id.clone(), PlayBehavior::Next))),
                button_for("Play last", Message::Extra(ExtraMsg::FolderPlay(id.clone(), PlayBehavior::Last))),
                button_for("Shuffle", Message::Extra(ExtraMsg::FolderShuffle(id.clone()))),
                button_for("Refresh", Message::Extra(ExtraMsg::FolderRefresh)),
            ]
            .spacing(metrics::space_sm())
            .align_y(Alignment::Center);
            let list = ListView::new(state, ListId::Folder, &folder.songs)
                .title(folder.name.clone())
                .subtitle(plural(folder.songs.len(), "track"))
                .loading(state.extra.folder_loading)
                .extra(extra)
                .empty(if folder.folders.is_empty() {
                    "This folder is empty"
                } else {
                    "No tracks directly in this folder"
                });
            let mut col = column![crumbs(state, current)];
            if let Some(strip) = subfolders(state, folder) {
                col = col.push(container(strip).padding(Padding { top: 12.0, ..Padding::ZERO }));
            }
            col.push(container(list.view()).height(Length::Fill)).into()
        }
    };
    row![
        tree_panel(state, current),
        super::divider_v(t),
        container(main).width(Length::Fill).height(Length::Fill)
    ]
    .height(Length::Fill)
    .into()
}
