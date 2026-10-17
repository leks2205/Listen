//! Appearance rows: accent picker, primary shade, custom themes, font, zoom,
//! header backgrounds and cover options.

use super::line;
use crate::app::appearance::AppearanceMsg as A;
use crate::app::{Listen, Message, Setting};
use crate::style::{self, metrics};
use crate::ui::{label, label_w};
use iced::font::Weight;
use iced::widget::{button, pick_list, row, slider, text_input};
use iced::{Alignment, Border, Color, Element};
use listen_core::settings::{FontKind, ImagePlaceholder, ZOOM_MAX, ZOOM_MIN, ZOOM_STEP};
use listen_core::theme::{ACCENT_SWATCHES, Rgb};

/// A theme in the pickers (built-in or from the themes folder).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeOption {
    pub id: String,
    pub name: String,
}

impl std::fmt::Display for ThemeOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

pub fn theme_options(state: &Listen) -> Vec<ThemeOption> {
    let builtin =
        listen_core::theme::theme_names().map(|(id, name)| ThemeOption { id: id.into(), name: name.into() });
    let custom = state
        .appearance
        .loaded
        .themes
        .iter()
        .map(|t| ThemeOption { id: t.id.clone(), name: format!("{} (custom)", t.name) });
    builtin.chain(custom).collect()
}

/// A theme picker for `current`.
pub fn theme_pick<'a>(state: &Listen, current: &str, on: fn(String) -> Setting) -> Element<'a, Message> {
    let options = theme_options(state);
    let selected = options.iter().find(|o| o.id == current).cloned();
    pick_list(options, selected, move |o| Message::Setting(on(o.id))).into()
}

fn small_button<'a>(state: &Listen, title: &'a str, msg: Option<Message>) -> Element<'a, Message> {
    let t = state.tokens;
    button(label_w(title, metrics::font_md(), t.foreground, Weight::Semibold))
        .padding(iced::Padding::from([6.0, 14.0]))
        .style(style::default_button(t))
        .on_press_maybe(msg)
        .into()
}

fn swatch<'a>(state: &Listen, color: Rgb, selected: bool, enabled: bool) -> Element<'a, Message> {
    let t = state.tokens;
    let fill = style::rgb(color);
    button(label("", 1.0, Color::TRANSPARENT))
        .width(24)
        .height(24)
        .style(move |_, _| button::Style {
            background: Some(iced::Background::Color(Color { a: if enabled { 1.0 } else { 0.4 }, ..fill })),
            border: Border {
                color: if selected { t.foreground } else { t.border },
                width: if selected { 2.0 } else { 1.0 },
                radius: 12.0.into(),
            },
            ..Default::default()
        })
        .on_press_maybe(enabled.then_some(Message::Appearance(A::AccentSwatch(color))))
        .into()
}

/// Accent colour (swatches + CSS colour field), primary shade and the
/// custom-themes status.
pub fn colour_rows(state: &Listen) -> Vec<Element<'_, Message>> {
    let t = state.tokens;
    let a = &state.config.settings.appearance;
    let enabled = !a.use_theme_accent;
    let mut swatches = row![].spacing(metrics::space_sm()).align_y(Alignment::Center);
    for color in ACCENT_SWATCHES {
        swatches = swatches.push(swatch(state, color, a.accent == Some(color), enabled));
    }
    let mut field = text_input(crate::i18n::tr("rgb(53, 116, 252)"), &state.accent_input)
        .style(style::input(t))
        .width(180);
    if enabled {
        field = field.on_input(|v| Message::Appearance(A::AccentText(v)));
    }
    let mut rows = vec![
        line(
            state,
            "Accent colour",
            if enabled {
                "Pick a swatch or type a CSS colour, e.g. rgb(53, 116, 252) or #3574fc."
            } else {
                "Turn off \"Use the theme's accent colour\" to choose your own."
            },
            row![swatches, field].spacing(metrics::space_md()).align_y(Alignment::Center).into(),
        ),
        line(
            state,
            "Use the theme's primary shade",
            "Otherwise the shade below decides which shade of the accent buttons and links use.",
            toggle(a.use_theme_primary_shade, A::UsePrimaryShade),
        ),
    ];
    rows.push(line(
        state,
        "Primary shade",
        "0 is the lightest shade, 9 the darkest.",
        if a.use_theme_primary_shade {
            label("Theme default", metrics::font_md(), t.muted).into()
        } else {
            row![
                slider(0.0..=9.0, f32::from(a.primary_shade), |v| Message::Appearance(A::PrimaryShade(
                    v as u8
                )))
                .step(1.0_f32)
                .width(160),
                label(a.primary_shade.to_string(), metrics::font_md(), t.muted),
            ]
            .spacing(metrics::space_sm())
            .align_y(Alignment::Center)
            .into()
        },
    ));
    rows.push(themes_folder(state));
    rows.extend(theme_problems(state));
    rows
}

fn themes_folder(state: &Listen) -> Element<'_, Message> {
    let loaded = &state.appearance.loaded;
    let hint = match &state.appearance.themes_dir {
        Some(dir) => format!(
            "{} custom theme(s){} in {}. Add .json or .toml files; overrides.json|toml changes colours, radii, spacing and font sizes of any theme.",
            loaded.themes.len(),
            if loaded.overrides.is_empty() { "" } else { ", overrides active" },
            dir.display()
        ),
        None => "Not available in the demo.".into(),
    };
    line(
        state,
        "Custom themes folder",
        hint,
        small_button(
            state,
            "Open folder",
            state.appearance.themes_dir.as_ref().map(|_| Message::Appearance(A::OpenThemesFolder)),
        ),
    )
}

/// Errors and warnings of the themes folder, one line each.
fn theme_problems(state: &Listen) -> Vec<Element<'_, Message>> {
    let t = state.tokens;
    let loaded = &state.appearance.loaded;
    let mut out: Vec<Element<'_, Message>> = Vec::new();
    for e in &loaded.errors {
        out.push(label(format!("{}: {}", e.file, e.message), metrics::font_sm(), t.error).into());
    }
    for theme in &loaded.themes {
        for w in &theme.warnings {
            out.push(label(format!("{}: {w}", theme.id), metrics::font_sm(), t.muted).into());
        }
    }
    for w in &loaded.override_warnings {
        out.push(label(format!("overrides: {w}"), metrics::font_sm(), t.muted).into());
    }
    out
}

fn kind_name(kind: FontKind) -> &'static str {
    match kind {
        FontKind::Builtin => "Built-in",
        FontKind::System => "System",
        FontKind::Custom => "Custom",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct KindChoice(FontKind);

impl std::fmt::Display for KindChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(kind_name(self.0))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PlaceholderChoice(ImagePlaceholder);

impl std::fmt::Display for PlaceholderChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self.0 {
            ImagePlaceholder::Off => "Off",
            ImagePlaceholder::DominantColor => "Colour",
            ImagePlaceholder::ThumbHash => "ThumbHash",
        })
    }
}

fn toggle<'a>(on: bool, f: fn(bool) -> A) -> Element<'a, Message> {
    iced::widget::toggler(on).on_toggle(move |v| Message::Appearance(f(v))).size(20).into()
}

fn zoom_row(state: &Listen) -> Element<'_, Message> {
    let t = state.tokens;
    let a = &state.config.settings.appearance;
    let shown = state.appearance.zoom_draft.unwrap_or_else(|| a.zoom_clamped());
    line(
        state,
        "Zoom",
        format!("{ZOOM_MIN}-{ZOOM_MAX} %, in steps of {ZOOM_STEP}. Applies when you let go of the slider."),
        row![
            small_button(state, "-", Some(Message::Appearance(A::ZoomOut))),
            slider(f32::from(ZOOM_MIN)..=f32::from(ZOOM_MAX), f32::from(shown), |v| {
                Message::Appearance(A::ZoomDraft(v as u16))
            })
            .step(f32::from(ZOOM_STEP))
            .on_release(Message::Appearance(A::ZoomCommit))
            .width(160),
            small_button(state, "+", Some(Message::Appearance(A::ZoomIn))),
            label(format!("{shown} %"), metrics::font_md(), t.muted),
            small_button(state, "Reset", Some(Message::Appearance(A::ZoomReset))),
        ]
        .spacing(metrics::space_sm())
        .align_y(Alignment::Center)
        .into(),
    )
}

fn font_rows(state: &Listen) -> Vec<Element<'_, Message>> {
    let t = state.tokens;
    let a = &state.config.settings.appearance;
    let kinds = [FontKind::Builtin, FontKind::System, FontKind::Custom].map(KindChoice).to_vec();
    let mut rows = vec![line(
        state,
        "Font type",
        "Built-in fonts, any font installed on this computer, or a font file.",
        pick_list(kinds, Some(KindChoice(a.font_kind)), |k| Message::Appearance(A::FontKind(k.0))).into(),
    )];
    match a.font_kind {
        FontKind::Builtin => {
            let names: Vec<String> = listen_core::settings::BUILTIN_FONTS.map(String::from).to_vec();
            rows.push(line(
                state,
                "Font",
                "Used for all text.",
                pick_list(names, Some(a.font.clone()), |n| Message::Appearance(A::FontFamily(n))).into(),
            ));
        }
        FontKind::System => {
            let names = state.appearance.system_fonts.clone();
            let control: Element<'_, Message> = if names.is_empty() {
                label("Looking for fonts...", metrics::font_md(), t.muted).into()
            } else {
                pick_list(names, Some(a.font.clone()), |n| Message::Appearance(A::FontFamily(n)))
                    .width(260)
                    .into()
            };
            rows.push(line(state, "Font", "Any font installed on this computer.", control));
        }
        FontKind::Custom => {
            let hint = state
                .appearance
                .font_error
                .clone()
                .unwrap_or_else(|| "Path to a .ttf, .otf or .ttc file.".into());
            rows.push(line(
                state,
                "Font file",
                hint,
                row![
                    text_input(crate::i18n::tr("/path/to/font.ttf"), &state.appearance.font_path_input)
                        .on_input(|v| Message::Appearance(A::FontPathInput(v)))
                        .on_submit(Message::Appearance(A::FontPathApply))
                        .style(style::input(t))
                        .width(280),
                    small_button(state, "Use", Some(Message::Appearance(A::FontPathApply))),
                ]
                .spacing(metrics::space_sm())
                .align_y(Alignment::Center)
                .into(),
            ));
        }
    }
    rows
}

fn blur_row<'a>(
    state: &'a Listen,
    name: &'static str,
    on: bool,
    on_toggle: fn(bool) -> A,
    blur: f32,
    on_blur: fn(f32) -> A,
) -> Element<'a, Message> {
    let t = state.tokens;
    line(
        state,
        name,
        "Blurred artwork behind the page header, and how blurred it is (0-6).",
        row![
            slider(0.0..=6.0, blur, move |v| Message::Appearance(on_blur(v)))
                .step(0.5_f32)
                .on_release(Message::Appearance(A::BlurCommit))
                .width(120),
            label(format!("{blur:.1}"), metrics::font_md(), t.muted),
            toggle(on, on_toggle),
        ]
        .spacing(metrics::space_sm())
        .align_y(Alignment::Center)
        .into(),
    )
}

/// Font, zoom, backgrounds and cover options ("General > Application").
pub fn application_rows(state: &Listen) -> Vec<Element<'_, Message>> {
    let a = &state.config.settings.appearance;
    let mut rows = font_rows(state);
    rows.push(language_row(state));
    rows.push(zoom_row(state));
    rows.push(blur_row(
        state,
        "Album background image",
        a.album_background,
        A::AlbumBackground,
        a.album_background_blur,
        A::AlbumBlur,
    ));
    rows.push(blur_row(
        state,
        "Artist background image",
        a.artist_background,
        A::ArtistBackground,
        a.artist_background_blur,
        A::ArtistBlur,
    ));
    rows.push(line(
        state,
        "Blur explicit images",
        "Covers of songs tagged explicit are blurred.",
        toggle(a.blur_explicit_images, A::BlurExplicit),
    ));
    rows.push(line(
        state,
        "Native cover aspect ratio",
        "Show covers whole instead of cropping them square; wide covers make their card shorter.",
        toggle(a.native_cover_aspect, A::NativeAspect),
    ));
    rows.push(line(
        state,
        "Image placeholder",
        "What a cover shows while its image loads. ThumbHash is made after a cover first loads, so it shows from the second time on.",
        pick_list(
            vec![
                PlaceholderChoice(ImagePlaceholder::ThumbHash),
                PlaceholderChoice(ImagePlaceholder::DominantColor),
                PlaceholderChoice(ImagePlaceholder::Off),
            ],
            Some(PlaceholderChoice(a.image_placeholder)),
            |c| Message::Appearance(A::Placeholder(c.0)),
        )
        .into(),
    ));
    rows
}

/// A language in the picker; an empty code follows the system language.
#[derive(Clone, PartialEq)]
struct LanguageChoice(&'static str, String);

impl std::fmt::Display for LanguageChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.1)
    }
}

/// General > Application > Language (applies after a restart).
fn language_row(state: &Listen) -> Element<'_, Message> {
    let current = state.config.settings.appearance.language.as_str();
    let mut options = vec![LanguageChoice("", crate::i18n::tr("System default").to_string())];
    options.extend(crate::i18n::LANGUAGES.iter().map(|(c, n)| LanguageChoice(c, (*n).to_string())));
    let selected = options.iter().find(|o| o.0 == current).cloned();
    line(
        state,
        "Language",
        "The language of the interface. It applies after a restart.",
        pick_list(options, selected, |c| Message::Setting(Setting::Language(c.0.to_string()))).into(),
    )
}
