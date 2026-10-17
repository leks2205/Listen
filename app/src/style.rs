//! Visual tokens (Feishin theme colors, layout metrics) and the iced style functions built from them.

use iced::widget::{button, container, scrollable, slider, svg, text_input};
use iced::{Background, Border, Color, Shadow, Theme, border};
use listen_core::theme::custom::Overrides;
use listen_core::theme::{Palette, Rgb, ThemeMode};

/// Layout metrics. Values follow Feishin's desktop layout (`docs/design-tokens.md`).
pub mod metrics {
    pub const SIDEBAR_WIDTH: f32 = 400.0;
    pub const SIDE_QUEUE_WIDTH: f32 = 600.0;
    pub const PLAYER_BAR_HEIGHT: f32 = 90.0;
    pub const ACTION_BAR_HEIGHT: f32 = 65.0;
    pub const NAV_ITEM_HEIGHT: f32 = 36.0;
    /// Mantine `sm` ActionIcon / Button / Input height.
    pub const CONTROL: f32 = 36.0;
    pub const ROW_HEIGHT: f32 = 66.0;
    pub const ROW_HEIGHT_COMPACT: f32 = 40.0;
    pub const RADIUS_SM: f32 = 3.0;
    pub const RADIUS_MD: f32 = 5.0;
    pub const PLAYER_ART: f32 = 60.0;
    pub const PAGE_PADDING_X: f32 = 32.0;
    /// Font scale (px): xs, sm, md, lg, xl.
    pub const FONT_XS: f32 = 11.0;
    pub const FONT_SM: f32 = 13.0;
    pub const FONT_MD: f32 = 14.0;
    pub const FONT_LG: f32 = 16.0;
    pub const FONT_XL: f32 = 18.0;
    pub const SPACE_XS: f32 = 4.0;
    pub const SPACE_SM: f32 = 8.0;
    pub const SPACE_MD: f32 = 12.0;
    pub const SPACE_LG: f32 = 16.0;
    pub const SPACE_XL: f32 = 24.0;

    use std::sync::atomic::{AtomicU32, Ordering};

    /// Live scales (defaults plus the override file): font xs..xl, space xs..xl, radius sm, md.
    static ACTIVE: [AtomicU32; 12] = [
        AtomicU32::new(FONT_XS.to_bits()),
        AtomicU32::new(FONT_SM.to_bits()),
        AtomicU32::new(FONT_MD.to_bits()),
        AtomicU32::new(FONT_LG.to_bits()),
        AtomicU32::new(FONT_XL.to_bits()),
        AtomicU32::new(SPACE_XS.to_bits()),
        AtomicU32::new(SPACE_SM.to_bits()),
        AtomicU32::new(SPACE_MD.to_bits()),
        AtomicU32::new(SPACE_LG.to_bits()),
        AtomicU32::new(SPACE_XL.to_bits()),
        AtomicU32::new(RADIUS_SM.to_bits()),
        AtomicU32::new(RADIUS_MD.to_bits()),
    ];

    /// Makes the scales of `t` (defaults plus the override file) the live ones.
    pub fn apply(t: &super::Tokens) {
        let all = t.font_size.iter().chain(&t.spacing).chain([&t.radius_sm, &t.radius_md]);
        for (slot, v) in ACTIVE.iter().zip(all) {
            slot.store(v.to_bits(), Ordering::Relaxed);
        }
    }

    fn get(i: usize) -> f32 {
        f32::from_bits(ACTIVE[i].load(Ordering::Relaxed))
    }

    pub fn font_xs() -> f32 {
        get(0)
    }

    pub fn font_sm() -> f32 {
        get(1)
    }

    pub fn font_md() -> f32 {
        get(2)
    }

    pub fn font_lg() -> f32 {
        get(3)
    }

    pub fn font_xl() -> f32 {
        get(4)
    }

    pub fn space_xs() -> f32 {
        get(5)
    }

    pub fn space_sm() -> f32 {
        get(6)
    }

    pub fn space_md() -> f32 {
        get(7)
    }

    pub fn space_lg() -> f32 {
        get(8)
    }

    pub fn space_xl() -> f32 {
        get(9)
    }

    pub fn radius_sm() -> f32 {
        get(10)
    }

    pub fn radius_md() -> f32 {
        get(11)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Tokens {
    pub dark: bool,
    pub background: Color,
    pub background_alt: Color,
    pub surface: Color,
    pub foreground: Color,
    pub muted: Color,
    /// `--theme-colors-primary-filled` (active nav, hovered sliders, filled buttons).
    pub primary: Color,
    /// `--theme-colors-primary` (shade 6: selection outlines, drag indicators).
    pub primary_shade: Color,
    pub error: Color,
    pub success: Color,
    /// Content borders (`--theme-colors-border`).
    pub border: Color,
    /// Structural borders between window areas (`alpha(border, .5)`).
    pub divider: Color,
    /// Sidebar nav hover (`lighten(bg, 10%)`, darken in light themes).
    pub nav_hover: Color,
    /// Subtle icon-button hover (`lighten(bg, 5%)`).
    pub icon_hover: Color,
    /// Player bar background (`darken(bg, 10%)`).
    pub player_bar: Color,
    /// Corner radii (`metrics::RADIUS_*` unless the override file changes them).
    pub radius_sm: f32,
    pub radius_md: f32,
    /// Spacing and font-size scales (xs..xl) with overrides applied; index with [`Scale`].
    pub spacing: [f32; 5],
    pub font_size: [f32; 5],
}

/// Index into [`Tokens::spacing`] / [`Tokens::font_size`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scale {
    Xs,
    Sm,
    Md,
    Lg,
    Xl,
}

/// How the primary colour's shade is chosen on top of the theme.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ShadeChoice(pub Option<u8>);

impl Tokens {
    pub fn from_palette(p: &Palette) -> Self {
        Self::build(p, ShadeChoice::default(), &Overrides::default())
    }

    /// Tokens for `p`. `shade` picks the primary shade (Feishin's "primary shade"); `ov` is the
    /// override file, whose palette colours must already be applied to `p`.
    pub fn build(p: &Palette, shade: ShadeChoice, ov: &Overrides) -> Self {
        let dark = p.mode == ThemeMode::Dark;
        let shades = listen_core::theme::shades::generate(p.primary);
        let background = rgb(p.background);
        let border = if dark { alpha(Color::WHITE, 0.10) } else { alpha(Color::BLACK, 0.05) };
        let shift = |c: Color, amount: f32| if dark { lighten(c, amount) } else { darken(c, amount) };
        let (filled, outline) = match shade.0 {
            Some(n) => {
                let c = rgb(shades[usize::from(n.min(9))]);
                (c, c)
            }
            None => (rgb(shades[if dark { 5 } else { 9 }]), rgb(shades[6])),
        };
        let color = |key: &str, default: Color| ov.color(key).map_or(default, rgb);
        let scale = |base: [f32; 5], ov: [Option<f32>; 5]| {
            let mut out = base;
            for (o, v) in out.iter_mut().zip(ov) {
                *o = v.unwrap_or(*o);
            }
            out
        };
        Self {
            dark,
            background,
            background_alt: rgb(p.background_alternate),
            surface: rgb(p.surface),
            foreground: rgb(p.foreground),
            muted: rgb(p.foreground_muted),
            // Feishin/Mantine: filled = shade 5 (dark) / 9 (light); primary = shade 6.
            primary: color("primary-filled", filled),
            primary_shade: color("primary-shade", outline),
            error: rgb(p.state_error),
            success: rgb(p.state_success),
            border: color("border", border),
            divider: color("divider", Color { a: border.a * 0.5, ..border }),
            nav_hover: color("nav-hover", shift(background, 0.10)),
            icon_hover: color("icon-hover", shift(background, 0.05)),
            player_bar: color("player-bar", darken(background, if dark { 0.10 } else { 0.05 })),
            radius_sm: ov.radius_sm.unwrap_or(metrics::RADIUS_SM),
            radius_md: ov.radius_md.unwrap_or(metrics::RADIUS_MD),
            spacing: scale(
                [
                    metrics::SPACE_XS,
                    metrics::SPACE_SM,
                    metrics::SPACE_MD,
                    metrics::SPACE_LG,
                    metrics::SPACE_XL,
                ],
                ov.spacing,
            ),
            font_size: scale(
                [metrics::FONT_XS, metrics::FONT_SM, metrics::FONT_MD, metrics::FONT_LG, metrics::FONT_XL],
                ov.font_size,
            ),
        }
    }

    pub fn space(&self, s: Scale) -> f32 {
        self.spacing[s as usize]
    }

    pub fn font(&self, s: Scale) -> f32 {
        self.font_size[s as usize]
    }

    /// The iced base theme; Listen's own styles override almost everything.
    pub fn iced_theme(&self, name: &str) -> Theme {
        Theme::custom(
            name.to_string(),
            iced::theme::Palette {
                background: self.background,
                text: self.foreground,
                primary: self.primary,
                success: self.success,
                warning: self.error,
                danger: self.error,
            },
        )
    }
}

pub fn rgb(c: Rgb) -> Color {
    Color::from_rgb8(c.0, c.1, c.2)
}

/// CSS `alpha(c, a)`: scales the colour's opacity.
pub fn alpha(c: Color, a: f32) -> Color {
    Color { a: c.a * a, ..c }
}

/// CSS `lighten(c, x)` = `color-mix(c, white x)`.
pub fn lighten(c: Color, x: f32) -> Color {
    Color { r: c.r + (1.0 - c.r) * x, g: c.g + (1.0 - c.g) * x, b: c.b + (1.0 - c.b) * x, a: c.a }
}

/// CSS `darken(c, x)` = `color-mix(c, black x)`.
pub fn darken(c: Color, x: f32) -> Color {
    Color { r: c.r * (1.0 - x), g: c.g * (1.0 - x), b: c.b * (1.0 - x), a: c.a }
}

fn bg(c: Color) -> Option<Background> {
    Some(Background::Color(c))
}

// ---- containers -----------------------------------------------------------

pub fn app_background(t: Tokens) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: bg(t.background),
        text_color: Some(t.foreground),
        ..Default::default()
    }
}

/// Window bar: `background` with a structural bottom border.
pub fn title_bar(t: Tokens) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: bg(t.background),
        text_color: Some(t.foreground),
        border: Border { color: t.divider, width: 0.0, radius: 0.0.into() },
        ..Default::default()
    }
}

/// Window-bar buttons; `close` turns red on hover.
pub fn window_button(t: Tokens, close: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let hover = if close { t.error } else { Color::from_rgba8(125, 125, 125, 0.3) };
        button::Style {
            background: hovered.then_some(Background::Color(hover)),
            text_color: t.foreground,
            snap: true,
            ..Default::default()
        }
    }
}

/// Left sidebar and right queue sidebar: `background-alternate`.
pub fn sidebar(t: Tokens) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: bg(t.background_alt),
        text_color: Some(t.foreground),
        ..Default::default()
    }
}

pub fn player_bar(t: Tokens) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: bg(t.player_bar),
        text_color: Some(t.foreground),
        ..Default::default()
    }
}

/// A 1px structural divider line (use inside a 1px-high/wide container).
pub fn divider(t: Tokens) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style { background: bg(t.divider), ..Default::default() }
}

// ---- buttons ----------------------------------------------------------------

/// Sidebar navigation entry (Mantine subtle button); `active` = current route.
pub fn nav_item(t: Tokens, active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: hovered.then_some(Background::Color(t.nav_hover)),
            text_color: if active { t.primary } else { t.foreground },
            border: border::rounded(t.radius_sm),
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// Floating pill button over the page (the compact layout's "Menu").
pub fn menu_float_button(t: Tokens) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: bg(if hovered { lighten_or_darken(t, t.surface, 0.08) } else { t.surface }),
            text_color: t.foreground,
            border: Border { color: t.border, width: 1.0, radius: t.radius_md.into() },
            shadow: Shadow {
                color: alpha(Color::BLACK, 0.35),
                offset: iced::Vector::new(0.0, 3.0),
                blur_radius: 10.0,
            },
            snap: true,
        }
    }
}

/// Default-variant button (sidebar action bar): `surface` background.
pub fn default_button(t: Tokens) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let background = match status {
            button::Status::Hovered => lighten_or_darken(t, t.surface, 0.05),
            button::Status::Pressed => lighten_or_darken(t, t.surface, 0.10),
            _ => t.surface,
        };
        button::Style {
            background: bg(background),
            text_color: t.foreground,
            border: Border { color: t.border, width: 1.0, radius: t.radius_sm.into() },
            snap: true,
            ..Default::default()
        }
    }
}

/// Subtle ActionIcon (player controls); `active` tints it with the primary colour.
pub fn icon_button(t: Tokens, active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: hovered.then_some(Background::Color(t.icon_hover)),
            text_color: match status {
                button::Status::Disabled => alpha(t.muted, 0.5),
                _ if active => t.primary,
                _ => t.foreground,
            },
            border: border::rounded(t.radius_sm),
            snap: true,
            ..Default::default()
        }
    }
}

/// The round main play button: `foreground` circle with a `background` icon.
pub fn play_button(t: Tokens) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let background = match status {
            button::Status::Hovered | button::Status::Pressed => alpha(t.foreground, 0.85),
            _ => t.foreground,
        };
        button::Style {
            background: bg(background),
            text_color: t.background,
            border: border::rounded(999.0),
            snap: true,
            ..Default::default()
        }
    }
}

/// Library-header "Play" button: white pill with black text.
pub fn wide_play_button(_t: Tokens) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let background = match status {
            button::Status::Hovered | button::Status::Pressed => darken(Color::WHITE, 0.2),
            _ => Color::WHITE,
        };
        button::Style {
            background: bg(background),
            text_color: Color::BLACK,
            border: border::rounded(16.0),
            snap: true,
            ..Default::default()
        }
    }
}

/// Filled accent button (form submit).
pub fn primary_button(t: Tokens) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let background = match status {
            button::Status::Hovered | button::Status::Pressed => darken(t.primary, 0.1),
            button::Status::Disabled => alpha(t.primary, 0.4),
            button::Status::Active => t.primary,
        };
        button::Style {
            background: bg(background),
            text_color: Color::WHITE,
            border: border::rounded(t.radius_sm),
            snap: true,
            ..Default::default()
        }
    }
}

/// A clickable table row: `surface` on hover; the playing row is tinted primary.
pub fn row_button(t: Tokens, current: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: hovered.then_some(Background::Color(t.surface)),
            text_color: if current { t.primary } else { t.foreground },
            border: border::rounded(t.radius_md),
            snap: true,
            ..Default::default()
        }
    }
}

/// Small round button drawn over artwork (expand / collapse the cover).
pub fn art_overlay_button(_t: Tokens) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: Some(Background::Color(Color {
                a: if hovered { 0.85 } else { 0.6 },
                ..Color::BLACK
            })),
            text_color: Color::WHITE,
            border: border::rounded(999),
            snap: true,
            ..Default::default()
        }
    }
}

/// Invisible button used to make cards/images clickable.
pub fn bare_button(_t: Tokens) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, _| button::Style { snap: true, ..Default::default() }
}

fn lighten_or_darken(t: Tokens, c: Color, x: f32) -> Color {
    if t.dark { lighten(c, x) } else { darken(c, x) }
}

// ---- inputs -----------------------------------------------------------------

/// Player-bar slider (seek & volume): 6px pill track; primary on hover,
/// thumb only visible while hovered/dragged.
pub fn bar_slider(t: Tokens) -> impl Fn(&Theme, slider::Status) -> slider::Style {
    move |_, status| {
        let hovered = matches!(status, slider::Status::Hovered | slider::Status::Dragged);
        let filled = if hovered { t.primary } else { t.foreground };
        slider::Style {
            rail: slider::Rail {
                backgrounds: (Background::Color(filled), Background::Color(alpha(t.muted, 0.3))),
                width: 6.0,
                border: border::rounded(3.0),
            },
            handle: slider::Handle {
                shape: slider::HandleShape::Circle { radius: if hovered { 8.0 } else { 0.0 } },
                background: Background::Color(t.foreground),
                border_width: 1.0,
                border_color: t.primary,
            },
        }
    }
}

pub fn input(t: Tokens) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    move |_, status| {
        let focused = matches!(status, text_input::Status::Focused { .. });
        text_input::Style {
            background: Background::Color(t.surface),
            border: Border {
                color: if focused { t.primary } else { t.border },
                width: 1.0,
                radius: t.radius_sm.into(),
            },
            icon: t.muted,
            placeholder: t.muted,
            value: t.foreground,
            selection: alpha(t.primary, 0.4),
        }
    }
}

/// Overlay-style scrollbar: transparent track, grey handle (.2 / .6 hovered).
pub fn scrollbar(_t: Tokens) -> impl Fn(&Theme, scrollable::Status) -> scrollable::Style {
    move |theme, status| {
        let mut style = scrollable::default(theme, status);
        let active =
            matches!(status, scrollable::Status::Hovered { .. } | scrollable::Status::Dragged { .. });
        let handle = Color::from_rgba8(160, 160, 160, if active { 0.6 } else { 0.2 });
        for rail in [&mut style.vertical_rail, &mut style.horizontal_rail] {
            rail.background = None;
            rail.border = Border::default();
            rail.scroller.background = Background::Color(handle);
            rail.scroller.border = Border::default();
        }
        style
    }
}

pub fn icon_color(color: Color) -> impl Fn(&Theme, svg::Status) -> svg::Style {
    move |_, _| svg::Style { color: Some(color) }
}
