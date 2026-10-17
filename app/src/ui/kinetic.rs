//! Kinetic touchpad scrolling: scales touchpad deltas like Chromium on Wayland ([`factor`]), tracks the
//! speed over `WINDOW` and on release glides on with exponential slow-down via synthetic wheel events.
//! Release = winit's zero-length scroll event (Wayland `axis_stop`) or no input for `IDLE`.
//! Mouse wheels (line deltas) pass through unchanged.

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Shell, Widget, mouse, overlay, renderer};
use iced::mouse::ScrollDelta;
use iced::time::Instant;
use iced::{Element, Event, Length, Point, Rectangle, Size, Vector, window};
use std::collections::VecDeque;
use std::time::Duration;

/// Chromium's Wayland touchpad scaling: compositor value / 10 × 120
/// (`kAxisValueScale`, ui/ozone/platform/wayland/host/wayland_event_source.cc),
/// in logical pixels.
pub const CHROMIUM: f32 = 12.0;

/// Multiplier for touchpad deltas: Chromium's scaling × the user's scroll speed (1.0 = Chromium), with the
/// display scale divided out (winit reports physical pixels, iced scrolls by them as logical).
pub fn factor(speed: f32, scale: f32) -> f32 {
    CHROMIUM * speed / scale.max(0.5)
}
/// Speed is measured over this much recent input.
const WINDOW: Duration = Duration::from_millis(100);
/// No input for this long counts as a release (when there's no stop event).
const IDLE: Duration = Duration::from_millis(90);
/// Time constant of the slow-down: a fling travels about `speed × TAU`.
const TAU: f32 = 0.33;
/// Flings start above and end below these speeds (px/s).
const MIN_FLING: f32 = 120.0;
const STOP: f32 = 12.0;
const MAX_SPEED: f32 = 40000.0;

#[derive(Default)]
struct State {
    /// Recent touchpad input: (time, dx, dy).
    samples: VecDeque<(Instant, f32, f32)>,
    last_input: Option<Instant>,
    coast: Option<Coast>,
}

struct Coast {
    velocity: Vector,
    at: Instant,
    cursor: Point,
}

impl State {
    fn velocity(&self, now: Instant) -> Vector {
        let recent: Vec<_> = self.samples.iter().filter(|(t, ..)| now.duration_since(*t) <= WINDOW).collect();
        let Some(first) = recent.first() else { return Vector::ZERO };
        // Span of the samples plus one frame, so a single event isn't "infinitely fast".
        let span = now.duration_since(first.0).as_secs_f32().max(1.0 / 60.0) + 1.0 / 120.0;
        let (dx, dy) = recent.iter().fold((0.0, 0.0), |(x, y), (_, dx, dy)| (x + dx, y + dy));
        let clamp = |v: f32| v.clamp(-MAX_SPEED, MAX_SPEED);
        Vector::new(clamp(dx / span), clamp(dy / span))
    }

    /// Fingers lifted: start gliding with the recent speed.
    fn release(&mut self, now: Instant, cursor: Option<Point>) {
        let velocity = self.velocity(now);
        self.samples.clear();
        self.last_input = None;
        if let Some(cursor) = cursor
            && (velocity.x.hypot(velocity.y)) > MIN_FLING
        {
            self.coast = Some(Coast { velocity, at: now, cursor });
        }
    }
}

pub struct Kinetic<'a, Message, Theme, Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    factor: f32,
}

/// Wraps `content`; touchpad deltas are multiplied by `factor` (see [`factor`]).
pub fn kinetic<'a, Message, Theme, Renderer>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
    factor: f32,
) -> Kinetic<'a, Message, Theme, Renderer> {
    Kinetic { content: content.into(), factor }
}

fn wheel(dx: f32, dy: f32) -> Event {
    Event::Mouse(mouse::Event::WheelScrolled { delta: ScrollDelta::Pixels { x: dx, y: dy } })
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Kinetic<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
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

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        self.content.as_widget_mut().layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
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
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let now = Instant::now();
        let factor = self.factor;
        let mut forward = |event: &Event, cursor: mouse::Cursor, shell: &mut Shell<'_, Message>| {
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
        };

        match event {
            // Touchpad: scaled, tracked; a zero delta is the "fingers lifted" signal.
            Event::Mouse(mouse::Event::WheelScrolled { delta: ScrollDelta::Pixels { x, y } }) => {
                state.coast = None;
                if *x == 0.0 && *y == 0.0 {
                    state.release(now, cursor.position());
                    shell.request_redraw();
                    return;
                }
                let (dx, dy) = (x * factor, y * factor);
                state.samples.push_back((now, dx, dy));
                while state.samples.front().is_some_and(|(t, ..)| now.duration_since(*t) > WINDOW * 2) {
                    state.samples.pop_front();
                }
                state.last_input = Some(now);
                shell.request_redraw();
                forward(&wheel(dx, dy), cursor, shell);
            }
            // Any other input stops a glide.
            Event::Mouse(mouse::Event::WheelScrolled { .. } | mouse::Event::ButtonPressed(_))
            | Event::Touch(_)
            | Event::Keyboard(_) => {
                state.coast = None;
                state.samples.clear();
                state.last_input = None;
                forward(event, cursor, shell);
            }
            Event::Window(window::Event::RedrawRequested(_)) => {
                // Release without a stop event (e.g. X11).
                if let Some(last) = state.last_input {
                    if now.duration_since(last) >= IDLE {
                        state.release(now, cursor.position());
                    } else {
                        shell.request_redraw();
                    }
                }
                if let Some(coast) = &mut state.coast {
                    let dt = now.duration_since(coast.at).as_secs_f32().min(0.05);
                    coast.at = now;
                    let step = coast.velocity * dt;
                    let decay = (-dt / TAU).exp();
                    coast.velocity *= decay;
                    let at = mouse::Cursor::Available(coast.cursor);
                    if coast.velocity.x.hypot(coast.velocity.y) < STOP {
                        state.coast = None;
                    } else {
                        shell.request_redraw();
                    }
                    forward(&wheel(step.x, step.y), at, shell);
                }
                forward(event, cursor, shell);
            }
            _ => forward(event, cursor, shell),
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
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
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(&mut tree.children[0], layout, renderer, viewport, translation)
    }
}

impl<'a, Message, Theme, Renderer> From<Kinetic<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + 'a,
{
    fn from(k: Kinetic<'a, Message, Theme, Renderer>) -> Self {
        Element::new(k)
    }
}
