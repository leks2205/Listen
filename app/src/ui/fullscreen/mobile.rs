//! The compact layout's full-screen player.

use super::lyrics::lyrics;
use crate::app::{FullscreenTab, Listen, Message};
use crate::i18n::tr;
use crate::icons::Icon;
use crate::style::{self, metrics};
use crate::ui::tip::{Position, tip, tip_raw};
use crate::ui::{label, label_w, song_cover};
use iced::font::Weight;
use iced::widget::{button, column, container, responsive, row, slider, space};
use iced::{Alignment, Length, Padding};

/// The compact layout's full-screen player (Feishin's MobileFullscreenPlayer): header, cover, metadata,
/// progress, controls and a bottom row (shuffle / repeat / queue / lyrics, the YouTube Music song/video
/// switch). Queue and lyrics swap the cover for their panel.
pub fn mobile(state: &Listen) -> iced::Element<'_, Message> {
    use listen_core::model::format_duration;
    use listen_core::queue::Repeat;
    let t = state.tokens;
    let Some(item) = state.queue.current() else {
        return container(
            column![
                crate::ui::tip::tip(
                    state,
                    button(crate::ui::icon(Icon::LuChevronDown, 20.0, t.foreground))
                        .padding(8)
                        .style(style::icon_button(t, false))
                        .on_press(Message::ToggleFullscreen),
                    "Minimize",
                    Some("toggleFullscreenPlayer"),
                    Position::Bottom,
                ),
                label("Nothing is playing.", metrics::font_lg(), t.muted),
            ]
            .align_x(Alignment::Center)
            .spacing(metrics::space_lg()),
        )
        .center(Length::Fill)
        .into();
    };
    let song = &item.song;
    let btn = |glyph: Icon, size: f32, active: bool, msg: Message, label: &str, action: Option<&str>| {
        tip_raw(
            state,
            button(
                container(crate::ui::icon(glyph, size, if active { t.primary } else { t.foreground }))
                    .center(Length::Fill),
            )
            .width(metrics::CONTROL + 4.0)
            .height(metrics::CONTROL + 4.0)
            .padding(0)
            .style(style::icon_button(t, active))
            .on_press(msg),
            tr(label),
            action,
            Position::Top,
        )
    };

    let header = row![
        btn(
            Icon::LuChevronDown,
            22.0,
            false,
            Message::ToggleFullscreen,
            "Minimize",
            Some("toggleFullscreenPlayer")
        ),
        space::horizontal(),
        label_w("Now playing", metrics::font_md(), t.muted, Weight::Medium),
        space::horizontal(),
        space().width(metrics::CONTROL + 4.0),
    ]
    .align_y(Alignment::Center);

    let main: iced::Element<'_, Message> = match state.mobile_panel {
        Some(FullscreenTab::Queue) => crate::ui::queue::list(state, state.window_size.width - 32.0),
        Some(FullscreenTab::Lyrics) => lyrics(state),
        _ => responsive(move |size| {
            let art = (size.width - 16.0).min(size.height - 90.0).max(60.0);
            let cover: iced::Element<'_, Message> = match &state.video.frame {
                Some(frame) if state.video.on => iced::widget::image(frame.clone())
                    .width(Length::Fixed(art))
                    .height(Length::Fixed(art))
                    .content_fit(iced::ContentFit::Contain)
                    .into(),
                _ => song_cover(state, song, art, metrics::radius_md()),
            };
            let text_px = size.width;
            column![
                cover,
                column![
                    label_w(
                        crate::ui::ellipsize_px(&song.title, text_px, 22.0),
                        22.0,
                        t.foreground,
                        Weight::Bold
                    ),
                    label(
                        crate::ui::ellipsize_px(&song.artist_line(), text_px, metrics::font_lg()),
                        metrics::font_lg(),
                        t.muted
                    ),
                    label(
                        crate::ui::ellipsize_px(
                            song.album.as_deref().unwrap_or_default(),
                            text_px,
                            metrics::font_md()
                        ),
                        metrics::font_md(),
                        t.muted
                    ),
                ]
                .align_x(Alignment::Center)
                .spacing(2),
            ]
            .spacing(metrics::space_md())
            .align_x(Alignment::Center)
            .width(Length::Fill)
            .into()
        })
        .into(),
    };

    let (position, duration) = (state.playback.position, state.duration());
    let seek = row![
        label(format_duration(position), metrics::font_xs(), t.muted),
        slider(0.0..=duration.as_secs_f32().max(1.0), position.as_secs_f32(), Message::Seek)
            .step(0.5_f32)
            .height(20)
            .style(style::bar_slider(t)),
        label(format_duration(duration), metrics::font_xs(), t.muted),
    ]
    .spacing(metrics::space_sm())
    .align_y(Alignment::Center);

    let play = button(
        container(crate::ui::icon(
            if state.playback.playing { Icon::LuPause } else { Icon::LuPlay },
            22.0,
            t.background,
        ))
        .center(Length::Fill),
    )
    .width(52)
    .height(52)
    .padding(0)
    .style(style::play_button(t))
    .on_press(Message::TogglePlay);
    let play = tip(
        state,
        play,
        if state.playback.playing { "Pause" } else { "Play" },
        Some("playPause"),
        Position::Top,
    );
    let transport = row![
        btn(Icon::LuSkipBack, 22.0, false, Message::Previous, "Previous track", Some("previous")),
        play,
        btn(Icon::LuSkipForward, 22.0, false, Message::Next, "Next track", Some("next")),
    ]
    .spacing(metrics::space_xl())
    .align_y(Alignment::Center);

    let repeat_icon = match state.queue.repeat {
        Repeat::One => Icon::RiRepeatOneLine,
        _ => Icon::RiRepeat2Line,
    };
    let mut bottom = row![
        {
            let mode = state.play_mode();
            let active = mode != crate::app::play_mode::PlayMode::Off;
            tip_raw(
                state,
                button(
                    container(crate::ui::mode_glyph(
                        mode,
                        metrics::font_xl(),
                        if active { t.primary } else { t.foreground },
                    ))
                    .center(Length::Fill),
                )
                .width(metrics::CONTROL + 4.0)
                .height(metrics::CONTROL + 4.0)
                .padding(0)
                .style(style::icon_button(t, active))
                .on_press(Message::ToggleShuffle),
                &crate::ui::tip::play_mode_tip(state),
                Some("toggleShuffle"),
                Position::Top,
            )
        },
        btn(
            repeat_icon,
            metrics::font_xl(),
            state.queue.repeat != Repeat::Off,
            Message::CycleRepeat,
            &crate::ui::tip::repeat_tip(state.queue.repeat),
            Some("toggleRepeat")
        ),
        btn(
            if state.mobile_panel == Some(FullscreenTab::Queue) {
                Icon::LuPanelRightClose
            } else {
                Icon::LuPanelRightOpen
            },
            metrics::font_xl(),
            state.mobile_panel == Some(FullscreenTab::Queue),
            Message::MobilePanel(FullscreenTab::Queue),
            "Queue",
            Some("toggleQueue")
        ),
        btn(
            Icon::LuMicVocal,
            metrics::font_xl(),
            state.mobile_panel == Some(FullscreenTab::Lyrics),
            Message::MobilePanel(FullscreenTab::Lyrics),
            "Lyrics",
            None
        ),
    ]
    .spacing(metrics::space_lg())
    .align_y(Alignment::Center);
    // The song/video switch for YouTube Music songs lives here in the compact layout.
    if state.has_video(song) {
        bottom = bottom.push(btn(
            Icon::LuVideo,
            metrics::font_xl(),
            state.video.on,
            Message::ToggleVideo,
            if state.video.on { "Show song" } else { "Show video" },
            None,
        ));
    }

    let base = t.primary;
    container(
        column![
            header,
            container(main).height(Length::Fill).width(Length::Fill),
            seek,
            container(transport).center_x(Length::Fill),
            container(bottom).center_x(Length::Fill),
        ]
        .spacing(metrics::space_md())
        .padding(Padding::from([metrics::space_md(), metrics::space_lg()])),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .style(move |_| container::Style {
        background: Some(iced::Background::Gradient(
            iced::gradient::Linear::new(iced::Radians(std::f32::consts::PI))
                .add_stop(0.0, style::darken(base, 0.55))
                .add_stop(1.0, style::darken(t.background, 0.3))
                .into(),
        )),
        text_color: Some(t.foreground),
        ..Default::default()
    })
    .into()
}
