//! CD ripping.

use super::*;

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
        }
    }
}
