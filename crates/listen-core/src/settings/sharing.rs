//! Share links: the default expiry of a new share (Feishin `shareExpiration`,
//! store/settings.store.ts). Months and years are 30 and 365 days.

use super::*;
use std::time::{Duration, SystemTime};

/// The unit of an expiry amount (Feishin `ShareExpirationUnit`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExpiryUnit {
    Second,
    Minute,
    Hour,
    Day,
    Week,
    Month,
    #[default]
    Year,
}

impl ExpiryUnit {
    pub const ALL: [ExpiryUnit; 7] = [
        ExpiryUnit::Second,
        ExpiryUnit::Minute,
        ExpiryUnit::Hour,
        ExpiryUnit::Day,
        ExpiryUnit::Week,
        ExpiryUnit::Month,
        ExpiryUnit::Year,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ExpiryUnit::Second => "Seconds",
            ExpiryUnit::Minute => "Minutes",
            ExpiryUnit::Hour => "Hours",
            ExpiryUnit::Day => "Days",
            ExpiryUnit::Week => "Weeks",
            ExpiryUnit::Month => "Months",
            ExpiryUnit::Year => "Years",
        }
    }

    pub fn seconds(self) -> u64 {
        match self {
            ExpiryUnit::Second => 1,
            ExpiryUnit::Minute => 60,
            ExpiryUnit::Hour => 3600,
            ExpiryUnit::Day => 86_400,
            ExpiryUnit::Week => 7 * 86_400,
            ExpiryUnit::Month => 30 * 86_400,
            ExpiryUnit::Year => 365 * 86_400,
        }
    }
}

impl std::fmt::Display for ExpiryUnit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// How long a new share lasts by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShareExpiration {
    /// Send no expiry: the server decides.
    pub use_server_default: bool,
    pub amount: u32,
    pub unit: ExpiryUnit,
}

impl Default for ShareExpiration {
    fn default() -> Self {
        Self { use_server_default: false, amount: 1, unit: ExpiryUnit::Year }
    }
}

impl ShareExpiration {
    /// When a share made at `now` expires; `None`: the server's default.
    pub fn expires_at(&self, now: SystemTime) -> Option<SystemTime> {
        if self.use_server_default {
            return None;
        }
        Some(add(now, self.amount.max(1), self.unit))
    }
}

/// `now` plus `amount` of `unit`.
pub fn add(now: SystemTime, amount: u32, unit: ExpiryUnit) -> SystemTime {
    now + Duration::from_secs(u64::from(amount.max(1)) * unit.seconds())
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SharingSettings {
    pub expiration: ShareExpiration,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_one_year_like_feishin() {
        let now = SystemTime::UNIX_EPOCH;
        assert_eq!(ShareExpiration::default().expires_at(now), Some(now + Duration::from_secs(365 * 86_400)));
    }

    #[test]
    fn server_default_sends_no_expiry_and_zero_counts_as_one() {
        let now = SystemTime::UNIX_EPOCH;
        let server = ShareExpiration { use_server_default: true, ..Default::default() };
        assert_eq!(server.expires_at(now), None);
        let zero = ShareExpiration { use_server_default: false, amount: 0, unit: ExpiryUnit::Hour };
        assert_eq!(zero.expires_at(now), Some(now + Duration::from_secs(3600)));
    }

    #[test]
    fn old_settings_files_get_the_default() {
        let s: SharingSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s, SharingSettings::default());
        let s: SharingSettings =
            serde_json::from_str(r#"{"expiration":{"amount":3,"unit":"week"}}"#).unwrap();
        assert_eq!(s.expiration.unit, ExpiryUnit::Week);
        assert_eq!(s.expiration.amount, 3);
    }
}
