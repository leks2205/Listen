//! CD ripping.

use super::*;

/// How a CD is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RipMode {
    /// One pass at the drive's full speed, no table scan (about 4 min for a CD).
    #[default]
    Fast,
    /// whipper: reads the table, then every track twice and compares (about 20 min).
    Secure,
}

impl RipMode {
    pub const ALL: [Self; 2] = [Self::Fast, Self::Secure];

    pub fn label(self) -> &'static str {
        match self {
            Self::Fast => "Fast",
            Self::Secure => "Secure",
        }
    }
}

impl std::fmt::Display for RipMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// CD ripping (Linux): whipper rips, Listen tags and files the album where Navidrome scans. FLAC level 8.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Ripping {
    /// Rip an audio CD as soon as it is inserted (MusicBee's auto-rip).
    pub auto_rip: bool,
    /// Where albums go (`Album Artist/Album/NN Title.flac`); empty = ask.
    pub destination: String,
    pub eject_when_done: bool,
    /// Desktop notification with a sound when an album is done.
    pub notify_sound: bool,
    pub replay_gain: bool,
    /// Ask the active Navidrome server to scan after an album was filed.
    pub scan_navidrome: bool,
    /// Several MusicBrainz releases match: take the best one after this long.
    pub picker_timeout_secs: u32,
    /// Drive read offset in samples; `None` = from AccurateRip's drive table (e.g. +48 for the LG GU70N).
    pub read_offset: Option<i32>,
    /// How discs are read (each rip can still switch before its first track).
    pub mode: RipMode,
}

impl Default for Ripping {
    fn default() -> Self {
        Self {
            auto_rip: true,
            destination: directories::UserDirs::new()
                .and_then(|d| d.audio_dir().map(|p| p.display().to_string()))
                .unwrap_or_default(),
            eject_when_done: true,
            notify_sound: true,
            replay_gain: true,
            scan_navidrome: true,
            picker_timeout_secs: 30,
            read_offset: None,
            mode: RipMode::Fast,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_defaults_to_fast_and_secure_stays_selectable() {
        let old: Ripping = toml::from_str("auto_rip = true\npicker_timeout_secs = 30\n").unwrap();
        assert_eq!(old.mode, RipMode::Fast);
        let secure: Ripping = toml::from_str("mode = \"secure\"\n").unwrap();
        assert_eq!(secure.mode, RipMode::Secure);
        assert!(toml::to_string(&Ripping::default()).unwrap().contains("mode = \"fast\""));
    }
}
