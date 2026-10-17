//! A wrapper that sends another message when its content is held for [`HOLD`] (Feishin's play
//! buttons: a long press plays shuffled, `LONG_PRESS_PLAY_BEHAVIOR`). On a long release the
//! content is told the pointer left, so a button inside doesn't fire its own message.

use crate::app::Message;
use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Shell, Widget, mouse, overlay, renderer};
use iced::time::Instant;
use iced::{Element, Event, Length, Rectangle, Size, Vector};
use std::time::Duration;

/// How long the button has to be held (Feishin's `usePlayButtonClick` waits 500 ms).
pub const HOLD: Duration = Duration::from_millis(500);

/// `content` that sends `on_long` instead of its own message after a long press.
pub fn long_press<'a>(content: impl Into<Element<'a, Message>>, on_long: Message) -> Element<'a, Message> {
    Element::new(LongPress { content: content.into(), on_long })
}

#[derive(Default)]
struct State {
    pressed_at: Option<Instant>,
}

/// Whether a press from `pressed` to `now` is a long one.
pub fn is_long(pressed: Instant, now: Instant) -> bool {
    now.saturating_duration_since(pressed) >= HOLD
}

struct LongPress<'a> {
    content: Element<'a, Message>,
    on_long: Message,
}

impl Widget<Message, iced::Theme, iced::Renderer> for LongPress<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content.as_widget_mut().layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content.as_widget_mut().operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let mut cursor = cursor;
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if cursor.is_over(layout.bounds()) =>
            {
                state.pressed_at = Some(Instant::now());
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if let Some(at) = state.pressed_at.take()
                    && cursor.is_over(layout.bounds())
                    && is_long(at, Instant::now())
                {
                    shell.publish(self.on_long.clone());
                    // The content sees the release with the pointer elsewhere, so it doesn't fire.
                    cursor = mouse::Cursor::Unavailable;
                    shell.capture_event();
                }
            }
            _ => {}
        }
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(&mut tree.children[0], layout, renderer, viewport, translation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_a_second_is_a_long_press() {
        let t = Instant::now();
        assert!(!is_long(t, t + Duration::from_millis(200)));
        assert!(is_long(t, t + HOLD));
        assert!(is_long(t, t + Duration::from_secs(2)));
    }
}
