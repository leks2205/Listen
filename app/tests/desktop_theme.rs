//! "Follow desktop theme": a desktop theme recolours the app live; turning
//! the option off goes back to the chosen theme.

use listen::Args;
use listen::app::{Listen, Message, Setting};
use listen_core::theme::desktop::{DesktopTheme, rice};

fn rgb(c: iced::Color) -> (u8, u8, u8) {
    ((c.r * 255.0).round() as u8, (c.g * 255.0).round() as u8, (c.b * 255.0).round() as u8)
}

#[test]
fn follows_desktop_theme_live() {
    let (mut s, _) = Listen::boot(&Args { ephemeral: true, ..Args::default() });
    s.config.settings.appearance.follow_desktop_theme = true;

    for key in ["catppuccin", "gruvbox"] {
        let r = rice(key).unwrap();
        let _ =
            s.update(Message::DesktopTheme(Some(DesktopTheme { name: r.name.into(), palette: r.palette })));
        assert_eq!(
            rgb(s.tokens.background),
            (r.palette.background.0, r.palette.background.1, r.palette.background.2)
        );
        assert_eq!(
            rgb(s.tokens.background_alt),
            (
                r.palette.background_alternate.0,
                r.palette.background_alternate.1,
                r.palette.background_alternate.2
            )
        );
    }

    // Off: back to the Feishin theme with the preset accent.
    let _ = s.update(Message::Setting(Setting::FollowDesktop(false)));
    let gruvbox = rice("gruvbox").unwrap().palette.background;
    assert_ne!(rgb(s.tokens.background), (gruvbox.0, gruvbox.1, gruvbox.2));
}

#[test]
fn rice_themes_are_manual_presets_too() {
    let names: Vec<_> = listen_core::theme::theme_names().map(|(id, _)| id).collect();
    for id in [
        "riceTokyoNight",
        "riceCatppuccinMocha",
        "riceGruvboxDark",
        "riceNord",
        "riceRosePine",
        "riceDracula",
        "riceKdeStock",
    ] {
        assert!(names.contains(&id), "{id}");
        assert!(listen_core::theme::palette(id).is_some());
    }
    assert!(names.contains(&"tokyoNight"), "Feishin's themes stay available");
}
