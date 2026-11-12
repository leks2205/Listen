//! Fullscreen player (Feishin's full-screen player): cover and track info on
//! the left, one panel on the right (Up Next / Related / Lyrics / Visualizer,
//! each toggleable), the cover-derived background, the gear's settings and
//! the full-window visualizer.

mod about;
mod background;
pub mod lyrics;
pub mod metadata;
pub mod mobile;
mod related;
pub mod settings;

pub use lyrics::LYRICS_ID;
pub use mobile::mobile;

use super::{data_label, data_label_w, icon, label, song_cover};
use crate::app::fullscreen_state::FullscreenMsg;
use crate::app::{FullscreenTab, Listen, Message};
use crate::icons::Icon;
use crate::style::{self, metrics};
use iced::alignment::Horizontal;
use iced::font::Weight;
use iced::widget::{button, column, container, responsive, row, space, stack, tooltip};
use iced::{Alignment, Element, Length, Padding};
use listen_core::model::Song;
use listen_core::settings::ItemAlignment;

/// The panel tabs in button order: tab, icon, name.
pub const TABS: [(FullscreenTab, Icon, &str); 5] = [
    (FullscreenTab::Queue, Icon::LuListMusic, "Up Next"),
    (FullscreenTab::Related, Icon::LuRadio, "Related"),
    (FullscreenTab::Lyrics, Icon::LuMicVocal, "Lyrics"),
    (FullscreenTab::Visualizer, Icon::LuActivity, "Visualizer"),
    (FullscreenTab::About, Icon::LuInfo, "About"),
];

/// Padding of the full-screen content and the gap between the image column and the panel. Tests compute the
/// top bar's click points from `PAD`.
pub const PAD: Padding = Padding { top: 8.0, right: 32.0, bottom: 24.0, left: 16.0 };
const COLUMN_GAP: f32 = 48.0;
/// Room under the cover for title, artists, album and badges.
const INFO_H: f32 = 190.0;

/// Size of the image column (and of the panel beside it) in a `window`, as [`view`] lays it out:
/// minus the paddings, the top bar and, with a panel open, the gap and the panel's half.
pub fn image_column_size(window: iced::Size, panel_open: bool) -> iced::Size {
    let w = window.width - PAD.left - PAD.right;
    let w = if panel_open { (w - COLUMN_GAP) / 2.0 } else { w };
    let h = window.height - PAD.top - PAD.bottom - metrics::CONTROL - metrics::space_md();
    iced::Size::new(w.max(0.0), h.max(0.0))
}

/// The cover's side in an image column of `size`: `pct` of the width, leaving [`INFO_H`] below it.
pub fn art_side(size: iced::Size, pct: f32) -> f32 {
    (size.width * pct).min((size.height - INFO_H).max(0.0)).max(120.0).min(size.width)
}

pub fn view(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let panel_open = state.fullscreen_tab != FullscreenTab::Hidden;
    let body: Element<'_, Message> = match state.queue.current() {
        None => {
            container(label("Nothing is playing.", metrics::font_lg(), t.muted)).center(Length::Fill).into()
        }
        // A video is wide: it goes on top, the panel (or lyrics and visualizer side by side) below.
        Some(item) if panel_open && state.video_showing() => video_stack(state, &item.song),
        Some(item) if panel_open => row![
            container(image_column(state, &item.song)).width(Length::FillPortion(1)).height(Length::Fill),
            container(panel(state, image_column_size(state.window_size, true).width))
                .width(Length::FillPortion(1))
                .height(Length::Fill),
        ]
        .spacing(COLUMN_GAP)
        .into(),
        Some(item) => {
            container(image_column(state, &item.song)).width(Length::Fill).height(Length::Fill).into()
        }
    };
    let content = container(column![top_bar(state), body].spacing(metrics::space_md()))
        .padding(PAD)
        .width(Length::Fill)
        .height(Length::Fill);
    stack![background::view(state), content].width(Length::Fill).height(Length::Fill).into()
}

/// Minimize and gear on the left, the panel toggles on the right.
fn top_bar(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let tip = |content: Element<'static, Message>, text: &'static str, action: Option<&'static str>| {
        crate::ui::tip::tip(state, content, text, action, tooltip::Position::Bottom)
    };
    let btn = |glyph: Icon, active: bool, msg: Message| -> Element<'static, Message> {
        button(
            container(icon(glyph, 20.0, if active { t.primary } else { t.foreground })).center(Length::Fill),
        )
        .width(metrics::CONTROL)
        .height(metrics::CONTROL)
        .padding(0)
        .style(style::icon_button(t, active))
        .on_press(msg)
        .into()
    };
    let mut tabs = iced::widget::Row::new().spacing(metrics::space_xs()).align_y(Alignment::Center);
    for (tab, glyph, name) in TABS {
        let active = state.fs_tab_active(tab);
        tabs = tabs.push(tip(
            btn(glyph, active, Message::Fullscreen(FullscreenMsg::TogglePanel(tab))),
            name,
            None,
        ));
    }
    row![
        tip(
            btn(Icon::LuChevronDown, false, Message::ToggleFullscreen),
            "Minimize",
            Some("toggleFullscreenPlayer")
        ),
        tip(
            btn(Icon::RiSettings2Line, false, Message::Fullscreen(FullscreenMsg::OpenSettings)),
            "Configure",
            None
        ),
        space::horizontal(),
        tabs,
    ]
    .spacing(metrics::space_xs())
    .align_y(Alignment::Center)
    .into()
}

/// The cover (or the video) with the track info under it.
fn image_column<'a>(state: &'a Listen, song: &'a Song) -> Element<'a, Message> {
    let s = &state.config.settings.fullscreen;
    let (h_align, x_align) = match s.alignment {
        ItemAlignment::Left => (Horizontal::Left, Alignment::Start),
        ItemAlignment::Center => (Horizontal::Center, Alignment::Center),
        ItemAlignment::Right => (Horizontal::Right, Alignment::End),
    };
    let pct = f32::from(s.cover_size_clamped()) / 100.0;
    responsive(move |size| {
        let art = art_side(size, pct);
        let cover: Element<'_, Message> = match &state.video.frame {
            Some(frame) if state.video.on => {
                let (w, h) = video_box(size.width, size.height - INFO_H);
                iced::widget::image(frame.clone())
                    .width(Length::Fixed(w))
                    .height(Length::Fixed(h))
                    .content_fit(iced::ContentFit::Contain)
                    .into()
            }
            _ => cover(state, song, art),
        };
        container(
            column![cover, metadata::view(state, song, art.max(240.0).min(size.width), h_align)]
                .spacing(metrics::space_lg())
                .align_x(x_align)
                .width(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(h_align)
        .center_y(Length::Fill)
        .into()
    })
    .into()
}

/// The video's box: the frames are 16:9, so it takes the full width the room allows instead of the
/// cover's square, which left wide bands above and below the picture.
pub(crate) fn video_box(width: f32, height: f32) -> (f32, f32) {
    let w = width.min(height.max(0.0) * 16.0 / 9.0).max(120.0).min(width.max(0.0));
    (w, w * 9.0 / 16.0)
}

/// The cover at `art` px; with "use image aspect ratio" the whole image fits in the square.
fn cover<'a>(state: &Listen, song: &Song, art: f32) -> Element<'a, Message> {
    if state.config.settings.fullscreen.use_image_aspect_ratio && !state.demo {
        let handle =
            song.image.as_ref().and_then(|i| state.images.get(&state.image_key_on(&song.server_id, i)));
        if let Some(handle) = handle {
            return iced::widget::image(handle.clone())
                .width(Length::Fixed(art))
                .height(Length::Fixed(art))
                .content_fit(iced::ContentFit::Contain)
                .border_radius(metrics::radius_md())
                .into();
        }
    }
    song_cover(state, song, art, metrics::radius_md())
}

/// The open panel's content; `queue_width` is the room the queue list gets.
/// The open tab's panel, `width` px wide.
fn panel(state: &Listen, width: f32) -> Element<'_, Message> {
    match state.fullscreen_tab {
        FullscreenTab::Queue => super::queue::list(state, width),
        FullscreenTab::Related => related::view(state, width),
        FullscreenTab::Lyrics => lyrics::lyrics(state),
        FullscreenTab::Visualizer => visualizer_panel(state),
        FullscreenTab::About => about::view(state),
        FullscreenTab::Hidden => space().into(),
    }
}

/// The visualizer with its expand button.
fn visualizer_panel(state: &Listen) -> Element<'_, Message> {
    let expand = crate::ui::tip::tip(
        state,
        button(icon(Icon::LuChevronUp, 20.0, state.tokens.foreground))
            .padding(8)
            .style(style::icon_button(state.tokens, false))
            .on_press(Message::Fullscreen(FullscreenMsg::ExpandVisualizer)),
        "Expand visualizer",
        None,
        tooltip::Position::Left,
    );
    stack![visualizer_surface(state), container(expand).width(Length::Fill).align_x(Horizontal::Right)].into()
}

/// A playing video with a panel open: the video on top at full width, title and artists under it,
/// then the panel; with lyrics and visualizer both on, they sit side by side.
fn video_stack<'a>(state: &'a Listen, song: &'a Song) -> Element<'a, Message> {
    let t = state.tokens;
    responsive(move |size| {
        // The video takes at most a bit more than half the height, so the panel keeps room.
        let (w, h) = video_box(size.width, size.height * 0.55);
        let picture: Element<'_, Message> = match &state.video.frame {
            Some(frame) => iced::widget::image(frame.clone())
                .width(Length::Fixed(w))
                .height(Length::Fixed(h))
                .content_fit(iced::ContentFit::Contain)
                .into(),
            None => space().into(),
        };
        let info = row![
            data_label_w(
                super::ellipsize_px(&song.title, w * 0.6, metrics::font_xl()),
                metrics::font_xl(),
                t.foreground,
                Weight::Bold
            ),
            data_label(
                super::ellipsize_px(&song.artist_line(), w * 0.35, metrics::font_lg()),
                metrics::font_lg(),
                t.muted
            ),
        ]
        .spacing(metrics::space_md())
        .align_y(Alignment::Center);
        let below: Element<'_, Message> = if state.fs_visualizer_beside_lyrics() {
            row![
                container(lyrics::lyrics(state)).width(Length::FillPortion(1)).height(Length::Fill),
                container(visualizer_panel(state)).width(Length::FillPortion(1)).height(Length::Fill),
            ]
            .spacing(COLUMN_GAP)
            .into()
        } else {
            panel(state, size.width)
        };
        column![
            container(picture).width(Length::Fill).align_x(Horizontal::Center),
            container(info).width(Length::Fill).align_x(Horizontal::Center),
            container(below).width(Length::Fill).height(Length::Fill),
        ]
        .spacing(metrics::space_md())
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    })
    .into()
}

/// The spectrum (or why there is none), filling its space.
fn visualizer_surface(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    if state.milkdrop_active() {
        return super::milkdrop::surface(state, true);
    }
    match &state.spectrum {
        Some(frame) => super::visualizer::view(frame, &state.config.settings.visualizer),
        None => container(label(
            state.visualizer_error.clone().unwrap_or_else(|| "Starting the visualizer…".into()),
            metrics::font_md(),
            t.muted,
        ))
        .center(Length::Fill)
        .into(),
    }
}

/// The visualizer over the whole window (Feishin's `visualizerExpanded`): the
/// song in a corner, a shrink button back to the player or the page.
pub fn visualizer_overlay(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let shrink = crate::ui::tip::tip(
        state,
        button(icon(Icon::LuChevronDown, 22.0, iced::Color::WHITE))
            .padding(10)
            .style(style::art_overlay_button(t))
            .on_press(Message::Fullscreen(FullscreenMsg::ShrinkVisualizer)),
        "Shrink visualizer",
        None,
        tooltip::Position::Left,
    );
    let mut layers = stack![
        container(space()).width(Length::Fill).height(Length::Fill).style(|_| container::Style {
            background: Some(iced::Background::Color(iced::Color::BLACK)),
            ..Default::default()
        }),
        container(visualizer_surface(state))
            .padding(Padding::from([0.0, 0.0]))
            .width(Length::Fill)
            .height(Length::Fill),
        container(shrink).padding(16).width(Length::Fill).align_x(Horizontal::Right),
    ];
    if let Some(item) = state.queue.current() {
        let song = &item.song;
        let info = column![
            data_label_w(
                crate::ui::ellipsize_px(&song.title, 420.0, 22.0),
                22.0,
                iced::Color::WHITE,
                Weight::Bold
            ),
            data_label(
                crate::ui::ellipsize_px(&song.artist_line(), 420.0, metrics::font_lg()),
                metrics::font_lg(),
                style::alpha(iced::Color::WHITE, 0.75)
            ),
        ]
        .spacing(2);
        layers = layers.push(
            container(info)
                .padding(24)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_y(iced::alignment::Vertical::Bottom),
        );
    }
    layers.width(Length::Fill).height(Length::Fill).into()
}

#[cfg(test)]
mod geometry_tests {
    use super::*;

    #[test]
    fn image_column_size_subtracts_what_the_view_lays_out() {
        let window = iced::Size::new(1600.0, 900.0);
        let top = metrics::CONTROL + metrics::space_md();
        assert_eq!(image_column_size(window, false), iced::Size::new(1552.0, 868.0 - top));
        assert_eq!(image_column_size(window, true).width, (1552.0 - 48.0) / 2.0);
    }
}
