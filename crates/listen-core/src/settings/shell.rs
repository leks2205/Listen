//! Settings page shell: the remembered tab and the home carousel style.

use super::*;

/// A tab of the settings page (Feishin: General / Playback / Hotkeys / Window / Advanced).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SettingsTab {
    #[default]
    General,
    Playback,
    Hotkeys,
    Window,
    Advanced,
    About,
}

impl SettingsTab {
    pub const ALL: [SettingsTab; 6] =
        [Self::General, Self::Playback, Self::Hotkeys, Self::Window, Self::Advanced, Self::About];

    pub fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Playback => "Playback",
            Self::Hotkeys => "Hotkeys",
            Self::Window => "Window",
            Self::Advanced => "Advanced",
            Self::About => "About",
        }
    }
}

/// Feishin's `homeFeatureStyle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HomeFeatureStyle {
    #[default]
    Single,
    Multiple,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_and_style_serialize_lowercase_and_old_configs_load() {
        let mut s = Settings { settings_tab: SettingsTab::Hotkeys, ..Settings::default() };
        s.home.feature_style = HomeFeatureStyle::Multiple;
        let toml = toml::to_string(&s).unwrap();
        assert!(toml.contains("settings_tab = \"hotkeys\""));
        let back: Settings = toml::from_str(&toml).unwrap();
        assert_eq!(back.settings_tab, SettingsTab::Hotkeys);
        let old: Settings = toml::from_str("global_media_keys = true").unwrap();
        assert_eq!(old.settings_tab, SettingsTab::General);
    }
}
