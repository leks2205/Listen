//! AccurateRip for the Fast rip mode (whipper does its own in Secure mode): the disc's three ids
//! from the TOC, the v1/v2 track checksums over the ripped samples, the database's binary answer
//! (`dBAR-….bin`) and the per-track verdict. The arithmetic follows whipper (`image/table.py`
//! `accuraterip_ids`/`getCDDBValues`, `common/accurip.py`, and the `accuraterip` C checksum).
//! Tracks are ripped with the drive's read offset applied, so their checksums compare directly.

use crate::toc::{PREGAP, Toc};
use crate::whipper::Ar;

pub const URL: &str = "http://www.accuraterip.com/accuraterip/";

/// 1/75 s.
const FRAMES_PER_SECOND: u32 = 75;
/// Stereo samples in a CD frame.
pub const SAMPLES_PER_FRAME: usize = 588;
/// The first track's first and the last track's last five frames are not checksummed.
const SKIP_SAMPLES: u32 = 5 * SAMPLES_PER_FRAME as u32;

/// The three ids AccurateRip files a disc under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiscIds {
    /// Audio tracks (data tracks are not counted).
    pub tracks: u32,
    pub id1: u32,
    pub id2: u32,
    pub cddb: u32,
}

impl DiscIds {
    pub fn from_toc(toc: &Toc) -> Self {
        let mut id1: u32 = 0;
        let mut id2: u32 = 0;
        let mut audio = 0u32;
        for t in toc.audio_tracks() {
            audio += 1;
            id1 = id1.wrapping_add(t.lba);
            id2 = id2.wrapping_add(t.lba.max(1).wrapping_mul(u32::from(t.number)));
        }
        // The lead-out (one past the end of the disc's last track, data tracks included).
        id1 = id1.wrapping_add(toc.leadout_lba);
        id2 = id2.wrapping_add(toc.leadout_lba.wrapping_mul(audio + 1));
        Self { tracks: audio, id1, id2, cddb: cddb_id(toc) }
    }

    /// `3/0/3/dBAR-012-000f9303-009076ea-96080e0c.bin`
    pub fn path(&self) -> String {
        let h = format!("{:08x}", self.id1);
        let c: Vec<char> = h.chars().collect();
        format!(
            "{}/{}/{}/dBAR-{:03}-{h}-{:08x}-{:08x}.bin",
            c[7], c[6], c[5], self.tracks, self.id2, self.cddb
        )
    }

    pub fn url(&self) -> String {
        format!("{URL}{}", self.path())
    }
}

/// The freedb/CDDB disc id (every track, data included; the real lead-out).
pub fn cddb_id(toc: &Toc) -> u32 {
    let digits = |mut n: u32| {
        let mut s = 0;
        while n > 0 {
            s += n % 10;
            n /= 10;
        }
        s
    };
    let sum: u32 = toc.tracks.iter().map(|t| digits((t.lba + PREGAP) / FRAMES_PER_SECOND)).sum();
    let start = toc.tracks.first().map_or(0, |t| t.lba) / FRAMES_PER_SECOND;
    let length = (toc.leadout_lba / FRAMES_PER_SECOND).saturating_sub(start);
    ((sum % 255) << 24) | ((length & 0xFFFF) << 8) | (toc.tracks.len() as u32 & 0xFF)
}

/// AccurateRip v1 and v2 checksums of one track: `pcm` is its 16-bit stereo little-endian audio,
/// `number` its position among the disc's `total` audio tracks.
pub fn checksums(pcm: &[u8], number: u32, total: u32) -> (u32, u32) {
    let mut c = Checksum::new(number, total, (pcm.len() / 4) as u32);
    c.update(pcm);
    c.finish()
}

/// [`checksums`] over a track fed in pieces (whole samples, i.e. multiples of 4 bytes).
#[derive(Debug, Clone)]
pub struct Checksum {
    from: u32,
    to: u32,
    mul: u32,
    lo: u32,
    hi: u32,
}

impl Checksum {
    /// Track `number` of `total`, `samples` stereo samples long.
    pub fn new(number: u32, total: u32, samples: u32) -> Self {
        let from = if number == 1 { SKIP_SAMPLES } else { 0 };
        let to = if number == total { samples.saturating_sub(SKIP_SAMPLES) } else { samples };
        Self { from, to, mul: 1, lo: 0, hi: 0 }
    }

    pub fn update(&mut self, pcm: &[u8]) {
        for s in pcm.as_chunks::<4>().0 {
            if self.mul >= self.from && self.mul <= self.to {
                let product = u64::from(u32::from_le_bytes(*s)) * u64::from(self.mul);
                self.lo = self.lo.wrapping_add(product as u32);
                self.hi = self.hi.wrapping_add((product >> 32) as u32);
            }
            self.mul = self.mul.wrapping_add(1);
        }
    }

    /// (v1, v2)
    pub fn finish(&self) -> (u32, u32) {
        (self.lo, self.lo.wrapping_add(self.hi))
    }
}

/// One pressing's entry in the answer: (confidence, checksum) per track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub ids: DiscIds,
    pub tracks: Vec<(u8, u32)>,
}

fn le(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

/// The `.bin` answer: one or more entries of `count, id1, id2, cddb` followed by
/// `confidence, checksum, (frame-450 checksum)` per track, little-endian. A cut-off tail is dropped.
pub fn parse(bin: &[u8]) -> Vec<Response> {
    let mut out = Vec::new();
    let mut rest = bin;
    while let Some(&count) = rest.first() {
        let len = 13 + usize::from(count) * 9;
        if count == 0 || rest.len() < len {
            break;
        }
        let tracks = (0..usize::from(count))
            .map(|i| {
                let t = &rest[13 + i * 9..];
                (t[0], le(&t[1..5]))
            })
            .collect();
        out.push(Response {
            ids: DiscIds {
                tracks: u32::from(count),
                id1: le(&rest[1..5]),
                id2: le(&rest[5..9]),
                cddb: le(&rest[9..13]),
            },
            tracks,
        });
        rest = &rest[len..];
    }
    out
}

/// The `.bin` layout for these entries (tests, and to check [`parse`]).
pub fn encode(responses: &[Response]) -> Vec<u8> {
    let mut out = Vec::new();
    for r in responses {
        out.push(r.tracks.len() as u8);
        for v in [r.ids.id1, r.ids.id2, r.ids.cddb] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        for (conf, crc) in &r.tracks {
            out.push(*conf);
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
        }
    }
    out
}

/// What the database says about track `index` (0-based) with these checksums: a match with the
/// highest confidence among the entries that have it, a mismatch when the disc is known but no
/// entry has it, `NotInDb` when no entry covers the track. Per version, like whipper.
pub fn verify(responses: &[Response], index: usize, v1: u32, v2: u32) -> (Ar, Ar) {
    let known: Vec<(u8, u32)> = responses.iter().filter_map(|r| r.tracks.get(index).copied()).collect();
    let judge = |crc: u32| {
        if known.is_empty() {
            return Ar::NotInDb;
        }
        known
            .iter()
            .filter(|(_, c)| *c == crc)
            .map(|(conf, _)| u32::from(*conf))
            .max()
            .map_or(Ar::Mismatch, |confidence| Ar::Match { confidence })
    };
    (judge(v1), judge(v2))
}

/// The database's answer for this disc: `Ok(None)` when AccurateRip doesn't know it (404).
pub async fn fetch(http: &reqwest::Client, ids: &DiscIds) -> Result<Option<Vec<u8>>, String> {
    let url = ids.url();
    tracing::info!("AccurateRip: {url}");
    let resp = http
        .get(&url)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| format!("AccurateRip: {e}"))?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !resp.status().is_success() {
        return Err(format!("AccurateRip: HTTP {}", resp.status()));
    }
    let bytes = resp.bytes().await.map_err(|e| format!("AccurateRip: {e}"))?;
    Ok(Some(bytes.to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toc::TocTrack;

    /// IVE — REVIVE+ (the TOC the pipeline tests use).
    fn revive() -> Toc {
        Toc::from_offsets(
            1,
            154_800,
            &[150, 14625, 27975, 40575, 53175, 66750, 79425, 92325, 105150, 117975, 130200, 139500],
        )
    }

    /// AccurateRip's answer for that disc, as downloaded from accuraterip.com.
    const REVIVE_BIN: &[u8] = include_bytes!("../tests/fixtures/dBAR-012-000f9303-009076ea-96080e0c.bin");

    #[test]
    fn disc_ids_match_the_database_path() {
        let ids = DiscIds::from_toc(&revive());
        assert_eq!(ids.path(), "3/0/3/dBAR-012-000f9303-009076ea-96080e0c.bin");
        assert_eq!(
            ids.url(),
            "http://www.accuraterip.com/accuraterip/3/0/3/dBAR-012-000f9303-009076ea-96080e0c.bin"
        );
        // The answer was filed under exactly these ids.
        let r = parse(REVIVE_BIN);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].ids, ids);
    }

    #[test]
    fn enhanced_cd_counts_audio_tracks_only() {
        let mut toc = revive();
        toc.tracks.push(TocTrack { number: 13, lba: 165_000, audio: false });
        toc.leadout_lba = 200_000;
        let ids = DiscIds::from_toc(&toc);
        assert_eq!(ids.tracks, 12);
        assert_eq!(ids.cddb & 0xFF, 13, "CDDB counts the data track");
        assert!(ids.path().contains("dBAR-012-"));
    }

    #[test]
    fn recorded_answer() {
        let r = parse(REVIVE_BIN);
        assert_eq!(r[0].tracks.len(), 12);
        assert_eq!(r[0].tracks[0], (15, 0xc9b1_255b));
        assert_eq!(r[0].tracks[11], (15, 0x43cc_4edd));
        assert!(r[0].tracks.iter().all(|(conf, _)| *conf == 15));
        assert_eq!(encode(&r)[..13], REVIVE_BIN[..13]);
        // A cut-off download keeps the complete entries only.
        assert!(parse(&REVIVE_BIN[..100]).is_empty());
        let two = [REVIVE_BIN, REVIVE_BIN].concat();
        assert_eq!(parse(&two).len(), 2);

        let (v1, v2) = verify(&r, 0, 0xc9b1_255b, 1);
        assert_eq!((v1, v2), (Ar::Match { confidence: 15 }, Ar::Mismatch));
        assert_eq!(verify(&r, 3, 1, 2), (Ar::Mismatch, Ar::Mismatch));
        assert_eq!(verify(&[], 0, 1, 2), (Ar::NotInDb, Ar::NotInDb));
    }

    /// Deterministic noise, the same as the reference values were computed from.
    fn lcg_pcm(frames: usize, seed: u32) -> Vec<u8> {
        let mut x = seed;
        let mut out = Vec::with_capacity(frames * SAMPLES_PER_FRAME * 4);
        for _ in 0..frames * SAMPLES_PER_FRAME {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            out.extend_from_slice(&x.to_le_bytes());
        }
        out
    }

    /// Reference values from whipper's `accuraterip.compute(wav, track, total)` on the same samples.
    #[test]
    fn checksums_match_whipper() {
        assert_eq!(checksums(&lcg_pcm(20, 1), 1, 3), (0x47b8_beb0, 0x49a9_9924));
        assert_eq!(checksums(&lcg_pcm(13, 2), 2, 3), (0x6c86_be4a, 0x6d64_e8c3));
        assert_eq!(checksums(&lcg_pcm(30, 3), 3, 3), (0x3453_5828, 0x378b_1555));
        assert_eq!(checksums(&lcg_pcm(12, 7), 1, 1), (0xd0ce_8204, 0xd0ee_0df0));
        // Fed in pieces, as a file is read.
        let pcm = lcg_pcm(20, 1);
        let mut c = Checksum::new(1, 3, (pcm.len() / 4) as u32);
        for piece in pcm.chunks(4 * 1000) {
            c.update(piece);
        }
        assert_eq!(c.finish(), (0x47b8_beb0, 0x49a9_9924));
    }

    #[test]
    fn a_synthetic_track_verifies_against_its_own_entry() {
        let toc = revive();
        let ids = DiscIds::from_toc(&toc);
        let pcm = lcg_pcm(40, 9);
        let (v1, v2) = checksums(&pcm, 2, 12);
        let mut tracks = vec![(3u8, 0u32); 12];
        tracks[1] = (9, v2);
        let other = Response { ids, tracks: vec![(4, v2); 12] };
        let bin = encode(&[Response { ids, tracks }, other]);
        let r = parse(&bin);
        assert_eq!(verify(&r, 1, v1, v2), (Ar::Mismatch, Ar::Match { confidence: 9 }));
        // One damaged sample and it no longer matches.
        let mut bad = pcm.clone();
        bad[4000] ^= 1;
        let (b1, b2) = checksums(&bad, 2, 12);
        assert_eq!(verify(&r, 1, b1, b2), (Ar::Mismatch, Ar::Mismatch));
    }
}
