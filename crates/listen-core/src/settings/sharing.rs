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
    /// When a share made at `now` expires; `None`: the server's default. The amount rule is the one of
    /// [`add`]: the app refuses a typed 0, and a 0 from a hand-edited config counts as 1.
    pub fn expires_at(&self, now: SystemTime) -> Option<SystemTime> {
        if self.use_server_default {
            return None;
        }
        Some(add(now, self.amount, self.unit))
    }
}

/// The longest expiry [`add`] gives: a typed amount (up to 99999 years in the app) must not overflow
/// `SystemTime`, which on Windows ends in the year 30828 and panics on overflow.
pub const MAX_SHARE_EXPIRY: Duration = Duration::from_secs(1000 * 365 * 86_400);

/// The expiry of a share made at `now`: `amount` of `unit`, at least 1 (an expiry of 0 would end the share
/// at once) and at most [`MAX_SHARE_EXPIRY`]. Never panics, whatever the amount.
pub fn add(now: SystemTime, amount: u32, unit: ExpiryUnit) -> SystemTime {
    let secs = u64::from(amount.max(1)).saturating_mul(unit.seconds());
    let span = Duration::from_secs(secs).min(MAX_SHARE_EXPIRY);
    now.checked_add(span).unwrap_or(now)
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
    fn share_expiry_never_overflows_system_time() {
        let now = SystemTime::now();
        let far = add(now, u32::MAX, ExpiryUnit::Year);
        assert_eq!(far, now + MAX_SHARE_EXPIRY);
        assert_eq!(add(now, 99_999, ExpiryUnit::Year), far);
        // Below the cap nothing changes.
        assert_eq!(add(now, 999, ExpiryUnit::Year), now + Duration::from_secs(999 * 365 * 86_400));
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
