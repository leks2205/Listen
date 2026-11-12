//! The UI language: the setting, the restart toast, and a German render.
//!
//! One test only: the language is process-wide, and the other tests of a binary run in
//! parallel (they all expect English).

mod common;

use iced_test::simulator::Simulator;
use listen::Args;
use listen::app::{Listen, Message, Route, Setting};

fn has_text(state: &Listen, text: &str) -> bool {
    let _gpu = common::gpu();
    let mut ui = Simulator::with_size(common::iced_settings(), iced::Size::new(1600.0, 2400.0), state.view());
    ui.find(text).is_ok()
}

fn snapshot(name: &str, state: &Listen) {
    let theme = state.theme();
    let _gpu = common::gpu();
    let mut ui = Simulator::with_size(common::iced_settings(), iced::Size::new(1600.0, 1100.0), state.view());
    let snapshot = ui.snapshot(&theme).expect("snapshot renders");
    common::save_png(&snapshot, name);
}

fn demo() -> Listen {
    let (mut state, _) = Listen::boot(&Args { demo: true, ephemeral: true, ..Args::default() });
    let _ = state.update(Message::Navigate(Route::Settings));
    state
}

#[test]
fn language_setting_translates_the_ui_after_a_restart() {
    // Demo boots always render English.
    let mut state = demo();
    assert_eq!(listen::i18n::language(), "en");
    assert!(has_text(&state, "Language"));
    assert!(state.config.settings.appearance.language.is_empty());

    // Picking a language saves it and offers the restart; the open UI stays English.
    let _ = state.update(Message::Setting(Setting::Language("de".into())));
    assert_eq!(state.config.settings.appearance.language, "de");
    assert!(has_text(&state, "Restart required"));
    assert!(has_text(&state, "Language"));

    // After the restart (a boot reads the setting) the strings are German.
    listen::i18n::set_language(listen::i18n::resolve("de"));
    assert_eq!(listen::i18n::language(), "de");
    assert!(has_text(&state, "Sprache"), "the settings row is translated");
    assert!(has_text(&state, "Neustart erforderlich"), "the toast is translated (Feishin's string)");
    assert!(!has_text(&state, "Restart required"));
    snapshot("language-de-settings", &state);

    let (mut home, _) = Listen::boot(&Args { demo: true, ephemeral: true, ..Args::default() });
    let _ = home.update(Message::Navigate(Route::Home));
    listen::i18n::set_language("de"); // the boot above reset it (demo = English)
    snapshot("language-de-home", &home);

    // Back to English and to the system default.
    listen::i18n::set_language("en");
    assert!(has_text(&state, "Language"));
    assert_eq!(
        listen::i18n::resolve(""),
        listen::i18n::from_locale(
            &["LC_ALL", "LC_MESSAGES", "LANG"]
                .iter()
                .filter_map(|v| std::env::var(v).ok())
                .find(|v| !v.is_empty() && v != "C" && v != "POSIX")
                .unwrap_or_default(),
        )
    );
}
