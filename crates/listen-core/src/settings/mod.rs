//! User settings. Defaults mirror Feishin's; `import_feishin` maps a Feishin
//! settings export (v34) onto them. Streaming quality is a plain setting.

use crate::theme::Rgb;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

mod advanced;
mod appearance;
mod artist;
mod collections;
mod dj;
mod fullscreen;
mod history;
mod hotkeys;
mod importer;
mod integrations;
mod lists;
mod lyrics;
mod milkdrop;
mod playback;
mod player_bar;
mod query_builder;
mod queue;
mod ripping;
mod sharing;
mod shell;
mod sidebar;
mod visualizer;
mod window;

pub use advanced::*;
pub use appearance::*;
pub use artist::*;
pub use collections::*;
pub use dj::*;
pub use fullscreen::*;
pub use history::*;
pub use hotkeys::*;
pub use importer::*;
pub use integrations::*;
pub use lists::*;
pub use lyrics::*;
pub use milkdrop::*;
pub use playback::*;
pub use player_bar::*;
pub use query_builder::*;
pub use queue::*;
pub use ripping::*;
pub use sharing::*;
pub use shell::*;
pub use sidebar::*;
pub use visualizer::*;
pub use window::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub appearance: Appearance,
    pub sidebar: Sidebar,
    pub home: Home,
    pub player_bar: PlayerBar,
    /// The full-screen player's look and auto-open timeout.
    pub fullscreen: FullscreenSettings,
    pub playback: Playback,
    pub auto_dj: AutoDj,
    pub ai_dj: AiDj,
    /// Audio analysis of songs (tempo, key, sections) for the DJs.
    pub analysis: Analysis,
    /// DJ-style transitions between songs the DJs pick.
    pub mixing: Mixing,
    pub ripping: Ripping,
    /// Local listening history (recently played, statistics, mixes).
    pub history: HistorySettings,
    pub lyrics: LyricsSettings,
    pub visualizer: Visualizer,
    /// The Milkdrop visualizer and the visualizer type.
    pub milkdrop: MilkdropSettings,
    /// Feishin binding action (e.g. `playPause`) → hotkey (e.g. `space`).
    pub hotkeys: BTreeMap<String, String>,
    /// Actions (Feishin `isGlobal`) registered system-wide through the desktop portal.
    pub hotkeys_global: std::collections::BTreeSet<String>,
    /// Grab the hardware media keys through the portal (MPRIS works either way).
    pub global_media_keys: bool,
    /// Legacy master switch of the earlier portal implementation; superseded by `hotkeys_global`.
    pub global_shortcuts: bool,
    /// Legacy: action → preferred portal trigger of the earlier implementation; no longer read.
    pub global_hotkeys: BTreeMap<String, String>,
    pub window: WindowSettings,
    /// Item lists: play-button behaviour and each list's table / grid look.
    pub lists: Lists,
    /// The artist page: sections, release-type groups, radio size, external links.
    pub artist: ArtistSettings,
    /// Share links: the default expiry.
    pub sharing: SharingSettings,
    /// The settings page's last open tab.
    pub settings_tab: SettingsTab,
    /// Custom fields of the smart-playlist builder.
    pub query_builder: QueryBuilder,
    /// The play queue panel / page and the player filters.
    pub queue: QueueSettings,
    /// Updates, log level.
    pub advanced: AdvancedSettings,
    /// Discord details, the remote control server, scrobble mode, notifications.
    pub integrations: Integrations,
    /// The first-start wizard was finished (or skipped).
    pub onboarded: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            appearance: Appearance::default(),
            sidebar: Sidebar::default(),
            home: Home::default(),
            player_bar: PlayerBar::default(),
            fullscreen: FullscreenSettings::default(),
            playback: Playback::default(),
            auto_dj: AutoDj::default(),
            ai_dj: AiDj::default(),
            analysis: Analysis::default(),
            mixing: Mixing::default(),
            ripping: Ripping::default(),
            history: HistorySettings::default(),
            lyrics: LyricsSettings::default(),
            visualizer: Visualizer::default(),
            milkdrop: MilkdropSettings::default(),
            hotkeys: DEFAULT_HOTKEYS.iter().map(|(a, k)| (a.to_string(), k.to_string())).collect(),
            hotkeys_global: std::collections::BTreeSet::new(),
            global_media_keys: true,
            global_shortcuts: false,
            global_hotkeys: GLOBAL_ACTIONS.iter().map(|(a, _, k)| (a.to_string(), k.to_string())).collect(),
            window: WindowSettings::default(),
            lists: Lists::default(),
            artist: ArtistSettings::default(),
            sharing: SharingSettings::default(),
            settings_tab: SettingsTab::default(),
            query_builder: QueryBuilder::default(),
            queue: QueueSettings::default(),
            advanced: AdvancedSettings::default(),
            integrations: Integrations::default(),
            onboarded: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "general": {
            "theme": "defaultDark", "followSystemTheme": true, "themeDark": "tokyoNight",
            "themeLight": "defaultLight", "accent": "rgb(53, 116, 252)", "useThemeAccentColor": false,
            "zoomFactor": 100, "homeFeature": true,
            "homeItems": [{"disabled": false, "id": "genres"}, {"disabled": true, "id": "random"}],
            "playerItems": [{"disabled": false, "id": "codec"}, {"disabled": false, "id": "codec"},
                            {"disabled": true, "id": "bpm"}],
            "sidebarItems": [{"disabled": true, "id": "Search", "label": "search", "route": "/search"},
                             {"disabled": false, "id": "Home", "label": "home", "route": "/"}],
            "skipButtons": {"enabled": true, "skipBackwardSeconds": 5, "skipForwardSeconds": 10}
        },
        "playback": {"mpvProperties": {"gaplessAudio": "weak", "replayGainMode": "track"},
                     "mpvExtraParameters": ["--cover-art-auto=all"]},
        "autoDJ": {"enabled": true, "itemCount": 5, "timing": 1, "songStrategy": "similar"},
        "hotkeys": {"bindings": {"playPause": {"hotkey": "space"}, "next": {"hotkey": ""}}},
        "remote": {"password": "must-not-leak ship-scan:allow"},
        "visualizer": {"audiomotionanalyzer": {"mode": 10, "gradient": "prism", "smoothing": 0.6}}
    }"#;

    #[test]
    fn imports_feishin_preset() {
        let s = import_feishin(&Settings::default(), SAMPLE).unwrap();
        assert_eq!(s.appearance.theme_dark, "tokyoNight");
        assert_eq!(s.appearance.accent, Some(Rgb(53, 116, 252)));
        assert_eq!(s.home.sections, ["genres"]);
        assert_eq!(s.player_bar.info_items, ["codec"]);
        assert_eq!(s.sidebar.items.len(), 2);
        assert!(!s.sidebar.items[0].visible);
        assert_eq!(s.playback.replay_gain, ReplayGain::Track);
        assert_eq!(s.playback.mpv_extra_args, ["--cover-art-auto=all"]);
        assert!(s.auto_dj.enabled);
        assert_eq!(s.hotkeys.len(), 1);
        assert_eq!(s.visualizer.mode, 10);
        let toml = toml::to_string(&s).unwrap();
        assert!(!toml.contains("must-not-leak"));
    }

    #[test]
    fn builds_mpv_equalizer_chain() {
        let mut p = Playback::default();
        assert_eq!(mpv_audio_filters(&p), "");
        p.equalizer_enabled = true;
        assert_eq!(mpv_audio_filters(&p), "");
        p.equalizer_preamp = -3.0;
        p.equalizer[0].gain = 4.0;
        p.equalizer[1].gain = 20.0; // clamped
        assert_eq!(
            mpv_audio_filters(&p),
            "volume=-3dB,lavfi=[equalizer=f=31.5:width_type=o:w=1.9:g=4],lavfi=[equalizer=f=63:width_type=o:w=1.3:g=12]"
        );
    }

    #[test]
    fn deck_chain_appends_mix_filters_to_the_user_eq() {
        let mut p = Playback::default();
        let deck = deck_audio_filters(&p);
        assert!(deck.starts_with("@mix:lavfi=[bass@lo="), "{deck}");
        assert!(deck.ends_with(",@ts:rubberband=engine=finer"), "{deck}");
        assert!(deck_audio_filters_with(&p, DECK_TEMPO_FALLBACK).ends_with(",@ts:scaletempo2"));
        p.equalizer_enabled = true;
        p.equalizer_preamp = -3.0;
        assert_eq!(deck_audio_filters(&p), format!("volume=-3dB,{deck}"));
    }

    #[test]
    fn settings_roundtrip_through_toml() {
        let s = Settings::default();
        let back: Settings = toml::from_str(&toml::to_string(&s).unwrap()).unwrap();
        assert_eq!(s, back);
        // Missing fields fall back to defaults.
        let partial: Settings = toml::from_str("[appearance]\ntheme = \"nord\"\n").unwrap();
        assert_eq!(partial.appearance.theme, "nord");
        assert_eq!(partial.playback.gapless, "weak");
        let dj: Settings = toml::from_str("[ai_dj]\nenabled = true\npersona = \"calm\"\n").unwrap();
        assert!(dj.ai_dj.enabled);
        assert_eq!(dj.ai_dj.persona, "calm");
        assert_eq!(dj.ai_dj.set_max, 5);
    }

    /// A v0.23 default config.toml round-trips unchanged; newer settings only append lines.
    #[test]
    fn v0_23_config_round_trips_unchanged() {
        let golden = include_str!("golden_v0_23.toml");
        let loaded: Settings = toml::from_str(golden).unwrap();
        let written = toml::to_string_pretty(&loaded).unwrap();
        let lines: std::collections::HashSet<&str> = written.lines().collect();
        for line in golden.lines() {
            assert!(lines.contains(line), "lost or changed on round trip: {line}");
        }
    }
}
