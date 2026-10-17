//! The "Get info" data of a Jellyfin `BaseItemDto` (pure, unit tested).

use listen_core::model::GainInfo;
use listen_source::ItemDetails;
use serde_json::Value;
use std::time::Duration;

fn text(v: &Value, key: &str) -> Option<String> {
    v[key].as_str().map(str::trim).filter(|s| !s.is_empty()).map(String::from)
}

/// Details of a song, album or artist item.
pub fn item(v: &Value) -> ItemDetails {
    let source = &v["MediaSources"][0];
    let audio = source["MediaStreams"]
        .as_array()
        .and_then(|s| s.iter().find(|m| m["Type"] == "Audio"))
        .unwrap_or(&Value::Null);
    let ids = &v["ProviderIds"];
    let provider = |k: &str| text(ids, k);
    let mut d = ItemDetails {
        title: text(v, "Name").unwrap_or_default(),
        path: text(v, "Path"),
        size: source["Size"].as_u64(),
        suffix: text(source, "Container").or_else(|| text(v, "Container")),
        bit_rate: audio["BitRate"].as_u64().or_else(|| source["Bitrate"].as_u64()).map(|b| (b / 1000) as u32),
        sample_rate: audio["SampleRate"].as_u64().map(|x| x as u32),
        bit_depth: audio["BitDepth"].as_u64().map(|x| x as u32),
        channels: audio["Channels"].as_u64().map(|x| x as u32),
        duration: v["RunTimeTicks"].as_u64().map(|t| Duration::from_micros(t / 10)),
        created: text(v, "DateCreated"),
        updated: text(v, "DateLastSaved"),
        last_played: text(&v["UserData"], "LastPlayedDate"),
        play_count: v["UserData"]["PlayCount"].as_u64().map(|x| x as u32),
        comment: text(v, "Overview"),
        explicit: v["IsExplicit"].as_bool(),
        gain: GainInfo {
            track_gain: v["NormalizationGain"].as_f64().map(|g| g as f32),
            ..Default::default()
        },
        record_labels: v["Studios"]
            .as_array()
            .map(|a| a.iter().filter_map(|s| s["Name"].as_str()).map(String::from).collect())
            .unwrap_or_default(),
        has_lyrics: v["HasLyrics"].as_bool(),
        ..Default::default()
    };
    // The id that matches the item kind comes first.
    let kind = v["Type"].as_str().unwrap_or("");
    let primary = match kind {
        "MusicAlbum" => "MusicBrainzAlbum",
        "MusicArtist" => "MusicBrainzArtist",
        _ => "MusicBrainzTrack",
    };
    d.musicbrainz_id = provider(primary);
    for (label, key) in [
        ("Track", "MusicBrainzTrack"),
        ("Recording", "MusicBrainzRecording"),
        ("Album", "MusicBrainzAlbum"),
        ("Album artist", "MusicBrainzAlbumArtist"),
        ("Artist", "MusicBrainzArtist"),
        ("Release group", "MusicBrainzReleaseGroup"),
    ] {
        if let Some(id) = provider(key).filter(|id| Some(id) != d.musicbrainz_id.as_ref()) {
            d.mbids.push((label.into(), id));
        }
    }
    if let Some(tags) = v["Tags"].as_array() {
        let tags: Vec<&str> = tags.iter().filter_map(Value::as_str).collect();
        if !tags.is_empty() {
            d.tags.push(("Tags".into(), tags.join(", ")));
        }
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_a_song_item() {
        let d = item(&json!({
            "Name": "Song", "Type": "Audio", "Path": "/music/a/b.flac",
            "RunTimeTicks": 2_450_000_000u64, "DateCreated": "2024-01-01T00:00:00Z",
            "UserData": {"PlayCount": 3, "LastPlayedDate": "2025-02-02T00:00:00Z"},
            "ProviderIds": {"MusicBrainzTrack": "t1", "MusicBrainzAlbum": "a1"},
            "NormalizationGain": -5.5, "Tags": ["x", "y"],
            "MediaSources": [{"Size": 5000, "Container": "flac", "Bitrate": 900_000,
                "MediaStreams": [{"Type": "Audio", "BitRate": 880_000, "SampleRate": 48000, "BitDepth": 24, "Channels": 2}]}]
        }));
        assert_eq!(d.path.as_deref(), Some("/music/a/b.flac"));
        assert_eq!((d.size, d.suffix.as_deref(), d.bit_rate), (Some(5000), Some("flac"), Some(880)));
        assert_eq!((d.sample_rate, d.bit_depth, d.channels), (Some(48000), Some(24), Some(2)));
        assert_eq!(d.duration, Some(Duration::from_secs(245)));
        assert_eq!((d.play_count, d.last_played.as_deref()), (Some(3), Some("2025-02-02T00:00:00Z")));
        assert_eq!(d.musicbrainz_id.as_deref(), Some("t1"));
        assert_eq!(d.mbids, [("Album".to_string(), "a1".to_string())]);
        assert_eq!(d.gain.track_gain, Some(-5.5));
        assert_eq!(d.tags, [("Tags".to_string(), "x, y".to_string())]);
    }

    #[test]
    fn an_album_uses_the_album_id_and_bare_items_are_fine() {
        let d = item(&json!({"Name": "Al", "Type": "MusicAlbum",
            "ProviderIds": {"MusicBrainzAlbum": "a1", "MusicBrainzTrack": "t1"}, "Studios": [{"Name": "Lab"}]}));
        assert_eq!(d.musicbrainz_id.as_deref(), Some("a1"));
        assert_eq!(d.record_labels, ["Lab"]);
        assert_eq!(d.size, None);
        assert_eq!(item(&json!({})).title, "");
    }
}
