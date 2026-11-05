//! YouTube's per-video loudness (volume levelling's fallback for songs without an analysis
//! record). The web clients' player responses no longer carry it (unplayable without a PO
//! token), the IOS client's does: `playerConfig.audioConfig.trackAbsoluteLoudnessLkfs` and
//! per-format `loudnessDb`, both relative to YouTube's −14 LUFS target. Measured against the
//! analysis store, the value matches our own EBU R128 loudness within ±0.3 LU.

use listen_source::Urgency;
use serde_json::{Value, json};
use std::time::Duration;

const PLAYER_URL: &str = "https://www.youtube.com/youtubei/v1/player?prettyPrint=false";
const IOS_VERSION: &str = "20.10.4";
const IOS_UA: &str = "com.google.ios.youtube/20.10.4 (iPhone16,2; U; CPU iOS 18_3_2 like Mac OS X;)";
/// YouTube's loudness target when the response does not name one.
const DEFAULT_TARGET_LKFS: f64 = -14.0;

/// The song's loudness above YouTube's −14 LUFS reference (`loudnessDb`) from a player
/// response: the track's absolute loudness, else the Opus audio format's, else any audio
/// format's. `None` when the response has none (not playable, or a video without audio info).
pub fn parse_loudness_db(player: &Value) -> Option<f32> {
    let cfg = &player["playerConfig"]["audioConfig"];
    let target = cfg["loudnessTargetLkfs"].as_f64().unwrap_or(DEFAULT_TARGET_LKFS);
    if let Some(abs) = cfg["trackAbsoluteLoudnessLkfs"].as_f64() {
        return finite(abs - target);
    }
    let formats = player["streamingData"]["adaptiveFormats"].as_array()?;
    let audio = || formats.iter().filter(|f| f["mimeType"].as_str().is_some_and(|m| m.starts_with("audio/")));
    audio()
        .find(|f| f["itag"].as_u64() == Some(251))
        .or_else(|| audio().find(|f| f["loudnessDb"].is_number()))
        .and_then(|f| f["loudnessDb"].as_f64())
        .and_then(finite)
}

fn finite(db: f64) -> Option<f32> {
    let db = db as f32;
    (db.is_finite() && (-40.0..=20.0).contains(&db)).then_some(db)
}

/// Asks YouTube for `video_id`'s loudness (one IOS-client player call, no cookies; counts
/// against the YouTube request budget, `urgency` as for any other call).
pub async fn fetch_loudness_db(video_id: &str, urgency: Urgency) -> Result<Option<f32>, String> {
    if !crate::budget::BUDGET.acquire(urgency, Duration::from_secs(15)).await {
        return Err("YouTube request budget used up".into());
    }
    let body = json!({
        "videoId": video_id,
        "context": {"client": {
            "clientName": "IOS",
            "clientVersion": IOS_VERSION,
            "deviceMake": "Apple",
            "deviceModel": "iPhone16,2",
            "osName": "iPhone",
            "osVersion": "18.3.2.22D82",
            "hl": "en",
            "gl": "US",
        }},
        "contentCheckOk": true,
        "racyCheckOk": true,
    });
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(IOS_UA)
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .post(PLAYER_URL)
        .header("X-YouTube-Client-Name", "5")
        .header("X-YouTube-Client-Version", IOS_VERSION)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("player call: HTTP {}", response.status()));
    }
    let player: Value = response.json().await.map_err(|e| e.to_string())?;
    Ok(parse_loudness_db(&player))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_loudness_relative_to_the_target() {
        let v = json!({"playerConfig": {"audioConfig": {
            "perceptualLoudnessDb": -13.01, "trackAbsoluteLoudnessLkfs": -13.01, "loudnessTargetLkfs": -14
        }}});
        assert!((parse_loudness_db(&v).unwrap() - 0.99).abs() < 1e-4);
        let quiet = json!({"playerConfig": {"audioConfig": {"trackAbsoluteLoudnessLkfs": -21.5}}});
        assert!((parse_loudness_db(&quiet).unwrap() + 7.5).abs() < 1e-4, "default target −14");
    }

    #[test]
    fn per_format_loudness_prefers_opus() {
        let v = json!({"streamingData": {"adaptiveFormats": [
            {"itag": 137, "mimeType": "video/mp4", "loudnessDb": 5.0},
            {"itag": 140, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "loudnessDb": 0.99},
            {"itag": 251, "mimeType": "audio/webm; codecs=\"opus\"", "loudnessDb": 0.98},
        ]}});
        assert_eq!(parse_loudness_db(&v), Some(0.98));
        let aac_only = json!({"streamingData": {"adaptiveFormats": [
            {"itag": 140, "mimeType": "audio/mp4", "loudnessDb": -3.25},
        ]}});
        assert_eq!(parse_loudness_db(&aac_only), Some(-3.25));
    }

    #[test]
    fn nothing_when_unplayable_or_absurd() {
        assert_eq!(parse_loudness_db(&json!({"playabilityStatus": {"status": "LOGIN_REQUIRED"}})), None);
        let nan = json!({"playerConfig": {"audioConfig": {"trackAbsoluteLoudnessLkfs": -99.0}}});
        assert_eq!(parse_loudness_db(&nan), None);
    }

    /// Live check against YouTube (`cargo test -p listen-ytmusic -- --ignored loudness_live`).
    #[tokio::test]
    #[ignore]
    async fn loudness_live() {
        let db = fetch_loudness_db("dQw4w9WgXcQ", Urgency::Now).await.unwrap();
        assert!(db.is_some_and(|d| (-10.0..10.0).contains(&d)), "{db:?}");
    }
}
