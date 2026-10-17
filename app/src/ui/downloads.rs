//! Downloads page: what is stored for offline play, with delete buttons.
//! Works without a connected server.

use super::{icon, label, label_w, songs};
use crate::app::offline::{OfflineMsg, format_bytes};
use crate::app::{Listen, Message};
use crate::icons::Icon;
use crate::style::{self, metrics};
use iced::font::Weight;
use iced::widget::{Column, button, column, container, row, scrollable, space};
use iced::{Alignment, Element, Length, Padding};

pub fn view(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let Some(store) = state.offline_store() else {
        return container(label("Offline storage is not available.", metrics::font_md(), t.muted))
            .padding(metrics::PAGE_PADDING_X)
            .into();
    };
    let songs_list = &state.offline.songs;
    let summary =
        format!("{} · {}", super::plural(songs_list.len(), "song"), format_bytes(store.total_bytes()));
    let mut remove_all = button(
        row![
            icon(Icon::LuTrash2, metrics::font_md(), t.foreground),
            label_w("Remove all", metrics::font_md(), t.foreground, Weight::Semibold)
        ]
        .spacing(metrics::space_sm())
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([8.0, 16.0]))
    .style(style::default_button(t));
    if !songs_list.is_empty() {
        remove_all = remove_all.on_press(Message::Offline(OfflineMsg::RemoveAll));
    }
    let mut head = column![
        row![label_w("Downloads", 36.0, t.foreground, Weight::Bold), space::horizontal(), remove_all,]
            .align_y(Alignment::Center),
        label(summary, metrics::font_md(), t.muted),
    ]
    .spacing(metrics::space_sm());
    if let Some(status) = state.offline.status() {
        head = head.push(label(status, metrics::font_md(), t.primary));
    }
    if !state.offline.failed.is_empty() {
        let mut failed = Column::new().spacing(2);
        for (title, reason) in &state.offline.failed {
            failed = failed.push(label(format!("✗ {title}: {reason}"), metrics::font_sm(), t.error));
        }
        head = head.push(failed).push(
            button(label("Dismiss errors", metrics::font_sm(), t.foreground))
                .padding(Padding::from([4.0, 10.0]))
                .style(style::default_button(t))
                .on_press(Message::Offline(OfflineMsg::ClearFailed)),
        );
    }
    let head = container(head).padding(Padding {
        top: 24.0,
        right: metrics::PAGE_PADDING_X,
        bottom: 12.0,
        left: metrics::PAGE_PADDING_X,
    });

    let body: Element<'_, Message> = if songs_list.is_empty() {
        container(label(
            "Nothing downloaded yet. Right-click a song, album or playlist and choose Download.",
            metrics::font_md(),
            t.muted,
        ))
        .padding(Padding::from([metrics::space_lg(), metrics::PAGE_PADDING_X]))
        .into()
    } else {
        let mut rows = Column::new();
        for (i, song) in songs_list.iter().enumerate() {
            let size = state.offline.sizes.get(i).copied().map(format_bytes).unwrap_or_default();
            rows = rows.push(
                row![
                    container(songs::song_row(state, i, song, Message::Offline(OfflineMsg::Play(i))))
                        .width(Length::Fill),
                    label(size, metrics::font_sm(), t.muted),
                    crate::ui::tip::tip(
                        state,
                        button(icon(Icon::LuTrash2, metrics::font_md(), t.muted))
                            .padding(8)
                            .style(style::icon_button(t, false))
                            .on_press(Message::Offline(OfflineMsg::Remove(
                                song.server_id.clone(),
                                song.id.clone()
                            ))),
                        "Remove download",
                        None,
                        crate::ui::tip::Position::Left,
                    ),
                ]
                .spacing(metrics::space_sm())
                .align_y(Alignment::Center),
            );
        }
        scrollable(container(rows).padding(Padding::from([0.0, 12.0])))
            .style(style::scrollbar(t))
            .height(Length::Fill)
            .into()
    };
    column![head, super::divider_h(t), body].into()
}
