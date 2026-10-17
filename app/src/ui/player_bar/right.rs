//! Right column: toggles, mute and the volume slider.

use crate::app::audio::{AudioMsg, Popup};
use crate::app::fullscreen_state::FullscreenMsg;
use crate::app::sleep_timer::{SleepMode, SleepMsg, format_remaining};
use crate::app::{FullscreenTab, Listen, Message};
use crate::icons::Icon;
use crate::style::{self, metrics};
use crate::ui::tip::{Position, tip, tip_raw};
use crate::ui::{icon, label_w};
use iced::font::Weight;
use iced::widget::{Column, Row, button, container, mouse_area, row, slider, tooltip};
use iced::{Alignment, Element, Length, Padding};

/// Whether the current song has the song/video switch (YouTube Music only).
fn has_video_switch(state: &Listen) -> bool {
    !state.dlna_casting() && state.queue.current().is_some_and(|i| state.has_video(&i.song))
}

/// Room the sleep timer's countdown ("1:02:05") takes next to its icon.
const COUNTDOWN_W: f32 = 56.0;

/// Whether the sleep timer button shows a countdown.
fn countdown(state: &Listen) -> Option<std::time::Duration> {
    state.sleep_timer.as_ref().filter(|t| t.mode == SleepMode::Timed).map(|t| t.remaining)
}

/// Width of the star row above the buttons.
const STARS_W: f32 = 5.0 * 22.0;

/// Minimum width of the column, so the volume slider stays on screen.
pub(super) fn intrinsic_width(state: &Listen) -> f32 {
    let mut width = if has_video_switch(state) { metrics::CONTROL + metrics::space_xs() } else { 0.0 };
    if state.dlna_visible() {
        width += metrics::CONTROL + metrics::space_xs();
    }
    for id in state.config.settings.player_bar.right_controls_ordered() {
        width += match id {
            "volume" => {
                metrics::CONTROL
                    + metrics::space_xs()
                    + f32::from(state.config.settings.player_bar.volume_width)
                    + metrics::space_xs()
            }
            "favorite" if !state.show_favorites() => 0.0,
            "sleep_timer" if countdown(state).is_some() => {
                metrics::CONTROL + metrics::space_xs() + COUNTDOWN_W
            }
            _ => metrics::CONTROL + metrics::space_xs(),
        };
    }
    width.max(if state.can_rate_current() { STARS_W } else { 0.0 }) + 2.0 * metrics::space_lg()
}

/// Five stars above the buttons: a click rates the playing song, the same star again clears it.
fn stars(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let rating = state.queue.current().and_then(|i| i.song.rating).unwrap_or(0);
    let mut out = Row::new().align_y(Alignment::Center);
    for n in 1..=5u8 {
        let on = n <= rating;
        let label = crate::ui::tip::rate_text(n);
        out = out.push(tip_raw(
            state,
            button(icon(
                if on { Icon::LuStarFill } else { Icon::LuStar },
                metrics::font_md(),
                if on { t.primary } else { t.muted },
            ))
            .padding(3)
            .style(style::icon_button(t, false))
            .on_press(Message::Fullscreen(FullscreenMsg::Rate(n))),
            &label,
            Some(&format!("rate{n}")),
            Position::Top,
        ));
    }
    out.into()
}

pub(super) fn view(state: &Listen, width: f32) -> Element<'_, Message> {
    let t = state.tokens;
    let favorite = state.queue.current().is_some_and(|i| i.song.favorite);
    let small = |glyph: Icon, size: f32, active: bool, msg: Message, label: &str, action: Option<&str>| {
        tip(
            state,
            button(
                container(icon(glyph, size, if active { t.primary } else { t.foreground }))
                    .center(Length::Fill),
            )
            .width(metrics::CONTROL)
            .height(metrics::CONTROL)
            .padding(0)
            .style(style::icon_button(t, active))
            .on_press(msg),
            label,
            action,
            Position::Top,
        )
    };
    let in_tab = |tab| state.fullscreen && state.fullscreen_tab == tab;
    let volume = if state.playback.muted { 0 } else { state.playback.volume };
    let volume_icon = match volume {
        0 => Icon::LuVolumeX,
        1..=50 => Icon::LuVolume1,
        _ => Icon::LuVolume2,
    };
    // While casting the control is the device's volume: tinted, and named in the tooltip.
    let volume_color = if state.playback.muted {
        t.muted
    } else if state.dlna_casting() {
        t.primary
    } else {
        t.foreground
    };
    let volume_control = || {
        let row = row![
            // Right-click: the audio output device menu.
            mouse_area(tip(
                state,
                button(container(icon(volume_icon, metrics::font_xl(), volume_color)).center(Length::Fill))
                    .width(metrics::CONTROL)
                    .height(metrics::CONTROL)
                    .padding(0)
                    .style(style::icon_button(t, false))
                    .on_press(Message::ToggleMute),
                match (state.dlna_casting(), state.playback.muted) {
                    (true, true) => "Unmute the device",
                    (true, false) => "Mute the device",
                    (false, true) => "Unmute",
                    (false, false) => "Mute",
                },
                Some("volumeMute"),
                Position::Top,
            ))
            .on_right_press(Message::Audio(AudioMsg::OpenDeviceMenu)),
            tooltip(
                slider(0.0..=100.0, f32::from(volume), Message::Volume)
                    .width(f32::from(state.config.settings.player_bar.volume_width))
                    .height(20)
                    .style(style::bar_slider(t)),
                container(crate::ui::label(super::volume_text(state), metrics::font_sm(), t.foreground))
                    .padding(6)
                    .style(move |_| container::Style {
                        background: Some(iced::Background::Color(t.background_alt)),
                        border: iced::border::rounded(4).color(t.border).width(1),
                        ..Default::default()
                    }),
                tooltip::Position::Top,
            ),
        ]
        .spacing(metrics::space_xs())
        .align_y(Alignment::Center);
        super::wheel_volume(row)
    };
    // The sleep timer button shows the countdown of a timed timer.
    let sleep_button = || -> Element<'_, Message> {
        let active = state.sleep_timer.is_some() || state.fs.sleep_ui.open;
        let color = if active { t.primary } else { t.foreground };
        let mut content = Row::new().align_y(Alignment::Center).spacing(metrics::space_xs());
        content = content.push(icon(Icon::LuTimer, metrics::font_lg(), color));
        let (width, content): (Length, Element<'_, Message>) = match countdown(state) {
            Some(left) => (
                Length::Fixed(metrics::CONTROL + COUNTDOWN_W),
                content
                    .push(label_w(format_remaining(left), metrics::font_sm(), color, Weight::Semibold))
                    .into(),
            ),
            None => (Length::Fixed(metrics::CONTROL), content.into()),
        };
        tip(
            state,
            button(container(content).center(Length::Fill))
                .width(width)
                .height(metrics::CONTROL)
                .padding(0)
                .style(style::icon_button(t, active))
                .on_press(Message::Fullscreen(FullscreenMsg::Sleep(SleepMsg::Toggle))),
            "Sleep timer",
            None,
            Position::Top,
        )
    };

    let mut controls = Row::new().spacing(metrics::space_xs()).align_y(Alignment::Center);
    // The song/video switch: only for YouTube Music songs, off by default.
    if has_video_switch(state) {
        controls = controls.push(small(
            Icon::LuVideo,
            metrics::font_lg(),
            state.video.on,
            Message::ToggleVideo,
            if state.video.on { "Show song" } else { "Show video" },
            None,
        ));
    }
    // The DLNA cast button, when renderers were found (or one is in use).
    if state.dlna_visible() {
        controls = controls.push(crate::ui::dlna::button_view(state));
    }
    for id in state.config.settings.player_bar.right_controls_ordered() {
        controls = match id {
            "sleep_timer" => controls.push(sleep_button()),
            "player_config" => controls.push(small(
                Icon::LuSlidersHorizontal,
                metrics::font_lg(),
                state.audio.popup == Some(Popup::Config),
                Message::Audio(AudioMsg::TogglePopover),
                "Player options",
                None,
            )),
            "visualizer" => controls.push(small(
                Icon::LuActivity,
                metrics::font_lg(),
                in_tab(FullscreenTab::Visualizer),
                Message::OpenFullscreenTab(FullscreenTab::Visualizer),
                "Visualizer",
                None,
            )),
            "lyrics" => controls.push(small(
                Icon::LuMicVocal,
                metrics::font_lg(),
                in_tab(FullscreenTab::Lyrics),
                Message::OpenFullscreenTab(FullscreenTab::Lyrics),
                "Lyrics",
                None,
            )),
            // Hidden with Settings > Show favourites off.
            "favorite" if state.show_favorites() => controls.push(small(
                Icon::LuHeart,
                metrics::font_lg(),
                favorite,
                Message::ToggleFavorite,
                if favorite { "Remove from favorites" } else { "Add to favorites" },
                Some("favoriteCurrentToggle"),
            )),
            "queue" => controls.push(small(
                if state.show_queue { Icon::LuPanelRightClose } else { Icon::LuPanelRightOpen },
                metrics::font_lg(),
                state.show_queue,
                Message::ToggleQueue,
                "Queue",
                Some("toggleQueue"),
            )),
            "volume" => controls.push(volume_control()),
            _ => controls,
        };
    }

    let mut column = Column::new().spacing(0).align_x(iced::alignment::Horizontal::Right);
    // Rating stars (Navidrome / Subsonic; never YouTube Music) above the buttons.
    if state.can_rate_current() {
        column = column.push(stars(state));
    }
    let column = column.push(controls);

    container(column)
        .padding(Padding::from([metrics::space_sm(), metrics::space_lg()]))
        .width(width)
        .height(Length::Fill)
        .align_right(Length::Fill)
        .center_y(Length::Fill)
        .into()
}
