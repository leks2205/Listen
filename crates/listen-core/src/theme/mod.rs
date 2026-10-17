//! Feishin's theme palettes (generated into `palettes.rs`) plus accent handling.

pub mod custom;
pub mod desktop;
mod palettes;
pub mod shades;

use serde::{Deserialize, Serialize};

pub use palettes::PALETTES;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// Parses `rgb(53, 116, 252)`, `rgb(53 116 252)` or `#3574fc`.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if let Some(hex) = s.strip_prefix('#') {
            if hex.len() != 6 {
                return None;
            }
            let v = u32::from_str_radix(hex, 16).ok()?;
            return Some(Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8));
        }
        let inner = s.strip_prefix("rgba(").or_else(|| s.strip_prefix("rgb("))?;
        let mut parts = inner
            .trim_end_matches(')')
            .split(|c: char| c == ',' || c.is_whitespace() || c == '/')
            .filter(|p| !p.is_empty())
            .map(|p| p.parse::<u8>());
        Some(Rgb(parts.next()?.ok()?, parts.next()?.ok()?, parts.next()?.ok()?))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeMode {
    Dark,
    Light,
}

/// The color variables of one Feishin theme (`--theme-colors-*`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Palette {
    pub mode: ThemeMode,
    pub background: Rgb,
    pub background_alternate: Rgb,
    pub foreground: Rgb,
    pub foreground_muted: Rgb,
    pub primary: Rgb,
    pub state_error: Rgb,
    pub state_info: Rgb,
    pub state_success: Rgb,
    pub state_warning: Rgb,
    pub surface: Rgb,
    pub surface_foreground: Rgb,
}

/// Looks up a palette by Feishin theme id (e.g. `tokyoNight`).
pub fn palette(id: &str) -> Option<Palette> {
    PALETTES
        .iter()
        .find(|(key, _, _)| *key == id)
        .map(|(_, _, p)| *p)
        .or_else(|| desktop::RICE.iter().find(|r| r.id == id).map(|r| r.palette))
}

/// Theme ids and display names for the settings picker: Feishin's themes, then the KDE rice presets.
pub fn theme_names() -> impl Iterator<Item = (&'static str, &'static str)> {
    PALETTES.iter().map(|(id, name, _)| (*id, *name)).chain(desktop::RICE.iter().map(|r| (r.id, r.name)))
}

/// Resolves the palette to use, applying Feishin's accent rules:
/// unless `use_theme_accent` is set, the custom accent replaces `primary`.
pub fn resolve(theme_id: &str, accent: Option<Rgb>, use_theme_accent: bool) -> Palette {
    let p = palette(theme_id).or_else(|| palette("defaultDark")).expect("defaultDark palette is generated");
    with_accent(p, accent, use_theme_accent)
}

/// [`resolve`] for an already looked-up palette (built-in or custom).
pub fn with_accent(mut p: Palette, accent: Option<Rgb>, use_theme_accent: bool) -> Palette {
    if let (false, Some(accent)) = (use_theme_accent, accent) {
        p.primary = accent;
    }
    p
}

/// The 5 accent swatches of Feishin's colour input.
pub const ACCENT_SWATCHES: [Rgb; 5] =
    [Rgb(53, 116, 252), Rgb(240, 170, 22), Rgb(29, 185, 84), Rgb(214, 81, 63), Rgb(170, 110, 216)];

impl Rgb {
    /// `rgb(r, g, b)`, the format Feishin's colour input uses.
    pub fn css(self) -> String {
        format!("rgb({}, {}, {})", self.0, self.1, self.2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_colors() {
        assert_eq!(Rgb::parse("rgb(53, 116, 252)"), Some(Rgb(53, 116, 252)));
        assert_eq!(Rgb::parse("rgb(26 27 38 / 85%)"), Some(Rgb(26, 27, 38)));
        assert_eq!(Rgb::parse("#3574fc"), Some(Rgb(0x35, 0x74, 0xfc)));
        assert_eq!(Rgb::parse("blue"), None);
    }

    #[test]
    fn all_feishin_themes_present() {
        assert_eq!(PALETTES.len(), 32);
        let tn = palette("tokyoNight").unwrap();
        assert_eq!(tn.background, Rgb(26, 27, 38));
        assert_eq!(tn.mode, ThemeMode::Dark);
        assert_eq!(palette("defaultLight").unwrap().mode, ThemeMode::Light);
    }

    #[test]
    fn custom_accent_overrides_primary() {
        let p = resolve("tokyoNight", Some(Rgb(53, 116, 252)), false);
        assert_eq!(p.primary, Rgb(53, 116, 252));
        let p = resolve("tokyoNight", Some(Rgb(53, 116, 252)), true);
        assert_eq!(p.primary, Rgb(125, 207, 255));
    }
}
