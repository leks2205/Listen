//! The desktop's colours for "Follow desktop theme", read from KDE files:
//! `~/.config/rice-profiles/current_theme` (a `theme-switcher` key) picks a hand-tuned
//! palette unless kdeglobals shows another scheme; otherwise the palette is derived
//! from kdeglobals' `[Colors:*]` groups and `[General] AccentColor` (`r,g,b` values).

use super::{Palette, Rgb, ThemeMode};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// A rice theme: `current_theme` key, preset id, display name, the KDE colour
/// scheme `theme-switcher` applies, and the palette.
pub struct Rice {
    pub key: &'static str,
    pub id: &'static str,
    pub name: &'static str,
    pub color_scheme: &'static str,
    pub palette: Palette,
}

const fn rgb(hex: u32) -> Rgb {
    Rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

#[allow(clippy::too_many_arguments)]
const fn dark(
    background: u32,
    background_alternate: u32,
    surface: u32,
    foreground: u32,
    muted: u32,
    primary: u32,
    error: u32,
    info: u32,
    success: u32,
    warning: u32,
) -> Palette {
    Palette {
        mode: ThemeMode::Dark,
        background: rgb(background),
        background_alternate: rgb(background_alternate),
        foreground: rgb(foreground),
        foreground_muted: rgb(muted),
        primary: rgb(primary),
        state_error: rgb(error),
        state_info: rgb(info),
        state_success: rgb(success),
        state_warning: rgb(warning),
        surface: rgb(surface),
        surface_foreground: rgb(foreground),
    }
}

/// Hand-tuned to the rice (`theme-switcher`'s THEMES: bg, base, card, text,
/// accent) plus each theme's own muted-text and state colours.
pub const RICE: &[Rice] = &[
    Rice {
        key: "tokyo-night",
        id: "riceTokyoNight",
        name: "Tokyo Night (desktop)",
        color_scheme: "TokyoNight",
        palette: dark(
            0x1a1b26, 0x16161e, 0x1f2335, 0xc0caf5, 0xa9b1d6, 0x7aa2f7, 0xf7768e, 0x7dcfff, 0x9ece6a,
            0xe0af68,
        ),
    },
    Rice {
        key: "catppuccin",
        id: "riceCatppuccinMocha",
        name: "Catppuccin Mocha (desktop)",
        color_scheme: "CatppuccinMocha",
        palette: dark(
            0x1e1e2e, 0x181825, 0x313244, 0xcdd6f4, 0xa6adc8, 0xcba6f7, 0xf38ba8, 0x89dceb, 0xa6e3a1,
            0xf9e2af,
        ),
    },
    Rice {
        key: "gruvbox",
        id: "riceGruvboxDark",
        name: "Gruvbox Dark (desktop)",
        color_scheme: "GruvboxDark",
        palette: dark(
            0x282828, 0x1d2021, 0x3c3836, 0xebdbb2, 0xa89984, 0xfe8019, 0xfb4934, 0x83a598, 0xb8bb26,
            0xfabd2f,
        ),
    },
    Rice {
        key: "nord",
        id: "riceNord",
        name: "Nord (desktop)",
        color_scheme: "Nord",
        palette: dark(
            0x2e3440, 0x242933, 0x3b4252, 0xeceff4, 0xa5abb6, 0x88c0d0, 0xbf616a, 0x81a1c1, 0xa3be8c,
            0xebcb8b,
        ),
    },
    Rice {
        key: "rose-pine",
        id: "riceRosePine",
        name: "Rosé Pine (desktop)",
        color_scheme: "RosePine",
        palette: dark(
            0x191724, 0x16141f, 0x26233a, 0xe0def4, 0x908caa, 0xeb6f92, 0xeb6f92, 0xc4a7e7, 0x9ccfd8,
            0xf6c177,
        ),
    },
    Rice {
        key: "dracula",
        id: "riceDracula",
        name: "Dracula (desktop)",
        color_scheme: "Dracula",
        palette: dark(
            0x282a36, 0x21222c, 0x44475a, 0xf8f8f2, 0xb6b8c8, 0xbd93f9, 0xff5555, 0x8be9fd, 0x50fa7b,
            0xf1fa8c,
        ),
    },
    Rice {
        key: "stock",
        id: "riceKdeStock",
        name: "KDE Stock / Breeze Dark (desktop)",
        color_scheme: "BreezeDark",
        palette: dark(
            0x202326, 0x141618, 0x292c30, 0xfcfcfc, 0xa1a9b1, 0x3daee9, 0xda4453, 0x3daee9, 0x27ae60,
            0xf67400,
        ),
    },
];

pub fn rice(key: &str) -> Option<&'static Rice> {
    RICE.iter().find(|r| r.key == key)
}

/// The desktop theme currently in effect.
#[derive(Debug, Clone, PartialEq)]
pub struct DesktopTheme {
    /// Display name ("Catppuccin Mocha", or the KDE colour scheme's name).
    pub name: String,
    pub palette: Palette,
}

/// `$XDG_CONFIG_HOME` or `~/.config`. `None` on Windows: there is no KDE/rice theme to follow.
pub fn config_dir() -> Option<PathBuf> {
    if !cfg!(unix) {
        return None;
    }
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
}

fn files(dir: &Path) -> [PathBuf; 2] {
    [dir.join("kdeglobals"), dir.join("rice-profiles").join("current_theme")]
}

/// Modification times of the watched files (cheap; poll this).
pub fn stamp(dir: &Path) -> [Option<SystemTime>; 2] {
    files(dir).map(|f| std::fs::metadata(f).and_then(|m| m.modified()).ok())
}

/// `[Section] key=value` of an INI file like kdeglobals.
type Ini = HashMap<String, HashMap<String, String>>;

fn parse_ini(text: &str) -> Ini {
    let mut out: Ini = HashMap::new();
    let mut section = String::new();
    for line in text.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name.to_string();
        } else if let Some((k, v)) = line.split_once('=') {
            out.entry(section.clone()).or_default().insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    out
}

/// `"30,30,46"` → `Rgb(30, 30, 46)`.
fn kde_rgb(ini: &Ini, section: &str, key: &str) -> Option<Rgb> {
    let v = ini.get(section)?.get(key)?;
    let mut parts = v.split(',').map(|p| p.trim().parse::<u8>().ok());
    Some(Rgb(parts.next()??, parts.next()??, parts.next()??))
}

fn luminance(c: Rgb) -> f32 {
    (0.2126 * f32::from(c.0) + 0.7152 * f32::from(c.1) + 0.0722 * f32::from(c.2)) / 255.0
}

/// A palette from kdeglobals' colour groups.
fn from_kdeglobals(ini: &Ini) -> Option<Palette> {
    let window = |k: &str| kde_rgb(ini, "Colors:Window", k);
    let background = window("BackgroundNormal")?;
    let foreground = window("ForegroundNormal")?;
    // The sidebar/list colour: the view background if it differs, else the alternate row colour.
    let view = kde_rgb(ini, "Colors:View", "BackgroundNormal").filter(|v| *v != background);
    let background_alternate = view.or_else(|| window("BackgroundAlternate")).unwrap_or(background);
    let primary = ini
        .get("General")
        .and_then(|g| g.get("AccentColor"))
        .and_then(|_| kde_rgb(ini, "General", "AccentColor"))
        .or_else(|| kde_rgb(ini, "Colors:Selection", "BackgroundNormal"))
        .unwrap_or(Rgb(61, 174, 233));
    let surface = kde_rgb(ini, "Colors:Button", "BackgroundNormal").unwrap_or(background_alternate);
    Some(Palette {
        mode: if luminance(background) < 0.5 { ThemeMode::Dark } else { ThemeMode::Light },
        background,
        background_alternate,
        foreground,
        foreground_muted: window("ForegroundInactive").unwrap_or(foreground),
        primary,
        state_error: window("ForegroundNegative").unwrap_or(Rgb(218, 68, 83)),
        state_info: window("ForegroundLink").unwrap_or(primary),
        state_success: window("ForegroundPositive").unwrap_or(Rgb(39, 174, 96)),
        state_warning: window("ForegroundNeutral").unwrap_or(Rgb(246, 116, 0)),
        surface,
        surface_foreground: kde_rgb(ini, "Colors:Button", "ForegroundNormal").unwrap_or(foreground),
    })
}

/// Reads the desktop theme from `dir` (usually [`config_dir`]); `None` when
/// neither file is there (not KDE, or not Linux).
pub fn read(dir: &Path) -> Option<DesktopTheme> {
    let [kdeglobals, current] = files(dir);
    let ini = std::fs::read_to_string(kdeglobals).map(|t| parse_ini(&t)).ok();
    let scheme = ini.as_ref().and_then(|i| i.get("General")).and_then(|g| g.get("ColorScheme")).cloned();
    let key = std::fs::read_to_string(current).ok().map(|k| k.trim().to_lowercase());

    // The rice palette, if kdeglobals agrees (or says nothing).
    if let Some(r) = key.as_deref().and_then(rice)
        && scheme.as_deref().is_none_or(|s| s == r.color_scheme)
    {
        let name = r.name.trim_end_matches(" (desktop)").to_string();
        return Some(DesktopTheme { name, palette: r.palette });
    }
    // Any other colour scheme (KDE Stock, System Settings): derive it.
    let palette = from_kdeglobals(ini.as_ref()?)?;
    Some(DesktopTheme { name: scheme.unwrap_or_else(|| "KDE colours".into()), palette })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CATPPUCCIN_KDEGLOBALS: &str = "[ColorEffects:Disabled]\nColor=56,56,56\n\n\
        [Colors:Button]\nBackgroundNormal=49,50,68\nForegroundNormal=205,214,244\n\n\
        [Colors:Selection]\nBackgroundNormal=203,166,247\n\n\
        [Colors:View]\nBackgroundNormal=30,30,46\n\n\
        [Colors:Window]\nBackgroundAlternate=24,24,37\nBackgroundNormal=30,30,46\nForegroundInactive=120,124,153\n\
        ForegroundLink=203,166,247\nForegroundNegative=226,140,140\nForegroundNeutral=247,193,150\n\
        ForegroundNormal=205,214,244\nForegroundPositive=179,225,163\n\n\
        [General]\nColorScheme=CatppuccinMocha\n";

    fn dir(name: &str, kdeglobals: Option<&str>, current: Option<&str>) -> PathBuf {
        let d = std::env::temp_dir().join(format!("listen-desktop-theme-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("rice-profiles")).unwrap();
        if let Some(k) = kdeglobals {
            std::fs::write(d.join("kdeglobals"), k).unwrap();
        }
        if let Some(c) = current {
            std::fs::write(d.join("rice-profiles/current_theme"), c).unwrap();
        }
        d
    }

    #[test]
    fn rice_theme_uses_hand_tuned_palette() {
        let d = dir("rice", Some(CATPPUCCIN_KDEGLOBALS), Some("catppuccin\n"));
        let t = read(&d).unwrap();
        assert_eq!(t.name, "Catppuccin Mocha");
        assert_eq!(t.palette.primary, Rgb(0xcb, 0xa6, 0xf7));
        assert_eq!(t.palette.background_alternate, Rgb(0x18, 0x18, 0x25));
    }

    #[test]
    fn stale_current_theme_falls_back_to_kdeglobals() {
        // KDE Stock was applied: current_theme still says nord, kdeglobals says Breeze.
        let breeze = "[Colors:Window]\nBackgroundNormal=32,35,38\nForegroundNormal=252,252,252\n\
                      ForegroundInactive=161,169,177\n[Colors:View]\nBackgroundNormal=20,22,24\n\
                      [General]\nColorScheme=BreezeDark\nAccentColor=61,174,233\n";
        let d = dir("stock", Some(breeze), Some("nord"));
        let t = read(&d).unwrap();
        assert_eq!(t.name, "BreezeDark");
        assert_eq!(t.palette.background, Rgb(32, 35, 38));
        assert_eq!(t.palette.background_alternate, Rgb(20, 22, 24));
        assert_eq!(t.palette.primary, Rgb(61, 174, 233));
        assert_eq!(t.palette.mode, ThemeMode::Dark);
    }

    #[test]
    fn derived_palette_from_kdeglobals_only() {
        let d = dir("kde", Some(CATPPUCCIN_KDEGLOBALS), None);
        let t = read(&d).unwrap();
        assert_eq!(t.name, "CatppuccinMocha");
        assert_eq!(t.palette.primary, Rgb(203, 166, 247)); // selection colour (no AccentColor)
        assert_eq!(t.palette.surface, Rgb(49, 50, 68));
        assert_eq!(t.palette.state_error, Rgb(226, 140, 140));
    }

    #[test]
    fn nothing_to_follow() {
        let d = dir("none", None, None);
        assert_eq!(read(&d), None);
        assert_eq!(stamp(&d), [None, None]);
    }

    #[test]
    fn every_rice_key_resolves() {
        for key in ["tokyo-night", "catppuccin", "gruvbox", "nord", "rose-pine", "dracula", "stock"] {
            assert!(rice(key).is_some(), "{key}");
        }
    }
}
