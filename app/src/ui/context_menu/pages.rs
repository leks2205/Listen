//! The pages that replace the main page inside the menu: the playlist picker,
//! the rename field and the delete confirmation.

use super::items::{act, plain};
use super::{ITEM, LIST_MAX};
use crate::app::menu::{MenuAction, MenuMsg, MenuTarget};
use crate::app::playlists::PlaylistPageMsg;
use crate::app::{Listen, Message};
use crate::icons::Icon;
use crate::style::{self, metrics};
use crate::ui::{icon, label, label_w};
use iced::font::Weight;
use iced::widget::{Column, button, column, container, row, scrollable, text_input};
use iced::{Alignment, Element, Padding};

/// What the picker needs besides its own state.
pub struct Picker<'a> {
    pub list: Option<&'a Result<Vec<listen_core::model::Playlist>, String>>,
    pub new_name: &'a str,
    pub search: &'a str,
    pub can_create: bool,
    /// The playlist songs were added to last (pinned on top).
    pub recent: Option<&'a str>,
    /// `Some(on)` where "Skip duplicates" applies (not YouTube Music, which asks itself).
    pub skip_duplicates: Option<bool>,
}

/// "Add to playlist": a sticky search, a new playlist by name, the "skip
/// duplicates" switch, then the playlists to pick from (the last used on top).
pub fn playlists<'a>(state: &'a Listen, p: Picker<'a>) -> (Element<'a, Message>, f32) {
    let t = state.tokens;
    let header = row![
        icon(Icon::LuListMusic, metrics::font_lg(), t.muted),
        label_w("Add to playlist", metrics::font_md(), t.muted, Weight::Semibold)
    ]
    .spacing(metrics::space_md())
    .align_y(Alignment::Center);
    let search = text_input(crate::i18n::tr("Search playlists…"), p.search)
        .font(crate::ui::font_for(p.search, crate::appearance::current_font()))
        .id(iced::widget::Id::new(SEARCH_ID))
        .font(crate::ui::font_for(p.search, crate::appearance::current_font()))
        .size(metrics::font_md())
        .padding(Padding::from([6.0, 8.0]))
        .style(style::input(t))
        .on_input(|s| Message::PlaylistPage(PlaylistPageMsg::PickerSearch(s)));
    let create = row![
        text_input(crate::i18n::tr("New playlist…"), p.new_name)
            .font(crate::ui::font_for(p.new_name, crate::appearance::current_font()))
            .font(crate::ui::font_for(p.new_name, crate::appearance::current_font()))
            .size(metrics::font_md())
            .padding(Padding::from([6.0, 8.0]))
            .style(style::input(t))
            .on_input(|s| Message::Menu(MenuMsg::NewName(s)))
            .on_submit(act(MenuAction::CreatePlaylist)),
        button(label_w("Create", metrics::font_sm(), t.foreground, Weight::Semibold))
            .padding(Padding::from([6.0, 10.0]))
            .style(style::default_button(t))
            .on_press_maybe((!p.new_name.trim().is_empty()).then(|| act(MenuAction::CreatePlaylist))),
    ]
    .spacing(metrics::space_xs())
    .align_y(Alignment::Center);
    let mut col = column![
        container(header).height(ITEM).padding(Padding::from([0.0, metrics::space_md()])).center_y(ITEM),
        container(search).padding(Padding::from([0.0, metrics::space_xs()])),
    ]
    .spacing(metrics::space_xs());
    let mut height = ITEM + 4.0 + 34.0 + 4.0;
    // Servers that cannot create playlists (Jellyfin) only list the existing ones.
    if p.can_create {
        col = col.push(container(create).padding(Padding::from([0.0, metrics::space_xs()])));
        height += ITEM + 4.0;
    }
    if let Some(on) = p.skip_duplicates {
        col = col.push(
            container(
                iced::widget::checkbox(on)
                    .label("Skip duplicates")
                    .size(16)
                    .text_size(metrics::font_sm())
                    .on_toggle(|v| Message::PlaylistPage(PlaylistPageMsg::SkipDuplicates(v))),
            )
            .padding(Padding::from([0.0, metrics::space_md()]))
            .height(ITEM - 6.0)
            .center_y(ITEM - 6.0),
        );
        height += ITEM - 6.0 + 4.0;
    }
    match p.list {
        None => {
            col = col.push(plain(state, Icon::LuListMusic, "Loading…".into(), None));
            height += ITEM;
        }
        Some(Err(e)) => {
            col = col.push(
                container(label(crate::ui::ellipsize(e, 60), metrics::font_sm(), t.error))
                    .padding(metrics::space_md()),
            );
            col = col.push(plain(
                state,
                Icon::RiRepeat2Line,
                "Retry".into(),
                Some(act(MenuAction::ChoosePlaylist)),
            ));
            height += ITEM * 2.0;
        }
        Some(Ok(list)) if list.is_empty() => {
            col = col.push(plain(state, Icon::LuListMusic, "No playlists yet".into(), None));
            height += ITEM;
        }
        Some(Ok(list)) => {
            let shown = crate::app::playlists::picker::ordered(list, p.search, p.recent);
            if shown.is_empty() {
                col = col.push(plain(state, Icon::LuSearch, "No playlist matches".into(), None));
                height += ITEM;
            } else {
                let mut rows = Column::new();
                for (i, pl) in shown.iter().enumerate() {
                    let name = crate::ui::ellipsize(&pl.name, 30);
                    // YouTube Music marks playlists that already hold the song.
                    let (title, msg) = if pl.editable {
                        (name, Some(act(MenuAction::AddToPlaylist(pl.id.clone(), pl.name.clone()))))
                    } else {
                        (format!("{name} ✓"), None)
                    };
                    let recent = i == 0 && p.recent == Some(pl.id.as_str());
                    rows = rows.push(plain(
                        state,
                        if recent { Icon::LuStar } else { Icon::LuListMusic },
                        title,
                        msg,
                    ));
                }
                let h = (shown.len() as f32 * ITEM).min(LIST_MAX);
                col = col.push(scrollable(rows).style(style::scrollbar(t)).height(h));
                height += h;
            }
        }
    }
    (col.into(), height)
}

/// Id of the picker's search box (focused when the page opens).
pub const SEARCH_ID: &str = "menu-picker-search";

/// "Rename playlist": a text field and a button.
pub fn rename<'a>(state: &'a Listen, name: &'a str) -> (Element<'a, Message>, f32) {
    let t = state.tokens;
    let header = label_w("Rename playlist", metrics::font_md(), t.muted, Weight::Semibold);
    let field = row![
        text_input(crate::i18n::tr("Name"), name)
            .font(crate::ui::font_for(name, crate::appearance::current_font()))
            .font(crate::ui::font_for(name, crate::appearance::current_font()))
            .size(metrics::font_md())
            .padding(Padding::from([6.0, 8.0]))
            .style(style::input(t))
            .on_input(|s| Message::Menu(MenuMsg::NewName(s)))
            .on_submit(act(MenuAction::Rename)),
        button(label_w("Save", metrics::font_sm(), t.foreground, Weight::Semibold))
            .padding(Padding::from([6.0, 10.0]))
            .style(style::default_button(t))
            .on_press_maybe((!name.trim().is_empty()).then(|| act(MenuAction::Rename))),
    ]
    .spacing(metrics::space_xs())
    .align_y(Alignment::Center);
    let col = column![
        container(header).height(ITEM).padding(Padding::from([0.0, metrics::space_md()])).center_y(ITEM),
        container(field).padding(Padding::from([0.0, metrics::space_xs()])),
    ]
    .spacing(metrics::space_xs());
    (col.into(), ITEM * 2.0 + 4.0)
}

/// "Delete playlist": one more question before it is gone.
pub fn confirm_delete<'a>(state: &'a Listen, target: &'a MenuTarget) -> (Element<'a, Message>, f32) {
    let t = state.tokens;
    let name = crate::ui::ellipsize(&target.first_name(), 30);
    let col = column![
        container(label(format!("Delete “{name}”? This can't be undone."), metrics::font_sm(), t.muted))
            .padding(metrics::space_md()),
        plain(state, Icon::LuX, "Delete".into(), Some(act(MenuAction::Delete))),
        plain(state, Icon::LuChevronLeft, "Cancel".into(), Some(Message::Menu(MenuMsg::Close))),
    ];
    (col.into(), ITEM * 2.0 + 60.0)
}
