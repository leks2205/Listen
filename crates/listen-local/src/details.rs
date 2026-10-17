//! The "Get info" data of a local file: file facts plus the tags `lofty` reads.

use listen_core::model::GainInfo;
use listen_source::ItemDetails;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::tag::{Accessor, ItemKey};
use std::path::Path;

fn first(tag: &lofty::tag::Tag, key: ItemKey) -> Option<String> {
    tag.get_string(&key).map(str::trim).filter(|s| !s.is_empty()).map(String::from)
}

/// Details of the audio file at `path`.
pub fn song(path: &Path) -> Result<ItemDetails, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let tagged = lofty::read_from_path(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let props = tagged.properties();
    let mut d = ItemDetails {
        path: Some(path.display().to_string()),
        size: Some(meta.len()),
        suffix: path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()),
        bit_rate: props.audio_bitrate(),
        sample_rate: props.sample_rate(),
        bit_depth: props.bit_depth().map(u32::from),
        channels: props.channels().map(u32::from),
        duration: Some(props.duration()),
        created: meta.created().ok().map(crate::tags::iso_utc),
        updated: meta.modified().ok().map(crate::tags::iso_utc),
        ..Default::default()
    };
    if let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) {
        d.title = tag.title().map(|t| t.trim().to_string()).unwrap_or_default();
        d.comment = tag.comment().map(|c| c.trim().to_string()).filter(|c| !c.is_empty());
        d.bpm = first(tag, ItemKey::Bpm).and_then(|b| b.parse::<f32>().ok()).map(|b| b.round() as u32);
        let db = |k: ItemKey| first(tag, k).and_then(|v| v.trim_end_matches("dB").trim().parse::<f32>().ok());
        let peak = |k: ItemKey| first(tag, k).and_then(|v| v.parse::<f32>().ok());
        d.gain = GainInfo {
            track_gain: db(ItemKey::ReplayGainTrackGain),
            track_peak: peak(ItemKey::ReplayGainTrackPeak),
            album_gain: db(ItemKey::ReplayGainAlbumGain),
            album_peak: peak(ItemKey::ReplayGainAlbumPeak),
        };
        d.has_lyrics = Some(first(tag, ItemKey::Lyrics).is_some());
        d.compilation =
            first(tag, ItemKey::FlagCompilation).map(|v| v == "1" || v.eq_ignore_ascii_case("true"));
        d.record_labels = tag
            .get_strings(&ItemKey::Label)
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect();
        d.version = first(tag, ItemKey::TrackSubtitle);
        for (label, key) in [
            ("Recording", ItemKey::MusicBrainzRecordingId),
            ("Track", ItemKey::MusicBrainzTrackId),
            ("Album", ItemKey::MusicBrainzReleaseId),
            ("Release group", ItemKey::MusicBrainzReleaseGroupId),
            ("Artist", ItemKey::MusicBrainzArtistId),
            ("Album artist", ItemKey::MusicBrainzReleaseArtistId),
        ] {
            if let Some(id) = first(tag, key) {
                d.mbids.push((label.into(), id));
            }
        }
        d.musicbrainz_id = d.mbids.iter().find(|(l, _)| l == "Recording").map(|(_, id)| id.clone());
        if let Some(composer) = first(tag, ItemKey::Composer) {
            d.participants.push(("composer".into(), composer));
        }
    }
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tags::{CoverEdit, TagEdit, TagField, fixtures, write};

    #[test]
    fn reads_file_facts_and_tags_of_a_generated_mp3() {
        let (dir, path) = fixtures::temp_file("d.mp3", &fixtures::mp3());
        let edit = TagEdit {
            fields: [(TagField::Title, "Hi"), (TagField::Comment, "note"), (TagField::Lyrics, "la")]
                .into_iter()
                .map(|(f, v)| (f, v.to_string()))
                .collect(),
            cover: CoverEdit::Keep,
        };
        write(&path, &edit).unwrap();
        let d = song(&path).unwrap();
        assert_eq!(d.title, "Hi");
        assert_eq!(d.comment.as_deref(), Some("note"));
        assert_eq!(d.has_lyrics, Some(true));
        assert_eq!(d.suffix.as_deref(), Some("mp3"));
        assert_eq!(d.sample_rate, Some(44_100));
        assert_eq!(d.channels, Some(2));
        assert!(d.size.unwrap() > 4000);
        assert!(d.updated.as_deref().is_some_and(|u| u.ends_with('Z')));
        assert_eq!(d.path.as_deref(), Some(path.to_str().unwrap()));
        fixtures::cleanup(&dir);
        assert!(song(&path).is_err());
    }
}
