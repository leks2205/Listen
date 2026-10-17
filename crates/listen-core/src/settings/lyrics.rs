//! Lyrics.

use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LyricsSettings {
    pub fetch: bool,
    pub follow: bool,
    pub alignment: String,
    /// Synced lyrics font size (Feishin `fontSize`).
    pub font_size: f32,
    /// Unsynced lyrics font size.
    pub font_size_unsync: f32,
    pub gap: f32,
    pub gap_unsync: f32,
    pub opacity_non_active: f32,
    /// Size of the lines that are not being sung, relative to the active one.
    pub scale_non_active: f32,
    /// Left / right padding, percent of the width.
    pub padding_left: f32,
    pub padding_right: f32,
    /// Where the followed line sits, -50 (up) to 50 (down), 0 = centred.
    pub follow_scroll_alignment: i32,
    /// The highlight arrives this many ms early.
    pub line_lead_time_ms: u32,
    /// Provider names in order (`NetEase`, `lrclib.net`, `Genius`, `SimpMusic`); missing = off.
    pub sources: Vec<String>,
    pub delay_ms: i32,
    /// The server's / YouTube Music's own lyrics win over the providers.
    pub prefer_local: bool,
    pub show_provider: bool,
    pub show_match: bool,
    pub enable_romaji: bool,
    /// Readings above the kanji of Japanese lines (needs the dictionary build).
    pub enable_furigana: bool,
    pub enable_netease_translation: bool,
    pub enable_auto_translation: bool,
    /// `Microsoft Azure`, `Google Cloud` or empty.
    pub translation_api_provider: String,
    pub translation_api_key: String,
    pub translation_target_language: String,
}

impl Default for LyricsSettings {
    fn default() -> Self {
        Self {
            fetch: true,
            follow: true,
            alignment: "center".into(),
            font_size: 24.0,
            font_size_unsync: 24.0,
            gap: 24.0,
            gap_unsync: 24.0,
            opacity_non_active: 0.2,
            scale_non_active: 0.95,
            padding_left: 0.0,
            padding_right: 0.0,
            follow_scroll_alignment: 0,
            line_lead_time_ms: 800,
            sources: vec!["NetEase".into(), "lrclib.net".into()],
            delay_ms: 0,
            prefer_local: true,
            show_provider: true,
            show_match: false,
            enable_romaji: false,
            enable_furigana: false,
            enable_netease_translation: false,
            enable_auto_translation: false,
            translation_api_provider: String::new(),
            translation_api_key: String::new(),
            translation_target_language: "en".into(),
        }
    }
}
