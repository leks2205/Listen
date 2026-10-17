//! Left column: cover + title / artists / album (or the AI DJ caption).

use crate::app::{Listen, Message};
use crate::icons::Icon;
use crate::style::{self, metrics};
use crate::ui::list::anchor::anchored;
use crate::ui::tip::{Position, tip};
use crate::ui::{ellipsize_px, icon, label, label_w, song_cover, song_links};
use iced::font::Weight;
use iced::widget::{Row, button, column, container, row, space};
use iced::{Alignment, Element, Length, Padding};

/// Side of the "⋮" button.
const MORE: f32 = 24.0;

pub(super) fn view(state: &Listen, width: f32) -> Element<'_, Message> {
    let t = state.tokens;
    if let Some(radio) = crate::ui::radio::now_playing(state, width) {
        return radio; // a radio station plays
    }
    let Some(item) = state.queue.current() else {
        return space().width(width).into();
    };
    let song = &item.song;
    // Text width: the column minus padding and the cover, capped at 20vw like Feishin.
    let cover = if state.config.settings.sidebar.cover_expanded { 0.0 } else { metrics::PLAYER_ART + 12.0 };
    let px = (width - metrics::space_lg() - cover - metrics::space_sm())
        .min(state.window_size.width * 0.2)
        .max(0.0);
    let fit = |s: &str, size: f32| ellipsize_px(s, px, size);
    // Title (link to Now Playing) with the "⋮" song menu next to it, artists and album as links.
    let target = crate::app::menu::MenuTarget::queue_item(song, item.uid);
    let title_line = || -> Element<'_, Message> {
        let more = anchored(
            tip(
                state,
                button(
                    container(icon(Icon::LuEllipsisVertical, metrics::font_lg(), t.muted))
                        .center(Length::Fill),
                )
                .width(MORE)
                .height(MORE)
                .padding(0)
                .style(style::icon_button(t, false))
                .on_press(Message::Noop),
                "More options",
                None,
                Position::Top,
            ),
            {
                let target = target.clone();
                move |bounds| {
                    Message::Menu(crate::app::menu::MenuMsg::OpenAt(target.clone(), bounds.position()))
                }
            },
        );
        row![song_links::title(state, song, metrics::font_md(), t.foreground, (px - MORE).max(0.0)), more]
            .align_y(Alignment::Center)
            .into()
    };
    let artist_line = || song_links::artists(state, &song.artists, metrics::font_md(), t.muted, px);
    let meta = match &state.dj.caption {
        // The AI DJ is talking (or its line is shown as text).
        Some((line, spoken)) if *spoken && state.dj_airing() => column![
            row![
                icon(Icon::LuMicVocal, metrics::font_md(), t.primary),
                label_w(
                    format!("AI DJ · {}", state.dj_persona().name),
                    metrics::font_md(),
                    t.primary,
                    Weight::Medium
                ),
            ]
            .spacing(metrics::space_xs())
            .align_y(Alignment::Center),
            crate::ui::t_text(line.clone())
                .font(crate::appearance::current_font())
                .size(metrics::font_sm())
                .color(t.muted),
        ]
        .spacing(metrics::space_xs()),
        Some((line, false)) => column![
            title_line(),
            artist_line(),
            row![
                icon(Icon::LuMicVocal, metrics::font_sm(), t.primary),
                label(fit(line, metrics::font_sm()), metrics::font_sm(), t.primary),
            ]
            .spacing(metrics::space_xs())
            .align_y(Alignment::Center),
        ]
        .spacing(metrics::space_xs()),
        _ => {
            // During a DJ transition the third line names the song coming in.
            let mixing_into = matches!(state.mix.phase, crate::app::mix::Phase::Running)
                .then(|| state.mix.pair())
                .flatten()
                .filter(|(out, _)| *out == item.uid)
                .and_then(|(_, inn)| state.queue.iter().find(|i| i.uid == inn));
            let third: Element<'_, Message> = match mixing_into {
                Some(next) => row![
                    icon(Icon::LuShuffle, metrics::font_sm(), t.primary),
                    label(
                        fit(&format!("Mixing into “{}”", next.song.title), metrics::font_sm()),
                        metrics::font_sm(),
                        t.primary
                    ),
                ]
                .spacing(metrics::space_xs())
                .align_y(Alignment::Center)
                .into(),
                None => song_links::album(state, song, metrics::font_md(), t.muted, px),
            };
            column![title_line(), artist_line(), third].spacing(metrics::space_xs())
        }
    };
    let mut info = Row::new().align_y(Alignment::Center);
    if !state.config.settings.sidebar.cover_expanded {
        let art = tip(
            state,
            button(song_cover(state, song, metrics::PLAYER_ART, 0.0))
                .padding(0)
                .style(style::bare_button(t))
                .on_press(Message::ToggleFullscreen),
            "Open full-screen player",
            Some("toggleFullscreenPlayer"),
            Position::Top,
        );
        info = info.push(
            container(crate::ui::cover_toggle(state, art, metrics::PLAYER_ART, true)).padding(Padding {
                top: 12.0,
                right: 12.0,
                bottom: 12.0,
                left: 0.0,
            }),
        );
    }
    let info =
        crate::ui::with_menu(info.push(meta), crate::app::menu::MenuTarget::queue_item(song, item.uid));
    container(info)
        .padding(Padding { left: metrics::space_lg(), ..Padding::ZERO })
        .width(width)
        .height(Length::Fill)
        .center_y(Length::Fill)
        .clip(true)
        .into()
}
