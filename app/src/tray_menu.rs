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

/// The menu in order: label and command, `None` = separator.
pub fn menu(playing: bool) -> Vec<Option<(&'static str, TrayCmd)>> {
    vec![
        Some((if playing { "Pause" } else { "Play" }, TrayCmd::PlayPause)),
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
}
