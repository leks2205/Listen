//! Table cell dates in Feishin's day.js formats: partial ISO dates and relative times ("3 days ago"),
//! the relative ones translated as whole sentences ([`RELATIVE`]).

use chrono::{DateTime, Datelike, NaiveDate, Utc};

/// `2001`, `2001-03` or `2001-03-12` (trailing time ignored) as "2001", "March 2001" or "Mar 12, 2001";
/// anything else unchanged.
pub fn partial_date(iso: &str) -> String {
    let iso = iso.trim();
    crate::ui::album_detail::format::partial_date(iso.split(['T', ' ']).next().unwrap_or(iso))
}

/// The year of a partial ISO date.
pub fn year_of(iso: &str) -> Option<i32> {
    iso.trim().get(..4)?.parse().ok()
}

/// day.js' `fromNow` buckets as (past, future) sentence patterns. Each is one catalog key, because
/// languages inflect the amount differently per sentence ("vor 5 Tagen", not "vor" + "5 Tage").
const RELATIVE: [(&str, &str); 11] = [
    ("a few seconds ago", "in a few seconds"),
    ("a minute ago", "in a minute"),
    ("{} minutes ago", "in {} minutes"),
    ("an hour ago", "in an hour"),
    ("{} hours ago", "in {} hours"),
    ("a day ago", "in a day"),
    ("{} days ago", "in {} days"),
    ("a month ago", "in a month"),
    ("{} months ago", "in {} months"),
    ("a year ago", "in a year"),
    ("{} years ago", "in {} years"),
];

/// `iso` relative to `now` ("3 days ago", day.js' `fromNow`), translated; `None` when it isn't a date.
pub fn relative(iso: &str, now: DateTime<Utc>) -> Option<String> {
    let iso = iso.trim();
    let then = DateTime::parse_from_rfc3339(iso)
        .map(|d| d.with_timezone(&Utc))
        .or_else(|_| {
            // "2024-05-01" or "2024-05-01 12:00:00": taken as UTC.
            let day = NaiveDate::parse_from_str(iso.get(..10).unwrap_or(iso), "%Y-%m-%d")?;
            Ok::<_, chrono::ParseError>(day.and_hms_opt(0, 0, 0).unwrap_or_default().and_utc())
        })
        .ok()?;
    let secs = (now - then).num_seconds();
    let ago = secs >= 0;
    let s = secs.abs();
    let (m, h, d) = (s / 60, s / 3600, s / 86_400);
    let months = ((now.year() - then.year()) * 12 + now.month() as i32 - then.month() as i32).abs();
    // (bucket in RELATIVE, the amount for its hole)
    let (bucket, n): (usize, i64) = if s < 45 {
        (0, 0)
    } else if s < 90 {
        (1, 0)
    } else if m < 45 {
        (2, m)
    } else if m < 90 {
        (3, 0)
    } else if h < 22 {
        (4, h)
    } else if h < 36 {
        (5, 0)
    } else if d < 26 {
        (6, d)
    } else if d < 46 {
        (7, 0)
    } else if d < 320 {
        (8, i64::from(months.max(2)))
    } else if d < 548 {
        (9, 0)
    } else {
        (10, (d as f64 / 365.25).round() as i64)
    };
    let (past, future) = RELATIVE[bucket];
    Some(crate::i18n::trf(if ago { past } else { future }, &[&n]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn partial_dates_follow_the_precision_they_have() {
        assert_eq!(partial_date("2001"), "2001");
        assert_eq!(partial_date("2001-03"), "March 2001");
        assert_eq!(partial_date("2001-03-12"), "Mar 12, 2001");
        assert_eq!(partial_date("2001-03-12T10:00:00Z"), "Mar 12, 2001");
        assert_eq!(partial_date("sometime"), "sometime");
        assert_eq!(partial_date("2001-13-40"), "2001-13-40");
        assert_eq!(year_of("1999-05-01"), Some(1999));
        assert_eq!(year_of("x"), None);
    }

    #[test]
    fn relative_times_read_like_dayjs() {
        let now = at("2026-10-16T12:00:00Z");
        assert_eq!(relative("2026-10-16T11:59:50Z", now).unwrap(), "a few seconds ago");
        assert_eq!(relative("2026-10-16T11:55:00Z", now).unwrap(), "5 minutes ago");
        assert_eq!(relative("2026-10-16T09:00:00Z", now).unwrap(), "3 hours ago");
        assert_eq!(relative("2026-10-15T09:00:00Z", now).unwrap(), "a day ago");
        assert_eq!(relative("2026-10-11T12:00:00Z", now).unwrap(), "5 days ago");
        assert_eq!(relative("2026-07-16T12:00:00Z", now).unwrap(), "3 months ago");
        assert_eq!(relative("2024-10-16T12:00:00Z", now).unwrap(), "2 years ago");
        assert_eq!(relative("2026-10-16T13:00:00Z", now).unwrap(), "in an hour");
        assert_eq!(relative("2026-10-01", now).unwrap(), "15 days ago");
        assert!(relative("nope", now).is_none());
    }

    /// Every sentence `relative` can return is a German key of its own with the same holes, so the
    /// "Last played" column never shows English or a glued "vor 5 Tage".
    #[test]
    fn relative_times_are_whole_sentences() {
        for (past, future) in RELATIVE {
            for key in [past, future] {
                let de =
                    crate::i18n::translate_in("de", key).unwrap_or_else(|| panic!("no German key {key:?}"));
                assert_eq!(de.matches("{}").count(), key.matches("{}").count(), "{key:?} -> {de:?}");
            }
        }
        assert_eq!(crate::i18n::translate_in("de", "{} days ago").as_deref(), Some("vor {} Tagen"));
    }
}
