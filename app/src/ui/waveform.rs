//! The waveform seek bar (player bar slider type “waveform”): loudness bars, the played
//! part in the accent colour, click or drag to seek. Without a waveform (loading, no
//! local audio) `seek_bar` is `None` and the plain slider is shown.

use crate::app::{Listen, Message};
use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Size, Theme, mouse};
use listen_analysis::waveform::Waveform;
use listen_core::settings::{SliderType, WaveformAlign};
use std::time::Duration;

/// Height of the bar (Feishin's wavesurfer is 18 px high).
pub const HEIGHT: f32 = 20.0;

/// Colour of the part not played yet (Feishin: rgba(96, 96, 96)).
const UNPLAYED: Color = Color { r: 96.0 / 255.0, g: 96.0 / 255.0, b: 96.0 / 255.0, a: 1.0 };

/// The look of the bars, from the player bar settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    pub align: WaveformAlign,
    pub bar_width: f32,
    pub gap: f32,
    pub radius: f32,
    pub stretch: bool,
}

impl Style {
    pub fn of(pb: &listen_core::settings::PlayerBar) -> Style {
        Style {
            align: pb.waveform_align,
            bar_width: pb.waveform_bar_width.max(1) as f32,
            gap: pb.waveform_gap as f32,
            radius: pb.waveform_radius as f32,
            stretch: pb.waveform_stretch,
        }
    }
}

/// How many bars fit `width`.
pub fn bar_count(width: f32, style: &Style) -> usize {
    (((width + style.gap) / (style.bar_width + style.gap)).floor() as usize).max(1)
}

/// Rectangle of a bar of `level` (0..1) in slot `i`.
pub fn bar_rect(i: usize, level: f32, height: f32, style: &Style) -> Rectangle {
    // Even silence leaves a hairline, like wavesurfer's minimal bar.
    let h = (level.clamp(0.0, 1.0) * height).max(1.0).min(height);
    let y = match style.align {
        WaveformAlign::Top => 0.0,
        WaveformAlign::Center => (height - h) / 2.0,
        WaveformAlign::Bottom => height - h,
    };
    Rectangle::new(Point::new(i as f32 * (style.bar_width + style.gap), y), Size::new(style.bar_width, h))
}

/// Seek target in seconds for a mouse x position over a bar `width` wide.
pub fn seek_secs(x: f32, width: f32, duration: f32) -> f32 {
    (x / width.max(1.0)).clamp(0.0, 1.0) * duration
}

pub struct Bar<'a> {
    pub wave: &'a Waveform,
    pub style: Style,
    pub played: Color,
    /// Played share of the song, 0..1.
    pub progress: f32,
    pub duration: f32,
}

#[derive(Default)]
pub struct Drag(bool);

impl canvas::Program<Message> for Bar<'_> {
    type State = Drag;

    fn update(
        &self,
        drag: &mut Drag,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        use iced::Event::Mouse;
        let at = |p: Point| Message::SeekPreview(seek_secs(p.x - bounds.x, bounds.width, self.duration));
        match event {
            Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) if self.duration > 0.0 => {
                let p = cursor.position_in(bounds)?;
                drag.0 = true;
                Some(canvas::Action::publish(at(Point::new(p.x + bounds.x, p.y))).and_capture())
            }
            Mouse(mouse::Event::CursorMoved { position }) if drag.0 => {
                Some(canvas::Action::publish(at(*position)).and_capture())
            }
            Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if drag.0 => {
                drag.0 = false;
                Some(canvas::Action::publish(Message::SeekRelease).and_capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _: &Drag,
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let n = bar_count(bounds.width, &self.style);
        let bars = self.wave.bars(n, self.style.stretch);
        let split = self.progress.clamp(0.0, 1.0) * bounds.width;
        for (i, &level) in bars.iter().enumerate() {
            let r = bar_rect(i, level, bounds.height, &self.style);
            // A bar belongs to the played part once its middle is passed.
            let color = if r.x + r.width / 2.0 <= split { self.played } else { UNPLAYED };
            let rad = self.style.radius.min(r.width / 2.0).min(r.height / 2.0);
            let path = if rad > 0.5 {
                Path::rounded_rectangle(r.position(), r.size(), rad.into())
            } else {
                Path::rectangle(r.position(), r.size())
            };
            frame.fill(&path, color);
        }
        // The position marker.
        frame.fill_rectangle(
            Point::new((split - 1.0).max(0.0), 0.0),
            Size::new(2.0, bounds.height),
            Color { a: 0.5, ..Color::WHITE },
        );
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, _: &Drag, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        if cursor.is_over(bounds) { mouse::Interaction::Pointer } else { mouse::Interaction::default() }
    }
}

/// The waveform in place of the plain seek slider, if enabled and available.
/// `shown` is the position (or drag target) to mark.
pub fn seek_bar<'a>(state: &'a Listen, shown: Duration, duration: Duration) -> Option<Element<'a, Message>> {
    let pb = &state.config.settings.player_bar;
    if pb.slider_type != SliderType::Waveform || duration.is_zero() {
        return None;
    }
    let wave = state.viz.waveform.data.as_deref()?;
    Some(
        Canvas::new(Bar {
            wave,
            style: Style::of(pb),
            played: state.tokens.primary,
            progress: shown.as_secs_f32() / duration.as_secs_f32(),
            duration: duration.as_secs_f32(),
        })
        .width(Length::Fill)
        .height(HEIGHT)
        .into(),
    )
}

/// The slider type and waveform rows of the settings page (Feishin's labels).
pub fn settings_rows(state: &Listen) -> Vec<Element<'_, Message>> {
    use crate::app::viz::{SliderSetting, VizMsg};
    use crate::style::metrics;
    use crate::ui::settings::line;
    use iced::Alignment;
    use iced::widget::{pick_list, row, slider, toggler};
    let t = state.tokens;
    let pb = &state.config.settings.player_bar;
    let msg = |c| Message::Viz(VizMsg::Slider(c));
    let amount = |range: std::ops::RangeInclusive<u32>,
                  value: u32,
                  unit: &'static str,
                  f: fn(u32) -> SliderSetting| {
        row![
            slider(*range.start() as f32..=*range.end() as f32, value as f32, move |v| msg(f(
                v.round() as u32
            )))
            .width(160),
            crate::ui::label(format!("{value}{unit}"), metrics::font_md(), t.muted),
        ]
        .spacing(metrics::space_sm())
        .align_y(Alignment::Center)
        .into()
    };
    let mut rows = vec![line(
        state,
        "Playerbar slider",
        "The waveform is not recommended if on a slow or metered internet connection",
        pick_list(
            [SliderType::Slider, SliderType::Waveform].map(SliderChoice),
            Some(SliderChoice(pb.slider_type)),
            move |c| msg(SliderSetting::Type(c.0)),
        )
        .into(),
    )];
    if pb.slider_type == SliderType::Waveform {
        rows.push(line(
            state,
            "Waveform align",
            "Waveform align",
            pick_list(
                [WaveformAlign::Top, WaveformAlign::Center, WaveformAlign::Bottom].map(AlignChoice),
                Some(AlignChoice(pb.waveform_align)),
                move |c| msg(SliderSetting::Align(c.0)),
            )
            .into(),
        ));
        rows.push(line(
            state,
            "Waveform bar width",
            "Waveform bar width",
            amount(1..=10, pb.waveform_bar_width, " px", SliderSetting::BarWidth),
        ));
        rows.push(line(
            state,
            "Waveform gap",
            "Waveform gap",
            amount(0..=10, pb.waveform_gap, " px", SliderSetting::Gap),
        ));
        rows.push(line(
            state,
            "Waveform radius",
            "Waveform radius",
            amount(0..=20, pb.waveform_radius, " px", SliderSetting::Radius),
        ));
        rows.push(line(
            state,
            "Waveform stretch",
            "Stretches the waveform to fill the available space",
            toggler(pb.waveform_stretch).on_toggle(move |v| msg(SliderSetting::Stretch(v))).size(20).into(),
        ));
        rows.push(line(
            state,
            "Waveform loading delay",
            "Delay in seconds before the waveform of a new song is computed. Increase this value if you notice stutters when songs change. Needs the song's audio: a YouTube Music song gets its waveform once a local copy exists (it never makes extra requests).",
            amount(0..=10, pb.waveform_delay_secs, " s", SliderSetting::Delay),
        ));
    }
    rows
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SliderChoice(SliderType);

impl std::fmt::Display for SliderChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self.0 {
            SliderType::Slider => "Slider",
            SliderType::Waveform => "Waveform",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AlignChoice(WaveformAlign);

impl std::fmt::Display for AlignChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self.0 {
            WaveformAlign::Top => "Top",
            WaveformAlign::Center => "Center",
            WaveformAlign::Bottom => "Bottom",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(align: WaveformAlign) -> Style {
        Style { align, bar_width: 2.0, gap: 1.0, radius: 0.0, stretch: false }
    }

    #[test]
    fn bars_fit_the_width_with_their_gaps() {
        assert_eq!(bar_count(300.0, &style(WaveformAlign::Center)), 100);
        assert_eq!(bar_count(301.0, &style(WaveformAlign::Center)), 100);
        assert_eq!(
            bar_count(302.0, &style(WaveformAlign::Center)),
            101,
            "the last bar needs no gap after it"
        );
        assert_eq!(bar_count(1.0, &style(WaveformAlign::Center)), 1);
    }

    #[test]
    fn alignment_places_the_bars() {
        let h = 20.0;
        let center = bar_rect(2, 0.5, h, &style(WaveformAlign::Center));
        assert_eq!((center.x, center.y, center.height), (6.0, 5.0, 10.0));
        let top = bar_rect(0, 0.5, h, &style(WaveformAlign::Top));
        assert_eq!((top.y, top.height), (0.0, 10.0));
        let bottom = bar_rect(0, 0.25, h, &style(WaveformAlign::Bottom));
        assert_eq!((bottom.y, bottom.height), (15.0, 5.0));
        let silent = bar_rect(0, 0.0, h, &style(WaveformAlign::Center));
        assert_eq!(silent.height, 1.0, "a hairline, not nothing");
        assert_eq!(bar_rect(0, 3.0, h, &style(WaveformAlign::Center)).height, h);
    }

    #[test]
    fn clicks_map_to_seconds() {
        assert_eq!(seek_secs(0.0, 200.0, 180.0), 0.0);
        assert_eq!(seek_secs(100.0, 200.0, 180.0), 90.0);
        assert_eq!(seek_secs(-5.0, 200.0, 180.0), 0.0, "dragging past the left edge");
        assert_eq!(seek_secs(500.0, 200.0, 180.0), 180.0, "and past the right edge");
    }
}
