//! Fixed keys of the item lists, as in Feishin: `mod+a` toggles select-all; arrows, Home / End and
//! Page Up / Down move the selection (left / right only in grids). The configurable list hotkeys
//! are `HotkeysMsg::List`, handled in `app/list.rs`. Keys reach a list only without a focused field.

use super::selection::Move;
use iced::keyboard::{Key, Modifiers, key::Named};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    SelectAll,
    Move(Move),
}

/// What a key press does in a list. `grid`: the list shows cards.
pub fn key_action(key: &Key, modifiers: Modifiers, grid: bool) -> Option<KeyAction> {
    if let Key::Character(c) = key
        && c.eq_ignore_ascii_case("a")
        && modifiers.command()
        && !modifiers.shift()
        && !modifiers.alt()
    {
        return Some(KeyAction::SelectAll);
    }
    if modifiers.command() || modifiers.alt() {
        return None;
    }
    let Key::Named(named) = key else { return None };
    let mv = match named {
        Named::ArrowUp => Move::Up,
        Named::ArrowDown => Move::Down,
        Named::ArrowLeft if grid => Move::Left,
        Named::ArrowRight if grid => Move::Right,
        Named::Home => Move::Home,
        Named::End => Move::End,
        Named::PageUp => Move::PageUp,
        Named::PageDown => Move::PageDown,
        _ => return None,
    };
    Some(KeyAction::Move(mv))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_the_fixed_list_keys() {
        let none = Modifiers::empty();
        let down = Key::Named(Named::ArrowDown);
        assert_eq!(key_action(&down, none, false), Some(KeyAction::Move(Move::Down)));
        assert_eq!(key_action(&Key::Named(Named::Home), none, false), Some(KeyAction::Move(Move::Home)));
        // Left / right are for grids only.
        let left = Key::Named(Named::ArrowLeft);
        assert_eq!(key_action(&left, none, false), None);
        assert_eq!(key_action(&left, none, true), Some(KeyAction::Move(Move::Left)));
        // mod+a (Ctrl on Linux).
        let a = Key::Character("a".into());
        assert_eq!(key_action(&a, Modifiers::CTRL, false), Some(KeyAction::SelectAll));
        assert_eq!(key_action(&a, none, false), None);
        // Chords with Ctrl / Alt are other hotkeys' business.
        assert_eq!(key_action(&down, Modifiers::CTRL, false), None);
        assert_eq!(key_action(&down, Modifiers::ALT, false), None);
    }
}
