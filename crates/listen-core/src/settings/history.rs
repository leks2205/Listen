//! Listening history retention.

use super::*;

/// How long listening history is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HistoryRetention {
    OneYear,
    TwoYears,
    Forever,
}

impl HistoryRetention {
    pub const ALL: [Self; 3] = [Self::OneYear, Self::TwoYears, Self::Forever];

    pub fn label(self) -> &'static str {
        match self {
            Self::OneYear => "1 year",
            Self::TwoYears => "2 years",
            Self::Forever => "Unlimited",
        }
    }

    /// Entries older than this many days are dropped (`None` = keep all).
    pub fn max_age_days(self) -> Option<i64> {
        match self {
            Self::OneYear => Some(365),
            Self::TwoYears => Some(730),
            Self::Forever => None,
        }
    }
}

impl std::fmt::Display for HistoryRetention {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// Listening history. Stored on this computer; it leaves it only when `sync` is on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HistorySettings {
    /// Off = nothing is written.
    pub enabled: bool,
    pub retention: HistoryRetention,
    /// Merge the plays with the other devices through listen-server (`docs/history-sync.md`).
    pub sync: bool,
    /// listen-server's address. Its token lives in the keyring.
    pub sync_server: String,
    /// Show what the other devices play and take it over (Play here, or a new audio output) through
    /// the same listen-server (`docs/handoff.md`).
    pub handoff: bool,
    /// "Not interested" on a YouTube Music song also dislikes it there (taking it back removes the dislike).
    pub dislike_not_interested: bool,
}

impl Default for HistorySettings {
    fn default() -> Self {
        Self {
            enabled: true,
            retention: HistoryRetention::Forever,
            sync: false,
            sync_server: String::new(),
            handoff: true,
            dislike_not_interested: true,
        }
    }
}
