//! Look: theme, fonts, scale, and the home page.

use super::*;

pub const ZOOM_MIN: u16 = 50;
pub const ZOOM_MAX: u16 = 300;
pub const ZOOM_STEP: u16 = 5;
/// Built-in font families (bundled with the app).
pub const BUILTIN_FONTS: [&str; 2] = ["Inter", "Poppins"];

/// Where the font comes from (Feishin: Built-in / System / Custom).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FontKind {
    #[default]
    Builtin,
    System,
    Custom,
}

/// What a cover shows while its image loads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImagePlaceholder {
    Off,
    /// A tint derived from the cover (stable per image).
    #[default]
    DominantColor,
    /// A blurred preview decoded from a ThumbHash made after the cover's first
    /// load (a few bytes per cover on disk); the tint until one exists.
    ThumbHash,
}

impl Appearance {
    /// Zoom clamped to the allowed range (a hand-edited config may hold anything).
    pub fn zoom_clamped(&self) -> u16 {
        self.zoom.clamp(ZOOM_MIN, ZOOM_MAX)
    }

    /// The scale factor handed to iced.
    pub fn scale_factor(&self) -> f32 {
        f32::from(self.zoom_clamped()) / 100.0
    }
}

/// `zoom` stepped by `delta` percent, kept in range.
pub fn step_zoom(zoom: u16, delta: i32) -> u16 {
    (i32::from(zoom) + delta).clamp(i32::from(ZOOM_MIN), i32::from(ZOOM_MAX)) as u16
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    /// Theme used when not following the system.
    pub theme: String,
    /// Use the desktop's colours (KDE colour scheme / the rice's theme
    /// switcher) and follow changes live. Overrides the choices below.
    pub follow_desktop_theme: bool,
    pub follow_system_theme: bool,
    pub theme_dark: String,
    pub theme_light: String,
    /// Custom accent; ignored when `use_theme_accent` is set.
    pub accent: Option<Rgb>,
    pub use_theme_accent: bool,
    /// Font family: a built-in name (`Inter`, `Poppins`), an installed family
    /// (`FontKind::System`) or, for `FontKind::Custom`, only a display name.
    pub font: String,
    pub font_kind: FontKind,
    /// Font file for `FontKind::Custom` (.ttf/.otf/.ttc).
    pub font_path: String,
    /// Percent, 100 = normal; kept within `ZOOM_MIN..=ZOOM_MAX`.
    pub zoom: u16,
    /// Feishin "use theme primary shade": when off, `primary_shade` picks the
    /// shade (0-9) buttons and links use.
    pub use_theme_primary_shade: bool,
    pub primary_shade: u8,
    /// Blurred artwork behind album headers.
    pub album_background: bool,
    pub artist_background: bool,
    /// Blur covers of explicit songs.
    pub blur_explicit_images: bool,
    /// Links to Last.fm, MusicBrainz, ... on album pages (Feishin `externalLinks`).
    pub external_links: bool,
    /// Show covers uncropped (letterboxed) instead of cropping to a square.
    pub native_cover_aspect: bool,
    pub image_placeholder: ImagePlaceholder,
    /// Touchpad scroll speed; 1.0 = like Chromium browsers (Ecosia, Discord).
    pub scroll_speed: f32,
    /// UI language: a Feishin locale code (`de`, `pt-BR`), or empty for the system language.
    pub language: String,
    pub album_background_blur: f32,
    pub artist_background_blur: f32,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: "defaultDark".into(),
            follow_desktop_theme: cfg!(target_os = "linux"),
            follow_system_theme: true,
            theme_dark: "tokyoNight".into(),
            theme_light: "defaultLight".into(),
            accent: Some(Rgb(53, 116, 252)),
            use_theme_accent: false,
            font: "Inter".into(),
            font_kind: FontKind::Builtin,
            font_path: String::new(),
            zoom: 100,
            use_theme_primary_shade: true,
            primary_shade: 6,
            album_background: true,
            artist_background: true,
            blur_explicit_images: false,
            external_links: true,
            native_cover_aspect: false,
            image_placeholder: ImagePlaceholder::DominantColor,
            scroll_speed: 1.0,
            language: String::new(),
            album_background_blur: 3.0,
            artist_background_blur: 3.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Home {
    pub feature: bool,
    /// Single or multiple featured albums (Feishin's `homeFeatureStyle`).
    pub feature_style: HomeFeatureStyle,
    /// Visible sections in order (`recentlyAdded`, `mostPlayed`, …).
    pub sections: Vec<String>,
}

impl Default for Home {
    fn default() -> Self {
        Self {
            feature: true,
            feature_style: HomeFeatureStyle::default(),
            sections: [
                "genres",
                "mostPlayed",
                "random",
                "recentlyAdded",
                "recentlyPlayed",
                "recentlyReleased",
                "playlists",
            ]
            .map(String::from)
            .to_vec(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_is_clamped() {
        assert_eq!(step_zoom(100, 5), 105);
        assert_eq!(step_zoom(50, -5), 50);
        assert_eq!(step_zoom(300, 5), 300);
        assert_eq!(step_zoom(100, 10_000), 300);
        let a = Appearance { zoom: 9999, ..Appearance::default() };
        assert_eq!(a.zoom_clamped(), 300);
        assert!((Appearance { zoom: 125, ..Appearance::default() }.scale_factor() - 1.25).abs() < 1e-6);
        assert_eq!(Appearance { zoom: 0, ..Appearance::default() }.zoom_clamped(), 50);
    }

    #[test]
    fn font_setting_round_trips() {
        let a = Appearance {
            font: "Fira Sans".into(),
            font_kind: FontKind::System,
            font_path: "/tmp/x.ttf".into(),
            zoom: 125,
            ..Appearance::default()
        };
        let text = toml::to_string(&a).unwrap();
        let back: Appearance = toml::from_str(&text).unwrap();
        assert_eq!(back, a);
        // A config missing newer fields still loads with defaults.
        let old: Appearance = toml::from_str("font = \"Poppins\"\nzoom = 90\n").unwrap();
        assert_eq!(old.font, "Poppins");
        assert_eq!(old.font_kind, FontKind::Builtin);
        assert_eq!(old.zoom, 90);
        assert!(old.album_background && old.use_theme_primary_shade);
    }
}
