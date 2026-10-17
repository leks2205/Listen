//! Runtime appearance helpers that are not tied to the `Listen` state: the
//! UI font (read everywhere text is built), font discovery and font files.

use iced::Font;
use iced::font::Family;
use listen_core::settings::{Appearance, BUILTIN_FONTS, FontKind};
use std::collections::HashSet;
use std::sync::{Mutex, RwLock};

static FONT: RwLock<Font> = RwLock::new(Font::with_name("Inter"));
static NAMES: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);

/// The family all text is drawn with (`appearance.font`).
pub fn current_font() -> Font {
    FONT.read().map(|f| *f).unwrap_or(crate::app::INTER)
}

pub fn set_font(font: Font) {
    if let Ok(mut f) = FONT.write() {
        *f = font;
    }
}

/// iced wants `&'static str` family names; each distinct name is leaked once.
pub fn intern(name: &str) -> &'static str {
    let mut guard = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    let set = guard.get_or_insert_with(HashSet::new);
    if let Some(found) = set.get(name) {
        return found;
    }
    let leaked: &'static str = Box::leak(name.to_string().into_boxed_str());
    set.insert(leaked);
    leaked
}

/// The font for a family name.
pub fn family(name: &str) -> Font {
    Font { family: Family::Name(intern(name)), ..Font::DEFAULT }
}

/// The font the settings ask for (a custom file is resolved by [`read_font_file`]).
pub fn font_for(a: &Appearance) -> Font {
    match a.font_kind {
        FontKind::Builtin => {
            let name =
                BUILTIN_FONTS.iter().find(|f| f.eq_ignore_ascii_case(&a.font)).unwrap_or(&BUILTIN_FONTS[0]);
            family(name)
        }
        FontKind::System | FontKind::Custom if a.font.trim().is_empty() => crate::app::INTER,
        FontKind::System | FontKind::Custom => family(&a.font),
    }
}

/// Family names installed on the system, sorted and de-duplicated.
pub fn system_families() -> Vec<String> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    let mut names: Vec<String> =
        db.faces().filter_map(|f| f.families.first().map(|(name, _)| name.clone())).collect();
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup();
    names
}

/// True when `bytes` start like a TrueType/OpenType/collection file.
pub fn is_font_data(bytes: &[u8]) -> bool {
    matches!(bytes.get(..4), Some([0, 1, 0, 0] | b"OTTO" | b"true" | b"ttcf"))
}

/// Reads a font file, checking its magic number (as Feishin does), and returns
/// the bytes with the family name inside them.
pub fn read_font_file(path: &str) -> Result<(Vec<u8>, String), String> {
    let path = path.trim().trim_matches('"');
    if path.is_empty() {
        return Err("Pick a .ttf, .otf or .ttc file.".into());
    }
    let bytes = std::fs::read(path).map_err(|e| format!("Could not read the font file: {e}"))?;
    if !is_font_data(&bytes) {
        return Err("That file is not a TrueType/OpenType font.".into());
    }
    family_of(&bytes).map(|name| (bytes, name))
}

fn family_of(bytes: &[u8]) -> Result<String, String> {
    let mut db = fontdb::Database::new();
    db.load_font_data(bytes.to_vec());
    db.faces()
        .next()
        .and_then(|f| f.families.first().map(|(name, _)| name.clone()))
        .ok_or_else(|| "The font file has no usable family name.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_and_unknown_names() {
        let a = Appearance { font: "Poppins".into(), ..Appearance::default() };
        assert_eq!(font_for(&a), family("Poppins"));
        let a = Appearance { font: "Nope".into(), ..Appearance::default() };
        assert_eq!(font_for(&a), family("Inter"));
        let a = Appearance { font: "Fira Sans".into(), font_kind: FontKind::System, ..Appearance::default() };
        assert_eq!(font_for(&a), family("Fira Sans"));
        assert!(std::ptr::eq(intern("Fira Sans"), intern("Fira Sans")));
    }

    #[test]
    fn font_files_are_checked() {
        let dir = std::env::temp_dir().join(format!("listen-font-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let good = dir.join("p.ttf");
        std::fs::write(&good, include_bytes!("../../assets/fonts/Poppins-Regular.ttf")).unwrap();
        let (bytes, name) = read_font_file(good.to_str().unwrap()).unwrap();
        assert!(is_font_data(&bytes));
        assert_eq!(name, "Poppins");
        let bad = dir.join("x.ttf");
        std::fs::write(&bad, b"not a font").unwrap();
        assert!(read_font_file(bad.to_str().unwrap()).is_err());
        assert!(read_font_file("").is_err());
        assert!(read_font_file("/definitely/missing.ttf").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
