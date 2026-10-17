//! External-link buttons for the album and artist pages: the site's icon, its name as tooltip.

use super::{Message, icon};
use crate::app::Listen;
use crate::icons::Icon;
use crate::style::{self, metrics};
use iced::widget::{button, container, tooltip};
use iced::{Background, Element, border};

/// The glyph for a site: brand icons from Simple Icons where it has one.
pub fn site_icon(name: &str) -> Icon {
    match name {
        "Last.fm" => Icon::SiLastdotfm,
        "MusicBrainz" => Icon::SiMusicbrainz,
        "Spotify" => Icon::SiSpotify,
        "ListenBrainz" => Icon::LuHeadphones,
        "Qobuz" => Icon::LuDisc3,
        _ => Icon::LuLink,
    }
}

/// An icon button that sends `msg`, with the site name as tooltip.
pub fn site_button<'a>(state: &Listen, name: &str, msg: Message) -> Element<'a, Message> {
    let t = state.tokens;
    tooltip(
        button(icon(site_icon(name), 20.0, t.foreground))
            .padding(8)
            .style(style::default_button(t))
            .on_press(msg),
        container(super::label(name.to_string(), metrics::font_sm(), t.foreground)).padding(6).style(
            move |_| container::Style {
                background: Some(Background::Color(t.background_alt)),
                border: border::rounded(4).color(t.border).width(1),
                ..Default::default()
            },
        ),
        tooltip::Position::Bottom,
    )
    .into()
}
