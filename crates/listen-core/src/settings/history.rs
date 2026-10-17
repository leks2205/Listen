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

/// Listening history. Stored only on this computer, never uploaded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HistorySettings {
    /// Off = nothing is written.
    pub enabled: bool,
    pub retention: HistoryRetention,
}

impl Default for HistorySettings {
    fn default() -> Self {
        Self { enabled: true, retention: HistoryRetention::Forever }
    }
}
