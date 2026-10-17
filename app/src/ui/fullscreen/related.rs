//! The Related tab: songs similar to the playing one, set through `FullscreenMsg::SetRelated`.

use crate::app::fullscreen_state::FullscreenMsg;
use crate::app::menu::MenuTarget;
use crate::app::{Listen, Message};
use crate::style::{self, metrics};
use crate::ui::{ellipsize_px, label, label_w, song_cover, with_menu};
use iced::font::Weight;
use iced::widget::{Column, button, column, container, row, scrollable, space};
use iced::{Alignment, Element, Length, Padding};
use listen_core::model::format_duration;

const ROW: f32 = 52.0;

pub fn view(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let current = state.queue.current().map(|i| i.song.id.as_str());
    let fresh = state.fs.related_for.as_deref() == current && current.is_some();
    if !fresh || state.fs.related.is_empty() {
        let text =
            if state.fs.related_loading { "Looking for similar songs…" } else { "No similar songs yet." };
        return container(label(text, metrics::font_lg(), t.muted)).center(Length::Fill).into();
    }
    let width = (state.window_size.width * 0.5 - 64.0).max(200.0);
    let mut list = Column::new().spacing(2).width(Length::Fill);
    for (i, song) in state.fs.related.iter().enumerate() {
        let text_w = (width - ROW - 70.0 - 2.0 * metrics::space_md()).max(0.0);
        let item = button(
            row![
                song_cover(state, song, ROW - 12.0, metrics::radius_sm()),
                column![
                    label_w(
                        ellipsize_px(&song.title, text_w, metrics::font_md()),
                        metrics::font_md(),
                        t.foreground,
                        Weight::Medium
                    ),
                    label(
                        ellipsize_px(&song.artist_line(), text_w, metrics::font_sm()),
                        metrics::font_sm(),
                        t.muted
                    ),
                ]
                .spacing(2),
                space::horizontal(),
                label(format_duration(song.duration), metrics::font_sm(), t.muted),
            ]
            .spacing(metrics::space_md())
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .height(ROW)
        .padding(Padding::from([6.0, metrics::space_md()]))
        .style(style::row_button(t, false))
        .on_press(Message::Fullscreen(FullscreenMsg::PlayRelated(i)));
        list = list.push(with_menu(item, MenuTarget::song(song)));
    }
    scrollable(list).style(style::scrollbar(t)).height(Length::Fill).into()
}
