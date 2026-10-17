//! A CD's table of contents and the IDs derived from it: the MusicBrainz
//! disc ID (SHA-1 over the TOC, MusicBrainz's base64 alphabet) and the TOC
//! string for MusicBrainz's fuzzy lookup.

use base64::Engine;
use sha1::{Digest, Sha1};

/// Frames (1/75 s) before LBA 0: MusicBrainz offsets are LBA + 150.
pub const PREGAP: u32 = 150;
/// Gap between the last audio session and a data session (enhanced CDs).
const SESSION_GAP: u32 = 11_400;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TocTrack {
    pub number: u8,
    /// Start as a logical block address (0 = first audio frame of the disc).
    pub lba: u32,
    pub audio: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toc {
    pub tracks: Vec<TocTrack>,
    /// Lead-out LBA (end of the disc).
    pub leadout_lba: u32,
}

impl Toc {
    /// From MusicBrainz-style numbers (offsets and lead-out include the 150
    /// frame pregap), all tracks audio.
    pub fn from_offsets(first: u8, leadout: u32, offsets: &[u32]) -> Self {
        Self {
            tracks: offsets
                .iter()
                .enumerate()
                .map(|(i, &o)| TocTrack { number: first + i as u8, lba: o - PREGAP, audio: true })
                .collect(),
            leadout_lba: leadout - PREGAP,
        }
    }

    pub fn audio_tracks(&self) -> impl Iterator<Item = &TocTrack> {
        self.tracks.iter().filter(|t| t.audio)
    }

    pub fn audio_count(&self) -> usize {
        self.audio_tracks().count()
    }

    /// (first, last, lead-out, offsets) as MusicBrainz counts them: audio
    /// tracks only; on an enhanced CD (data track last) the lead-out is the
    /// data session's start minus the 11400-frame gap.
    pub fn musicbrainz_parts(&self) -> (u8, u8, u32, Vec<u32>) {
        let audio: Vec<&TocTrack> = self.audio_tracks().collect();
        let first = audio.first().map_or(1, |t| t.number);
        let last = audio.last().map_or(0, |t| t.number);
        let data_after = self.tracks.iter().find(|t| !t.audio && t.number > last);
        let leadout = match data_after {
            Some(d) => d.lba + PREGAP - SESSION_GAP,
            None => self.leadout_lba + PREGAP,
        };
        (first, last, leadout, audio.iter().map(|t| t.lba + PREGAP).collect())
    }

    /// The MusicBrainz disc ID, e.g. `7rEapOzTbtsuZwvusWs2r_d8duc-`.
    pub fn musicbrainz_id(&self) -> String {
        let (first, last, leadout, offsets) = self.musicbrainz_parts();
        let mut s = format!("{first:02X}{last:02X}{leadout:08X}");
        for i in 0..99 {
            s.push_str(&format!("{:08X}", offsets.get(i).copied().unwrap_or(0)));
        }
        let digest = Sha1::digest(s.as_bytes());
        base64::engine::general_purpose::STANDARD
            .encode(digest)
            .replace('+', ".")
            .replace('/', "_")
            .replace('=', "-")
    }

    /// `first last leadout offset…` for `/ws/2/discid/…?toc=`.
    pub fn toc_string(&self) -> String {
        let (first, last, leadout, offsets) = self.musicbrainz_parts();
        let mut s = format!("{first} {last} {leadout}");
        for o in offsets {
            s.push_str(&format!(" {o}"));
        }
        s
    }

    /// Where a disc can be submitted to MusicBrainz (unknown discs).
    pub fn submit_url(&self) -> String {
        let (first, last, leadout, offsets) = self.musicbrainz_parts();
        let toc: Vec<String> =
            [first as u32, last as u32, leadout].into_iter().chain(offsets).map(|n| n.to_string()).collect();
        format!(
            "https://musicbrainz.org/cdtoc/attach?id={}&tracks={}&toc={}",
            self.musicbrainz_id(),
            last.saturating_sub(first) + 1,
            toc.join("+")
        )
    }

    /// Length of the audio part in milliseconds.
    pub fn duration_ms(&self) -> u64 {
        let (_, _, leadout, offsets) = self.musicbrainz_parts();
        let start = offsets.first().copied().unwrap_or(PREGAP);
        u64::from(leadout.saturating_sub(start)) * 1000 / 75
    }

    /// Length of each audio track in milliseconds, in order.
    pub fn track_lengths_ms(&self) -> Vec<u64> {
        let (_, _, leadout, offsets) = self.musicbrainz_parts();
        offsets
            .iter()
            .enumerate()
            .map(|(i, &o)| {
                let end = offsets.get(i + 1).copied().unwrap_or(leadout);
                u64::from(end.saturating_sub(o)) * 1000 / 75
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// IVE — REVIVE+ (2026), as listed on MusicBrainz.
    pub fn revive() -> Toc {
        Toc::from_offsets(
            1,
            154_800,
            &[150, 14625, 27975, 40575, 53175, 66750, 79425, 92325, 105150, 117975, 130200, 139500],
        )
    }

    #[test]
    fn musicbrainz_disc_id() {
        assert_eq!(revive().musicbrainz_id(), "7rEapOzTbtsuZwvusWs2r_d8duc-");
        assert_eq!(
            revive().toc_string(),
            "1 12 154800 150 14625 27975 40575 53175 66750 79425 92325 105150 117975 130200 139500"
        );
    }

    #[test]
    fn enhanced_cd_ignores_the_data_track() {
        // Same audio session, plus a data track in a second session.
        let mut toc = revive();
        toc.tracks.push(TocTrack { number: 13, lba: 154_800 - PREGAP + SESSION_GAP, audio: false });
        toc.leadout_lba = 200_000;
        assert_eq!(toc.musicbrainz_id(), "7rEapOzTbtsuZwvusWs2r_d8duc-");
        assert_eq!(toc.audio_count(), 12);
    }

    #[test]
    fn lengths() {
        let toc = revive();
        assert_eq!(toc.duration_ms(), (154_800 - 150) * 1000 / 75);
        let l = toc.track_lengths_ms();
        assert_eq!(l.len(), 12);
        assert_eq!(l[0], (14625 - 150) * 1000 / 75);
        assert!(toc.submit_url().contains("tracks=12&toc=1+12+154800+150+"));
    }
}
