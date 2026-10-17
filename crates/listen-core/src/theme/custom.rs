//! User themes and token overrides from the config directory's `themes` folder (parsing and merging only;
//! the app watches the folder and calls [`load_dir`]):
//!
//! * `<id>.json` / `<id>.toml`: a theme (`mode`, `extends`, `colors`); see `docs/themes.md`.
//! * `overrides.json` / `overrides.toml`: colours, radii, spacing and font sizes over the active theme
//!   (stands in for Feishin's custom CSS).

use super::{Palette, Rgb, ThemeMode, palette};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// An `extends` chain longer than this is assumed to be a mistake (Feishin: 10).
pub const MAX_EXTENDS_DEPTH: usize = 10;
/// File stem of the override file.
pub const OVERRIDES_STEM: &str = "overrides";

/// The themes folder inside the config directory.
pub fn themes_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("themes")
}

/// One theme from the folder, fully resolved.
#[derive(Debug, Clone, PartialEq)]
pub struct CustomTheme {
    /// File stem; what `appearance.theme` stores.
    pub id: String,
    pub name: String,
    pub palette: Palette,
    /// The theme this one `extends` (built-in id or another custom id).
    pub extends: Option<String>,
    /// Non-fatal problems (dropped colours, ...).
    pub warnings: Vec<String>,
}

/// Why a file in the folder could not be used.
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeError {
    pub file: String,
    pub message: String,
}

/// Overrides from `overrides.json|toml`. Unset values keep the theme's.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Overrides {
    /// Palette colours (`background`, `primary`, ...) and token colours
    /// (`border`, `divider`, `nav-hover`, `icon-hover`, `player-bar`,
    /// `primary-filled`, `primary-shade`), keyed by normalised name.
    pub colors: BTreeMap<String, Rgb>,
    pub radius_sm: Option<f32>,
    pub radius_md: Option<f32>,
    /// xs, sm, md, lg, xl
    pub spacing: [Option<f32>; 5],
    /// xs, sm, md, lg, xl
    pub font_size: [Option<f32>; 5],
}

impl Overrides {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// A colour override by (any spelling of) its key.
    pub fn color(&self, key: &str) -> Option<Rgb> {
        self.colors.get(&norm(key)).copied()
    }

    /// Applies the palette colours onto `p`.
    pub fn apply_palette(&self, p: &mut Palette) {
        let set = |slot: &mut Rgb, key: &str| {
            if let Some(c) = self.color(key) {
                *slot = c;
            }
        };
        set(&mut p.background, "background");
        set(&mut p.background_alternate, "background-alternate");
        set(&mut p.foreground, "foreground");
        set(&mut p.foreground_muted, "foreground-muted");
        set(&mut p.primary, "primary");
        set(&mut p.state_error, "state-error");
        set(&mut p.state_info, "state-info");
        set(&mut p.state_success, "state-success");
        set(&mut p.state_warning, "state-warning");
        set(&mut p.surface, "surface");
        set(&mut p.surface_foreground, "surface-foreground");
    }
}

/// Everything found in the folder.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Loaded {
    pub themes: Vec<CustomTheme>,
    pub errors: Vec<ThemeError>,
    pub overrides: Overrides,
    pub override_warnings: Vec<String>,
}

impl Loaded {
    pub fn theme(&self, id: &str) -> Option<&CustomTheme> {
        self.themes.iter().find(|t| t.id == id)
    }

    /// Palette for `id`: a custom theme, else a built-in one.
    pub fn palette(&self, id: &str) -> Option<Palette> {
        self.theme(id).map(|t| t.palette).or_else(|| palette(id))
    }

    pub fn warning_count(&self) -> usize {
        self.themes.iter().map(|t| t.warnings.len()).sum::<usize>() + self.override_warnings.len()
    }
}

fn norm(key: &str) -> String {
    key.chars().filter(|c| *c != '-' && *c != '_' && *c != ' ').flat_map(char::to_lowercase).collect()
}

/// Parses a colour; also accepts `#rgb`, `#rrggbbaa` (alpha dropped).
pub fn parse_color(value: &Value) -> Option<Rgb> {
    let s = value.as_str()?.trim();
    if let Some(hex) = s.strip_prefix('#') {
        let hex = match hex.len() {
            3 => hex.chars().flat_map(|c| [c, c]).collect::<String>(),
            6 => hex.to_string(),
            8 => hex[..6].to_string(),
            _ => return None,
        };
        return Rgb::parse(&format!("#{hex}"));
    }
    Rgb::parse(s)
}

/// Parses JSON or TOML (by file extension) into a JSON value.
pub fn parse_text(text: &str, ext: &str) -> Result<Value, String> {
    let value: Value = if ext.eq_ignore_ascii_case("toml") {
        toml::from_str(text).map_err(|e| e.to_string())?
    } else {
        serde_json::from_str(text).map_err(|e| e.to_string())?
    };
    if value.is_object() { Ok(value) } else { Err("the file must contain an object".into()) }
}

/// Palette colour keys a theme may set (normalised).
const THEME_KEYS: [&str; 11] = [
    "background",
    "backgroundalternate",
    "foreground",
    "foregroundmuted",
    "primary",
    "stateerror",
    "stateinfo",
    "statesuccess",
    "statewarning",
    "surface",
    "surfaceforeground",
];

/// Extra keys the override file accepts on top of [`THEME_KEYS`].
const TOKEN_KEYS: [&str; 7] =
    ["border", "divider", "navhover", "iconhover", "playerbar", "primaryfilled", "primaryshade"];

const SIZE_NAMES: [&str; 5] = ["xs", "sm", "md", "lg", "xl"];

struct RawTheme {
    mode: Option<ThemeMode>,
    extends: Option<String>,
    name: Option<String>,
    colors: BTreeMap<String, Rgb>,
    warnings: Vec<String>,
}

fn read_colors(obj: Option<&Value>, allowed: &[&str], warnings: &mut Vec<String>) -> BTreeMap<String, Rgb> {
    let mut out = BTreeMap::new();
    let Some(map) = obj.and_then(Value::as_object) else { return out };
    for (key, value) in map {
        let k = norm(key);
        if !allowed.contains(&k.as_str()) {
            warnings.push(format!("unknown colour \"{key}\" ignored"));
            continue;
        }
        match parse_color(value) {
            Some(c) => {
                out.insert(k, c);
            }
            None => warnings.push(format!("invalid colour for \"{key}\" dropped: {value}")),
        }
    }
    out
}

fn raw_theme(value: &Value) -> RawTheme {
    let mut warnings = Vec::new();
    let mode = match value.get("mode").and_then(Value::as_str) {
        Some(m) if m.eq_ignore_ascii_case("dark") => Some(ThemeMode::Dark),
        Some(m) if m.eq_ignore_ascii_case("light") => Some(ThemeMode::Light),
        Some(other) => {
            warnings.push(format!("unknown mode \"{other}\" (use dark or light)"));
            None
        }
        None => None,
    };
    RawTheme {
        mode,
        extends: value.get("extends").and_then(Value::as_str).map(str::to_string),
        name: value.get("name").and_then(Value::as_str).map(str::to_string),
        colors: read_colors(value.get("colors"), &THEME_KEYS, &mut warnings),
        warnings,
    }
}

fn label_from_id(id: &str) -> String {
    id.split(['-', '_']).filter(|w| !w.is_empty()).map(capitalise).collect::<Vec<_>>().join(" ")
}

fn capitalise(w: &str) -> String {
    let mut c = w.chars();
    c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
}

/// Resolves `id` by following `extends` (custom ids first, then built-in); child colours win.
/// Errors on cycles or excessive depth.
fn resolve(id: &str, raws: &HashMap<String, RawTheme>) -> Result<(Palette, Option<String>), String> {
    // Collect the chain child -> ... -> root.
    let mut chain: Vec<&RawTheme> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut cur = id;
    let mut base: Option<Palette> = None;
    loop {
        if !seen.insert(cur) {
            return Err(format!("\"extends\" loops back to \"{cur}\""));
        }
        if chain.len() > MAX_EXTENDS_DEPTH {
            return Err(format!("\"extends\" chain is longer than {MAX_EXTENDS_DEPTH}"));
        }
        let raw = &raws[cur];
        chain.push(raw);
        match raw.extends.as_deref() {
            Some(next) if raws.contains_key(next) => cur = next,
            Some(builtin) => {
                base = Some(palette(builtin).ok_or_else(|| {
                    format!("\"extends\": no theme called \"{builtin}\" (built-in or in this folder)")
                })?);
                break;
            }
            None => break,
        }
    }
    // Without any base, start from the default theme of the declared mode.
    let mode = chain.iter().find_map(|r| r.mode);
    let mut p = base.unwrap_or_else(|| {
        let id = if mode == Some(ThemeMode::Light) { "defaultLight" } else { "defaultDark" };
        palette(id).expect("default palettes exist")
    });
    // Apply from the root towards the child so the child wins.
    for raw in chain.iter().rev() {
        let o = Overrides { colors: raw.colors.clone(), ..Default::default() };
        o.apply_palette(&mut p);
    }
    if let Some(mode) = mode {
        p.mode = mode;
    }
    Ok((p, raws[id].extends.clone()))
}

/// Resolves a set of already-parsed theme files (id -> JSON value).
pub fn resolve_all(files: &BTreeMap<String, Value>) -> (Vec<CustomTheme>, Vec<ThemeError>) {
    let raws: HashMap<String, RawTheme> = files.iter().map(|(id, v)| (id.clone(), raw_theme(v))).collect();
    let mut themes = Vec::new();
    let mut errors = Vec::new();
    for id in files.keys() {
        match resolve(id, &raws) {
            Ok((palette, extends)) => {
                let raw = &raws[id];
                themes.push(CustomTheme {
                    id: id.clone(),
                    name: raw.name.clone().unwrap_or_else(|| label_from_id(id)),
                    palette,
                    extends,
                    warnings: raw.warnings.clone(),
                });
            }
            Err(message) => errors.push(ThemeError { file: id.clone(), message }),
        }
    }
    (themes, errors)
}

/// Parses an override file's contents.
pub fn parse_overrides(value: &Value) -> (Overrides, Vec<String>) {
    let mut warnings = Vec::new();
    let allowed: Vec<&str> = THEME_KEYS.iter().chain(TOKEN_KEYS.iter()).copied().collect();
    let colors = read_colors(value.get("colors"), &allowed, &mut warnings);
    let mut ov = Overrides { colors, ..Default::default() };
    let number = |section: &str, key: &str, min: f32, max: f32, warnings: &mut Vec<String>| {
        let v = value.get(section)?.get(key)?;
        match v.as_f64() {
            Some(n) if n.is_finite() => Some((n as f32).clamp(min, max)),
            _ => {
                warnings.push(format!("{section}.{key} must be a number"));
                None
            }
        }
    };
    ov.radius_sm = number("radius", "sm", 0.0, 32.0, &mut warnings);
    ov.radius_md = number("radius", "md", 0.0, 32.0, &mut warnings);
    for (i, name) in SIZE_NAMES.iter().enumerate() {
        ov.spacing[i] = number("spacing", name, 0.0, 64.0, &mut warnings);
        ov.font_size[i] = number("font_size", name, 6.0, 48.0, &mut warnings);
    }
    if let Some(obj) = value.as_object() {
        for key in obj.keys() {
            if !matches!(key.as_str(), "colors" | "radius" | "spacing" | "font_size") {
                warnings.push(format!("unknown section \"{key}\" ignored"));
            }
        }
    }
    (ov, warnings)
}

/// Reads the whole folder. A missing folder is not an error.
pub fn load_dir(dir: &Path) -> Loaded {
    let mut out = Loaded::default();
    let Ok(entries) = std::fs::read_dir(dir) else { return out };
    let mut files: BTreeMap<String, Value> = BTreeMap::new();
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        let (Some(stem), Some(ext)) = (
            path.file_stem().and_then(|s| s.to_str()).map(str::to_string),
            path.extension().and_then(|s| s.to_str()).map(str::to_lowercase),
        ) else {
            continue;
        };
        if ext != "json" && ext != "toml" {
            continue;
        }
        let file = format!("{stem}.{ext}");
        let parsed =
            std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| parse_text(&t, &ext));
        match parsed {
            Ok(value) if stem == OVERRIDES_STEM => {
                let (ov, warnings) = parse_overrides(&value);
                out.overrides = ov;
                out.override_warnings = warnings;
            }
            Ok(value) => {
                if files.insert(stem.clone(), value).is_some() {
                    out.errors.push(ThemeError { file, message: "two files with the same name".into() });
                }
            }
            Err(message) => out.errors.push(ThemeError { file, message }),
        }
    }
    let (themes, errors) = resolve_all(&files);
    out.themes = themes;
    out.errors.extend(errors);
    out
}

/// A cheap fingerprint of the folder (names, sizes, mtimes) to detect changes.
pub fn stamp(dir: &Path) -> Vec<(String, u64, Option<std::time::SystemTime>)> {
    let mut out: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            Some((e.file_name().to_string_lossy().into_owned(), meta.len(), meta.modified().ok()))
        })
        .collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(list: &[(&str, &str)]) -> BTreeMap<String, Value> {
        list.iter().map(|(id, text)| ((*id).to_string(), parse_text(text, "json").unwrap())).collect()
    }

    #[test]
    fn extends_a_builtin_theme() {
        let f = files(&[("mine", r##"{"extends":"tokyoNight","colors":{"primary":"#ff0000"}}"##)]);
        let (themes, errors) = resolve_all(&f);
        assert!(errors.is_empty());
        let t = &themes[0];
        let base = palette("tokyoNight").unwrap();
        assert_eq!(t.palette.primary, Rgb(255, 0, 0));
        assert_eq!(t.palette.background, base.background);
        assert_eq!(t.palette.mode, ThemeMode::Dark);
        assert_eq!(t.extends.as_deref(), Some("tokyoNight"));
        assert_eq!(t.name, "Mine");
    }

    #[test]
    fn extends_chain_child_wins() {
        let f = files(&[
            ("a", r##"{"extends":"b","colors":{"surface":"#111111"}}"##),
            ("b", r##"{"extends":"c","colors":{"surface":"#222222","foreground":"#333333"}}"##),
            ("c", r##"{"extends":"defaultLight","colors":{"background":"#444444"}}"##),
        ]);
        let (themes, errors) = resolve_all(&f);
        assert!(errors.is_empty(), "{errors:?}");
        let a = themes.iter().find(|t| t.id == "a").unwrap();
        assert_eq!(a.palette.surface, Rgb(0x11, 0x11, 0x11));
        assert_eq!(a.palette.foreground, Rgb(0x33, 0x33, 0x33));
        assert_eq!(a.palette.background, Rgb(0x44, 0x44, 0x44));
        assert_eq!(a.palette.mode, ThemeMode::Light);
    }

    #[test]
    fn cycles_depth_and_unknown_parents_are_errors() {
        let f = files(&[
            ("x", r#"{"extends":"y"}"#),
            ("y", r#"{"extends":"x"}"#),
            ("z", r#"{"extends":"noSuchTheme"}"#),
            ("ok", r#"{"mode":"light"}"#),
        ]);
        let (themes, errors) = resolve_all(&f);
        assert_eq!(themes.len(), 1);
        assert_eq!(themes[0].palette.mode, ThemeMode::Light);
        assert_eq!(errors.len(), 3);
        // Longer than the limit.
        let mut list = Vec::new();
        for i in 0..=MAX_EXTENDS_DEPTH + 2 {
            list.push((format!("t{i}"), format!(r#"{{"extends":"t{}"}}"#, i + 1)));
        }
        list.push((format!("t{}", MAX_EXTENDS_DEPTH + 3), r#"{"extends":"nord"}"#.into()));
        let f: BTreeMap<String, Value> =
            list.iter().map(|(i, t)| (i.clone(), parse_text(t, "json").unwrap())).collect();
        let (_, errors) = resolve_all(&f);
        assert!(errors.iter().any(|e| e.file == "t0" && e.message.contains("longer")));
    }

    #[test]
    fn invalid_colours_are_dropped_with_a_warning() {
        let f = files(&[(
            "w",
            r##"{"colors":{"primary":"not a colour","surface":"#abc","foreground-muted":"rgb(1, 2, 3)","bogus":"#fff"}}"##,
        )]);
        let (themes, _) = resolve_all(&f);
        let t = &themes[0];
        assert_eq!(t.warnings.len(), 2);
        assert_eq!(t.palette.primary, palette("defaultDark").unwrap().primary);
        assert_eq!(t.palette.surface, Rgb(0xaa, 0xbb, 0xcc));
        assert_eq!(t.palette.foreground_muted, Rgb(1, 2, 3));
    }

    #[test]
    fn override_file_json_and_toml() {
        let json = parse_text(
            r##"{"colors":{"background":"#101010","nav-hover":"#222222"},
                 "radius":{"md":9},"spacing":{"lg":20},"font_size":{"md":15.5,"xl":900}}"##,
            "json",
        )
        .unwrap();
        let (ov, warnings) = parse_overrides(&json);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(ov.radius_md, Some(9.0));
        assert_eq!(ov.spacing[3], Some(20.0));
        assert_eq!(ov.font_size[2], Some(15.5));
        assert_eq!(ov.font_size[4], Some(48.0), "clamped");
        assert_eq!(ov.color("navHover"), Some(Rgb(0x22, 0x22, 0x22)));

        let toml = parse_text(
            "[colors]\nbackground = \"#101010\"\nnav-hover = \"#222222\"\n[radius]\nmd = 9\n[spacing]\nlg = 20\n[font_size]\nmd = 15.5\nxl = 900\n",
            "toml",
        )
        .unwrap();
        let (ov2, _) = parse_overrides(&toml);
        assert_eq!(ov2, ov);
    }

    #[test]
    fn overrides_apply_to_a_palette() {
        let (ov, _) =
            parse_overrides(&parse_text(r##"{"colors":{"background":"#010203"}}"##, "json").unwrap());
        let mut p = palette("tokyoNight").unwrap();
        ov.apply_palette(&mut p);
        assert_eq!(p.background, Rgb(1, 2, 3));
        assert_eq!(p.foreground, palette("tokyoNight").unwrap().foreground);
        let (bad, warnings) = parse_overrides(
            &parse_text(r#"{"colors":{"background":"nope"},"radius":{"md":"x"},"wat":1}"#, "json").unwrap(),
        );
        assert!(bad.is_empty());
        assert_eq!(warnings.len(), 3);
    }

    #[test]
    fn loads_a_folder() {
        let dir = std::env::temp_dir().join(format!("listen-themes-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("sunset.json"), r##"{"extends":"nord","colors":{"primary":"#ff8800"}}"##)
            .unwrap();
        std::fs::write(dir.join("moss.toml"), "mode = \"dark\"\n[colors]\nbackground = \"#0a1a0a\"\n")
            .unwrap();
        std::fs::write(dir.join("broken.json"), "{ not json").unwrap();
        std::fs::write(dir.join("overrides.toml"), "[radius]\nmd = 2\n").unwrap();
        std::fs::write(dir.join("readme.txt"), "ignored").unwrap();
        let loaded = load_dir(&dir);
        assert_eq!(loaded.themes.len(), 2);
        assert_eq!(loaded.errors.len(), 1);
        assert_eq!(loaded.errors[0].file, "broken.json");
        assert_eq!(loaded.overrides.radius_md, Some(2.0));
        assert_eq!(loaded.palette("sunset").unwrap().primary, Rgb(255, 136, 0));
        assert!(loaded.palette("nord").is_some(), "built-ins still resolve");
        let before = stamp(&dir);
        std::fs::write(dir.join("moss.toml"), "mode = \"light\"\n").unwrap();
        assert_ne!(before, stamp(&dir));
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(load_dir(&dir), Loaded::default());
    }
}
