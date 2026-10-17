//! The Playlists page, on the item list component (`ui::list`): a table or a
//! grid of playlists with the count, Play, search, sort, refresh and Create
//! playlist (Feishin `playlist-list-route.tsx`).

use super::list::ListView;
use super::{icon, label, label_w, plural};
use crate::app::list::ListId;
use crate::app::playlists::{PlaylistPageMsg as P, PlaylistSort};
use crate::app::{Listen, Message};
use crate::icons::Icon;
use crate::style::{self, metrics};
use iced::font::Weight;
use iced::widget::{Column, Row, button, container, pick_list, row, space, text_input};
use iced::{Alignment, Element, Length, Padding};

fn msg(m: P) -> Message {
    Message::PlaylistPage(m)
}

fn tool<'a>(state: &Listen, glyph: Icon, on_press: Option<Message>) -> Element<'a, Message> {
    let t = state.tokens;
    crate::ui::tip::tip_icon(
        state,
        button(icon(glyph, metrics::font_lg(), if on_press.is_some() { t.foreground } else { t.muted }))
            .padding(6)
            .style(style::icon_button(t, false))
            .on_press_maybe(on_press),
        glyph,
        crate::ui::tip::Position::Bottom,
    )
}

pub fn view(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let page = &state.playlist_page;
    let id = ListId::Playlists;
    let config = state.config.settings.lists.config(id.kind());
    let loaded = state.library.playlists.len();

    let play = button(
        row![
            icon(Icon::LuPlay, metrics::font_lg(), iced::Color::WHITE),
            label_w(
                state.play_behavior().button_label(),
                metrics::font_md(),
                iced::Color::WHITE,
                Weight::Semibold
            )
        ]
        .spacing(metrics::space_sm())
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([6.0, 14.0]))
    .style(style::primary_button(t))
    .on_press_maybe((!page.list.is_empty()).then(|| msg(P::ListPlay)));
    let count = if page.list.len() == loaded {
        plural(loaded, "playlist")
    } else {
        format!("{} of {loaded} playlists", page.list.len())
    };
    let top = row![
        label_w("Playlists", 36.0, t.foreground, Weight::Bold),
        container(label(count, metrics::font_md(), t.muted)).padding(Padding::from([0.0, 4.0])),
        space::horizontal(),
        play,
    ]
    .spacing(metrics::space_md())
    .align_y(Alignment::Center);

    let search = text_input(crate::i18n::tr("Search"), &page.list_search)
        .font(crate::ui::font_for(&page.list_search, crate::appearance::current_font()))
        .font(crate::ui::font_for(&page.list_search, crate::appearance::current_font()))
        .size(metrics::font_md())
        .padding(Padding::from([6.0, 10.0]))
        .style(style::input(t))
        .on_input(|s| msg(P::ListSearch(s)))
        .width(Length::Fixed(220.0));
    let mut find = Row::new().align_y(Alignment::Center).spacing(2.0).push(search);
    if !page.list_search.is_empty() {
        find = find.push(tool(state, Icon::LuX, Some(msg(P::ListSearch(String::new())))));
    }
    let order = if page.list_desc { Icon::LuArrowDown } else { Icon::LuArrowUp };
    let mut controls = Row::new().spacing(metrics::space_xs()).align_y(Alignment::Center);
    controls = controls
        .push(pick_list(PlaylistSort::ALL, Some(page.list_sort), |s| msg(P::ListSort(s))).width(150))
        .push(tool(state, order, Some(msg(P::ListOrder))))
        .push(tool(state, Icon::LuRefreshCw, Some(msg(P::ListRefresh))));
    if state.can_create_playlist() {
        controls = controls.push(
            button(
                row![
                    icon(Icon::LuPlus, metrics::font_lg(), t.foreground),
                    label_w("Create playlist", metrics::font_md(), t.foreground, Weight::Medium)
                ]
                .spacing(metrics::space_sm())
                .align_y(Alignment::Center),
            )
            .padding(Padding::from([6.0, 12.0]))
            .style(style::default_button(t))
            .on_press(msg(P::Create(Vec::new()))),
        );
    }
    if state.smart_supported() {
        controls = controls.push(
            button(
                row![
                    icon(Icon::LuPlus, metrics::font_lg(), t.foreground),
                    label_w("Create smart playlist", metrics::font_md(), t.foreground, Weight::Medium)
                ]
                .spacing(metrics::space_sm())
                .align_y(Alignment::Center),
            )
            .padding(Padding::from([6.0, 12.0]))
            .style(style::default_button(t))
            .on_press(Message::Smart(crate::app::smart_playlist::SmartMsg::Create)),
        );
    }
    controls =
        controls.push(space::horizontal()).push(find).push(super::list::header::toolbar(state, &id, &config));

    let header =
        container(Column::new().spacing(metrics::space_md()).push(top).push(controls)).padding(Padding {
            top: 24.0,
            right: metrics::PAGE_PADDING_X,
            bottom: 12.0,
            left: metrics::PAGE_PADDING_X,
        });
    let searching = !page.list_search.trim().is_empty();
    iced::widget::column![
        header,
        ListView::new(state, id, &page.list)
            .no_header()
            .loading(false)
            .empty(if searching { "No playlists match." } else { "No playlists yet." })
            .view()
    ]
    .into()
}
