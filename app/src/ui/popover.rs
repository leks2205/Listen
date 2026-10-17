//! A floating panel over the whole window (popover, dropdown): drawn on top
//! of `root` with a backdrop that closes it. Shared by the context menu and
//! the player bar's popups.

use crate::app::Message;
use iced::widget::{mouse_area, pin, space, stack};
use iced::{Element, Length, Point, Rectangle, Size};

/// Where a panel of `size` opens at `at` so it stays inside `window`: it
/// flips to the left of / above `at` when it would run off the right / bottom
/// edge, and keeps a 4 px margin.
pub fn place(at: Point, size: Size, window: Size) -> Point {
    let x = if at.x + size.width + 4.0 > window.width { (at.x - size.width).max(4.0) } else { at.x };
    let y = if at.y + size.height + 4.0 > window.height {
        (at.y - size.height).max(4.0).min((window.height - size.height - 4.0).max(4.0))
    } else {
        at.y
    };
    Point::new(x, y)
}

/// Where a submenu of `size` opens next to `anchor` (its row): to the right overlapping by 4 px, else to
/// the left; top-aligned with the row, moved up to fit, kept 4 px inside the window.
pub fn place_beside(anchor: Rectangle, size: Size, window: Size) -> Point {
    let right = anchor.x + anchor.width - 4.0;
    let x =
        if right + size.width + 4.0 > window.width { (anchor.x + 4.0 - size.width).max(4.0) } else { right };
    let y = anchor.y.min(window.height - size.height - 4.0).max(4.0);
    Point::new(x, y)
}

/// `root` with `panel` at `position` over a full-window backdrop that sends `close` on any click outside.
/// Wrap `panel` in `iced::widget::opaque`; keep `root` the first stack child whether open or not so
/// scrollables keep their state (see `context_menu::overlay`).
pub fn over<'a>(
    root: Element<'a, Message>,
    panel: Element<'a, Message>,
    position: Point,
    close: Message,
) -> Element<'a, Message> {
    over_all(root, vec![(panel, position)], close)
}

/// Like [`over`] with several panels (a menu and its submenu), later ones on top.
pub fn over_all<'a>(
    root: Element<'a, Message>,
    panels: Vec<(Element<'a, Message>, Point)>,
    close: Message,
) -> Element<'a, Message> {
    let backdrop = mouse_area(space().width(Length::Fill).height(Length::Fill))
        .on_press(close.clone())
        .on_right_press(close);
    let mut out = stack![root, backdrop];
    for (panel, position) in panels {
        out = out.push(pin(panel).position(position));
    }
    out.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIN: Size = Size::new(1000.0, 700.0);

    #[test]
    fn opens_at_the_point_when_it_fits() {
        assert_eq!(place(Point::new(100.0, 100.0), Size::new(200.0, 150.0), WIN), Point::new(100.0, 100.0));
    }

    #[test]
    fn flips_left_and_up_at_the_edges() {
        let p = place(Point::new(900.0, 650.0), Size::new(200.0, 150.0), WIN);
        assert_eq!(p, Point::new(700.0, 500.0));
    }

    #[test]
    fn side_panels_open_right_flip_left_and_stay_inside() {
        let row = |x, y| Rectangle::new(Point::new(x, y), Size::new(248.0, 34.0));
        let sub = Size::new(200.0, 150.0);
        assert_eq!(place_beside(row(100.0, 100.0), sub, WIN), Point::new(344.0, 100.0));
        // No room on the right: left of the row, still overlapping its edge.
        assert_eq!(place_beside(row(800.0, 100.0), sub, WIN), Point::new(604.0, 100.0));
        // No room below: moved up to fit.
        assert_eq!(place_beside(row(100.0, 650.0), sub, WIN), Point::new(344.0, 546.0));
        // Never left of / above the window.
        assert_eq!(place_beside(row(0.0, 100.0), Size::new(900.0, 150.0), Size::new(500.0, 700.0)).x, 4.0);
    }

    #[test]
    fn sticks_to_the_top_margin_when_taller_than_the_space_above() {
        let p = place(Point::new(10.0, 100.0), Size::new(200.0, 650.0), WIN);
        assert_eq!(p, Point::new(10.0, 4.0));
    }
}
