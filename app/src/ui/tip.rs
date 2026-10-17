//! Hover tooltips for buttons: the function plus the action's current hotkey
//! ("Next track · Ctrl+→"), rebuilt on every view so rebindings show at once.

use crate::app::{Listen, Message};
use crate::i18n::{tr, trf};
use crate::style::{self, Tokens};
use iced::widget::{container, text, tooltip};
use iced::{Background, Element, border};
use listen_core::queue::Repeat;
use std::time::Duration;

pub use iced::widget::tooltip::Position;

/// How long the pointer rests on a button before the tooltip shows.
pub const DELAY: Duration = Duration::from_millis(450);

/// A key name of Feishin's notation, for display (`arrowright` -> `→`, `space` -> `Space`).
fn key_text(key: &str) -> String {
    let fixed = match key {
        "arrowleft" => "←",
        "arrowright" => "→",
        "arrowup" => "↑",
        "arrowdown" => "↓",
        "space" => "Space",
        "enter" => "Enter",
        "escape" => "Esc",
        "tab" => "Tab",
        "backspace" => "Backspace",
        "delete" => "Del",
        "home" => "Home",
        "end" => "End",
        "insert" => "Ins",
        "pageup" => "PgUp",
        "pagedown" => "PgDn",
        "equal" => "=",
        "minus" => "-",
        "numpadadd" => "Num +",
        "numpaddecimal" => "Num .",
        "numpaddivide" => "Num /",
        "numpadenter" => "Num Enter",
        "numpadmultiply" => "Num *",
        "numpadsubtract" => "Num -",
        "mediaplaypause" => "Play/Pause",
        "mediatracknext" => "Next",
        "mediatrackprevious" => "Previous",
        _ => "",
    };
    if !fixed.is_empty() {
        return fixed.into();
    }
    if let Some(n) = key.strip_prefix("numpad").filter(|n| n.len() == 1) {
        return format!("Num {n}");
    }
    key.to_uppercase()
}

const META: &str = if cfg!(target_os = "macos") {
    "Cmd"
} else if cfg!(target_os = "windows") {
    "Win"
} else {
    "Super"
};

/// A binding in Feishin's notation as shown to the user: `mod+arrowright` ->
/// `Ctrl+→`, `space` -> `Space`. Modifiers come in the order Ctrl, Alt, Shift, Super.
pub fn binding_text(binding: &str) -> Option<String> {
    let b = crate::hotkeys::parse(binding)?;
    let mut parts: Vec<String> = Vec::new();
    for (on, name) in [(b.ctrl, "Ctrl"), (b.alt, "Alt"), (b.shift, "Shift"), (b.meta, META)] {
        if on {
            parts.push(name.into());
        }
    }
    parts.push(key_text(&b.key));
    Some(parts.join("+"))
}

/// The display text of the action's current binding (window or global); `None` when unbound.
pub fn action_binding(state: &Listen, action: &str) -> Option<String> {
    state.config.settings.hotkeys.get(action).filter(|h| !h.is_empty()).and_then(|h| binding_text(h))
}

/// The tooltip text: the translated function, then ` · ` and the binding when there is one.
pub fn tip_text(state: &Listen, label: &str, action: Option<&str>) -> String {
    tip_text_raw(state, crate::i18n::tr(label), action)
}

/// [`tip_text`] for a label that is already translated.
pub fn tip_text_raw(state: &Listen, label: &str, action: Option<&str>) -> String {
    match action.and_then(|a| action_binding(state, a)) {
        Some(key) => format!("{label} · {key}"),
        None => label.to_string(),
    }
}

fn bubble<'a>(t: Tokens, content: String) -> Element<'a, Message> {
    let content = super::compose(content.into());
    let font = super::font_for(&content, super::font(iced::font::Weight::Medium));
    container(text(content.into_owned()).font(font).size(style::metrics::font_sm()).color(t.foreground))
        .padding([4.0, 8.0])
        .style(move |_| container::Style {
            background: Some(Background::Color(t.background_alt)),
            border: border::rounded(style::metrics::radius_sm()).color(t.border).width(1),
            ..Default::default()
        })
        .into()
}

/// Wraps `content` in a tooltip showing `label` (translated) and the binding of `action`.
pub fn tip<'a>(
    state: &Listen,
    content: impl Into<Element<'a, Message>>,
    label: &str,
    action: Option<&str>,
    position: Position,
) -> Element<'a, Message> {
    tip_raw(state, content, crate::i18n::tr(label), action, position)
}

/// [`tip`] for a label that is already translated (built with `trf`).
pub fn tip_raw<'a>(
    state: &Listen,
    content: impl Into<Element<'a, Message>>,
    label: &str,
    action: Option<&str>,
    position: Position,
) -> Element<'a, Message> {
    let text = tip_text_raw(state, label, action);
    tooltip(content, bubble(state.tokens, text), position).delay(DELAY).gap(4).into()
}

/// "Play mode: Shuffle → next: Auto-DJ": names the mode and where a click goes.
pub fn play_mode_tip(state: &Listen) -> String {
    let mode = state.play_mode();
    let next = mode.next(state.ai_dj_available());
    trf("Play mode: {} → next: {}", &[&tr(mode.label()), &tr(next.label())])
}

/// "Repeat: Off / All / One".
pub fn repeat_tip(repeat: Repeat) -> String {
    let mode = match repeat {
        Repeat::Off => "Off",
        Repeat::All => "All",
        Repeat::One => "One",
    };
    trf("Repeat: {}", &[&tr(mode)])
}

/// "Rate 3 stars" (the singular for one).
pub fn rate_text(n: u8) -> String {
    if n == 1 { tr("Rate 1 star").to_string() } else { trf("Rate {} stars", &[&n]) }
}

/// The usual meaning of an icon-only button, for the small toolbars whose
/// helpers take just the glyph. `None` for icons that need their own words.
pub fn icon_label(glyph: crate::icons::Icon) -> Option<&'static str> {
    use crate::icons::Icon;
    Some(match glyph {
        Icon::LuX | Icon::RiCloseLine => "Clear",
        Icon::LuArrowUp => "Sort order: ascending",
        Icon::LuArrowDown => "Sort order: descending",
        Icon::LuRefreshCw | Icon::RiRepeat2Line => "Refresh",
        Icon::LuFilter => "Filters",
        Icon::LuPencil => "Edit",
        Icon::LuTrash2 => "Delete",
        Icon::LuPlus => "Add",
        Icon::LuChevronLeft => "Previous",
        Icon::LuChevronRight => "Next",
        Icon::LuChevronUp => "Move up",
        Icon::LuChevronDown => "Move down",
        Icon::LuEllipsis | Icon::LuEllipsisVertical => "More options",
        Icon::LuSlidersHorizontal => "Options",
        Icon::LuRadio => "Start radio",
        Icon::LuHeart | Icon::RiHeartLine => "Add to favorites",
        Icon::RiHeartFill => "Remove from favorites",
        Icon::LuPlay => "Play",
        Icon::LuStepForward | Icon::LuSkipForward => "Play next",
        Icon::LuUpload => "Upload",
        _ => return None,
    })
}

/// A tooltip without a hotkey, for views that have only the theme tokens.
pub fn tip_plain<'a>(
    t: Tokens,
    content: impl Into<Element<'a, Message>>,
    label: &str,
    position: Position,
) -> Element<'a, Message> {
    tooltip(content, bubble(t, crate::i18n::tr(label).to_string()), position).delay(DELAY).gap(4).into()
}

/// [`tip`] named after the glyph of the button (see [`icon_label`]); unchanged when it has no name.
pub fn tip_icon<'a>(
    state: &Listen,
    content: impl Into<Element<'a, Message>>,
    glyph: crate::icons::Icon,
    position: Position,
) -> Element<'a, Message> {
    match icon_label(glyph) {
        Some(name) => tip(state, content, name, None, position),
        None => content.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_bindings() {
        assert_eq!(binding_text("space").as_deref(), Some("Space"));
        assert_eq!(binding_text("mod+arrowright").as_deref(), Some("Ctrl+→"));
        assert_eq!(binding_text("shift+alt+mod+k").as_deref(), Some("Ctrl+Alt+Shift+K"));
        assert_eq!(binding_text("meta+f5").as_deref(), Some(&*format!("{META}+F5")));
        assert_eq!(binding_text("numpad3").as_deref(), Some("Num 3"));
        assert_eq!(binding_text("mod+equal").as_deref(), Some("Ctrl+="));
        assert_eq!(binding_text(""), None);
    }
}
