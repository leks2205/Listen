//! The lyrics panel of the full-screen player (also shown by the compact
//! layout's full-screen player). The view lives in `ui::lyrics`.

use crate::app::{Listen, Message};
use iced::Element;

pub use crate::ui::lyrics::LYRICS_ID;

/// Line height used for auto-scrolling; the lyrics view computes its own (`ui::lyrics::follow_offset`).
pub fn lyric_line_height(state: &Listen) -> f32 {
    let l = &state.config.settings.lyrics;
    l.font_size * 1.2 + l.gap
}

pub fn lyrics(state: &Listen) -> Element<'_, Message> {
    crate::ui::lyrics::view(state)
}
