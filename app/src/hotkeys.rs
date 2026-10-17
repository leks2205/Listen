//! Window hotkeys in Feishin's notation (`space`, `mod+k`, `shift+arrowright`):
//! parsing, serializing, recording a pressed combo, duplicate detection and the
//! conversion to the desktop portal's trigger notation.

use iced::keyboard::{Key, Modifiers, key::Named, key::Physical};
use std::collections::{BTreeMap, BTreeSet};

/// A parsed binding. `ctrl` is Feishin's `mod`; `meta` is the logo key.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Binding {
    pub key: String,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
}

pub fn parse(s: &str) -> Option<Binding> {
    let mut b = Binding::default();
    for part in s.to_lowercase().split('+') {
        match part.trim() {
            "mod" | "ctrl" | "control" => b.ctrl = true,
            "meta" | "cmd" | "super" | "win" => b.meta = true,
            "shift" => b.shift = true,
            "alt" | "option" => b.alt = true,
            "" => return None,
            key => b.key = key.to_string(),
        }
    }
    (!b.key.is_empty()).then_some(b)
}

impl Binding {
    /// Feishin's notation, in the order its recorder writes it: `mod+alt+shift+meta+key`.
    pub fn serialize(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("mod");
        }
        if self.alt {
            parts.push("alt");
        }
        if self.shift {
            parts.push("shift");
        }
        if self.meta {
            parts.push("meta");
        }
        parts.push(&self.key);
        parts.join("+")
    }

    /// The XDG GlobalShortcuts "preferred trigger" (`CTRL+ALT+space`).
    pub fn portal_trigger(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        for (on, name) in [(self.ctrl, "CTRL"), (self.alt, "ALT"), (self.shift, "SHIFT"), (self.meta, "LOGO")]
        {
            if on {
                parts.push(name.into());
            }
        }
        parts.push(keysym(&self.key));
        parts.join("+")
    }
}

/// X11 keysym name of a Feishin key name.
fn keysym(key: &str) -> String {
    let fixed = match key {
        "arrowleft" => "Left",
        "arrowright" => "Right",
        "arrowup" => "Up",
        "arrowdown" => "Down",
        "enter" => "Return",
        "escape" => "Escape",
        "tab" => "Tab",
        "backspace" => "BackSpace",
        "delete" => "Delete",
        "home" => "Home",
        "end" => "End",
        "insert" => "Insert",
        "pageup" => "Page_Up",
        "pagedown" => "Page_Down",
        "numpadadd" => "KP_Add",
        "numpaddecimal" => "KP_Decimal",
        "numpaddivide" => "KP_Divide",
        "numpadmultiply" => "KP_Multiply",
        "numpadsubtract" => "KP_Subtract",
        "numpadenter" => "KP_Enter",
        "mediaplaypause" => "XF86AudioPlay",
        "mediatracknext" => "XF86AudioNext",
        "mediatrackprevious" => "XF86AudioPrev",
        _ => "",
    };
    if !fixed.is_empty() {
        return fixed.into();
    }
    if let Some(n) = key.strip_prefix("numpad").filter(|n| n.len() == 1) {
        return format!("KP_{n}");
    }
    let mut chars = key.chars();
    if chars.next() == Some('f') && chars.as_str().parse::<u8>().is_ok() {
        return key.to_uppercase();
    }
    key.to_string()
}

/// Feishin's `keyboardCodeToHotkeyKey` over the W3C code name (`KeyA`, `Digit1`, `ArrowUp`).
pub fn code_to_key(code: &str) -> Option<String> {
    let fixed = match code {
        "ArrowDown" => "arrowdown",
        "ArrowLeft" => "arrowleft",
        "ArrowRight" => "arrowright",
        "ArrowUp" => "arrowup",
        "Backquote" => "`",
        "Backslash" => "\\",
        "Backspace" => "backspace",
        "BracketLeft" => "[",
        "BracketRight" => "]",
        "Comma" => ",",
        "Delete" => "delete",
        "End" => "end",
        "Enter" => "enter",
        "Equal" => "equal",
        "Escape" => "escape",
        "Home" => "home",
        "Insert" => "insert",
        "Minus" => "minus",
        "PageDown" => "pagedown",
        "PageUp" => "pageup",
        "Period" => ".",
        "Quote" => "'",
        "Semicolon" => ";",
        "Slash" => "/",
        "Space" => "space",
        "Tab" => "tab",
        _ => "",
    };
    if !fixed.is_empty() {
        return Some(fixed.into());
    }
    if let Some(k) = code.strip_prefix("Key") {
        return Some(k.to_lowercase());
    }
    if let Some(d) = code.strip_prefix("Digit") {
        return Some(d.into());
    }
    if let Some(n) = code.strip_prefix('F')
        && n.parse::<u8>().is_ok_and(|n| (1..=24).contains(&n))
    {
        return Some(code.to_lowercase());
    }
    if let Some(suffix) = code.strip_prefix("Numpad") {
        return match suffix {
            "Add" => Some("numpadadd".into()),
            "Decimal" => Some("numpaddecimal".into()),
            "Divide" => Some("numpaddivide".into()),
            "Enter" => Some("numpadenter".into()),
            "Multiply" => Some("numpadmultiply".into()),
            "Subtract" => Some("numpadsubtract".into()),
            d if d.len() == 1 && d.chars().all(|c| c.is_ascii_digit()) => Some(format!("numpad{d}")),
            _ => None,
        };
    }
    None
}

/// The Feishin key name of a key press: from the physical key like Feishin
/// (so bindings do not depend on the layout), else from the logical key.
pub fn press_name(key: &Key, physical: &Physical) -> Option<String> {
    if let Physical::Code(code) = physical
        && let Some(name) = code_to_key(&format!("{code:?}"))
    {
        return Some(name);
    }
    key_name(key)
}

/// The combo Feishin's recorder would store for this press: modifiers plus the
/// key. `None` for a lone modifier press (the recorder waits for the key).
pub fn record(name: Option<&str>, mods: Modifiers) -> Option<String> {
    let key = name?;
    let b = Binding {
        key: key.to_string(),
        ctrl: mods.control(),
        shift: mods.shift(),
        alt: mods.alt(),
        meta: mods.logo(),
    };
    Some(b.serialize())
}

/// Bindings used by more than one action, normalized (`ctrl+k` equals `mod+k`).
pub fn duplicates(bindings: &BTreeMap<String, String>) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut dup = BTreeSet::new();
    for hotkey in bindings.values() {
        let Some(b) = parse(hotkey) else { continue };
        let norm = b.serialize();
        if !seen.insert(norm.clone()) {
            dup.insert(norm);
        }
    }
    dup
}

/// Whether a stored hotkey string is one of the duplicated bindings.
pub fn is_duplicate(dups: &BTreeSet<String>, hotkey: &str) -> bool {
    parse(hotkey).is_some_and(|b| dups.contains(&b.serialize()))
}

/// Normalizes an iced key to Feishin's names.
pub fn key_name(key: &Key) -> Option<String> {
    Some(match key {
        Key::Character(c) => c.to_lowercase(),
        Key::Named(n) => match n {
            Named::Space => "space".into(),
            Named::Enter => "enter".into(),
            Named::Escape => "escape".into(),
            Named::ArrowLeft => "arrowleft".into(),
            Named::ArrowRight => "arrowright".into(),
            Named::ArrowUp => "arrowup".into(),
            Named::ArrowDown => "arrowdown".into(),
            Named::Tab => "tab".into(),
            Named::Backspace => "backspace".into(),
            Named::Delete => "delete".into(),
            Named::Home => "home".into(),
            Named::End => "end".into(),
            Named::PageUp => "pageup".into(),
            Named::PageDown => "pagedown".into(),
            Named::Insert => "insert".into(),
            Named::MediaPlayPause => "mediaplaypause".into(),
            Named::MediaTrackNext => "mediatracknext".into(),
            Named::MediaTrackPrevious => "mediatrackprevious".into(),
            _ => return None,
        },
        _ => return None,
    })
}

/// The Feishin actions bound to this key press (a duplicated binding fires all of
/// them, like Feishin). Actions in `global` are handled system-wide instead.
pub fn actions_for<'a>(
    bindings: &'a BTreeMap<String, String>,
    global: &BTreeSet<String>,
    name: &str,
    mods: Modifiers,
) -> Vec<&'a str> {
    bindings
        .iter()
        .filter(|(action, _)| !global.contains(action.as_str()))
        .filter_map(|(action, hotkey)| {
            let b = parse(hotkey)?;
            (b.key == name
                && b.ctrl == mods.control()
                && b.shift == mods.shift()
                && b.alt == mods.alt()
                && b.meta == mods.logo())
            .then_some(action.as_str())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(a, k)| (a.to_string(), k.to_string())).collect()
    }

    #[test]
    fn parses_feishin_bindings() {
        assert_eq!(parse("space").unwrap().key, "space");
        let b = parse("mod+k").unwrap();
        assert!(b.ctrl && !b.shift && b.key == "k");
        assert!(parse("").is_none());
        assert!(parse("mod+").is_none());
        assert!(parse("meta+x").unwrap().meta);
        let bindings = map(&[("playPause", "space"), ("globalSearch", "mod+k")]);
        let none = BTreeSet::new();
        assert_eq!(actions_for(&bindings, &none, "space", Modifiers::empty()), ["playPause"]);
        assert_eq!(actions_for(&bindings, &none, "k", Modifiers::CTRL), ["globalSearch"]);
        assert!(actions_for(&bindings, &none, "k", Modifiers::empty()).is_empty());
        assert!(actions_for(&bindings, &none, "k", Modifiers::CTRL | Modifiers::SHIFT).is_empty());
    }

    #[test]
    fn serialize_round_trips() {
        for s in ["space", "mod+k", "mod+alt+shift+meta+arrowright", "shift+f5", "numpad3", "alt+equal"] {
            let b = parse(s).unwrap();
            assert_eq!(b.serialize(), s);
            assert_eq!(parse(&b.serialize()), Some(b));
        }
        // Aliases normalize to Feishin's spelling.
        assert_eq!(parse("ctrl+shift+K").unwrap().serialize(), "mod+shift+k");
    }

    #[test]
    fn recording_produces_the_binding() {
        assert_eq!(record(Some("k"), Modifiers::CTRL).as_deref(), Some("mod+k"));
        assert_eq!(
            record(Some("arrowright"), Modifiers::CTRL | Modifiers::ALT | Modifiers::SHIFT).as_deref(),
            Some("mod+alt+shift+arrowright")
        );
        assert_eq!(record(Some("space"), Modifiers::empty()).as_deref(), Some("space"));
        assert_eq!(record(Some("x"), Modifiers::LOGO).as_deref(), Some("meta+x"));
        // A lone modifier keeps waiting for the key.
        assert_eq!(record(None, Modifiers::CTRL), None);
        assert_eq!(
            press_name(
                &Key::Character("A".into()),
                &Physical::Unidentified(iced::keyboard::key::NativeCode::Unidentified)
            )
            .as_deref(),
            Some("a")
        );
        assert_eq!(
            press_name(&Key::Character("z".into()), &Physical::Code(iced::keyboard::key::Code::KeyY))
                .as_deref(),
            Some("y")
        );
        assert_eq!(
            press_name(&Key::Named(Named::Space), &Physical::Code(iced::keyboard::key::Code::Space))
                .as_deref(),
            Some("space")
        );
        assert_eq!(code_to_key("Digit4").as_deref(), Some("4"));
        assert_eq!(code_to_key("F13").as_deref(), Some("f13"));
        assert_eq!(code_to_key("ShiftLeft"), None);
    }

    #[test]
    fn detects_duplicates() {
        let b = map(&[("play", "mod+k"), ("pause", "ctrl+k"), ("stop", "space"), ("next", "")]);
        let d = duplicates(&b);
        assert_eq!(d.len(), 1);
        assert!(is_duplicate(&d, "mod+k") && is_duplicate(&d, "ctrl+k"));
        assert!(!is_duplicate(&d, "space") && !is_duplicate(&d, ""));
    }

    #[test]
    fn global_actions_skip_the_window_hotkey() {
        let b = map(&[("play", "f5"), ("pause", "f6")]);
        let global: BTreeSet<String> = ["play".to_string()].into();
        assert!(actions_for(&b, &global, "f5", Modifiers::empty()).is_empty());
        assert_eq!(actions_for(&b, &global, "f6", Modifiers::empty()), ["pause"]);
    }

    #[test]
    fn portal_trigger_notation() {
        assert_eq!(parse("mod+alt+arrowright").unwrap().portal_trigger(), "CTRL+ALT+Right");
        assert_eq!(parse("shift+f5").unwrap().portal_trigger(), "SHIFT+F5");
        assert_eq!(parse("space").unwrap().portal_trigger(), "space");
        assert_eq!(parse("meta+enter").unwrap().portal_trigger(), "LOGO+Return");
    }
}
