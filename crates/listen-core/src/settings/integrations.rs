//! Integrations: Discord rich presence details, the remote control server,
//! the scrobble minimum mode and song change notifications (Feishin's
//! `discord`, `remote` and `playback.scrobble` settings). The remote
//! server's password is not here: it lives in the keyring.

use super::*;

/// Which part of the presence the Discord status line shows (Feishin `DiscordDisplayType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiscordDisplay {
    /// The application name.
    #[default]
    Feishin,
    Song,
    Artist,
}

impl DiscordDisplay {
    pub const ALL: [Self; 3] = [Self::Feishin, Self::Song, Self::Artist];

    pub fn label(self) -> &'static str {
        match self {
            Self::Feishin => "Listen",
            Self::Song => "Song name",
            Self::Artist => "Artist name",
        }
    }
}

impl std::fmt::Display for DiscordDisplay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// Where the presence lines link to (Feishin `DiscordLinkType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscordLink {
    #[default]
    None,
    LastFm,
    MusicBrainz,
    MusicBrainzLastFm,
}

impl DiscordLink {
    pub const ALL: [Self; 4] = [Self::None, Self::LastFm, Self::MusicBrainz, Self::MusicBrainzLastFm];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::LastFm => "Last.fm",
            Self::MusicBrainz => "MusicBrainz",
            Self::MusicBrainzLastFm => "MusicBrainz (Last.fm fallback)",
        }
    }
}

impl std::fmt::Display for DiscordLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// When a play counts as a scrobble (Feishin `ScrobbleMinimumMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScrobbleMode {
    /// The first of the two thresholds.
    #[default]
    Both,
    Percentage,
    Seconds,
}

impl ScrobbleMode {
    pub const ALL: [Self; 3] = [Self::Both, Self::Percentage, Self::Seconds];

    pub fn label(self) -> &'static str {
        match self {
            Self::Both => "Percentage or seconds",
            Self::Percentage => "Percentage",
            Self::Seconds => "Seconds",
        }
    }

    /// Whether `played` of a song `total` long is enough (Feishin `checkScrobbleConditions`).
    pub fn reached(
        self,
        percent: u8,
        secs: u32,
        played: std::time::Duration,
        total: std::time::Duration,
    ) -> bool {
        let by_percent =
            !total.is_zero() && played.as_secs_f64() / total.as_secs_f64() * 100.0 >= f64::from(percent);
        let by_secs = played >= std::time::Duration::from_secs(u64::from(secs));
        match self {
            Self::Both => by_percent || by_secs,
            Self::Percentage => by_percent,
            Self::Seconds => by_secs,
        }
    }
}

impl std::fmt::Display for ScrobbleMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// Discord details beyond "enabled" and the application id (those are in [`WindowSettings`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiscordSettings {
    /// "Listening to" instead of "Playing".
    pub show_as_listening: bool,
    /// Keep the presence (paused icon, no timer) while paused.
    pub show_paused: bool,
    /// The small playing icon while playing.
    pub state_icon: bool,
    pub display: DiscordDisplay,
    pub link: DiscordLink,
    /// Send the server's cover to Discord (only for a server on a public address).
    pub server_image: bool,
    /// Last.fm API key for the album cover fallback; empty = off.
    pub lastfm_key: String,
}

impl Default for DiscordSettings {
    fn default() -> Self {
        Self {
            show_as_listening: false,
            show_paused: true,
            state_icon: true,
            display: DiscordDisplay::Feishin,
            link: DiscordLink::None,
            server_image: false,
            lastfm_key: String::new(),
        }
    }
}

/// The phone remote (Feishin `remote`): HTTP + WebSocket, off by default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RemoteSettings {
    pub enabled: bool,
    pub port: u16,
    /// Address the server listens on (`0.0.0.0` = every network, `127.0.0.1` = this computer).
    pub bind: String,
    /// Basic-auth user name; with an empty password too, no sign-in is asked.
    pub username: String,
}

impl Default for RemoteSettings {
    fn default() -> Self {
        Self { enabled: false, port: 4333, bind: "0.0.0.0".into(), username: String::new() }
    }
}

/// The MCP server: lets an AI assistant on this computer control Listen.
/// The access token lives in the keyring, never here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct McpSettings {
    pub enabled: bool,
    pub port: u16,
    /// Also accept connections from other computers (otherwise 127.0.0.1 only).
    pub allow_lan: bool,
}

impl Default for McpSettings {
    fn default() -> Self {
        Self { enabled: false, port: 4334, allow_lan: false }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Integrations {
    pub discord: DiscordSettings,
    pub remote: RemoteSettings,
    pub mcp: McpSettings,
    pub scrobble_mode: ScrobbleMode,
    /// Desktop notification when the song changes (Feishin `scrobble.notify`).
    pub notify_song_change: bool,
}

impl Default for Integrations {
    fn default() -> Self {
        Self {
            discord: DiscordSettings::default(),
            remote: RemoteSettings::default(),
            mcp: McpSettings::default(),
            scrobble_mode: ScrobbleMode::Both,
            notify_song_change: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn d(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    #[test]
    fn scrobble_threshold_matrix() {
        // 200 s song, 50 %, 120 s.
        let (p, s, total) = (50, 120, d(200));
        for (mode, played, want) in [
            (ScrobbleMode::Both, 99, false),
            (ScrobbleMode::Both, 100, true), // 50 %
            (ScrobbleMode::Both, 120, true),
            (ScrobbleMode::Percentage, 100, true),
            (ScrobbleMode::Percentage, 99, false),
            (ScrobbleMode::Percentage, 150, true),
            (ScrobbleMode::Seconds, 100, false),
            (ScrobbleMode::Seconds, 119, false),
            (ScrobbleMode::Seconds, 120, true),
        ] {
            assert_eq!(mode.reached(p, s, d(played), total), want, "{mode:?} at {played} s");
        }
        // Seconds wins over a short song in Both mode; an unknown length only counts seconds.
        assert!(ScrobbleMode::Both.reached(75, 240, d(60), d(80)));
        assert!(!ScrobbleMode::Percentage.reached(25, 0, d(500), Duration::ZERO));
        assert!(ScrobbleMode::Seconds.reached(75, 0, Duration::ZERO, Duration::ZERO));
        assert!(ScrobbleMode::Both.reached(75, 30, d(30), Duration::ZERO));
    }

    #[test]
    fn feishin_export_maps_discord_scrobble_and_remote() {
        let json = r#"{"discord":{"enabled":true,"clientId":"42","displayType":"song","linkType":"musicbrainz_last_fm",
            "showAsListening":true,"showPaused":false,"showStateIcon":false,"showServerImage":true},
            "playback":{"scrobble":{"minimumMode":"seconds","notify":true}},
            "remote":{"enabled":true,"port":4444,"username":"u","password":"never imported"}}"#;
        let s = crate::settings::import_feishin(&Settings::default(), json).unwrap();
        let ig = &s.integrations;
        assert!(ig.discord.show_as_listening && !ig.discord.show_paused && !ig.discord.state_icon);
        assert_eq!(
            (ig.discord.display, ig.discord.link),
            (DiscordDisplay::Song, DiscordLink::MusicBrainzLastFm)
        );
        assert!(ig.discord.server_image && ig.notify_song_change);
        assert_eq!(ig.scrobble_mode, ScrobbleMode::Seconds);
        assert_eq!((ig.remote.enabled, ig.remote.port, ig.remote.username.as_str()), (true, 4444, "u"));
        assert!(!toml::to_string(&s).unwrap().contains("never imported"));
    }

    #[test]
    fn old_configs_get_the_defaults() {
        let s: Integrations = toml::from_str("notify_song_change = true").unwrap();
        assert!(s.notify_song_change);
        assert_eq!(s.remote.port, 4333);
        assert!(!s.remote.enabled);
        assert!(s.discord.show_paused);
    }
}
