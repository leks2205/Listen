//! The "Get info" data of OpenSubsonic `getSong` / `getAlbum` / `getArtist`
//! and the parameters of `createShare` (both pure, unit tested).

use listen_core::model::GainInfo;
use listen_source::ItemDetails;
use serde_json::Value;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn text(v: &Value, key: &str) -> Option<String> {
    v[key].as_str().map(str::trim).filter(|s| !s.is_empty()).map(String::from)
}

fn num(v: &Value, key: &str) -> Option<u64> {
    v[key].as_u64()
}

fn names(v: &Value, key: &str) -> Vec<String> {
    v[key]
        .as_array()
        .map(|a| {
            a.iter().filter_map(|n| n.as_str().or_else(|| n["name"].as_str())).map(String::from).collect()
        })
        .unwrap_or_default()
}

fn explicit(v: &Value) -> Option<bool> {
    match v["explicitStatus"].as_str()? {
        "explicit" => Some(true),
        "clean" => Some(false),
        _ => None,
    }
}

/// Details of a `song` object (OpenSubsonic `Child`).
pub fn song(v: &Value) -> ItemDetails {
    let gain = &v["replayGain"];
    let f = |k: &str| gain[k].as_f64().map(|x| x as f32);
    let mut d = ItemDetails {
        title: text(v, "title").unwrap_or_default(),
        path: text(v, "path"),
        size: num(v, "size"),
        suffix: text(v, "suffix"),
        bit_rate: num(v, "bitRate").map(|x| x as u32),
        sample_rate: num(v, "samplingRate").map(|x| x as u32),
        bit_depth: num(v, "bitDepth").map(|x| x as u32),
        channels: num(v, "channelCount").map(|x| x as u32),
        duration: num(v, "duration").map(Duration::from_secs),
        bpm: num(v, "bpm").filter(|b| *b > 0).map(|x| x as u32),
        created: text(v, "created"),
        last_played: text(v, "played"),
        play_count: num(v, "playCount").map(|x| x as u32),
        comment: text(v, "comment"),
        musicbrainz_id: text(v, "musicBrainzId"),
        gain: GainInfo {
            track_gain: f("trackGain"),
            track_peak: f("trackPeak"),
            album_gain: f("albumGain"),
            album_peak: f("albumPeak"),
        },
        explicit: explicit(v),
        ..Default::default()
    };
    if let Some(a) = v["contributors"].as_array() {
        for c in a {
            let role = c["role"].as_str().unwrap_or("");
            let who = c["artist"]["name"].as_str().unwrap_or("");
            if !role.is_empty() && !who.is_empty() {
                let role = match c["subRole"].as_str().filter(|s| !s.is_empty()) {
                    Some(sub) => format!("{role} ({sub})"),
                    None => role.to_string(),
                };
                d.participants.push((role, who.to_string()));
            }
        }
    }
    let moods = names(v, "moods");
    if !moods.is_empty() {
        d.tags.push(("Moods".into(), moods.join(", ")));
    }
    let isrc = names(v, "isrc");
    if !isrc.is_empty() {
        d.tags.push(("ISRC".into(), isrc.join(", ")));
    }
    if let Some(t) = text(v, "mediaType") {
        d.tags.push(("Media type".into(), t));
    }
    if let Some(t) = text(v, "contentType") {
        d.tags.push(("Content type".into(), t));
    }
    d
}

/// Details of an `album` object.
pub fn album(v: &Value) -> ItemDetails {
    ItemDetails {
        title: text(v, "name").unwrap_or_default(),
        duration: num(v, "duration").map(Duration::from_secs),
        created: text(v, "created"),
        last_played: text(v, "played"),
        play_count: num(v, "playCount").map(|x| x as u32),
        musicbrainz_id: text(v, "musicBrainzId"),
        compilation: v["isCompilation"].as_bool(),
        explicit: explicit(v),
        record_labels: names(v, "recordLabels"),
        release_type: {
            let t = names(v, "releaseTypes");
            (!t.is_empty()).then(|| t.join(", "))
        },
        version: text(v, "version"),
        comment: text(v, "comment"),
        ..Default::default()
    }
}

/// Details of an `artist` object.
pub fn artist(v: &Value) -> ItemDetails {
    ItemDetails {
        title: text(v, "name").unwrap_or_default(),
        play_count: num(v, "playCount").map(|x| x as u32),
        last_played: text(v, "played"),
        musicbrainz_id: text(v, "musicBrainzId"),
        ..Default::default()
    }
}

/// Query parameters of `createShare`: one `id` per item, the description and
/// the expiry as milliseconds since the epoch.
pub fn share_params(
    ids: &[String],
    description: Option<&str>,
    expires: Option<SystemTime>,
) -> Vec<(&'static str, String)> {
    let mut p: Vec<(&'static str, String)> = ids.iter().map(|id| ("id", id.clone())).collect();
    if let Some(d) = description.map(str::trim).filter(|d| !d.is_empty()) {
        p.push(("description", d.to_string()));
    }
    if let Some(ms) = expires.and_then(|e| e.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_millis()) {
        p.push(("expires", ms.to_string()));
    }
    p
}

/// A `share` object of `createShare` / `getShares`.
pub fn share(v: &Value, requested_expiry: Option<SystemTime>) -> Option<listen_source::Share> {
    let id = text(v, "id")?;
    let url = text(v, "url")?;
    let expires = text(v, "expires")
        .and_then(|s| parse_iso(&s))
        .filter(|t| t.duration_since(UNIX_EPOCH).is_ok_and(|d| d.as_secs() > 86_400))
        .or(requested_expiry);
    Some(listen_source::Share { id, url, description: text(v, "description"), expires })
}

/// "2026-10-16T12:30:00Z" (fractions and `+hh:mm` offsets accepted) as a time.
fn parse_iso(s: &str) -> Option<SystemTime> {
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-').map(|p| p.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let clock = time.trim_end_matches('Z');
    let (clock, offset) = match clock.rfind(['+', '-']) {
        Some(i) if i > 0 => {
            let (c, o) = clock.split_at(i);
            let sign = if o.starts_with('-') { -1 } else { 1 };
            let mut parts = o[1..].split(':').map(|p| p.parse::<i64>().unwrap_or(0));
            let secs = parts.next().unwrap_or(0) * 3600 + parts.next().unwrap_or(0) * 60;
            (c, sign * secs)
        }
        _ => (clock, 0),
    };
    let mut t = clock.split(':');
    let h: i64 = t.next()?.parse().ok()?;
    let mi: i64 = t.next()?.parse().ok()?;
    let sec: f64 = t.next().unwrap_or("0").parse().ok()?;
    // Days from civil (Howard Hinnant).
    let (y2, m2) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * m2 + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let secs = days * 86_400 + h * 3600 + mi * 60 - offset;
    u64::try_from(secs)
        .ok()
        .map(|s| UNIX_EPOCH + Duration::from_secs(s) + Duration::from_secs_f64(sec.fract()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn song_json() -> Value {
        json!({
            "id": "s1", "title": "Song One", "path": "Artist/Album/01 Song One.flac",
            "size": 31_457_280u64, "suffix": "flac", "contentType": "audio/flac",
            "bitRate": 912, "samplingRate": 96000, "bitDepth": 24, "channelCount": 2,
            "duration": 245, "bpm": 128, "created": "2024-03-01T10:00:00Z",
            "played": "2025-01-02T03:04:05Z", "playCount": 7, "comment": "live take",
            "musicBrainzId": "mbid-song",
            "replayGain": {"trackGain": -6.5, "trackPeak": 0.98, "albumGain": -7.25, "albumPeak": 1.0},
            "explicitStatus": "explicit",
            "contributors": [{"role": "producer", "artist": {"id": "p", "name": "Pat"}},
                             {"role": "performer", "subRole": "guitar", "artist": {"id": "g", "name": "Gil"}}],
            "moods": ["calm"], "isrc": ["USX123"], "mediaType": "song"
        })
    }

    #[test]
    fn maps_a_getsong_response() {
        let d = song(&song_json());
        assert_eq!(d.title, "Song One");
        assert_eq!(d.path.as_deref(), Some("Artist/Album/01 Song One.flac"));
        assert_eq!(
            (d.size, d.bit_rate, d.sample_rate, d.bit_depth, d.channels),
            (Some(31_457_280), Some(912), Some(96000), Some(24), Some(2))
        );
        assert_eq!(d.duration, Some(Duration::from_secs(245)));
        assert_eq!(d.bpm, Some(128));
        assert_eq!((d.play_count, d.last_played.as_deref()), (Some(7), Some("2025-01-02T03:04:05Z")));
        assert_eq!(d.musicbrainz_id.as_deref(), Some("mbid-song"));
        assert_eq!(d.gain.track_gain, Some(-6.5));
        assert_eq!(d.gain.album_peak, Some(1.0));
        assert_eq!(d.explicit, Some(true));
        assert_eq!(
            d.participants,
            [("producer".into(), "Pat".into()), ("performer (guitar)".into(), "Gil".into())]
        );
        assert!(d.tags.contains(&("ISRC".into(), "USX123".into())));
        assert!(d.tags.contains(&("Moods".into(), "calm".into())));
    }

    #[test]
    fn missing_fields_stay_empty() {
        let d = song(&json!({"id": "x", "title": "T", "bpm": 0, "comment": "  "}));
        assert_eq!(d.bpm, None);
        assert_eq!(d.comment, None);
        assert_eq!(d.explicit, None);
        assert!(d.participants.is_empty() && d.tags.is_empty());
    }

    #[test]
    fn maps_album_and_artist() {
        let d = album(&json!({"name": "Alb", "isCompilation": true, "version": "Deluxe",
            "releaseTypes": ["album", "live"], "recordLabels": [{"name": "Lab"}],
            "musicBrainzId": "mb-al", "playCount": 3, "explicitStatus": "clean"}));
        assert_eq!(d.compilation, Some(true));
        assert_eq!(d.release_type.as_deref(), Some("album, live"));
        assert_eq!(d.record_labels, ["Lab"]);
        assert_eq!(d.explicit, Some(false));
        let a = artist(&json!({"name": "Art", "musicBrainzId": "mb-ar"}));
        assert_eq!((a.title.as_str(), a.musicbrainz_id.as_deref()), ("Art", Some("mb-ar")));
    }

    #[test]
    fn share_params_repeat_id_and_send_milliseconds() {
        let at = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let p = share_params(&["a".into(), "b".into()], Some("  for you "), Some(at));
        assert_eq!(
            p,
            [
                ("id", "a".to_string()),
                ("id", "b".to_string()),
                ("description", "for you".to_string()),
                ("expires", "1800000000000".to_string())
            ]
        );
        assert_eq!(share_params(&["a".into()], Some(" "), None), [("id", "a".to_string())]);
    }

    #[test]
    fn parses_a_share_and_iso_dates() {
        let at = UNIX_EPOCH + Duration::from_secs(5);
        let s = share(
            &json!({"id": "i", "url": "https://x/share/i", "description": "d",
            "expires": "2030-01-01T00:00:00Z"}),
            Some(at),
        )
        .unwrap();
        assert_eq!(s.url, "https://x/share/i");
        assert_eq!(s.expires, Some(UNIX_EPOCH + Duration::from_secs(1_893_456_000)));
        // Navidrome's "never" is a zero date: keep what was asked for.
        let s = share(&json!({"id": "i", "url": "u", "expires": "0001-01-01T00:00:00Z"}), Some(at)).unwrap();
        assert_eq!(s.expires, Some(at));
        assert!(share(&json!({"id": "i"}), None).is_none());
        assert_eq!(parse_iso("2026-10-16T12:30:00+02:00"), parse_iso("2026-10-16T10:30:00Z"));
    }
}
