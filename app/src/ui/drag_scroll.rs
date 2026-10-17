//! A wrapper that turns a horizontal mouse drag over its content into a message
//! with the distance dragged (as Feishin's carousels). The content gets every event
//! until the pointer moves [`THRESHOLD`] px; the release that ends a drag is swallowed.

use crate::app::Message;
use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Shell, Widget, mouse, overlay, renderer};
use iced::{Element, Event, Length, Point, Rectangle, Size, Vector};

/// Pixels the pointer has to move before a press becomes a drag.
pub const THRESHOLD: f32 = 8.0;

/// `content` that publishes `on_drag(dx)` when a drag over it ends; `dx` is the
/// horizontal distance from where the button went down (negative: to the left).
pub fn drag_scroll<'a>(
    content: impl Into<Element<'a, Message>>,
    on_drag: impl Fn(f32) -> Message + 'a,
) -> Element<'a, Message> {
    Element::new(DragScroll { content: content.into(), on_drag: Box::new(on_drag) })
}

#[derive(Default)]
struct State {
    /// Where the left button went down over the content.
    origin: Option<Point>,
    dragging: bool,
    dx: f32,
}

struct DragScroll<'a> {
    content: Element<'a, Message>,
    on_drag: Box<dyn Fn(f32) -> Message + 'a>,
}

impl Widget<Message, iced::Theme, iced::Renderer> for DragScroll<'_> {
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
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                state.origin = cursor.position_over(layout.bounds());
                state.dragging = false;
                state.dx = 0.0;
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                if let Some(origin) = state.origin {
                    state.dx = position.x - origin.x;
                    if state.dx.abs() > THRESHOLD {
                        state.dragging = true;
                    }
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                // Where the pointer is now decides, whatever moves were seen.
                if let (Some(origin), Some(now)) = (state.origin, cursor.position()) {
                    state.dx = now.x - origin.x;
                    state.dragging = state.dragging || state.dx.abs() > THRESHOLD;
                }
                let was = state.origin.take().is_some();
                if was && state.dragging {
                    state.dragging = false;
                    shell.publish((self.on_drag)(state.dx));
                    // The release belongs to the drag, not to the card under the pointer.
                    shell.capture_event();
                    return;
                }
            }
            Event::Mouse(mouse::Event::CursorLeft) => {
                state.origin = None;
                state.dragging = false;
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
        if tree.state.downcast_ref::<State>().dragging {
            return mouse::Interaction::Grabbing;
        }
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
