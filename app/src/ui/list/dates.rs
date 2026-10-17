//! Table cell dates in Feishin's day.js formats: partial ISO dates and relative times ("3 days ago").

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

/// `iso` relative to `now` ("3 days ago", day.js' `fromNow`); `None` when it isn't a date.
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
    let text = if s < 45 {
        "a few seconds".to_string()
    } else if s < 90 {
        "a minute".to_string()
    } else if m < 45 {
        format!("{m} minutes")
    } else if m < 90 {
        "an hour".to_string()
    } else if h < 22 {
        format!("{h} hours")
    } else if h < 36 {
        "a day".to_string()
    } else if d < 26 {
        format!("{d} days")
    } else if d < 46 {
        "a month".to_string()
    } else if d < 320 {
        format!("{} months", months.max(2))
    } else if d < 548 {
        "a year".to_string()
    } else {
        format!("{} years", (d as f64 / 365.25).round() as i64)
    };
    Some(if ago { format!("{text} ago") } else { format!("in {text}") })
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
}
