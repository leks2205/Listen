//! Fullscreen player (Feishin's full-screen player): cover and track info on
//! the left, one panel on the right (Up Next / Related / Lyrics / Visualizer,
//! each toggleable), the cover-derived background, the gear's settings and
//! the full-window visualizer.

mod background;
pub mod lyrics;
pub mod metadata;
pub mod mobile;
mod related;
pub mod settings;

pub use lyrics::{LYRICS_ID, lyric_line_height};
pub use mobile::mobile;

use super::{icon, label, label_w, song_cover};
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
pub const TABS: [(FullscreenTab, Icon, &str); 4] = [
    (FullscreenTab::Queue, Icon::LuListMusic, "Up Next"),
    (FullscreenTab::Related, Icon::LuRadio, "Related"),
    (FullscreenTab::Lyrics, Icon::LuMicVocal, "Lyrics"),
    (FullscreenTab::Visualizer, Icon::LuActivity, "Visualizer"),
];

pub fn view(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let panel_open = state.fullscreen_tab != FullscreenTab::Hidden;
    let body: Element<'_, Message> = match state.queue.current() {
        None => {
            container(label("Nothing is playing.", metrics::font_lg(), t.muted)).center(Length::Fill).into()
        }
        Some(item) if panel_open => row![
            container(image_column(state, &item.song)).width(Length::FillPortion(1)).height(Length::Fill),
            container(panel(state)).width(Length::FillPortion(1)).height(Length::Fill),
        ]
        .spacing(48)
        .into(),
        Some(item) => {
            container(image_column(state, &item.song)).width(Length::Fill).height(Length::Fill).into()
        }
    };
    let content = container(column![top_bar(state), body].spacing(metrics::space_md()))
        .padding(Padding { top: 8.0, right: 32.0, bottom: 24.0, left: 16.0 })
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
        let active = state.fullscreen_tab == tab;
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
    let pct = f32::from(s.cover_size) / 100.0;
    responsive(move |size| {
        // Room under the cover for title, artists, album and badges.
        let art = (size.width * pct).min((size.height - 190.0).max(0.0)).max(120.0).min(size.width);
        let cover: Element<'_, Message> = match &state.video.frame {
            Some(frame) if state.video.on => iced::widget::image(frame.clone())
                .width(Length::Fixed(art))
                .height(Length::Fixed(art))
                .content_fit(iced::ContentFit::Contain)
                .into(),
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

/// The open panel's content.
fn panel(state: &Listen) -> Element<'_, Message> {
    match state.fullscreen_tab {
        // The right half of the fullscreen player, minus its padding.
        FullscreenTab::Queue => super::queue::list(state, state.window_size.width * 0.5 - 64.0),
        FullscreenTab::Related => related::view(state),
        FullscreenTab::Lyrics => lyrics::lyrics(state),
        FullscreenTab::Visualizer => {
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
            stack![
                visualizer_surface(state),
                container(expand).width(Length::Fill).align_x(Horizontal::Right)
            ]
            .into()
        }
        FullscreenTab::Hidden => space().into(),
    }
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
            label_w(
                crate::ui::ellipsize_px(&song.title, 420.0, 22.0),
                22.0,
                iced::Color::WHITE,
                Weight::Bold
            ),
            label(
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
