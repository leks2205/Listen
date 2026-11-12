//! The tray menu both platform trays show (`tray` on Linux, `tray_win` on Windows): Feishin's
//! entries and what each one does. Change the menu here, not in a platform module. Labels are English
//! keys: the platform modules show them through `i18n::tr` (text handed to the OS never passes the
//! iced text helpers that translate everything else).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCmd {
    PlayPause,
    Next,
    Previous,
    Stop,
    /// "Open main window".
    Show,
    /// A click on the icon.
    Toggle,
    Quit,
}

/// The label of the [`TrayCmd::PlayPause`] entry. A tray that relabels the entry when playback
/// changes finds it by its command, not by its place in [`menu`].
pub fn play_pause_label(playing: bool) -> &'static str {
    if playing { "Pause" } else { "Play" }
}

/// The menu in order: label and command, `None` = separator.
pub fn menu(playing: bool) -> Vec<Option<(&'static str, TrayCmd)>> {
    vec![
        Some((play_pause_label(playing), TrayCmd::PlayPause)),
        Some(("Next Track", TrayCmd::Next)),
        Some(("Previous Track", TrayCmd::Previous)),
        Some(("Stop", TrayCmd::Stop)),
        None,
        Some(("Open main window", TrayCmd::Show)),
        Some(("Quit", TrayCmd::Quit)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_has_feishins_entries() {
        let labels: Vec<Option<&str>> = menu(false).into_iter().map(|e| e.map(|(l, _)| l)).collect();
        assert_eq!(
            labels,
            [
                Some("Play"),
                Some("Next Track"),
                Some("Previous Track"),
                Some("Stop"),
                None,
                Some("Open main window"),
                Some("Quit")
            ]
        );
        assert_eq!(menu(true)[0], Some(("Pause", TrayCmd::PlayPause)));
    }

    #[test]
    fn the_play_pause_entry_carries_the_play_pause_label() {
        for playing in [false, true] {
            let entry = menu(playing).into_iter().flatten().find(|(_, cmd)| *cmd == TrayCmd::PlayPause);
            assert_eq!(entry.map(|(label, _)| label), Some(play_pause_label(playing)));
        }
    }

    /// Both platform trays render `menu` as it is: neither names an entry's label or command itself
    /// (only the icon click, `Toggle`, and the Play/Pause relabelling, which finds its entry by
    /// `PlayPause`), so an entry added here shows on Linux and Windows alike.
    #[test]
    fn platform_trays_take_every_entry_from_tray_menu() {
        let labels: Vec<&str> = menu(false).into_iter().chain(menu(true)).flatten().map(|(l, _)| l).collect();
        for (file, src) in
            [("tray.rs", include_str!("tray.rs")), ("tray_win.rs", include_str!("tray_win.rs"))]
        {
            assert!(src.contains("crate::tray_menu::menu"), "{file} does not render tray_menu::menu");
            for cmd in src.split("TrayCmd::").skip(1).map(|r| r.split(|c: char| !c.is_alphanumeric()).next())
            {
                assert!(matches!(cmd, Some("Toggle" | "PlayPause")), "{file} handles a menu command itself");
            }
            for label in labels.iter().filter(|l| !["Play", "Pause"].contains(l)) {
                assert!(!src.contains(&format!("\"{label}\"")), "{file} spells the entry {label:?}");
            }
        }
    }
}
