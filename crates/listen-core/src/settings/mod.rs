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
    fn echo_commands_schedule_the_gates_at_the_plan_times() {
        let on = DeckEcho { delay_s: 0.5, taps: [0.5, 0.25, 0.125, 0.0625], send: 1.0, dry: 1.0 };
        let stop = DeckEcho { send: 0.0, dry: 0.0, ..on };
        let open = deck_echo_commands(&DeckEcho::IDLE, &on, 99.5);
        let names: Vec<_> = open.iter().map(|(t, o, _)| format!("{t} {o}")).collect();
        assert_eq!(
            names,
            [
                "adelay@e1 delays",
                "adelay@e2 delays",
                "adelay@e3 delays",
                "adelay@e4 delays",
                "amix@em weights",
                "volume@es volume",
                "afade@eo start_time"
            ]
        );
        assert_eq!(open[1].2, ["1000.0"; 8].join("|"));
        // The dry stays at weight 1: only the gates move it.
        assert_eq!(open[4].2, "1.0000 0.5000 0.2500 0.1250 0.0625");
        assert_eq!(open[6].2, "99.500000");
        // The stop touches other filters than the opening (the two tasks may run in any order).
        let close = deck_echo_commands(&on, &stop, 100.0);
        assert_eq!(
            close,
            [
                ("afade@ec", "start_time", "100.000000".to_string()),
                ("afade@ed", "silence", "0.0000".to_string()),
                ("afade@ed", "start_time", "100.000000".to_string()),
            ]
        );
        assert!(open.iter().all(|(t, ..)| close.iter().all(|(u, ..)| t != u)));
        assert!(deck_echo_commands(&on, &on, 1.0).is_empty());
        // Idle resets every gate (dry back, send shut) and the repeat levels.
        let idle = deck_echo_commands(&stop, &DeckEcho::IDLE, 0.0);
        assert_eq!(idle.len(), 4);
        assert!(idle[..3].iter().all(|(_, o, v)| *o == "start_time" && v == "1000000"));
        assert_eq!(idle[3].2, "1.0000 0.0000 0.0000 0.0000 0.0000");
    }

    #[test]
    fn echo_levels_are_gates_not_faders() {
        let on = DeckEcho { delay_s: 0.5, taps: [0.5, 0.25, 0.125, 0.0625], send: 1.0, dry: 1.0 };
        let ducked = DeckEcho { dry: 0.3, ..on };
        // The dry signal may drop to any level (one gate with that floor)…
        assert_eq!(
            deck_echo_commands(&on, &ducked, 5.0),
            [
                ("afade@ed", "silence", "0.3000".to_string()),
                ("afade@ed", "start_time", "5.000000".to_string()),
            ]
        );
        // …and comes back to full by disarming the gate.
        assert_eq!(
            deck_echo_commands(&ducked, &on, 6.0),
            [("afade@ed", "start_time", "1000000".to_string())]
        );
        // The repeats are retuned while the send is closed.
        let closed = DeckEcho { send: 0.0, ..on };
        let retuned = DeckEcho { delay_s: 0.25, ..closed };
        assert_eq!(deck_echo_commands(&closed, &retuned, 7.0).len(), 4);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "the dry signal only comes back to full")]
    fn echo_dry_never_comes_back_to_a_partial_level() {
        let on = DeckEcho { delay_s: 0.5, taps: [0.5, 0.25, 0.125, 0.0625], send: 1.0, dry: 1.0 };
        deck_echo_commands(&DeckEcho { dry: 0.0, ..on }, &DeckEcho { dry: 0.5, ..on }, 1.0);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "the send is a gate")]
    fn echo_send_is_not_faded_while_open() {
        let on = DeckEcho { delay_s: 0.5, taps: [0.5, 0.25, 0.125, 0.0625], send: 1.0, dry: 1.0 };
        deck_echo_commands(&on, &DeckEcho { send: 0.6, ..on }, 1.0);
    }

    #[test]
    fn deck_chain_appends_mix_filters_to_the_user_eq() {
        let mut p = Playback::default();
        let deck = deck_audio_filters(&p);
        assert!(deck.starts_with("@mix:lavfi=[bass@lo="), "{deck}");
        assert!(deck.ends_with(",@ts:rubberband=engine=finer"), "{deck}");
        // The echo sits between the bass shelf and the passes, idle (send 0, dry only, no delay).
        let (bass, echo, hp) = (
            deck.find("bass@lo=").unwrap(),
            deck.find("volume@es=0,").unwrap(),
            deck.find(",highpass@hp=").unwrap(),
        );
        assert!(bass < echo && echo < hp, "{deck}");
        assert!(deck.contains("amix@em=inputs=5:normalize=0:duration=first:weights=1 0 0 0 0"), "{deck}");
        assert_eq!(deck.matches("=delays=0:all=1").count(), 4, "{deck}");
        // Gates idle: dry open, send closed, both ramps 5 ms.
        assert!(deck.contains("[dry]afade@ed=t=out:st=1000000:d=0.005[dg]"), "{deck}");
        assert!(
            deck.contains("volume@es=0,afade@eo=t=in:st=1000000:d=0.005,afade@ec=t=out:st=1000000"),
            "{deck}"
        );
        assert!(!deck.contains('\n') && !deck.contains("  "), "{deck}");
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

    /// A new secret-named field would be written to the world-readable config.toml: secrets belong in the
    /// app's secret store. The known ones are public Last.fm API keys and the legacy translation key, which
    /// the app moves into the secret store at start.
    #[test]
    fn settings_hold_no_secret_fields_beyond_the_known_ones() {
        fn walk(v: &serde_json::Value, path: &str, out: &mut Vec<String>) {
            if let serde_json::Value::Object(map) = v {
                for (k, child) in map {
                    let here = if path.is_empty() { k.clone() } else { format!("{path}.{k}") };
                    if is_secret_key(k) {
                        out.push(here.clone());
                    }
                    walk(child, &here, out);
                }
            }
        }
        let mut found = Vec::new();
        walk(&serde_json::to_value(Settings::default()).unwrap(), "", &mut found);
        found.sort();
        assert_eq!(
            found,
            ["integrations.discord.lastfm_key", "integrations.lastfm.api_key", "lyrics.translation_api_key"],
            "keep secrets in the secret store, not in Settings"
        );
    }

    /// A v0.23 default config.toml round-trips unchanged; newer settings only append lines.
    #[test]
    fn v0_23_config_round_trips_unchanged() {
        let golden = include_str!("golden_v0_23.toml");
        let loaded: Settings = toml::from_str(golden).unwrap();
        let written = toml::to_string_pretty(&loaded).unwrap();
        let lines: std::collections::HashSet<&str> = written.lines().collect();
        for line in golden.lines() {
            assert!(
                lines.contains(line),
                "lost or changed on round trip: {line} (a settings field is never removed or renamed: keep it and \
                 document it as unused, so older config files still load)"
            );
        }
    }
}
