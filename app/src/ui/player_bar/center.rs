//! Center column: transport buttons + seek bar.

use crate::app::fullscreen_state::FullscreenMsg;
use crate::app::{Listen, Message};
use crate::i18n::tr;
use crate::icons::Icon;
use crate::style::{self, metrics};
use crate::ui::tip::{Position, play_mode_tip, repeat_tip, tip, tip_raw};
use crate::ui::{icon, label_w};
use iced::font::Weight;
use iced::widget::{Row, button, column, container, row, slider, space};
use iced::{Alignment, Element, Length};
use listen_core::model::format_duration;
use listen_core::queue::Repeat;
use std::time::Duration;

/// How many transport buttons the narrow bar gives up: 0 none, 1 the
/// +/-10 s skips, 2 also stop and "shuffle all" (volume never goes).
pub const MAX_SQUEEZE: u8 = 2;

/// Narrowest the centre column may get at `squeeze`: its transport buttons side by side.
pub(super) fn min_width(state: &Listen, squeeze: u8) -> f32 {
    let skips = state.config.settings.player_bar.skip_buttons && squeeze == 0;
    let n = if squeeze >= 2 {
        5.0
    } else if skips {
        9.0
    } else {
        7.0
    };
    n * metrics::CONTROL + (n - 1.0) * metrics::space_sm()
}

use crate::app::play_mode::PlayMode;

pub(super) fn view(state: &Listen, width: f32, squeeze: u8) -> Element<'_, Message> {
    let t = state.tokens;
    let s = &state.config.settings.player_bar;
    let size = f32::from(s.button_size);
    let radio = state.radio_active();
    // A radio stream has only Stop and Play (Feishin `isRadioActive`).
    let has_track = state.queue.current().is_some() && !radio;

    let ctl_if = |glyph: Icon,
                  icon_size: f32,
                  active: bool,
                  msg: Message,
                  enabled: bool,
                  label: &str,
                  action: Option<&str>| {
        tip_raw(
            state,
            button(
                container(icon(glyph, icon_size, if active { t.primary } else { t.foreground }))
                    .center(Length::Fill),
            )
            .width(metrics::CONTROL)
            .height(metrics::CONTROL)
            .padding(0)
            .style(style::icon_button(t, active))
            .on_press_maybe(enabled.then_some(msg)),
            label,
            action,
            Position::Top,
        )
    };
    let ctl = |glyph: Icon, icon_size: f32, active: bool, msg: Message, label: &str, action: Option<&str>| {
        ctl_if(glyph, icon_size, active, msg, has_track, tr(label), action)
    };

    let mode = state.play_mode();
    let repeat_icon = match state.queue.repeat {
        Repeat::One => Icon::RiRepeatOneLine,
        _ => Icon::RiRepeat2Line,
    };
    let play = button(
        container(icon(
            if state.playback.playing { Icon::LuPause } else { Icon::LuPlay },
            metrics::font_lg(),
            t.background,
        ))
        .center(Length::Fill),
    )
    .width(metrics::CONTROL)
    .height(metrics::CONTROL)
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

    let skips = s.skip_buttons && squeeze == 0;
    let mut controls = Row::new().spacing(metrics::space_sm()).align_y(Alignment::Center);
    // A radio stream keeps Stop even in the narrowest bar.
    if squeeze < 2 || radio {
        controls = controls.push(ctl_if(
            Icon::LuSquare,
            size - 2.0,
            false,
            Message::Stop,
            has_track || radio,
            tr("Stop"),
            Some("stop"),
        ));
    }
    controls = controls
        .push({
            let active = mode != PlayMode::Off;
            tip_raw(
                state,
                button(
                    container(crate::ui::mode_glyph(
                        mode,
                        size,
                        if active { t.primary } else { t.foreground },
                    ))
                    .center(Length::Fill),
                )
                .width(metrics::CONTROL)
                .height(metrics::CONTROL)
                .padding(0)
                .style(style::icon_button(t, active))
                .on_press_maybe(has_track.then_some(Message::ToggleShuffle)),
                &play_mode_tip(state),
                Some("toggleShuffle"),
                Position::Top,
            )
        })
        .push(ctl(Icon::LuSkipBack, size, false, Message::PreviousClick, "Previous track", Some("previous")));
    if skips {
        controls = controls.push(ctl(
            Icon::LuStepBack,
            size,
            false,
            Message::SkipBackward,
            "Skip backward",
            Some("skipBackward"),
        ));
    }
    controls = controls.push(play);
    if skips {
        controls = controls.push(ctl(
            Icon::LuStepForward,
            size,
            false,
            Message::SkipForward,
            "Skip forward",
            Some("skipForward"),
        ));
    }
    controls = controls
        .push(ctl(Icon::LuSkipForward, size, false, Message::NextClick, "Next track", Some("next")))
        .push(ctl_if(
            repeat_icon,
            size,
            state.queue.repeat != Repeat::Off,
            Message::CycleRepeat,
            has_track,
            &repeat_tip(state.queue.repeat),
            Some("toggleRepeat"),
        ));
    if squeeze < 2 {
        controls = controls.push(ctl(
            Icon::RiPlayListAddLine,
            size,
            false,
            Message::ShuffleAll,
            "Shuffle all songs",
            None,
        ));
    }

    let (position, duration) = match state.queue.current() {
        Some(_) => (state.playback.position, state.duration()),
        None => Default::default(),
    };
    let time = |d| label_w(format_duration(d), metrics::font_xs(), t.muted, Weight::Semibold);
    // While dragging, the slider and the time label show the target; the seek is sent on release.
    let shown = state.seek_preview.map_or(position, Duration::from_secs_f32);
    // The waveform seek bar when the slider type is "waveform" and the playing song's
    // waveform is loaded; the plain slider otherwise (ui/waveform.rs).
    let seek: Element<'_, Message> =
        crate::ui::waveform::seek_bar(state, shown, duration).unwrap_or_else(|| {
            slider(0.0..=duration.as_secs_f32().max(1.0), shown.as_secs_f32(), Message::SeekPreview)
                .on_release(Message::SeekRelease)
                .step(0.5_f32)
                .height(20)
                .style(style::bar_slider(t))
                .into()
        });
    // The right label is the total, or the time left ("-m:ss") after a click on it.
    let remaining = s.show_time_remaining;
    let end_text = if remaining {
        format!("-{}", format_duration(duration.saturating_sub(shown)))
    } else {
        format_duration(duration)
    };
    let end = button(label_w(end_text, metrics::font_xs(), t.muted, Weight::Semibold))
        .padding(0)
        .style(style::bare_button(t))
        .on_press_maybe(has_track.then_some(Message::Fullscreen(FullscreenMsg::ToggleRemaining)));
    let seek_row = row![container(time(shown)).width(76).align_right(76), seek, container(end).width(76),]
        .spacing(metrics::space_sm())
        .align_y(Alignment::Center)
        .width(Length::FillPortion(95));

    container(
        column![
            container(controls).height(35).center_y(35),
            row![
                space::horizontal().width(Length::FillPortion(2)),
                seek_row,
                space::horizontal().width(Length::FillPortion(3))
            ],
        ]
        .spacing(metrics::space_sm())
        .align_x(Alignment::Center),
    )
    .width(width)
    .height(Length::Fill)
    .clip(true)
    .center_x(width)
    .center_y(Length::Fill)
    .into()
}
