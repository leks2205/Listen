//! Bottom player bar: now-playing info | transport + seek | volume & toggles.
//! One file per column so the columns can change independently. The compact
//! layout (window <= 768 px) has its own bar in `compact`.

mod center;
pub use center::HEART_ID;
pub(crate) use center::heart_button;
pub mod compact;
mod left;
mod right;

use crate::app::{Listen, Message};
use crate::style::metrics;
use iced::widget::{mouse_area, row};
use iced::{Element, Length};

/// Gap between the three columns (Feishin: 1 rem).
const GAP: f32 = metrics::SPACE_LG;
/// A left column narrower than this is dropped: it must hold a readable stretch of title (a column
/// that fits the cover but not the text shows a lone "…").
const MIN_LEFT: f32 = 130.0;
/// Below this the left column shows the text without the cover, so the song can still be read.
pub(super) const COVER_FROM: f32 = 200.0;

pub fn view(state: &Listen) -> Element<'_, Message> {
    if state.layout_mode() == super::LayoutMode::Compact {
        return compact::view(state);
    }
    let (left_w, center_w, right_w, squeeze) = layout(state, state.window_size.width);
    let bar: Element<'_, Message> =
        row![left::view(state, left_w), center::view(state, center_w, squeeze), right::view(state, right_w)]
            .spacing(GAP)
            .height(Length::Fill)
            .into();
    // A click on the empty bar opens the full-screen player (Feishin `playerbarOpenDrawer`);
    // the buttons, sliders and links take their own clicks.
    if state.config.settings.player_bar.open_drawer && state.queue.current().is_some() {
        return mouse_area(bar).on_press(Message::ToggleFullscreen).into();
    }
    bar
}

/// Widths of the three columns for a bar `width` px wide. The right column keeps its
/// controls; the centre drops transport buttons (skips, then stop and "shuffle all")
/// before the left column is dropped.
pub fn column_widths(state: &Listen, width: f32) -> (f32, f32, f32) {
    let (l, c, r, _) = layout(state, width);
    (l, c, r)
}

/// `column_widths` plus how many transport buttons were given up (0..=2).
pub fn layout(state: &Listen, width: f32) -> (f32, f32, f32, u8) {
    let avail = (width - 2.0 * GAP).max(0.0);
    let right = right::intrinsic_width(state).max(avail / 3.0).min(avail);
    let rest = avail - right;
    let mut squeeze = 0;
    loop {
        let center = (rest / 2.0).max(center::min_width(state, squeeze)).min(rest);
        let left = rest - center;
        if left >= MIN_LEFT {
            return (left, center, right, squeeze);
        }
        if squeeze >= center::MAX_SQUEEZE {
            // Too narrow to show the cover: give the room to the transport instead.
            return (0.0, rest, right, squeeze);
        }
        squeeze += 1;
    }
}

/// Mouse wheel over `content` changes the volume by `volume_wheel_step`.
pub(crate) fn wheel_volume<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    mouse_area(content)
        .on_scroll(|delta| {
            let (iced::mouse::ScrollDelta::Lines { y, .. } | iced::mouse::ScrollDelta::Pixels { y, .. }) =
                delta;
            Message::VolumeWheel(y)
        })
        .into()
}

/// "42" or "Muted" (translated), after the DLNA device's name while casting: the text of a volume control.
pub(crate) fn volume_text(state: &Listen) -> String {
    let level = if state.playback.muted {
        crate::i18n::tr("Muted").to_string()
    } else {
        state.playback.volume.to_string()
    };
    match state.dlna_device() {
        Some(name) => crate::i18n::trf("{}: {}", &[&name, &level]),
        None => level,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_left_column_is_readable_or_gone_at_every_width() {
        let (s, _) = crate::app::Listen::boot(&crate::Args { demo: true, ..crate::Args::default() });
        for w in (770..=1600).step_by(10) {
            let (l, c, r, _) = layout(&s, w as f32);
            // A sliver that only fits the cover (a lone "…" for the title) is never shown.
            assert!(l == 0.0 || l >= MIN_LEFT, "{w}: left {l}");
            assert!(c + r + l <= w as f32, "{w}: columns {l} {c} {r}");
        }
    }
}
