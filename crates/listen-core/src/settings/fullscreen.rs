//! The full-screen player (Feishin's `full-screen-player.store`) and its
//! application settings (`fullscreenAutoOpenTimeout`).

use super::*;

/// Where the cover and the text under it sit in their column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemAlignment {
    Left,
    #[default]
    Center,
    Right,
}

/// How a long title is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TitleDisplay {
    /// One line that scrolls when it does not fit.
    #[default]
    Scroll,
    /// Wrapped over up to `title_lines` lines.
    MultiLine,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FullscreenSettings {
    /// Background from the cover's average colour (off: the theme background).
    pub dynamic_background: bool,
    /// The blurred cover instead of a flat colour.
    pub dynamic_is_image: bool,
    /// Blur of that image in rem (0-6).
    pub image_blur: f32,
    /// Darkening over the background, 0-100 (alpha = opacity / 120).
    pub opacity: u8,
    /// Cover size, percent of its column (55-100).
    pub cover_size: u8,
    pub alignment: ItemAlignment,
    pub title_display: TitleDisplay,
    /// Lines of a wrapped title (1-10).
    pub title_lines: u8,
    /// Keep the cover's own aspect ratio instead of a square.
    pub use_image_aspect_ratio: bool,
    /// Minutes without input before the player opens while a song plays (0 = off).
    pub auto_open_minutes: u32,
}

impl Default for FullscreenSettings {
    fn default() -> Self {
        Self {
            dynamic_background: true,
            dynamic_is_image: false,
            image_blur: 6.0,
            opacity: 25,
            cover_size: 75,
            alignment: ItemAlignment::Center,
            title_display: TitleDisplay::Scroll,
            title_lines: 1,
            use_image_aspect_ratio: false,
            auto_open_minutes: 0,
        }
    }
}

impl FullscreenSettings {
    /// The darkening alpha: Feishin divides by 120 so 100 is not pitch black.
    pub fn overlay_alpha(&self) -> f32 {
        (f32::from(self.opacity) / 120.0).clamp(0.0, 1.0)
    }
}

/// Badge ids shown under the title (`PlayerBar::info_items`), in menu order.
pub const BADGES: &[(&str, &str)] = &[
    ("codec", "Codec"),
    ("bit_rate", "Bit rate"),
    ("bit_depth", "Bit depth"),
    ("sample_rate", "Sample rate"),
    ("bpm", "BPM"),
    ("year", "Year"),
    ("release_year", "Release year"),
    ("date", "Date"),
    ("release_date", "Release date"),
    ("release_type", "Release type"),
    ("genres", "Genres"),
    ("disc_number", "Disc number"),
    ("track_number", "Track number"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_feishin() {
        let f = FullscreenSettings::default();
        assert!(f.dynamic_background && !f.dynamic_is_image);
        assert_eq!((f.opacity, f.cover_size, f.title_lines), (25, 75, 1));
        assert!((f.overlay_alpha() - 25.0 / 120.0).abs() < 1e-6);
    }

    #[test]
    fn missing_fields_fall_back() {
        let f: FullscreenSettings = toml::from_str("opacity = 50\nalignment = \"left\"").unwrap();
        assert_eq!(f.opacity, 50);
        assert_eq!(f.alignment, ItemAlignment::Left);
        assert_eq!(f.cover_size, 75);
    }
}
