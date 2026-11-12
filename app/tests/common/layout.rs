//! Click points computed from the app's layout constants (`listen::style::metrics` and the `pub const`s in
//! `listen::ui`), so a layout change moves the tests' click points with it instead of onto another widget.
//! Where the target shows a text, find it by that text ([`text_center`]) instead of computing its place.

use iced::{Rectangle, Size};
use iced_test::selector::Candidate;
use iced_test::simulator::Simulator;
use listen::app::{FullscreenTab, Listen};
use listen::style::metrics;

/// Height above the app's content: the custom title bar and its 1 px divider, or nothing with the system frame.
pub fn content_top(s: &Listen) -> f32 {
    if s.config.settings.window.custom_title_bar { listen::ui::title_bar::HEIGHT + 1.0 } else { 0.0 }
}

/// Top edge of the player bar: it fills the bottom `PLAYER_BAR_HEIGHT` px of the window.
pub fn bar_top(window: Size) -> f32 {
    window.height - metrics::PLAYER_BAR_HEIGHT
}

/// Vertical centre of the player bar.
pub fn bar_center_y(window: Size) -> f32 {
    window.height - metrics::PLAYER_BAR_HEIGHT / 2.0
}

/// The queue panel at the right of the window, between the content top and the player bar.
pub fn queue_panel(s: &Listen, window: Size) -> Rectangle {
    let width = s.config.settings.sidebar.queue_width;
    let top = content_top(s);
    Rectangle { x: window.width - width, y: top, width, height: bar_top(window) - top }
}

/// Horizontal centre of the `i`th icon button (`CONTROL` wide, `space_xs` apart) of a row whose first button
/// starts at `left`.
pub fn icon_x(left: f32, i: usize) -> f32 {
    left + i as f32 * (metrics::CONTROL + metrics::space_xs()) + metrics::CONTROL / 2.0
}

/// Like [`icon_x`], counted from the right: `i` = 0 is the button that ends at `right`.
pub fn icon_x_from_right(right: f32, i: usize) -> f32 {
    right - i as f32 * (metrics::CONTROL + metrics::space_xs()) - metrics::CONTROL / 2.0
}

/// Vertical centre of the full-screen player's top bar.
fn fullscreen_bar_y(s: &Listen) -> f32 {
    content_top(s) + listen::ui::fullscreen::PAD.top + metrics::CONTROL / 2.0
}

/// Centre of the `i`th button at the left of the full-screen player's top bar (0 Minimize, 1 Configure).
pub fn fullscreen_left_button(s: &Listen, i: usize) -> (f32, f32) {
    (icon_x(listen::ui::fullscreen::PAD.left, i), fullscreen_bar_y(s))
}

/// Centre of the full-screen player's panel button for `tab` (the buttons at the right, in `TABS` order).
pub fn fullscreen_tab_button(s: &Listen, tab: FullscreenTab) -> (f32, f32) {
    let tabs = &listen::ui::fullscreen::TABS;
    let at = tabs.iter().position(|(t, _, _)| *t == tab).expect("a panel tab");
    let right = s.window_size.width - listen::ui::fullscreen::PAD.right;
    (icon_x_from_right(right, tabs.len() - 1 - at), fullscreen_bar_y(s))
}

/// Centre of the `i`th of the four squares right of the sidebar's search box (0 Collapse, 1 Menu, 2 Back,
/// 3 Forward). The action bar is padded by `space_md`; search box and squares share it half and half, `space_sm`
/// apart, and the squares split their half with `space_sm` gaps.
pub fn sidebar_square(s: &Listen, i: usize) -> (f32, f32) {
    let inner = s.config.settings.sidebar.width - 2.0 * metrics::space_md();
    let half = (inner - metrics::space_sm()) / 2.0;
    let square = (half - 3.0 * metrics::space_sm()) / 4.0;
    let left = metrics::space_md() + half + metrics::space_sm();
    let x = left + i as f32 * (square + metrics::space_sm()) + square / 2.0;
    (x, content_top(s) + metrics::ACTION_BAR_HEIGHT / 2.0)
}

/// Centre of a visible text `text` whose centre lies in `region` of a `window`-sized render of `s`, if any.
pub fn text_center(s: &Listen, window: Size, text: &str, region: Rectangle) -> Option<(f32, f32)> {
    let _gpu = super::gpu();
    let mut ui = Simulator::with_size(super::iced_settings(), window, s.view());
    let found = ui.find(|c: Candidate<'_>| match c {
        Candidate::Text { content, visible_bounds: Some(b), .. }
            if content == text && region.contains(b.center()) =>
        {
            Some(b.center())
        }
        _ => None,
    });
    found.ok().map(|p| (p.x, p.y))
}
