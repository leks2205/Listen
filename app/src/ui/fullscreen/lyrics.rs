//! The lyrics panel of the full-screen player (also shown by the compact
//! layout's full-screen player). The view lives in `ui::lyrics`.

use crate::app::{Listen, Message};
use iced::Element;

pub use crate::ui::lyrics::LYRICS_ID;

pub fn lyrics(state: &Listen) -> Element<'_, Message> {
    crate::ui::lyrics::view(state)
}
