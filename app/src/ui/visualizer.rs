//! Spectrum drawing after audioMotion-analyzer: graph and bar modes, bar styles, gradients, colour modes,
//! mirror, reflex, radial, dual channels, scales and peaks. Analysis settings are in
//! [`crate::visualizer::analyzer_config`].

use crate::app::Message;
use iced::widget::canvas::{
    self, Canvas, Fill, Frame, Geometry, Gradient, LineCap, Path, Stroke, Text, gradient, stroke,
};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Size, Theme, mouse};
use listen_core::settings::{ChannelLayout, ColorMode, Visualizer};
use listen_visualizer::{Channel, Frame as Spectrum};
use std::cell::Cell;
use std::sync::OnceLock;
use std::time::Instant;

// ---------------------------------------------------------------- colours

/// A colour name or `#rgb` / `#rrggbb` (audioMotion uses CSS colours).
pub fn parse_color(s: &str) -> Color {
    let s = s.trim().to_lowercase();
    let named = match s.as_str() {
        "red" => Some("#ff0000"),
        "yellow" => Some("#ffff00"),
        "lime" => Some("#00ff00"),
        "orangered" => Some("#ff4500"),
        "steelblue" => Some("#4682b4"),
        "white" => Some("#ffffff"),
        "black" => Some("#000000"),
        "blue" => Some("#0000ff"),
        "green" => Some("#008000"),
        "orange" => Some("#ffa500"),
        "cyan" | "aqua" => Some("#00ffff"),
        "magenta" | "fuchsia" => Some("#ff00ff"),
        "purple" => Some("#800080"),
        "pink" => Some("#ffc0cb"),
        "gray" | "grey" => Some("#808080"),
        _ => None,
    };
    let hex = named.unwrap_or(&s).trim_start_matches('#');
    let v = match hex.len() {
        3 => hex.chars().flat_map(|c| [c, c]).collect::<String>(),
        6 | 8 => hex[..6].to_string(),
        _ => return Color::WHITE,
    };
    let n = u32::from_str_radix(&v, 16).unwrap_or(0xff_ff_ff);
    Color::from_rgb8((n >> 16) as u8, (n >> 8) as u8, n as u8)
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    let l = |x: f32, y: f32| x + (y - x) * t;
    Color { r: l(a.r, b.r), g: l(a.g, b.g), b: l(a.b, b.b), a: l(a.a, b.a) }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Stop {
    color: Color,
    /// Position along the gradient: 0 = top (loud) / left.
    pos: f32,
    /// Level (0..1) from which the colour applies, for the bar-level mode.
    level: f32,
}

/// A resolved gradient (built-in or custom).
#[derive(Debug, Clone, PartialEq)]
pub struct Grad {
    pub horizontal: bool,
    stops: Vec<Stop>,
}

const BUILTIN: [&str; 5] = ["classic", "prism", "rainbow", "orangered", "steelblue"];

/// Names of the built-in gradients.
pub fn builtin_gradients() -> &'static [&'static str] {
    &BUILTIN
}

impl Grad {
    fn from_colors(horizontal: bool, colors: &[(&str, Option<f32>, Option<f32>)]) -> Grad {
        let n = colors.len().max(1);
        let stops = colors
            .iter()
            .enumerate()
            .map(|(i, (c, pos, level))| {
                let auto = if n == 1 { 0.0 } else { i as f32 / (n - 1) as f32 };
                let pos = pos.unwrap_or_else(|| level.map_or(auto, |l| 1.0 - l));
                Stop { color: parse_color(c), pos, level: level.unwrap_or(1.0 - pos) }
            })
            .collect();
        Grad { horizontal, stops }
    }

    /// The gradient called `name`: a custom one, a built-in, or prism.
    pub fn resolve(settings: &Visualizer, name: &str) -> Grad {
        if let Some(g) = settings.custom_gradient(name)
            && !g.color_stops.is_empty()
        {
            let colors: Vec<(&str, Option<f32>, Option<f32>)> =
                g.color_stops.iter().map(|s| (s.color.as_str(), s.pos, s.level)).collect();
            return Grad::from_colors(g.dir == "h", &colors);
        }
        const PRISM: [&str; 10] =
            ["#a35", "#c66", "#e94", "#ed0", "#9d5", "#4d8", "#2cb", "#0bc", "#09c", "#36b"];
        let plain = |c: [&'static str; 10]| c.map(|c| (c, None, None));
        match name {
            "classic" => Grad::from_colors(
                false,
                &[("red", None, None), ("yellow", Some(0.6), Some(0.85)), ("lime", None, Some(0.475))],
            ),
            "rainbow" => {
                let mut c = PRISM;
                c[0] = "#817";
                c[1] = "#a35";
                c[2] = "#c66";
                c[3] = "#e94";
                c[4] = "#ed0";
                c[5] = "#9d5";
                c[6] = "#4d8";
                c[7] = "#2cb";
                c[8] = "#0bc";
                c[9] = "#09c";
                Grad::from_colors(true, &plain(c))
            }
            "orangered" => Grad::from_colors(false, &[("orangered", None, None)]),
            "steelblue" => Grad::from_colors(false, &[("steelblue", None, None)]),
            _ => Grad::from_colors(false, &plain(PRISM)),
        }
    }

    /// Colour at `t` along the gradient axis (0 = top / left).
    pub fn at(&self, t: f32) -> Color {
        let t = t.clamp(0.0, 1.0);
        let mut sorted = self.stops.clone();
        sorted.sort_by(|a, b| a.pos.total_cmp(&b.pos));
        interpolate(&sorted, t, |s| s.pos)
    }

    /// Colour of a bar of height `level` (the bar-level colour mode).
    pub fn at_level(&self, level: f32) -> Color {
        let mut sorted = self.stops.clone();
        sorted.sort_by(|a, b| b.level.total_cmp(&a.level));
        // Highest level first = t runs from loud (1) to quiet (0).
        interpolate_rev(&sorted, level.clamp(0.0, 1.0))
    }

    /// The `i`-th colour, cycling (the bar-index colour mode).
    pub fn cycle(&self, i: usize) -> Color {
        let mut sorted = self.stops.clone();
        sorted.sort_by(|a, b| a.pos.total_cmp(&b.pos));
        sorted[i % sorted.len().max(1)].color
    }

    /// At most 8 stops for iced, colours scaled by `bright`, opacity by `alpha`; `reverse` flips the axis.
    fn iced_stops(&self, bright: f32, alpha: f32, reverse: bool) -> Vec<(f32, Color)> {
        const MAX: usize = 8;
        let mut v: Vec<(f32, Color)> = (0..MAX)
            .map(|k| {
                let t = k as f32 / (MAX - 1) as f32;
                (t, self.at(if reverse { 1.0 - t } else { t }))
            })
            .collect();
        // A flat gradient needs no more than 2 stops, and keeping it exact avoids drift.
        if self.stops.len() == 1 {
            v.truncate(2);
            v[1].0 = 1.0;
        }
        for (_, c) in &mut v {
            *c = shade(*c, bright, alpha);
        }
        v
    }
}

fn interpolate(sorted: &[Stop], t: f32, key: impl Fn(&Stop) -> f32) -> Color {
    let Some(first) = sorted.first() else { return Color::WHITE };
    if t <= key(first) {
        return first.color;
    }
    for w in sorted.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        if t <= key(b) {
            let span = (key(b) - key(a)).max(1e-6);
            return mix(a.color, b.color, (t - key(a)) / span);
        }
    }
    sorted[sorted.len() - 1].color
}

/// `sorted` by descending level; the colour at `level`.
fn interpolate_rev(sorted: &[Stop], level: f32) -> Color {
    let Some(first) = sorted.first() else { return Color::WHITE };
    if level >= first.level {
        return first.color;
    }
    for w in sorted.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        if level >= b.level {
            let span = (a.level - b.level).max(1e-6);
            return mix(b.color, a.color, (level - b.level) / span);
        }
    }
    sorted[sorted.len() - 1].color
}

fn shade(c: Color, bright: f32, alpha: f32) -> Color {
    Color {
        r: (c.r * bright).min(1.0),
        g: (c.g * bright).min(1.0),
        b: (c.b * bright).min(1.0),
        a: c.a * alpha,
    }
}

// ---------------------------------------------------------------- geometry

/// `levels` squeezed into half the bars and mirrored (`mirror` −1: original on the left, 1: right).
pub fn mirrored(levels: &[f32], mirror: i8) -> Vec<f32> {
    if mirror == 0 || levels.len() < 2 {
        return levels.to_vec();
    }
    let half: Vec<f32> = levels.chunks(2).map(|c| c.iter().cloned().fold(0.0, f32::max)).collect();
    let rev: Vec<f32> = half.iter().rev().cloned().collect();
    let (a, b) = if mirror < 0 { (half, rev) } else { (rev, half) };
    a.into_iter().chain(b).collect()
}

/// Panes of the canvas for the channel layout: `(area, channel, flipped)`;
/// channel 0 = left (or the mix), 1 = right.
pub fn panes(layout: ChannelLayout, area: Rectangle) -> Vec<(Rectangle, usize, bool)> {
    let Rectangle { x, y, width, height } = area;
    match layout {
        ChannelLayout::Single | ChannelLayout::DualCombined => vec![(area, 0, false)],
        ChannelLayout::DualHorizontal => vec![
            (Rectangle::new(Point::new(x, y), Size::new(width / 2.0, height)), 0, false),
            (Rectangle::new(Point::new(x + width / 2.0, y), Size::new(width / 2.0, height)), 1, false),
        ],
        ChannelLayout::DualVertical => vec![
            (Rectangle::new(Point::new(x, y), Size::new(width, height / 2.0)), 0, false),
            (Rectangle::new(Point::new(x, y + height / 2.0), Size::new(width, height / 2.0)), 1, true),
        ],
    }
}

/// Width of one bar and the gap before it, from audioMotion's `barSpace`
/// (< 1: share of the slot, ≥ 1: pixels).
pub fn bar_slot(slot: f32, bar_space: f32) -> (f32, f32) {
    let gap = if bar_space >= 1.0 { bar_space } else { slot * bar_space.clamp(0.0, 1.0) };
    let gap = gap.min(slot - 1.0).max(0.0);
    (slot - gap, gap)
}

/// Number of LEDs in a bar of height `h` and how many are lit at `level`.
pub fn leds(h: f32, level: f32) -> (usize, usize) {
    let count = ((h / 8.0).floor() as usize).clamp(4, 40);
    (count, ((level.clamp(0.0, 1.0) * count as f32).ceil() as usize).min(count))
}

/// Frequencies labelled on the X axis as `(position 0..1, text)`.
pub fn x_ticks(cfg: &listen_visualizer::Config, notes: bool) -> Vec<(f32, String)> {
    let bands = listen_visualizer::band_centers(cfg);
    let (min, max) = (cfg.min_freq.max(1.0), cfg.max_freq);
    let freqs: Vec<(f32, String)> = if notes {
        const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
        // C notes only: one label per octave.
        (0..10)
            .map(|o| (16.351_6 * 2f32.powi(o), format!("{}{o}", NAMES[0])))
            .filter(|(f, _)| *f >= min && *f <= max)
            .collect()
    } else {
        [20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0, 20000.0]
            .into_iter()
            .filter(|f| *f >= min && *f <= max)
            .map(|f| (f, if f >= 1000.0 { format!("{}k", f / 1000.0) } else { format!("{f}") }))
            .collect()
    };
    freqs
        .into_iter()
        .map(|(f, text)| {
            let pos = if cfg.octave.is_some() && !bands.is_empty() {
                // Evenly spaced bands: the band closest to the frequency.
                let i = bands
                    .iter()
                    .enumerate()
                    .min_by(|a, b| (a.1.ln() - f.ln()).abs().total_cmp(&(b.1.ln() - f.ln()).abs()))
                    .map_or(0, |(i, _)| i);
                (i as f32 + 0.5) / bands.len() as f32
            } else {
                cfg.scale.position(f, cfg.min_freq, cfg.max_freq)
            };
            (pos, text)
        })
        .collect()
}

/// Levels labelled on the Y axis as `(level 0..1, text)`.
pub fn y_ticks(min_db: f32, max_db: f32, linear: bool, boost: f32) -> Vec<(f32, String)> {
    if linear {
        return [0.25f32, 0.5, 0.75, 1.0]
            .into_iter()
            .map(|a| (a.powf(1.0 / boost.max(1.0)), format!("{}%", (a * 100.0) as u32)))
            .collect();
    }
    let range = (max_db - min_db).max(1.0);
    let mut db = (min_db / 10.0).ceil() * 10.0;
    let mut out = Vec::new();
    while db <= max_db {
        out.push(((db - min_db) / range, format!("{db}")));
        db += 10.0;
    }
    out
}

// ---------------------------------------------------------------- drawing

fn started() -> Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    *START.get_or_init(Instant::now)
}

/// Measures how often the canvas is redrawn (for “show FPS”).
#[derive(Default)]
pub struct Meter {
    last: Cell<Option<Instant>>,
    fps: Cell<f32>,
}

pub struct Graph<'a> {
    pub spectrum: &'a Spectrum,
    pub settings: &'a Visualizer,
}

/// What every bar of one pane is drawn with.
struct Look<'a> {
    s: &'a Visualizer,
    grad: Grad,
    /// Opacity of everything (the setting × the pane's own, e.g. the reflection's).
    alpha: f32,
    bright: f32,
}

impl Look<'_> {
    /// Solid colour of bar `i` of `n`.
    fn solid(&self, i: usize, n: usize, level: f32, y_frac: f32) -> Color {
        let c = match self.s.color_mode {
            ColorMode::BarIndex => self.grad.cycle(i),
            ColorMode::BarLevel => self.grad.at_level(level),
            ColorMode::Gradient if self.grad.horizontal => self.grad.at(i as f32 / (n.max(2) - 1) as f32),
            ColorMode::Gradient => self.grad.at(y_frac),
        };
        shade(c, self.bright, self.alpha)
    }
}

/// Where a channel is drawn: area, baseline side and the reach of a level-1 bar.
struct Pane {
    area: Rectangle,
    flip: bool,
    reach: f32,
}

impl Pane {
    fn baseline(&self) -> f32 {
        if self.flip { self.area.y } else { self.area.y + self.area.height }
    }

    /// Y of the top of a bar of `level` (the end away from the baseline).
    fn tip(&self, level: f32) -> f32 {
        let h = (level.clamp(0.0, 1.0) * self.reach).min(self.area.height);
        if self.flip { self.baseline() + h } else { self.baseline() - h }
    }
}

fn paint(frame: &mut Frame, path: &Path, p: &Paint, outline: bool) {
    match (p, outline) {
        (Paint::Solid(c), false) => frame.fill(path, *c),
        (Paint::Solid(c), true) => frame.stroke(path, Stroke::default().with_color(*c).with_width(1.5)),
        (Paint::Grad(g), false) => frame.fill(path, Fill::from(*g)),
        (Paint::Grad(g), true) => {
            frame.stroke(path, Stroke { style: stroke::Style::Gradient(*g), width: 1.5, ..Stroke::default() })
        }
    }
}

enum Paint {
    Solid(Color),
    Grad(Gradient),
}

/// The whole-bar gradient of a pane (vertical by level, or horizontal across `span`).
fn pane_gradient(look: &Look, pane: &Pane, span: (f32, f32)) -> Gradient {
    let stops = look.grad.iced_stops(look.bright, look.alpha, false);
    let mut g = if look.grad.horizontal {
        gradient::Linear::new(Point::new(span.0, 0.0), Point::new(span.1, 0.0))
    } else {
        // t = 0 (loud colour) at the tip of a full bar, 1 at the baseline.
        let far = if pane.flip { pane.baseline() + pane.reach } else { pane.baseline() - pane.reach };
        gradient::Linear::new(Point::new(0.0, far), Point::new(0.0, pane.baseline()))
    };
    for (t, c) in stops {
        g = g.add_stop(t, c);
    }
    Gradient::from(g)
}

/// Draws one channel into `pane`. `span` is the x range of the horizontal gradient.
#[allow(clippy::too_many_arguments)]
fn draw_channel(
    frame: &mut Frame,
    look: &Look,
    pane: &Pane,
    levels: &[f32],
    peaks: &[f32],
    peak_alpha: &[f32],
    span: (f32, f32),
    fill_peaks: bool,
) {
    let s = look.s;
    let n = levels.len();
    if n < 2 {
        return;
    }
    let area = pane.area;
    let w = area.width;
    let whole = pane_gradient(look, pane, span);
    let peak_color = |i: usize| Color {
        a: look.alpha * peak_alpha.get(i).copied().unwrap_or(1.0).clamp(0.0, 1.0) * 0.9,
        ..Color::WHITE
    };
    if s.mode == 10 {
        let x = |i: usize| area.x + w * i as f32 / (n - 1) as f32;
        let line = Path::new(|p| {
            p.move_to(Point::new(x(0), pane.tip(levels[0])));
            for (i, &v) in levels.iter().enumerate().skip(1) {
                p.line_to(Point::new(x(i), pane.tip(v)));
            }
        });
        if s.fill_alpha > 0.0 {
            let area_path = Path::new(|p| {
                p.move_to(Point::new(x(0), pane.baseline()));
                for (i, &v) in levels.iter().enumerate() {
                    p.line_to(Point::new(x(i), pane.tip(v)));
                }
                p.line_to(Point::new(x(n - 1), pane.baseline()));
                p.close();
            });
            let faded = Look { alpha: look.alpha * s.fill_alpha.clamp(0.0, 1.0), ..clone_look(look) };
            if matches!(s.color_mode, ColorMode::Gradient) {
                frame.fill(&area_path, Fill::from(pane_gradient(&faded, pane, span)));
            } else {
                frame.fill(&area_path, faded.solid(0, n, levels.iter().cloned().fold(0.0, f32::max), 0.3));
            }
        }
        if s.line_width > 0.0 {
            if matches!(s.color_mode, ColorMode::Gradient) {
                frame.stroke(
                    &line,
                    Stroke {
                        style: stroke::Style::Gradient(whole),
                        width: s.line_width.max(1.0),
                        ..Stroke::default()
                    },
                );
            } else {
                // One colour per segment.
                for i in 1..n {
                    let seg = Path::line(
                        Point::new(x(i - 1), pane.tip(levels[i - 1])),
                        Point::new(x(i), pane.tip(levels[i])),
                    );
                    let c = look.solid(i, n, levels[i], 1.0 - levels[i]);
                    frame.stroke(&seg, Stroke::default().with_color(c).with_width(s.line_width.max(1.0)));
                }
            }
        }
        if s.peak_line && fill_peaks && peaks.len() == n {
            let peak_path = Path::new(|p| {
                p.move_to(Point::new(x(0), pane.tip(peaks[0])));
                for (i, &v) in peaks.iter().enumerate().skip(1) {
                    p.line_to(Point::new(x(i), pane.tip(v)));
                }
            });
            frame.stroke(
                &peak_path,
                Stroke::default().with_color(Color { a: look.alpha * 0.6, ..Color::WHITE }).with_width(1.0),
            );
        }
        return;
    }

    // Bar modes.
    let slot = w / n as f32;
    let (bw, gap) = if s.mode == 0 { (slot.max(1.0), 0.0) } else { bar_slot(slot, s.bar_space) };
    let led = s.led_bars;
    for (i, &v) in levels.iter().enumerate() {
        let x0 = area.x + i as f32 * slot + gap / 2.0;
        let alpha_scaled = s.alpha_bars || s.lumi_bars;
        let a = if alpha_scaled { v.clamp(0.05, 1.0) } else { 1.0 };
        let reach_h =
            if s.lumi_bars { area.height } else { (v.clamp(0.0, 1.0) * pane.reach).min(area.height) };
        if reach_h < 0.5 && !led {
            continue;
        }
        let solid_mode = !matches!(s.color_mode, ColorMode::Gradient) || led;
        let bar_look = Look { alpha: look.alpha * a, ..clone_look(look) };
        let base = pane.baseline();
        let top = if pane.flip { base + reach_h } else { base - reach_h };
        let rect_for = |y_a: f32, y_b: f32| {
            let (y0, y1) = (y_a.min(y_b), y_a.max(y_b));
            Rectangle::new(Point::new(x0, y0), Size::new(bw, (y1 - y0).max(0.5)))
        };
        let rounded = |r: Rectangle| {
            if s.round_bars {
                let rad = (r.width / 2.0).min(r.height / 2.0);
                Path::rounded_rectangle(r.position(), r.size(), rad.into())
            } else {
                Path::rectangle(r.position(), r.size())
            }
        };
        if led {
            let (count, lit) = leds(area.height, if s.lumi_bars { 1.0 } else { v });
            let seg = area.height / count as f32;
            for k in 0..lit {
                let (a0, a1) = (k as f32 * seg, (k + 1) as f32 * seg - 1.5);
                let (y_a, y_b) = if pane.flip { (base + a0, base + a1) } else { (base - a0, base - a1) };
                let frac = 1.0 - (k as f32 + 0.5) / count as f32; // 0 = top of the bar area
                let colour_level = if s.true_leds { v } else { (k as f32 + 0.5) / count as f32 };
                let c = if s.true_leds {
                    bar_look.solid(i, n, v, 1.0 - v)
                } else {
                    bar_look.solid(i, n, colour_level, frac)
                };
                paint(frame, &rounded(rect_for(y_a, y_b)), &Paint::Solid(c), s.outline_bars);
            }
        } else if solid_mode {
            let c = bar_look.solid(i, n, v, 1.0 - v);
            paint(frame, &rounded(rect_for(base, top)), &Paint::Solid(c), s.outline_bars);
        } else {
            let g = if alpha_scaled { pane_gradient(&bar_look, pane, span) } else { whole };
            paint(frame, &rounded(rect_for(base, top)), &Paint::Grad(g), s.outline_bars);
        }
        if s.show_peaks
            && fill_peaks
            && let Some(&p) = peaks.get(i)
        {
            let y = pane.tip(p);
            let cap = rect_for(y, if pane.flip { y + 2.0 } else { y - 2.0 });
            frame.fill_rectangle(cap.position(), cap.size(), peak_color(i));
        }
    }
}

fn clone_look<'a>(l: &Look<'a>) -> Look<'a> {
    Look { s: l.s, grad: l.grad.clone(), alpha: l.alpha, bright: l.bright }
}

/// Radial spectrum: bars (or the graph) around a ring.
fn draw_radial(frame: &mut Frame, look: &Look, bounds: Size, levels: &[f32], spin: f32, peaks: &[f32]) {
    let s = look.s;
    let n = levels.len();
    if n < 2 {
        return;
    }
    let centre = Point::new(bounds.width / 2.0, bounds.height / 2.0);
    let outer = (bounds.width.min(bounds.height) / 2.0 - 2.0).max(4.0);
    let ring = outer * s.radius.clamp(0.1, 1.0);
    let reach = if s.radial_invert { ring * 0.9 } else { (outer - ring).max(2.0) };
    let sign = if s.radial_invert { -1.0 } else { 1.0 };
    let angle = |i: f32| spin + std::f32::consts::TAU * i / n as f32 - std::f32::consts::FRAC_PI_2;
    let at = |i: f32, r: f32| Point::new(centre.x + r * angle(i).cos(), centre.y + r * angle(i).sin());
    if s.mode == 10 {
        let pts: Vec<Point> = (0..=n).map(|i| at(i as f32, ring + sign * levels[i % n] * reach)).collect();
        if s.fill_alpha > 0.0 {
            let poly = Path::new(|p| {
                p.move_to(pts[0]);
                for q in &pts[1..] {
                    p.line_to(*q);
                }
                p.close();
            });
            let c = look.solid(n / 2, n, levels.iter().cloned().fold(0.0, f32::max), 0.3);
            frame.fill(&poly, Color { a: c.a * s.fill_alpha, ..c });
        }
        if s.line_width > 0.0 {
            for i in 0..n {
                let c = look.solid(i, n, levels[i], 1.0 - levels[i]);
                frame.stroke(
                    &Path::line(pts[i], pts[i + 1]),
                    Stroke {
                        line_cap: LineCap::Round,
                        ..Stroke::default().with_color(c).with_width(s.line_width.max(1.0))
                    },
                );
            }
        }
        if s.peak_line && peaks.len() == n {
            let ring_path = Path::new(|p| {
                p.move_to(at(0.0, ring + sign * peaks[0] * reach));
                for i in 1..=n {
                    p.line_to(at(i as f32, ring + sign * peaks[i % n] * reach));
                }
            });
            frame.stroke(
                &ring_path,
                Stroke::default().with_color(Color { a: look.alpha * 0.6, ..Color::WHITE }).with_width(1.0),
            );
        }
        return;
    }
    let width =
        (std::f32::consts::TAU * ring / n as f32 * (1.0 - s.bar_space.clamp(0.0, 0.9)).max(0.1)).max(1.0);
    for (i, &v) in levels.iter().enumerate() {
        let a = if s.alpha_bars || s.lumi_bars { v.clamp(0.05, 1.0) } else { 1.0 };
        let bar_look = Look { alpha: look.alpha * a, ..clone_look(look) };
        let len = if s.lumi_bars { reach } else { v * reach };
        if len < 0.5 {
            continue;
        }
        let c = bar_look.solid(i, n, v, 1.0 - v);
        let seg = Path::line(at(i as f32, ring), at(i as f32, ring + sign * len));
        if s.led_bars {
            let (count, lit) = leds(reach, if s.lumi_bars { 1.0 } else { v });
            for k in 0..lit {
                let (r0, r1) =
                    (k as f32 / count as f32 * reach, ((k + 1) as f32 / count as f32 * reach - 1.5).max(0.5));
                let frac = 1.0 - (k as f32 + 0.5) / count as f32;
                let lc =
                    if s.true_leds { c } else { bar_look.solid(i, n, (k as f32 + 0.5) / count as f32, frac) };
                frame.stroke(
                    &Path::line(at(i as f32, ring + sign * r0), at(i as f32, ring + sign * r1)),
                    Stroke::default().with_color(lc).with_width(width),
                );
            }
        } else {
            frame.stroke(&seg, Stroke::default().with_color(c).with_width(width));
        }
    }
}

impl canvas::Program<Message> for Graph<'_> {
    type State = Meter;

    fn draw(
        &self,
        meter: &Meter,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let s = self.settings;
        let text_color = theme.palette().text;
        let (w, full_h) = (bounds.width, bounds.height);
        let scale_h = if s.show_scale_x || s.note_labels { 14.0 } else { 0.0 };
        let area_h = (full_h - scale_h).max(1.0);
        let cfg = crate::visualizer::analyzer_config(s);
        let left = Channel {
            levels: self.spectrum.levels.clone(),
            peaks: self.spectrum.peaks.clone(),
            peak_alpha: self.spectrum.peak_alpha.clone(),
        };
        let right = self.spectrum.right.clone();
        let spin = s.spin_speed * std::f32::consts::TAU / 60.0 * started().elapsed().as_secs_f32();
        let layout = if right.is_some() { s.channel_layout } else { ChannelLayout::Single };
        let grad_of = |name: &Option<String>| Grad::resolve(s, name.as_deref().unwrap_or(&s.gradient));
        let look = |grad: Grad, alpha: f32, bright: f32| Look { s, grad, alpha: s.opacity * alpha, bright };
        let layer = |ch: &Channel| {
            (mirrored(&ch.levels, s.mirror), mirrored(&ch.peaks, s.mirror), {
                if ch.peak_alpha.is_empty() { Vec::new() } else { mirrored(&ch.peak_alpha, s.mirror) }
            })
        };
        let (l_levels, l_peaks, l_alpha) = layer(&left);
        let r_layer = right.as_ref().map(layer);

        if s.radial {
            let g_l = if right.is_some() { grad_of(&s.gradient_left) } else { Grad::resolve(s, &s.gradient) };
            let size = Size::new(w, area_h);
            let blend = if r_layer.is_some() { 0.8 } else { 1.0 };
            draw_radial(&mut frame, &look(g_l, blend, 1.0), size, &l_levels, spin, &l_peaks);
            if let Some((rl, rp, _)) = &r_layer {
                draw_radial(&mut frame, &look(grad_of(&s.gradient_right), blend, 1.0), size, rl, spin, rp);
            }
        } else {
            let reflex_on = s.reflex_ratio > 0.0 && layout != ChannelLayout::DualVertical;
            let main_h = if reflex_on { area_h * (1.0 - s.reflex_ratio) } else { area_h };
            let main = Rectangle::new(Point::ORIGIN, Size::new(w, main_h));
            let reflection = Rectangle::new(Point::new(0.0, main_h), Size::new(w, area_h - main_h));
            let dual = r_layer.is_some();
            for pass in 0..2 {
                // Pass 0 the analyzer, pass 1 its reflection.
                if pass == 1 && !reflex_on {
                    break;
                }
                let (zone, flip) = if pass == 0 {
                    (
                        if layout == ChannelLayout::DualVertical {
                            Rectangle::new(Point::ORIGIN, Size::new(w, area_h))
                        } else {
                            main
                        },
                        false,
                    )
                } else {
                    (reflection, true)
                };
                let (alpha, bright) = if pass == 0 { (1.0, 1.0) } else { (s.reflex_alpha, s.reflex_bright) };
                let reach_of = |p: &Pane| if pass == 1 && !s.reflex_fit { main_h } else { p.area.height };
                for (area, ch, vflip) in panes(layout, zone) {
                    let flip = flip || vflip;
                    let mut pane = Pane { area, flip, reach: 0.0 };
                    pane.reach = if pass == 1 { reach_of(&pane) } else { area.height };
                    if layout == ChannelLayout::DualVertical {
                        // Back to back around the middle: no empty band between the channels.
                        pane.reach = area.height;
                    }
                    let span = if s.split_gradient || layout != ChannelLayout::DualHorizontal {
                        (0.0, w)
                    } else {
                        (area.x, area.x + area.width)
                    };
                    let one = |frame: &mut Frame, which: usize, alpha_mul: f32| {
                        let (lv, pk, pa, name) = if which == 0 {
                            (&l_levels, &l_peaks, &l_alpha, &s.gradient_left)
                        } else {
                            let r = r_layer.as_ref().expect("dual");
                            (&r.0, &r.1, &r.2, &s.gradient_right)
                        };
                        let grad = if dual { grad_of(name) } else { Grad::resolve(s, &s.gradient) };
                        draw_channel(
                            frame,
                            &look(grad, alpha * alpha_mul, bright),
                            &pane,
                            lv,
                            pk,
                            pa,
                            span,
                            pass == 0,
                        );
                    };
                    match layout {
                        ChannelLayout::DualCombined if dual => {
                            one(&mut frame, 0, 0.85);
                            one(&mut frame, 1, 0.85);
                        }
                        _ => one(&mut frame, ch.min(usize::from(dual)), 1.0),
                    }
                }
            }
            if s.show_scale_y {
                let mut label_color = text_color;
                label_color.a = 0.45;
                for (level, text) in y_ticks(s.min_db, s.max_db, s.linear_amplitude, s.linear_boost) {
                    let y = main_h - level * main_h;
                    frame.fill_rectangle(
                        Point::new(0.0, y),
                        Size::new(w, 1.0),
                        Color { a: 0.1, ..text_color },
                    );
                    frame.fill_text(Text {
                        content: text,
                        position: Point::new(2.0, (y - 11.0).max(0.0)),
                        color: label_color,
                        size: 10.0.into(),
                        ..Text::default()
                    });
                }
            }
        }
        if scale_h > 0.0 {
            let mut c = text_color;
            c.a = 0.5;
            for (pos, text) in x_ticks(&cfg, s.note_labels) {
                frame.fill_text(Text {
                    content: text,
                    position: Point::new(
                        (pos * w - 8.0).clamp(0.0, (w - 18.0).max(0.0)),
                        full_h - scale_h + 2.0,
                    ),
                    color: c,
                    size: 10.0.into(),
                    ..Text::default()
                });
            }
        }
        if s.show_fps {
            let now = Instant::now();
            if let Some(last) = meter.last.replace(Some(now)) {
                let dt = now.duration_since(last).as_secs_f32().max(1e-3);
                meter.fps.set(meter.fps.get() * 0.9 + 0.1 / dt);
            }
            frame.fill_text(Text {
                content: format!("{:.0} FPS", meter.fps.get()),
                position: Point::new(w - 52.0, 2.0),
                color: Color { a: 0.7, ..text_color },
                size: 11.0.into(),
                ..Text::default()
            });
        }
        vec![frame.into_geometry()]
    }
}

/// The visualizer filling its space (full screen tab).
pub fn view<'a>(spectrum: &'a Spectrum, settings: &'a Visualizer) -> Element<'a, Message> {
    Canvas::new(Graph { spectrum, settings }).width(Length::Fill).height(Length::Fill).into()
}

/// The strip at the bottom of the queue panel (same options as the full-screen one).
pub fn strip<'a>(spectrum: &'a Spectrum, settings: &'a Visualizer) -> Element<'a, Message> {
    view(spectrum, settings)
}

// ---------------------------------------------------------------- options modal

mod modal {
    use super::*;
    use crate::app::viz::VizMsg;
    use crate::app::{Listen, Message};
    use crate::style::{self, metrics};
    use crate::ui::overlay::ModalParts;
    use crate::ui::{label, label_w};
    use iced::font::Weight;
    use iced::widget::{
        Column, button, column, container, pick_list, row, scrollable, slider, space, text_input, toggler,
    };
    use iced::{Alignment, Padding};
    use serde_json::json;

    /// A pick-list entry: the stored value and the text shown.
    #[derive(Debug, Clone, PartialEq)]
    pub struct Choice(pub String, pub String);

    impl std::fmt::Display for Choice {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.1)
        }
    }

    pub const MODES: [(u8, &str); 10] = [
        (0, "[0] Discrete Frequencies"),
        (1, "[1] 1/24th octave / 240 bands"),
        (2, "[2] 1/12th octave / 120 bands"),
        (3, "[3] 1/8th octave / 80 bands"),
        (4, "[4] 1/6th octave / 60 bands"),
        (5, "[5] 1/4th octave / 40 bands"),
        (6, "[6] 1/3rd octave / 30 bands"),
        (7, "[7] Half octave / 20 bands"),
        (8, "[8] Full octave / 10 bands"),
        (10, "[10] Line / Area graph"),
    ];

    /// Rounds to a few decimals so saved values stay readable.
    fn tidy(v: f32, step: f32) -> f64 {
        let step = f64::from(step.max(1e-4));
        let snapped = (f64::from(v) / step).round() * step;
        (snapped * 1e4).round() / 1e4
    }

    fn setting_row<'a>(state: &Listen, name: &'a str, control: Element<'a, Message>) -> Element<'a, Message> {
        row![label(name, metrics::font_md(), state.tokens.foreground), space::horizontal(), control]
            .spacing(metrics::space_md())
            .align_y(Alignment::Center)
            .into()
    }

    fn heading<'a>(state: &Listen, name: &'a str) -> Element<'a, Message> {
        container(label_w(name, metrics::font_md(), state.tokens.primary, Weight::Semibold))
            .padding(Padding { top: metrics::space_sm(), ..Padding::ZERO })
            .into()
    }

    fn toggle<'a>(state: &Listen, name: &'a str, key: &'static str, value: bool) -> Element<'a, Message> {
        setting_row(
            state,
            name,
            toggler(value).on_toggle(move |v| Message::Viz(VizMsg::Set(key, json!(v)))).size(18).into(),
        )
    }

    fn number<'a>(
        state: &Listen,
        name: &'a str,
        key: &'static str,
        value: f32,
        range: std::ops::RangeInclusive<f32>,
        step: f32,
        unit: &'static str,
    ) -> Element<'a, Message> {
        let t = state.tokens;
        let shown = if step >= 1.0 { format!("{value:.0}{unit}") } else { format!("{value:.2}{unit}") };
        setting_row(
            state,
            name,
            row![
                slider(range, value, move |v| Message::Viz(VizMsg::Set(key, json!(tidy(v, step)))))
                    .step(step)
                    .width(180),
                container(label(shown, metrics::font_sm(), t.muted)).width(70).align_right(70),
            ]
            .spacing(metrics::space_sm())
            .align_y(Alignment::Center)
            .into(),
        )
    }

    fn integer<'a>(
        state: &Listen,
        name: &'a str,
        key: &'static str,
        value: u32,
        range: std::ops::RangeInclusive<u32>,
        step: u32,
        unit: &'static str,
    ) -> Element<'a, Message> {
        let t = state.tokens;
        setting_row(
            state,
            name,
            row![
                slider(*range.start() as f32..=*range.end() as f32, value as f32, move |v| {
                    Message::Viz(VizMsg::Set(key, json!(v.round() as u32)))
                })
                .step(step as f32)
                .width(180),
                container(label(format!("{value}{unit}"), metrics::font_sm(), t.muted))
                    .width(70)
                    .align_right(70),
            ]
            .spacing(metrics::space_sm())
            .align_y(Alignment::Center)
            .into(),
        )
    }

    fn choose<'a>(
        state: &Listen,
        name: &'a str,
        key: &'static str,
        options: Vec<(String, String)>,
        current: &str,
        as_number: bool,
    ) -> Element<'a, Message> {
        let list: Vec<Choice> = options.into_iter().map(|(v, l)| Choice(v, l)).collect();
        let selected = list.iter().find(|c| c.0 == current).cloned();
        setting_row(
            state,
            name,
            pick_list(list, selected, move |c: Choice| {
                let value =
                    if as_number { c.0.parse::<f64>().map_or(json!(c.0), |n| json!(n)) } else { json!(c.0) };
                Message::Viz(VizMsg::Set(key, value))
            })
            .width(240)
            .into(),
        )
    }

    fn pairs<const N: usize>(items: [(&str, &str); N]) -> Vec<(String, String)> {
        items.into_iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    fn gradient_options(s: &listen_core::settings::Visualizer, none: bool) -> Vec<(String, String)> {
        let mut v = Vec::new();
        if none {
            v.push((String::new(), "Same as gradient".to_string()));
        }
        for name in builtin_gradients() {
            let mut label = name.to_string();
            label[..1].make_ascii_uppercase();
            v.push((name.to_string(), label));
        }
        v.extend(s.custom_gradients.iter().map(|g| (g.name.clone(), g.name.clone())));
        v
    }

    fn small_button<'a>(state: &Listen, text: &'a str, msg: Option<Message>) -> Element<'a, Message> {
        button(label(text, metrics::font_sm(), state.tokens.foreground))
            .padding(Padding::from([4.0, 10.0]))
            .style(style::default_button(state.tokens))
            .on_press_maybe(msg)
            .into()
    }

    fn presets<'a>(state: &'a Listen) -> Vec<Element<'a, Message>> {
        let t = state.tokens;
        let s = &state.config.settings.visualizer;
        let all: Vec<String> = s.all_presets().into_iter().map(|p| p.name).collect();
        let user = state.viz.preset.as_ref().is_some_and(|p| s.presets.iter().any(|u| &u.name == p));
        vec![
            heading(state, "Presets"),
            setting_row(
                state,
                "Select Preset",
                pick_list(all, state.viz.preset.clone(), |n| Message::Viz(VizMsg::PresetPick(n)))
                    .placeholder("Pick a preset")
                    .width(240)
                    .into(),
            ),
            setting_row(
                state,
                "Preset Name",
                row![
                    text_input(crate::i18n::tr("Enter preset name"), &state.viz.preset_name)
                        .on_input(|n| Message::Viz(VizMsg::PresetName(n)))
                        .style(style::input(t))
                        .width(160),
                    small_button(state, "Save as Preset", Some(Message::Viz(VizMsg::PresetSave))),
                    small_button(state, "Delete", user.then_some(Message::Viz(VizMsg::PresetDelete))),
                ]
                .spacing(metrics::space_sm())
                .into(),
            ),
            setting_row(
                state,
                "Configuration",
                row![
                    small_button(state, "Copy Configuration", Some(Message::Viz(VizMsg::CopyConfig))),
                    small_button(state, "Paste Configuration", Some(Message::Viz(VizMsg::PasteConfig))),
                ]
                .spacing(metrics::space_sm())
                .into(),
            ),
        ]
    }

    fn gradients<'a>(state: &'a Listen) -> Vec<Element<'a, Message>> {
        let t = state.tokens;
        let s = &state.config.settings.visualizer;
        let mut out = vec![heading(state, "Custom Gradients")];
        for (i, g) in s.custom_gradients.iter().enumerate() {
            let horizontal = g.dir == "h";
            let mut stops = Column::new().spacing(metrics::space_xs());
            for (j, stop) in g.color_stops.iter().enumerate() {
                let swatch = parse_color(&stop.color);
                stops = stops.push(
                    row![
                        container(space::horizontal()).width(22).height(22).style(move |_| {
                            container::Style {
                                background: Some(swatch.into()),
                                border: iced::Border { color: t.border, width: 1.0, radius: 3.0.into() },
                                ..Default::default()
                            }
                        }),
                        text_input(crate::i18n::tr("#rrggbb"), &stop.color)
                            .on_input(move |c| Message::Viz(VizMsg::StopColor(i, j, c)))
                            .style(style::input(t))
                            .width(110),
                        small_button(
                            state,
                            "Remove",
                            (g.color_stops.len() > 1).then_some(Message::Viz(VizMsg::StopRemove(i, j)))
                        ),
                    ]
                    .spacing(metrics::space_sm())
                    .align_y(Alignment::Center),
                );
            }
            out.push(
                container(
                    column![
                        row![
                            text_input(crate::i18n::tr("Gradient Name"), &g.name)
                                .on_input(move |n| Message::Viz(VizMsg::GradientName(i, n)))
                                .style(style::input(t))
                                .width(160),
                            pick_list(
                                vec![
                                    Choice("v".into(), "Vertical".into()),
                                    Choice("h".into(), "Horizontal".into())
                                ],
                                Some(Choice(
                                    if horizontal { "h" } else { "v" }.into(),
                                    if horizontal { "Horizontal" } else { "Vertical" }.into()
                                )),
                                move |c: Choice| Message::Viz(VizMsg::GradientHorizontal(i, c.0 == "h")),
                            ),
                            space::horizontal(),
                            small_button(state, "Copy", Some(Message::Viz(VizMsg::GradientCopy(i)))),
                            small_button(state, "Remove", Some(Message::Viz(VizMsg::GradientRemove(i)))),
                        ]
                        .spacing(metrics::space_sm())
                        .align_y(Alignment::Center),
                        stops,
                        small_button(state, "Add Color", Some(Message::Viz(VizMsg::StopAdd(i)))),
                    ]
                    .spacing(metrics::space_sm()),
                )
                .padding(metrics::space_sm())
                .style(move |_| container::Style {
                    border: iced::Border { color: t.border, width: 1.0, radius: 4.0.into() },
                    ..Default::default()
                })
                .into(),
            );
        }
        out.push(
            row![
                small_button(state, "Add Custom Gradient", Some(Message::Viz(VizMsg::GradientAdd))),
                small_button(state, "Paste Gradient", Some(Message::Viz(VizMsg::GradientPaste))),
            ]
            .spacing(metrics::space_sm())
            .into(),
        );
        out
    }

    fn options<'a>(state: &'a Listen) -> Vec<Element<'a, Message>> {
        let s = &state.config.settings.visualizer;
        let bars = s.mode != 10;
        let mut v: Vec<Element<'a, Message>> = vec![heading(state, "General")];
        v.push(choose(
            state,
            "Mode",
            "mode",
            MODES.iter().map(|(m, l)| (m.to_string(), l.to_string())).collect(),
            &s.mode.to_string(),
            true,
        ));
        if s.mode == 10 {
            v.push(number(state, "Line Width", "lineWidth", s.line_width, 0.0..=10.0, 0.1, ""));
            v.push(number(state, "Fill Alpha", "fillAlpha", s.fill_alpha, 0.0..=1.0, 0.05, ""));
        } else if s.mode >= 1 {
            v.push(number(state, "Bar Space", "barSpace", s.bar_space, 0.0..=1.0, 0.05, ""));
        }
        v.push(choose(
            state,
            "Channel Layout",
            "channelLayout",
            pairs([
                ("single", "Single"),
                ("dual-combined", "Dual-Combined"),
                ("dual-horizontal", "Dual-Horizontal"),
                ("dual-vertical", "Dual-Vertical"),
            ]),
            &json_str(serde_json::to_value(s.channel_layout)),
            false,
        ));
        v.push(integer(state, "Max FPS", "maxFPS", s.max_fps, 0..=60, 1, ""));
        v.push(number(state, "Opacity", "opacity", s.opacity, 0.0..=1.0, 0.05, ""));

        v.push(heading(state, "Colors"));
        v.push(choose(
            state,
            "Color Mode",
            "colorMode",
            pairs([("gradient", "Gradient"), ("bar-index", "Bar-Index"), ("bar-level", "Bar-Level")]),
            &json_str(serde_json::to_value(s.color_mode)),
            false,
        ));
        v.push(choose(state, "Gradient", "gradient", gradient_options(s, false), &s.gradient, false));
        if s.channel_layout != listen_core::settings::ChannelLayout::Single {
            v.push(choose(
                state,
                "Gradient Left",
                "gradientLeft",
                gradient_options(s, true),
                s.gradient_left.as_deref().unwrap_or(""),
                false,
            ));
            v.push(choose(
                state,
                "Gradient Right",
                "gradientRight",
                gradient_options(s, true),
                s.gradient_right.as_deref().unwrap_or(""),
                false,
            ));
            v.push(toggle(state, "Split Gradient", "splitGradient", s.split_gradient));
        }

        v.push(heading(state, "FFT"));
        v.push(choose(
            state,
            "FFT Size",
            "fftSize",
            [1024, 2048, 4096, 8192, 16384, 32768].map(|n| (n.to_string(), n.to_string())).to_vec(),
            &s.fft_size.to_string(),
            true,
        ));
        v.push(number(state, "Smoothing", "smoothing", s.smoothing, 0.0..=0.95, 0.05, ""));

        v.push(heading(state, "Frequency range and scaling"));
        v.push(choose(
            state,
            "Minimum Frequency",
            "minFreq",
            [20, 30, 40, 50].map(|n| (n.to_string(), format!("{n} Hz"))).to_vec(),
            &format!("{:.0}", s.min_freq),
            true,
        ));
        v.push(choose(
            state,
            "Maximum Frequency",
            "maxFreq",
            [8000, 10000, 15000, 20000, 22050].map(|n| (n.to_string(), format!("{n} Hz"))).to_vec(),
            &format!("{:.0}", s.max_freq),
            true,
        ));
        v.push(choose(
            state,
            "Frequency Scale",
            "frequencyScale",
            pairs([
                ("log", "Log Scale"),
                ("linear", "Linear Scale"),
                ("bark", "Bark Scale"),
                ("mel", "Mel Scale"),
            ]),
            &json_str(serde_json::to_value(s.frequency_scale)),
            false,
        ));

        v.push(heading(state, "Sensitivity"));
        v.push(choose(
            state,
            "Weighting Filter",
            "weightingFilter",
            pairs([("", "None"), ("A", "A"), ("B", "B"), ("C", "C"), ("D", "D"), ("Z", "Z")]),
            &json_str(serde_json::to_value(s.weighting_filter)),
            false,
        ));
        v.push(number(state, "Minimum Decibels", "minDecibels", s.min_db, -120.0..=-60.0, 1.0, " dB"));
        v.push(number(state, "Maximum Decibels", "maxDecibels", s.max_db, -40.0..=0.0, 1.0, " dB"));
        v.push(toggle(state, "Linear Amplitude", "linearAmplitude", s.linear_amplitude));
        if s.linear_amplitude {
            v.push(number(state, "Linear Boost", "linearBoost", s.linear_boost, 1.0..=5.0, 0.1, ""));
        }
        v.push(number(state, "Volume", "volume", s.volume, 0.0..=4.0, 0.1, ""));

        v.push(heading(state, "Peak Behavior"));
        if bars {
            v.push(toggle(state, "Show Peaks", "showPeaks", s.show_peaks));
        } else {
            v.push(toggle(state, "Peak Line", "peakLine", s.peak_line));
        }
        v.push(toggle(state, "Fade Peaks", "fadePeaks", s.fade_peaks));
        v.push(number(state, "Gravity", "gravity", s.gravity, 1.0..=50.0, 0.5, ""));
        v.push(integer(state, "Peak Fade Time (ms)", "peakFadeTime", s.peak_fade_time, 100..=2000, 50, ""));
        v.push(integer(state, "Peak Hold Time (ms)", "peakHoldTime", s.peak_hold_time, 0..=2000, 50, ""));

        v.push(heading(state, "Radial Spectrum"));
        v.push(toggle(state, "Radial", "radial", s.radial));
        if s.radial {
            v.push(toggle(state, "Radial Invert", "radialInvert", s.radial_invert));
            v.push(number(state, "Spin Speed", "spinSpeed", s.spin_speed, -3.0..=3.0, 0.1, " rpm"));
            v.push(number(state, "Radius", "radius", s.radius, 0.1..=1.0, 0.05, ""));
        }

        v.push(heading(state, "Reflex"));
        v.push(number(state, "Reflex Ratio", "reflexRatio", s.reflex_ratio, 0.0..=0.9, 0.05, ""));
        v.push(number(state, "Reflex Alpha", "reflexAlpha", s.reflex_alpha, 0.0..=1.0, 0.05, ""));
        v.push(number(state, "Reflex Brightness", "reflexBright", s.reflex_bright, 0.0..=2.0, 0.05, ""));
        v.push(toggle(state, "Reflex Fit", "reflexFit", s.reflex_fit));
        v.push(choose(
            state,
            "Mirror",
            "mirror",
            pairs([("-1", "Left"), ("0", "Off"), ("1", "Right")]),
            &s.mirror.to_string(),
            true,
        ));

        v.push(heading(state, "Miscellaneous Settings"));
        if bars {
            v.push(toggle(state, "LED Bars", "ledBars", s.led_bars));
            if s.led_bars {
                v.push(toggle(state, "True LEDs", "trueLeds", s.true_leds));
            }
            v.push(toggle(state, "Alpha Bars", "alphaBars", s.alpha_bars));
            v.push(toggle(state, "Lumi Bars", "lumiBars", s.lumi_bars));
            v.push(toggle(state, "Outline Bars", "outlineBars", s.outline_bars));
            v.push(toggle(state, "Round Bars", "roundBars", s.round_bars));
            if s.mode == 0 {
                v.push(toggle(state, "Low Resolution", "loRes", s.lo_res));
            } else {
                v.push(toggle(state, "ANSI Bands", "ansiBands", s.ansi_bands));
            }
        }
        v.push(toggle(state, "Show Scale X", "showScaleX", s.show_scale_x));
        v.push(toggle(state, "Note Labels", "noteLabels", s.note_labels));
        v.push(toggle(state, "Show Scale Y", "showScaleY", s.show_scale_y));
        v.push(toggle(state, "Show FPS", "showFPS", s.show_fps));
        v
    }

    /// The serde name of a unit-only enum value (what Feishin's JSON uses).
    fn json_str(v: serde_json::Result<serde_json::Value>) -> String {
        v.ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default()
    }

    /// Body of the “audioMotion settings” modal.
    pub fn settings_modal(state: &Listen) -> Option<ModalParts<'_>> {
        let t = state.tokens;
        let mut col =
            Column::new().spacing(metrics::space_sm()).padding(Padding { right: 14.0, ..Padding::ZERO });
        for e in presets(state).into_iter().chain(options(state)).chain(gradients(state)) {
            col = col.push(e);
        }
        let body =
            scrollable(col).height(Length::Fixed((state.window_size.height - 220.0).clamp(240.0, 560.0)));
        let reset = button(label("Reset to default", metrics::font_md(), t.foreground))
            .padding(Padding::from([8.0, 16.0]))
            .style(style::default_button(t))
            .on_press(Message::Viz(VizMsg::Reset));
        let close = button(label_w("Close", metrics::font_md(), t.background, Weight::Semibold))
            .padding(Padding::from([8.0, 16.0]))
            .style(style::primary_button(t))
            .on_press(Message::Overlay(crate::app::overlay::OverlayMsg::CloseModal));
        Some(("Visualizer settings", body.into(), vec![reset.into(), close.into()]))
    }

    /// The gear button that opens the settings modal.
    pub fn settings_button(state: &Listen) -> Element<'_, Message> {
        let t = state.tokens;
        crate::ui::tip::tip(
            state,
            button(crate::ui::icon(
                crate::icons::Icon::LuSlidersHorizontal,
                metrics::font_md(),
                t.foreground,
            ))
            .padding(6)
            .style(style::icon_button(t, false))
            .on_press(Message::Viz(VizMsg::Open)),
            "Visualizer settings",
            None,
            crate::ui::tip::Position::Bottom,
        )
    }
}

pub use modal::{settings_button, settings_modal};

#[cfg(test)]
mod tests {
    use super::*;
    use listen_core::settings::{CustomGradient, GradientStop};

    fn near(a: Color, b: Color) -> bool {
        (a.r - b.r).abs() < 0.01 && (a.g - b.g).abs() < 0.01 && (a.b - b.b).abs() < 0.01
    }

    #[test]
    fn colours_parse_names_and_short_hex() {
        assert!(near(parse_color("#f00"), Color::from_rgb8(255, 0, 0)));
        assert!(near(parse_color("#aa3355"), Color::from_rgb8(0xaa, 0x33, 0x55)));
        assert!(near(parse_color("a35"), Color::from_rgb8(0xaa, 0x33, 0x55)));
        assert!(near(parse_color("SteelBlue"), Color::from_rgb8(0x46, 0x82, 0xb4)));
        assert!(near(parse_color("nonsense"), Color::WHITE), "unknown colours are white, not a crash");
    }

    #[test]
    fn built_in_gradients_run_top_to_bottom() {
        let v = Visualizer::default();
        let prism = Grad::resolve(&v, "prism");
        assert!(!prism.horizontal);
        assert!(near(prism.at(0.0), Color::from_rgb8(0xaa, 0x33, 0x55)), "loud end is red");
        assert!(near(prism.at(1.0), Color::from_rgb8(0x33, 0x66, 0xbb)), "quiet end is blue");
        assert!(Grad::resolve(&v, "rainbow").horizontal);
        assert!(near(Grad::resolve(&v, "steelblue").at(0.7), Color::from_rgb8(0x46, 0x82, 0xb4)));
        assert_eq!(Grad::resolve(&v, "doesn't exist"), prism, "unknown names fall back to prism");
        for name in builtin_gradients() {
            assert!(
                Grad::resolve(&v, name).iced_stops(1.0, 1.0, false).len() <= 8,
                "{name}: iced allows 8 stops"
            );
        }
    }

    #[test]
    fn classic_uses_levels_in_bar_level_mode() {
        let classic = Grad::resolve(&Visualizer::default(), "classic");
        assert!(near(classic.at_level(1.0), Color::from_rgb8(255, 0, 0)), "loudest red");
        assert!(near(classic.at_level(0.85), Color::from_rgb8(255, 255, 0)));
        assert!(near(classic.at_level(0.475), Color::from_rgb8(0, 255, 0)));
        assert!(near(classic.at_level(0.0), Color::from_rgb8(0, 255, 0)), "below the last stop stays green");
        let mid = classic.at_level(0.66);
        assert!(mid.r > 0.3 && mid.g > 0.9, "between green and yellow: {mid:?}");
    }

    #[test]
    fn custom_gradients_win_over_built_ins() {
        let mut v = Visualizer::default();
        v.custom_gradients.push(CustomGradient {
            name: "prism".into(),
            dir: "h".into(),
            color_stops: vec![
                GradientStop { color: "#000".into(), pos: None, level: None },
                GradientStop { color: "#fff".into(), pos: None, level: None },
            ],
        });
        let g = Grad::resolve(&v, "prism");
        assert!(g.horizontal);
        assert!(near(g.at(0.5), Color::from_rgb(0.5, 0.5, 0.5)));
        assert!(
            near(g.cycle(0), Color::BLACK)
                && near(g.cycle(1), Color::WHITE)
                && near(g.cycle(2), Color::BLACK)
        );
        // A custom gradient with no stops doesn't break drawing.
        v.custom_gradients[0].color_stops.clear();
        assert!(!Grad::resolve(&v, "prism").horizontal);
    }

    #[test]
    fn mirror_squeezes_and_reflects() {
        let l = [1.0, 0.0, 0.5, 0.25];
        assert_eq!(mirrored(&l, 0), l);
        assert_eq!(mirrored(&l, -1), [1.0, 0.5, 0.5, 1.0], "original left, mirror right");
        assert_eq!(mirrored(&l, 1), [0.5, 1.0, 1.0, 0.5], "mirror left, original right");
        assert_eq!(mirrored(&[0.3], -1), [0.3]);
    }

    #[test]
    fn channel_layouts_split_the_canvas() {
        let area = Rectangle::new(Point::new(10.0, 20.0), Size::new(200.0, 100.0));
        assert_eq!(panes(ChannelLayout::Single, area), [(area, 0, false)]);
        assert_eq!(panes(ChannelLayout::DualCombined, area), [(area, 0, false)]);
        let h = panes(ChannelLayout::DualHorizontal, area);
        assert_eq!(h.len(), 2);
        assert_eq!((h[0].0.width, h[1].0.x, h[1].1), (100.0, 110.0, 1));
        let v = panes(ChannelLayout::DualVertical, area);
        assert_eq!((v[0].0.height, v[1].0.y), (50.0, 70.0));
        assert!(!v[0].2 && v[1].2, "the right channel grows down from the middle");
    }

    #[test]
    fn bar_space_is_a_share_or_pixels() {
        assert_eq!(bar_slot(10.0, 0.0), (10.0, 0.0));
        assert_eq!(bar_slot(10.0, 0.5), (5.0, 5.0));
        assert_eq!(bar_slot(10.0, 3.0), (7.0, 3.0), "≥ 1 is pixels");
        let (w, gap) = bar_slot(4.0, 9.0);
        assert!(w >= 1.0 && gap <= 3.0, "never swallows the bar");
    }

    #[test]
    fn leds_light_up_with_the_level() {
        let (count, lit) = leds(100.0, 0.5);
        assert_eq!(count, 12);
        assert_eq!(lit, 6);
        assert_eq!(leds(100.0, 0.0).1, 0);
        assert_eq!(leds(100.0, 1.0).1, count);
        assert_eq!(leds(10.0, 1.0).0, 4, "at least a few LEDs in a small area");
        assert_eq!(leds(1000.0, 1.0).0, 40);
    }

    #[test]
    fn scale_ticks_follow_the_frequency_scale() {
        let cfg = |scale, octave| listen_visualizer::Config {
            scale,
            octave,
            ..listen_visualizer::Config::default()
        };
        let log = x_ticks(&cfg(listen_visualizer::FreqScale::Log, None), false);
        assert_eq!(log.first().map(|t| t.1.as_str()), Some("20"));
        assert!(log.iter().any(|t| t.1 == "1k") && log.iter().any(|t| t.1 == "20k"));
        assert!(log.windows(2).all(|w| w[0].0 < w[1].0), "left to right");
        let lin = x_ticks(&cfg(listen_visualizer::FreqScale::Linear, None), false);
        let pos = |ticks: &[(f32, String)], text: &str| ticks.iter().find(|t| t.1 == text).unwrap().0;
        assert!(
            pos(&lin, "1k") < 0.1 && pos(&log, "1k") > 0.5,
            "1 kHz is near the left edge on a linear scale"
        );
        // Octave bands: evenly spaced slots.
        let oct = x_ticks(
            &cfg(
                listen_visualizer::FreqScale::Log,
                Some(listen_visualizer::Octave { per_octave: 1, ansi: false }),
            ),
            false,
        );
        assert!(oct.iter().all(|t| (0.0..=1.0).contains(&t.0)));
        let notes = x_ticks(&cfg(listen_visualizer::FreqScale::Log, None), true);
        assert!(notes.iter().all(|t| t.1.starts_with('C')) && notes.len() >= 8, "{notes:?}");
    }

    #[test]
    fn level_ticks_every_ten_decibels() {
        let t = y_ticks(-85.0, -25.0, false, 4.0);
        assert_eq!(
            t.iter().map(|t| t.1.as_str()).collect::<Vec<_>>(),
            ["-80", "-70", "-60", "-50", "-40", "-30"]
        );
        assert!(t.iter().all(|t| (0.0..=1.0).contains(&t.0)));
        let lin = y_ticks(-85.0, -25.0, true, 1.0);
        assert_eq!(lin.last().map(|t| (t.1.as_str(), t.0)), Some(("100%", 1.0)));
    }
}
