//! Heart and star controls for albums and artists, and the small segmented tab control of the genre and
//! favourites pages. They honour the Show favourites / Show ratings settings and source capabilities.

use super::{icon, label};
use crate::app::library_extra::ExtraMsg;
use crate::app::{Listen, Message};
use crate::icons::Icon;
use crate::style::{self, metrics};
use iced::widget::{Row, button, row};
use iced::{Alignment, Element, Padding};
use listen_core::model::ServerId;
use listen_source::{Capabilities, ItemKind};

/// The heart of an album or artist; `None` when hearts are hidden or the
/// source cannot favourite this kind.
pub fn heart<'a>(
    state: &Listen,
    kind: ItemKind,
    server: &ServerId,
    id: &str,
    on: bool,
) -> Option<Element<'a, Message>> {
    if !state.show_favorites() || !state.can_favorite_items(server) {
        return None;
    }
    let t = state.tokens;
    let (glyph, color) = if on { (Icon::RiHeartFill, t.primary) } else { (Icon::RiHeartLine, t.muted) };
    Some(crate::ui::tip::tip(
        state,
        button(icon(glyph, metrics::font_xl(), color))
            .padding(6)
            .style(style::icon_button(t, false))
            .on_press(Message::Extra(ExtraMsg::Favorite(kind, server.clone(), id.to_string(), !on))),
        if on { "Remove from favorites" } else { "Add to favorites" },
        None,
        crate::ui::tip::Position::Bottom,
    ))
}

/// Five stars; the same star again clears the rating. `None` when ratings are
/// hidden or the source has none.
pub fn stars<'a>(
    state: &Listen,
    kind: ItemKind,
    server: &ServerId,
    id: &str,
    rating: u8,
) -> Option<Element<'a, Message>> {
    let supported =
        state.sources.get(server).is_some_and(|s| s.capabilities().contains(Capabilities::RATINGS));
    if !state.show_ratings() || !supported {
        return None;
    }
    let t = state.tokens;
    let mut out = Row::new().align_y(Alignment::Center);
    for n in 1..=5u8 {
        let on = n <= rating;
        let to = if rating == n { 0 } else { n };
        out = out.push(crate::ui::tip::tip_raw(
            state,
            button(icon(
                if on { Icon::LuStarFill } else { Icon::LuStar },
                metrics::font_lg(),
                if on { t.primary } else { t.muted },
            ))
            .padding(2)
            .style(style::icon_button(t, false))
            .on_press(Message::Extra(ExtraMsg::Rate(kind, server.clone(), id.to_string(), to))),
            &crate::ui::tip::rate_text(n),
            None,
            crate::ui::tip::Position::Bottom,
        ));
    }
    Some(out.into())
}

/// Segments of which one is selected: `(label, selected, message)`.
pub fn tabs<'a>(state: &Listen, items: Vec<(String, bool, Message)>) -> Element<'a, Message> {
    let t = state.tokens;
    let mut out = row![].spacing(metrics::space_xs()).align_y(Alignment::Center);
    for (text, selected, msg) in items {
        let b = button(label(text, metrics::font_md(), t.foreground))
            .padding(Padding::from([6.0, 14.0]))
            .on_press(msg);
        out = out.push(if selected {
            b.style(style::primary_button(t))
        } else {
            b.style(style::default_button(t))
        });
    }
    out.into()
}
