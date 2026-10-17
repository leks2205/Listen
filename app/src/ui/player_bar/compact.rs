//! Compact player bar (window <= 768 px), Feishin's mobile bar: a 90 px `1fr auto` grid with
//! cover, title, artists and album on the left and transport buttons on the right,
//! plus a speaker button that opens a volume slider above it.

use crate::app::menu::MenuTarget;
use crate::app::{Listen, Message};
use crate::icons::Icon;
use crate::style::{self, metrics};
use crate::ui::tip::{Position, tip};
use crate::ui::{icon, label, song_cover, song_links, with_menu};
use iced::widget::{button, column, container, mouse_area, row, slider, space};
use iced::{Alignment, Element, Length, Padding, Point, Size};

/// Four 36 px buttons (speaker, previous, play, next), three 4 px gaps, 8 px padding each side.
const RIGHT_WIDTH: f32 = 4.0 * metrics::CONTROL + 3.0 * metrics::SPACE_XS + 2.0 * metrics::SPACE_SM;
/// Width of the text column, from the bar's `width`.
fn text_width(width: f32) -> f32 {
    (width - metrics::space_md() - metrics::PLAYER_ART - metrics::space_md() - RIGHT_WIDTH).max(0.0)
}

/// Size of the volume popover (slider + number).
pub const POPOVER: Size = Size::new(144.0, 24.0);

/// Height of the seek bar's strip.
const SEEK_HEIGHT: f32 = 14.0;

/// Width of the horizontal slider track.
const SLIDER_WIDTH: f32 = 100.0;

/// Horizontal centre of the speaker icon in a bar `width` px wide.
fn speaker_center_x(width: f32) -> f32 {
    width - metrics::space_sm() - 3.0 * (metrics::CONTROL + metrics::space_xs()) - metrics::CONTROL / 2.0
}

/// Top-left of the volume popover: above the speaker icon, kept inside the window.
pub fn popover_position(window: Size) -> Point {
    // The speaker button is vertically centred in the bar.
    let speaker_top =
        window.height - metrics::PLAYER_BAR_HEIGHT + (metrics::PLAYER_BAR_HEIGHT - metrics::CONTROL) / 2.0;
    let x = (speaker_center_x(window.width) - POPOVER.width / 2.0)
        .min(window.width - POPOVER.width - 4.0)
        .max(4.0);
    Point::new(x, (speaker_top - POPOVER.height - 2.0).max(0.0))
}

/// The horizontal volume slider box (wheel over it changes the volume too).
pub fn popover_panel(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let volume = if state.playback.muted { 0 } else { state.playback.volume };
    let bar = slider(0.0..=100.0, f32::from(volume), Message::Volume)
        .width(SLIDER_WIDTH)
        .height(14)
        .style(style::bar_slider(t));
    let body = row![bar, container(label(super::volume_text(state), metrics::font_xs(), t.muted)).width(26)]
        .spacing(6)
        .align_y(Alignment::Center);
    let boxed = container(super::wheel_volume(body))
        .width(POPOVER.width)
        .height(POPOVER.height)
        .padding(Padding::from([0.0, 8.0]))
        .center_y(POPOVER.height)
        .style(move |_| container::Style {
            background: Some(iced::Background::Color(t.background_alt)),
            border: iced::border::rounded(POPOVER.height / 2.0).color(t.border).width(1),
            shadow: iced::Shadow {
                color: style::alpha(iced::Color::BLACK, 0.35),
                offset: iced::Vector::new(0.0, 2.0),
                blur_radius: 8.0,
            },
            ..Default::default()
        });
    iced::widget::opaque(boxed)
}

pub fn view(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let width = state.window_size.width;
    let current = state.queue.current();
    let has_track = current.is_some();

    let ctl = |glyph: Icon, msg: Message, label: &str, action: &str| {
        tip(
            state,
            button(container(icon(glyph, metrics::font_xl(), t.foreground)).center(Length::Fill))
                .width(metrics::CONTROL)
                .height(metrics::CONTROL)
                .padding(0)
                .style(style::icon_button(t, false))
                .on_press_maybe(has_track.then_some(msg)),
            label,
            Some(action),
            Position::Top,
        )
    };
    let volume = if state.playback.muted { 0 } else { state.playback.volume };
    let speaker = match volume {
        0 => Icon::LuVolumeX,
        1..=50 => Icon::LuVolume1,
        _ => Icon::LuVolume2,
    };
    let speaker = mouse_area(tip(
        state,
        button(
            container(icon(
                speaker,
                metrics::font_xl(),
                if state.playback.muted { t.muted } else { t.foreground },
            ))
            .center(Length::Fill),
        )
        .width(metrics::CONTROL)
        .height(metrics::CONTROL)
        .padding(0)
        .style(style::icon_button(t, state.volume_popup))
        .on_press(Message::ToggleVolumePopup),
        "Volume",
        Some("volumeMute"),
        Position::Top,
    ))
    .on_right_press(Message::ToggleMute);
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
    let controls = container(
        row![
            super::wheel_volume(speaker),
            ctl(Icon::LuSkipBack, Message::Previous, "Previous track", "previous"),
            play,
            ctl(Icon::LuSkipForward, Message::Next, "Next track", "next"),
        ]
        .spacing(metrics::space_xs())
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([0.0, metrics::space_sm()]))
    .center_y(Length::Fill);

    let info: Element<'_, Message> = match current {
        None => space::horizontal().into(),
        Some(item) => {
            let song = &item.song;
            let target = MenuTarget::queue_item(song, item.uid);
            let text_px = text_width(width);
            let cover = song_cover(state, song, metrics::PLAYER_ART, 4.0);
            let meta = column![
                container(song_links::title(state, song, metrics::font_md(), t.foreground, text_px))
                    .width(text_px.max(0.0))
                    .clip(true),
                container(song_links::artists(state, &song.artists, metrics::font_sm(), t.muted, text_px))
                    .width(text_px.max(0.0))
                    .clip(true),
                container(song_links::album(state, song, metrics::font_sm(), t.muted, text_px))
                    .width(text_px.max(0.0))
                    .clip(true),
            ];
            with_menu(row![cover, meta].spacing(metrics::space_md()).align_y(Alignment::Center), target)
        }
    };
    let main = row![
        container(info)
            .padding(Padding { left: metrics::space_md(), ..Padding::ZERO })
            .width(Length::Fill)
            .height(Length::Fill)
            .center_y(Length::Fill)
            .clip(true),
        controls
    ]
    .height(Length::Fill);
    column![seek_bar(state), main].into()
}

/// Thin seek bar along the top of the bar; it seeks on release like the main one. Hidden (the
/// space stays) for radio streams and tracks without a length.
fn seek_bar(state: &Listen) -> Element<'_, Message> {
    let duration = state.duration();
    if state.queue.current().is_none() || state.radio_active() || duration.is_zero() {
        return space::vertical().height(SEEK_HEIGHT).into();
    }
    let shown = state.seek_preview.unwrap_or(state.playback.position.as_secs_f32());
    container(
        slider(0.0..=duration.as_secs_f32().max(1.0), shown, Message::SeekPreview)
            .on_release(Message::SeekRelease)
            .step(0.5_f32)
            .height(SEEK_HEIGHT)
            .style(style::bar_slider(state.tokens)),
    )
    .padding(Padding::from([0.0, metrics::space_md()]))
    .into()
}
