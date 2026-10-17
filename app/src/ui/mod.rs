//! View code. Each submodule renders one area of Feishin's desktop layout.

pub mod album_detail;
pub mod albums;
pub mod app_menu;
pub mod artist_detail;
pub mod artists;
pub mod browse;
pub mod compact;
pub mod context_menu;
pub mod dlna;
pub mod downloads;
pub mod drag_scroll;
pub mod favorites;
pub mod fullscreen;
pub mod history;
pub mod home;
pub mod item;
pub mod item_controls;
pub mod kinetic;
pub mod list;
pub mod login;
pub mod long_press;
pub mod lyrics;
pub mod lyrics_settings;
pub mod milkdrop;
pub mod onboarding;
pub mod overlay;
pub mod page;
pub mod palette;
pub mod player_bar;
pub mod player_config;
pub mod playlist_detail;
pub mod playlist_page;
pub mod playlists;
pub mod popover;
pub mod queue;
pub mod radio;
pub mod right_click;
pub mod rip_panel;
pub mod search;
pub mod servers;
pub mod settings;
pub mod sidebar;
pub mod site_icon;
pub mod sleep_popover;
pub mod smart_playlist;
pub mod song_links;
pub mod songs;
pub mod tip;
pub mod title_bar;
pub mod update_dialog;
pub mod visualizer;
pub mod waveform;

use crate::app::Message;
use crate::icons::Icon;
use crate::style::{self, Tokens};
use iced::font::Weight;
use iced::widget::text::Wrapping;
use iced::widget::{Text, container, svg, text};
use iced::{Background, Color, Element, Font, Length, border};
use std::borrow::Cow;
use std::sync::OnceLock;
use unicode_normalization::UnicodeNormalization;

/// Feishin's `useIsMobile()`: at this window width or less the app switches
/// to the compact layout (no side panels, small player bar).
pub const COMPACT_BREAKPOINT: f32 = 768.0;

/// Which layout the main window uses, picked from its width like Feishin's
/// `ResponsiveLayout` (there is no button for it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    Full,
    Compact,
}

impl LayoutMode {
    pub fn for_width(width: f32) -> Self {
        if width <= COMPACT_BREAKPOINT { Self::Compact } else { Self::Full }
    }
}

pub fn font(weight: Weight) -> Font {
    Font { weight, ..crate::appearance::current_font() }
}

fn is_hangul(c: char) -> bool {
    matches!(c, '\u{1100}'..='\u{11FF}' | '\u{3130}'..='\u{318F}' | '\u{A960}'..='\u{A97F}' | '\u{AC00}'..='\u{D7AF}' | '\u{D7B0}'..='\u{D7FF}')
}

/// Scripts Inter doesn't cover, each drawn with its own system font: iced's fallback
/// splits Hangul syllables and mis-shapes Indic conjuncts.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Script {
    Hangul,
    Han,
    Arabic,
    Bengali,
    Devanagari,
}

impl Script {
    const ALL: [Script; 5] = [Self::Hangul, Self::Han, Self::Arabic, Self::Bengali, Self::Devanagari];

    fn of(c: char) -> Option<Script> {
        match c {
            c if is_hangul(c) => Some(Self::Hangul),
            '\u{3040}'..='\u{30FF}'
            | '\u{3400}'..='\u{4DBF}'
            | '\u{4E00}'..='\u{9FFF}'
            | '\u{F900}'..='\u{FAFF}' => Some(Self::Han),
            '\u{0600}'..='\u{06FF}'
            | '\u{0750}'..='\u{077F}'
            | '\u{FB50}'..='\u{FDFF}'
            | '\u{FE70}'..='\u{FEFF}' => Some(Self::Arabic),
            '\u{0980}'..='\u{09FF}' => Some(Self::Bengali),
            '\u{0900}'..='\u{097F}' => Some(Self::Devanagari),
            _ => None,
        }
    }

    /// Linux (Noto and friends) first, then the fonts Windows ships.
    fn preferred(self) -> &'static [&'static str] {
        match self {
            Self::Hangul => &[
                "Noto Sans CJK KR",
                "Noto Sans KR",
                "Source Han Sans K",
                "Pretendard",
                "NanumGothic",
                "NanumBarunGothic",
                "Malgun Gothic",
                "Apple SD Gothic Neo",
            ],
            Self::Han => {
                &["Noto Sans CJK SC", "Noto Sans SC", "Source Han Sans SC", "Microsoft YaHei", "SimHei"]
            }
            Self::Arabic => &["Noto Sans Arabic", "Noto Naskh Arabic", "Segoe UI", "Tahoma"],
            Self::Bengali => &["Noto Sans Bengali", "Nirmala UI", "Vrinda"],
            Self::Devanagari => &["Noto Sans Devanagari", "Nirmala UI", "Mangal"],
        }
    }

    /// The first preferred font that is installed (looked up once per script).
    fn family(self) -> Option<&'static str> {
        static FAMILIES: OnceLock<[Option<&'static str>; 5]> = OnceLock::new();
        let all = FAMILIES.get_or_init(|| {
            let Ok(mut fonts) = iced::advanced::graphics::text::font_system().write() else {
                return [None; 5];
            };
            let db = fonts.raw().db();
            Script::ALL.map(|script| {
                script
                    .preferred()
                    .iter()
                    .copied()
                    .find(|name| db.faces().any(|f| f.families.iter().any(|(n, _)| n == name)))
            })
        });
        all[Script::ALL.iter().position(|s| *s == self).unwrap_or(0)]
    }
}

/// `base`, or a font for the first script in `s` that Inter can't draw, with the same weight.
pub fn font_for(s: &str, base: Font) -> Font {
    match s.chars().find_map(Script::of).and_then(Script::family) {
        Some(family) => Font { family: iced::font::Family::Name(family), ..base },
        None => base,
    }
}

/// Composes decomposed Hangul (NFD, common in tags written on macOS) into syllables.
pub(crate) fn compose<'a>(s: Cow<'a, str>) -> Cow<'a, str> {
    if s.chars().any(|c| matches!(c, '\u{1100}'..='\u{11FF}')) { Cow::Owned(s.nfc().collect()) } else { s }
}

/// Translated plain text (no font change); for the few widgets that skip [`text_in`].
pub fn t_text<'a>(content: impl text::IntoFragment<'a>) -> Text<'a> {
    text(crate::i18n::tr_cow(content.into_fragment()))
}

/// Text in `font` (switched to a script's own font where Inter has no glyphs).
pub fn text_in<'a>(content: impl text::IntoFragment<'a>, base: Font) -> Text<'a> {
    let content = compose(crate::i18n::tr_cow(content.into_fragment()));
    let f = font_for(&content, base);
    text(content).font(f)
}

/// Single-line text in Inter at `size` px.
pub fn label<'a>(content: impl text::IntoFragment<'a>, size: f32, color: Color) -> Text<'a> {
    label_w(content, size, color, Weight::Normal)
}

/// Like [`label`] with an explicit weight.
pub fn label_w<'a>(
    content: impl text::IntoFragment<'a>,
    size: f32,
    color: Color,
    weight: Weight,
) -> Text<'a> {
    text_in(content, font(weight)).size(size).color(color).wrapping(Wrapping::None)
}

/// Cuts `s` to about `max` characters with an ellipsis. iced 0.14 has no
/// text-overflow ellipsis, so callers pass a width-derived budget.
pub fn ellipsize(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{}…", cut.trim_end())
    }
}

/// Rough rendered width of `s` at `size` px.
pub fn text_px(s: &str, size: f32) -> f32 {
    s.chars().map(char_em).sum::<f32>() * size
}

/// Rough rendered width of `c` in em (Inter; CJK/Hangul are full-width).
fn char_em(c: char) -> f32 {
    match c {
        '\u{1100}'..='\u{11FF}'
        | '\u{2E80}'..='\u{9FFF}'
        | '\u{AC00}'..='\u{D7AF}'
        | '\u{F900}'..='\u{FAFF}'
        | '\u{FF00}'..='\u{FF60}' => 1.0,
        'i' | 'l' | 'j' | 'I' | '.' | ',' | '\'' | '|' | '!' | ':' | ';' | ' ' => 0.3,
        'm' | 'w' | 'M' | 'W' | '@' => 0.85,
        c if c.is_uppercase() => 0.66,
        _ => 0.52,
    }
}

/// Cuts `s` with an ellipsis so it fits about `max_px` at font `size`
/// (iced 0.14 has no text-overflow ellipsis). Pair with a clipping container.
pub fn ellipsize_px(s: &str, max_px: f32, size: f32) -> String {
    let budget = (max_px / size).max(1.0);
    let total: f32 = s.chars().map(char_em).sum();
    if total <= budget {
        return s.to_string();
    }
    let mut used = char_em('…');
    let mut out = String::new();
    for c in s.chars() {
        used += char_em(c);
        if used > budget {
            break;
        }
        out.push(c);
    }
    format!("{}…", out.trim_end())
}

/// The glyph of a play mode: the letters "AI" for the AI DJ,
/// the mode's icon otherwise.
pub fn mode_glyph<'a>(
    mode: crate::app::play_mode::PlayMode,
    size: f32,
    color: Color,
) -> Element<'a, Message> {
    if mode == crate::app::play_mode::PlayMode::AiDj {
        label_w("AI", size * 0.8, color, Weight::Bold).into()
    } else {
        icon(mode.icon(), size, color)
    }
}

pub fn icon<'a>(icon: Icon, size: f32, color: Color) -> Element<'a, Message> {
    svg(icon.handle()).width(size).height(size).style(style::icon_color(color)).into()
}

/// 1px vertical structural divider.
pub fn divider_v<'a>(t: Tokens) -> Element<'a, Message> {
    container(text("")).width(1).height(Length::Fill).style(style::divider(t)).into()
}

/// A vertical divider you can drag to resize the panel next to it.
pub fn resize_handle<'a>(t: Tokens, side: crate::app::PanelSide) -> Element<'a, Message> {
    let line = container(text("")).width(1).height(Length::Fill).style(style::divider(t));
    iced::widget::mouse_area(container(line).width(5).height(Length::Fill).center_x(5))
        .on_press(Message::ResizeStart(side))
        .interaction(iced::mouse::Interaction::ResizingHorizontally)
        .into()
}

/// 1px horizontal structural divider.
pub fn divider_h<'a>(t: Tokens) -> Element<'a, Message> {
    container(text("")).width(Length::Fill).height(1).style(style::divider(t)).into()
}

/// Artwork placeholder: a coloured square with a centred glyph.
pub fn art<'a>(color: (u8, u8, u8), size: f32, radius: f32, glyph: Icon) -> Element<'a, Message> {
    let bg = Color::from_rgb8(color.0, color.1, color.2);
    container(icon(glyph, size * 0.4, Color { a: 0.55, ..Color::WHITE }))
        .width(size)
        .height(size)
        .center(size)
        .style(move |_| container::Style {
            background: Some(Background::Gradient(
                iced::gradient::Linear::new(iced::Radians(2.4))
                    .add_stop(0.0, bg)
                    .add_stop(1.0, style::darken(bg, 0.45))
                    .into(),
            )),
            border: border::rounded(radius),
            ..Default::default()
        })
        .into()
}

/// Cover art from the image cache, or a placeholder while it loads.
pub fn cover<'a>(
    state: &crate::app::Listen,
    image: Option<&listen_core::model::ImageRef>,
    size: f32,
    radius: f32,
    glyph: Icon,
) -> Element<'a, Message> {
    let key = image.map(|i| state.image_key(i));
    cover_keyed(state, key, size, radius, glyph)
}

/// Like [`cover`], for an image from a specific server.
pub fn cover_on<'a>(
    state: &crate::app::Listen,
    server: &listen_core::model::ServerId,
    image: Option<&listen_core::model::ImageRef>,
    size: f32,
    radius: f32,
    glyph: Icon,
) -> Element<'a, Message> {
    let key = image.map(|i| state.image_key_on(server, i));
    cover_keyed(state, key, size, radius, glyph)
}

fn cover_keyed<'a>(
    state: &crate::app::Listen,
    key: Option<crate::images::Key>,
    size: f32,
    radius: f32,
    glyph: Icon,
) -> Element<'a, Message> {
    let a = &state.config.settings.appearance;
    if let Some(handle) = key.as_ref().and_then(|k| state.images.get(k)) {
        // "Native cover aspect ratio": show the whole cover instead of cropping it square.
        let fit = if a.native_cover_aspect { iced::ContentFit::Contain } else { iced::ContentFit::Cover };
        let ratio = key.as_ref().and_then(|k| state.images.ratio(k));
        let height = cover_height(size, ratio, a.native_cover_aspect);
        return iced::widget::image(handle.clone())
            .width(size)
            .height(height)
            .content_fit(fit)
            .border_radius(radius)
            .into();
    }
    placeholder(state, key.as_deref(), size, radius, glyph)
}

/// Height of a cover drawn `size` wide: square, unless "native cover aspect ratio" is on
/// and the cover is wider than tall (capped at 3:1).
pub fn cover_height(size: f32, ratio: Option<f32>, native: bool) -> f32 {
    match ratio {
        Some(r) if native && r.is_finite() && r > 1.0 => (size / r.min(3.0)).round().max(1.0),
        _ => size,
    }
}

/// What a cover shows while its image loads (or when there is none).
fn placeholder<'a>(
    state: &crate::app::Listen,
    key: Option<&str>,
    size: f32,
    radius: f32,
    glyph: Icon,
) -> Element<'a, Message> {
    let t = state.tokens;
    let a = &state.config.settings.appearance;
    if a.image_placeholder == listen_core::settings::ImagePlaceholder::ThumbHash
        && let Some(handle) = key.and_then(|k| state.images.preview(k))
    {
        // A blurred preview of the cover from its ThumbHash.
        return iced::widget::image(handle.clone())
            .width(size)
            .height(size)
            .content_fit(iced::ContentFit::Cover)
            .border_radius(radius)
            .into();
    }
    let fill = match (key, a.image_placeholder) {
        (
            Some(key),
            listen_core::settings::ImagePlaceholder::DominantColor
            | listen_core::settings::ImagePlaceholder::ThumbHash,
        ) => crate::app::appearance::placeholder_tint(key, t.dark),
        _ => t.surface,
    };
    container(icon(glyph, size * 0.4, t.muted))
        .width(size)
        .height(size)
        .center(size)
        .style(move |_| container::Style {
            background: Some(Background::Color(fill)),
            border: border::rounded(radius),
            ..Default::default()
        })
        .into()
}

/// Cover for a song: demo colours in demo mode, cached art otherwise.
pub fn song_cover<'a>(
    state: &crate::app::Listen,
    song: &listen_core::model::Song,
    size: f32,
    radius: f32,
) -> Element<'a, Message> {
    if state.demo {
        art(crate::demo::color_for_album(song.album_id.as_deref()), size, radius, Icon::LuMusic)
    } else if song.explicit && state.config.settings.appearance.blur_explicit_images {
        // Explicit covers show a blurred stand-in; never the sharp image.
        let key = song.image.as_ref().map(|i| state.image_key_on(&song.server_id, i));
        match key.as_ref().and_then(|k| state.images.explicit(k)) {
            Some(handle) => iced::widget::image(handle.clone())
                .width(size)
                .height(size)
                .content_fit(iced::ContentFit::Cover)
                .border_radius(radius)
                .into(),
            None => placeholder(state, key.as_deref(), size, radius, Icon::LuMusic),
        }
    } else {
        cover_on(state, &song.server_id, song.image.as_ref(), size, radius, Icon::LuMusic)
    }
}

/// Cover for an album: demo colours in demo mode, cached art otherwise.
pub fn album_cover<'a>(
    state: &crate::app::Listen,
    album: &listen_core::model::Album,
    size: f32,
    radius: f32,
) -> Element<'a, Message> {
    if state.demo {
        art(crate::demo::color_for_album(Some(&album.id)), size, radius, Icon::LuDisc3)
    } else {
        cover(state, album.image.as_ref(), size, radius, Icon::LuDisc3)
    }
}

/// `content` with a right-click menu for `target`.
pub fn with_menu<'a>(
    content: impl Into<Element<'a, Message>>,
    target: crate::app::menu::MenuTarget,
) -> Element<'a, Message> {
    iced::widget::mouse_area(content)
        .on_right_press(Message::Menu(crate::app::menu::MenuMsg::Open(target)))
        .into()
}

/// "124 BPM · 8A" from the audio analysis, if the song has been analysed.
pub fn tempo_key(state: &crate::app::Listen, song: &listen_core::model::Song) -> Option<String> {
    // Straight from the shared record (no `FlowFeatures` copy): this runs per queue row.
    let record = state.analysis.as_ref()?.lookup(song)?;
    let f = record.features()?;
    let mut parts = Vec::new();
    if let Some(grid) = f.grid.as_ref().filter(|g| g.confidence >= 0.35) {
        parts.push(format!("{} BPM", grid.bpm.round()));
    }
    if let Some((key, _)) = f.key.filter(|(_, c)| *c >= 0.3) {
        parts.push(key.label());
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// "1 album" / "3 albums".
pub fn plural(n: usize, word: &str) -> String {
    if n == 1 { format!("1 {word}") } else { format!("{n} {word}s") }
}

/// Library-header title: Feishin's clamp(28px, …, 44px) shrinks long titles;
/// the text wraps instead of overflowing.
pub fn header_title<'a>(title: &str, color: Color) -> Element<'a, Message> {
    let len = title.chars().count();
    let size = match len {
        0..=20 => 44.0,
        21..=30 => 38.0,
        31..=45 => 32.0,
        _ => 28.0,
    };
    text_in(title.to_string(), font(Weight::ExtraBold))
        .size(size)
        .color(color)
        .line_height(1.2)
        .width(Length::Fill)
        .into()
}

/// Largest size from `max` down to `min` at which `title` fits `width` px on one line;
/// `min` if none (it wraps). Feishin's clamp()ed header font.
pub fn fit_title_size(title: &str, width: f32, max: f32, min: f32) -> f32 {
    let em = text_px(title, 1.0) * 1.06;
    if em <= 0.0 { max } else { (width / em).clamp(min, max) }
}

pub fn header_title_fit<'a>(title: &str, color: Color, width: f32) -> Element<'a, Message> {
    let size = fit_title_size(title, width, 44.0, 24.0);
    text_in(title.to_string(), font(Weight::Black))
        .size(size)
        .color(color)
        .line_height(1.2)
        .width(Length::Fill)
        .into()
}

/// Feishin's library-header background: the blurred artwork (opacity .7)
/// under a gradient that fades into the page background.
pub fn header_background<'a>(
    state: &crate::app::Listen,
    image: Option<&listen_core::model::ImageRef>,
    height: f32,
    kind: crate::app::appearance::BlurKind,
) -> Element<'a, Message> {
    let t = state.tokens;
    // "Album/Artist background" off: only the fade remains.
    let blurred = image
        .filter(|_| state.background_enabled(kind))
        .and_then(|i| state.images.blurred(&state.blur_key(i, kind)))
        .cloned();
    let fade = container(text("")).width(Length::Fill).height(height).style(move |_| container::Style {
        background: Some(Background::Gradient(
            iced::gradient::Linear::new(iced::Radians(std::f32::consts::PI))
                .add_stop(0.0, Color { a: 0.05, ..t.background })
                .add_stop(1.0, t.background)
                .into(),
        )),
        ..Default::default()
    });
    match blurred {
        Some(handle) => iced::widget::stack![
            container(
                iced::widget::image(handle)
                    .width(Length::Fill)
                    .height(height)
                    .content_fit(iced::ContentFit::Fill)
                    .opacity(0.7_f32),
            )
            .width(Length::Fill)
            .height(height)
            .clip(true),
            fade,
        ]
        .into(),
        None => fade.into(),
    }
}

/// Remove control for a server: an ✕ that turns into "Remove? Yes / No".
pub fn server_remove<'a>(
    state: &crate::app::Listen,
    server: &listen_core::model::ServerId,
) -> Element<'a, Message> {
    use iced::widget::{button, row};
    let t = state.tokens;
    let small = |title: &'static str, color: Color, msg: Message| {
        button(label_w(title, style::metrics::font_sm(), color, Weight::Semibold))
            .padding(iced::Padding::from([4.0, 8.0]))
            .style(style::default_button(t))
            .on_press(msg)
    };
    if state.confirm_remove.as_ref() == Some(server) {
        row![
            label("Remove?", style::metrics::font_sm(), t.muted),
            small("Yes", t.error, Message::RemoveServer(server.clone())),
            small("No", t.foreground, Message::ConfirmRemoveServer(None)),
        ]
        .spacing(style::metrics::space_xs())
        .align_y(iced::Alignment::Center)
        .into()
    } else {
        iced::widget::tooltip(
            button(icon(Icon::LuX, style::metrics::font_md(), t.muted))
                .padding(6)
                .style(style::icon_button(t, false))
                .on_press(Message::ConfirmRemoveServer(Some(server.clone()))),
            container(label("Remove server", style::metrics::font_sm(), t.foreground)).padding(6).style(
                move |_| container::Style {
                    background: Some(Background::Color(t.background_alt)),
                    border: border::rounded(4).color(t.border).width(1),
                    ..Default::default()
                },
            ),
            iced::widget::tooltip::Position::Bottom,
        )
        .into()
    }
}

/// Hover arrow over a cover that expands it into the sidebar or collapses
/// it back into the player bar.
pub fn cover_toggle<'a>(
    state: &crate::app::Listen,
    cover: Element<'a, Message>,
    size: f32,
    expand: bool,
) -> Element<'a, Message> {
    use iced::widget::button;
    // Settings > Expand album art button: off hides the arrow.
    if !state.config.settings.sidebar.show_image {
        return cover;
    }
    let glyph = if expand { Icon::LuChevronUp } else { Icon::LuChevronDown };
    let (button_size, glyph_size) = if size < 100.0 { (20.0, 14.0) } else { (30.0, 20.0) };
    let arrow = button(container(icon(glyph, glyph_size, Color::WHITE)).center(Length::Fill))
        .width(button_size)
        .height(button_size)
        .padding(0)
        .style(style::art_overlay_button(state.tokens))
        .on_press(Message::ToggleCoverExpanded);
    let arrow = tip::tip(
        state,
        arrow,
        if expand { "Expand cover" } else { "Collapse cover" },
        None,
        if expand { tip::Position::Right } else { tip::Position::Bottom },
    );
    let corner = container(arrow)
        .padding(if size < 100.0 { 3.0 } else { 8.0 })
        .width(size)
        .height(size)
        .align_x(iced::alignment::Horizontal::Right)
        .align_y(iced::alignment::Vertical::Top);
    iced::widget::hover(cover, corner)
}

/// The playing song's cover, large (expanded sidebar image). Click opens the
/// fullscreen player; the hover arrow puts it back into the player bar.
pub fn expanded_cover<'a>(state: &crate::app::Listen, size: f32) -> Option<Element<'a, Message>> {
    use iced::widget::button;
    if !state.config.settings.sidebar.cover_expanded || !state.config.settings.sidebar.show_image {
        return None;
    }
    let song = &state.queue.current()?.song;
    let art = tip::tip(
        state,
        button(song_cover(state, song, size, style::metrics::radius_md()))
            .padding(0)
            .style(style::bare_button(state.tokens))
            .on_press(Message::ToggleFullscreen),
        "Open full-screen player",
        Some("toggleFullscreenPlayer"),
        tip::Position::Top,
    );
    Some(cover_toggle(state, art, size, false))
}

#[cfg(test)]
mod cover_height_tests {
    use super::cover_height;

    #[test]
    fn wide_covers_shorten_the_card_only_with_native_aspect() {
        assert_eq!(cover_height(200.0, Some(2.0), true), 100.0);
        assert_eq!(cover_height(200.0, Some(2.0), false), 200.0);
        // Square, tall, unknown: the square box.
        assert_eq!(cover_height(200.0, Some(1.0), true), 200.0);
        assert_eq!(cover_height(200.0, Some(0.5), true), 200.0);
        assert_eq!(cover_height(200.0, None, true), 200.0);
        // Very wide covers are capped at 3:1.
        assert_eq!(cover_height(300.0, Some(10.0), true), 100.0);
    }
}

#[cfg(test)]
mod title_tests {
    use super::fit_title_size;

    #[test]
    fn long_titles_shrink_to_the_width() {
        assert_eq!(fit_title_size("Night Drive", 1000.0, 44.0, 24.0), 44.0);
        let s = fit_title_size("Night Drive and the Endless Highway", 300.0, 44.0, 24.0);
        assert!((24.0..44.0).contains(&s));
        assert_eq!(fit_title_size(&"x".repeat(200), 200.0, 44.0, 24.0), 24.0);
    }
}
