//! Front cover for a rip. Apple Music (iTunes search), the Cover Art Archive
//! (release, then its release group) and Deezer are asked at the same time;
//! every answer is measured from its own bytes and the best one is kept:
//! the highest resolution, and Apple → Cover Art Archive → Deezer when two
//! are within 10 % of each other. Larger than 3000 px or 8 MB is scaled down.

use crate::musicbrainz::Release;
use listen_core::text::fold;
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Cover {
    pub bytes: Vec<u8>,
    /// Where it came from and its stored size, for the log ("Apple Music (1400×1400)").
    pub source: String,
}

impl Cover {
    pub fn is_jpeg(&self) -> bool {
        self.bytes.starts_with(&[0xFF, 0xD8, 0xFF])
    }
    pub fn is_png(&self) -> bool {
        self.bytes.starts_with(b"\x89PNG")
    }
    /// `cover.jpg` / `cover.png`.
    pub fn file_name(&self) -> &'static str {
        if self.is_png() { "cover.png" } else { "cover.jpg" }
    }
}

/// Apple serves the artwork's native size for any requested size up to this one (measured:
/// 10000 answers, 10001 and above are HTTP 400), so asking for it gets the original.
const APPLE_SIZE: u32 = 10_000;
/// Smallest side a cover may have.
const MIN_SIDE: u32 = 500;
/// Longest side over shortest side, at most (CAA also holds back covers and booklet scans).
const MAX_ASPECT: f64 = 1.15;
/// Smaller files are placeholders or error pages.
const MIN_BYTES: usize = 10 * 1024;
/// Two candidates whose shortest sides differ by at most this are a tie, won by the preferred source.
const TIE: f64 = 0.10;
/// The stored cover's limits; a larger winner is scaled down to fit `MAX_SIDE`.
const MAX_SIDE: u32 = 3000;
const MAX_STORED_BYTES: usize = 8 << 20;
/// A Cover Art Archive original larger than this (a 4000 px PNG is 43 MB) isn't downloaded;
/// its 1200 px thumbnail is used instead.
const MAX_ORIGINAL_BYTES: usize = 25 << 20;
/// Tries per request when the server is busy (503/429).
const TRIES: u32 = 3;

/// Where a candidate came from, in the order the owner prefers them at a tie.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    Apple,
    CoverArtArchive,
    Deezer,
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub source: Source,
    /// "Apple Music", "Cover Art Archive original", …
    pub label: String,
    pub bytes: Vec<u8>,
}

impl Candidate {
    fn size(&self) -> Option<(u32, u32)> {
        pixel_size(&self.bytes)
    }
    fn describe(&self) -> String {
        match self.size() {
            Some((w, h)) => format!("{} {w}×{h} {} KB", self.label, self.bytes.len() / 1024),
            None => format!("{} {} KB", self.label, self.bytes.len() / 1024),
        }
    }
}

/// Width × height read from a JPEG's SOF or a PNG's IHDR; `None` for anything else.
pub fn pixel_size(b: &[u8]) -> Option<(u32, u32)> {
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        if b.get(12..16)? != b"IHDR" {
            return None;
        }
        let w = u32::from_be_bytes(b.get(16..20)?.try_into().ok()?);
        let h = u32::from_be_bytes(b.get(20..24)?.try_into().ok()?);
        return (w > 0 && h > 0).then_some((w, h));
    }
    if !b.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut i = 2;
    loop {
        // Markers may be padded with any number of 0xFF.
        if *b.get(i)? != 0xFF {
            return None;
        }
        while *b.get(i)? == 0xFF {
            i += 1;
        }
        let marker = *b.get(i)?;
        i += 1;
        match marker {
            // Standalone markers: no length.
            0x01 | 0xD0..=0xD7 => continue,
            0xD9 | 0xDA => return None, // end of image / start of scan before any frame header
            _ => {}
        }
        let len = u16::from_be_bytes(b.get(i..i + 2)?.try_into().ok()?) as usize;
        // SOF0..SOF15, except DHT (C4), JPG (C8) and DAC (CC).
        if matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            let h = u16::from_be_bytes(b.get(i + 3..i + 5)?.try_into().ok()?) as u32;
            let w = u16::from_be_bytes(b.get(i + 5..i + 7)?.try_into().ok()?) as u32;
            return (w > 0 && h > 0).then_some((w, h));
        }
        if len < 2 {
            return None;
        }
        i += len;
    }
}

/// Why `c` can't be the cover, or `None` when it can.
pub fn reject_reason(c: &Candidate) -> Option<&'static str> {
    if c.bytes.len() < MIN_BYTES {
        return Some("under 10 KB");
    }
    let Some((w, h)) = c.size() else { return Some("not a JPEG or PNG") };
    if w.min(h) < MIN_SIDE {
        return Some("smaller than 500×500");
    }
    if w.max(h) as f64 / w.min(h) as f64 > MAX_ASPECT {
        return Some("not square");
    }
    None
}

/// The best acceptable candidate: the largest shortest side, but any candidate within 10 % of it
/// counts as equal, and among those the preferred source wins (then the larger one, then the
/// earlier one).
pub fn choose(candidates: &[Candidate]) -> Option<usize> {
    let ok: Vec<(usize, u32)> = candidates
        .iter()
        .enumerate()
        .filter(|(_, c)| reject_reason(c).is_none())
        .filter_map(|(i, c)| c.size().map(|(w, h)| (i, w.min(h))))
        .collect();
    let best = ok.iter().map(|&(_, side)| side).max()?;
    let floor = best as f64 * (1.0 - TIE);
    ok.into_iter()
        .filter(|&(_, side)| side as f64 >= floor)
        .min_by_key(|&(i, side)| (candidates[i].source, std::cmp::Reverse(side), i))
        .map(|(i, _)| i)
}

/// Does a search hit (`ra` artist, `rt` title) name this album? Artist one way or the other
/// contains the other, the title starts the same ("REVIVE+" fits "REVIVE+ - EP").
fn fits(artist: &str, album: &str, ra: &str, rt: &str) -> bool {
    let (a, t, ra, rt) = (fold(artist), fold(album), fold(ra), fold(rt));
    !a.is_empty()
        && !ra.is_empty()
        && !t.is_empty()
        && !rt.is_empty()
        && (ra.contains(&a) || a.contains(&ra))
        && (rt.starts_with(&t) || t.starts_with(&rt))
}

/// The full-size artwork URL of the iTunes album matching artist + title.
pub fn itunes_pick(body: &Value, artist: &str, album: &str) -> Option<String> {
    body["results"].as_array()?.iter().find_map(|r| {
        let ok = fits(
            artist,
            album,
            r["artistName"].as_str().unwrap_or(""),
            r["collectionName"].as_str().unwrap_or(""),
        );
        let url = r["artworkUrl100"].as_str().filter(|_| ok)?;
        // Plain `bb.jpg`: a quality suffix (`bb-60.jpg`) is a lossier re-encode.
        url.contains("100x100bb").then(|| url.replace("100x100bb", &format!("{APPLE_SIZE}x{APPLE_SIZE}bb")))
    })
}

/// The `cover_xl` URL of the Deezer album matching artist + title.
pub fn deezer_pick(body: &Value, artist: &str, album: &str) -> Option<String> {
    body["data"].as_array()?.iter().find_map(|r| {
        let ok = fits(
            artist,
            album,
            r["artist"]["name"].as_str().unwrap_or(""),
            r["title"].as_str().unwrap_or(""),
        );
        r["cover_xl"].as_str().filter(|u| ok && !u.is_empty()).map(str::to_string)
    })
}

/// The front image of a Cover Art Archive listing: (original, 1200 px thumbnail).
pub fn caa_front(body: &Value) -> Option<(String, Option<String>)> {
    let img = body["images"]
        .as_array()?
        .iter()
        .find(|i| i["front"].as_bool() == Some(true) && i["approved"].as_bool() != Some(false))?;
    let original = img["image"].as_str()?.to_string();
    let thumb = img["thumbnails"]["1200"].as_str().map(str::to_string);
    Some((original, thumb))
}

/// GET `url`, waiting out a busy server (503/429, honouring `retry-after` up to 10 s) at most
/// `TRIES` times. `None` for any other failure, a 404 included.
async fn get(http: &reqwest::Client, url: &str, timeout: Duration) -> Option<reqwest::Response> {
    for attempt in 1..=TRIES {
        let resp = match http.get(url).timeout(timeout).send().await {
            Ok(r) => r,
            Err(e) => {
                tracing::debug!("cover: {}", e.without_url());
                return None;
            }
        };
        let status = resp.status().as_u16();
        if resp.status().is_success() {
            return Some(resp);
        }
        if !matches!(status, 503 | 429) || attempt == TRIES {
            return None;
        }
        let wait = resp
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok())
            .unwrap_or(attempt as u64 * 2)
            .min(10);
        tokio::time::sleep(Duration::from_secs(wait)).await;
    }
    None
}

async fn json(http: &reqwest::Client, url: &str) -> Option<Value> {
    get(http, url, Duration::from_secs(20)).await?.json().await.ok()
}

/// The bytes at `url`, `None` past `max` bytes.
async fn download(http: &reqwest::Client, url: &str, max: usize, timeout: Duration) -> Option<Vec<u8>> {
    let mut resp = get(http, url, timeout).await?;
    if resp.content_length().is_some_and(|n| n as usize > max) {
        return None;
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = resp.chunk().await.ok()? {
        bytes.extend_from_slice(&chunk);
        if bytes.len() > max {
            return None;
        }
    }
    Some(bytes)
}

async fn image(http: &reqwest::Client, url: &str) -> Option<Vec<u8>> {
    download(http, url, MAX_ORIGINAL_BYTES, Duration::from_secs(30)).await
}

async fn apple(http: &reqwest::Client, artist: &str, album: &str) -> Option<Candidate> {
    let term = format!("{artist} {album}");
    let url = reqwest::Url::parse_with_params(
        "https://itunes.apple.com/search",
        &[("term", term.as_str()), ("entity", "album"), ("limit", "10")],
    )
    .ok()?;
    let art = itunes_pick(&json(http, url.as_str()).await?, artist, album)?;
    let bytes = image(http, &art).await?;
    Some(Candidate { source: Source::Apple, label: "Apple Music".into(), bytes })
}

async fn deezer(http: &reqwest::Client, artist: &str, album: &str) -> Option<Candidate> {
    let q = format!("{artist} {album}");
    let url = reqwest::Url::parse_with_params(
        "https://api.deezer.com/search/album",
        &[("q", q.as_str()), ("limit", "5")],
    )
    .ok()?;
    let art = deezer_pick(&json(http, url.as_str()).await?, artist, album)?;
    let bytes = image(http, &art).await?;
    Some(Candidate { source: Source::Deezer, label: "Deezer".into(), bytes })
}

/// The release's front, else its release group's: the original, or the 1200 px thumbnail when
/// the original is missing, too large or unusable. Every image tried is returned (for the log).
async fn cover_art_archive(http: &reqwest::Client, release: &Release) -> Vec<Candidate> {
    let mut tried = Vec::new();
    let kinds = [
        ("release", release.id.as_str(), "Cover Art Archive"),
        ("release-group", release.release_group_id.as_str(), "Cover Art Archive release group"),
    ];
    // A release parsed from an incomplete answer can lack either id: skip it, don't ask for "release//".
    for (kind, id, name) in kinds.into_iter().filter(|(_, id, _)| !id.is_empty()) {
        let Some((original, thumb)) = json(http, &format!("https://coverartarchive.org/{kind}/{id}"))
            .await
            .as_ref()
            .and_then(caa_front)
        else {
            continue;
        };
        // Originals can be tens of MB from a slow archive: give them longer. The thumbnail only
        // stands in for an original that couldn't be had; a rejected original's is no better.
        let bytes = match download(http, &original, MAX_ORIGINAL_BYTES, Duration::from_secs(60)).await {
            Some(b) => Some((b, "original")),
            None => match thumb {
                Some(t) => image(http, &t).await.map(|b| (b, "1200 px")),
                None => None,
            },
        };
        if let Some((bytes, which)) = bytes {
            let c = Candidate { source: Source::CoverArtArchive, label: format!("{name} {which}"), bytes };
            let ok = reject_reason(&c).is_none();
            tried.push(c);
            if ok {
                break;
            }
        }
    }
    tried
}

/// Every candidate for this album, the sources asked at the same time.
pub async fn candidates(
    http: &reqwest::Client,
    artist: &str,
    album: &str,
    release: Option<&Release>,
) -> Vec<Candidate> {
    let caa = async {
        match release {
            Some(r) => cover_art_archive(http, r).await,
            None => Vec::new(),
        }
    };
    let (a, c, d) = tokio::join!(apple(http, artist, album), caa, deezer(http, artist, album));
    a.into_iter().chain(c).chain(d).collect()
}

/// `bytes` scaled down to fit 3000×3000 (and re-encoded as JPEG when over 8 MB), with its size.
/// Within the limits, or when it can't be decoded, it is kept as it is.
pub fn cap(bytes: Vec<u8>) -> (Vec<u8>, Option<(u32, u32)>) {
    let size = pixel_size(&bytes);
    let within = size.is_some_and(|(w, h)| w <= MAX_SIDE && h <= MAX_SIDE) && bytes.len() <= MAX_STORED_BYTES;
    if within {
        return (bytes, size);
    }
    let Ok(img) = image::load_from_memory(&bytes) else {
        tracing::warn!("cover: can't decode the image to scale it down, keeping it as it is");
        return (bytes, size);
    };
    let img = if img.width() > MAX_SIDE || img.height() > MAX_SIDE {
        img.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };
    let rgb = img.to_rgb8();
    let mut out = Vec::new();
    match image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 92).encode_image(&rgb) {
        Ok(()) => (out, Some(rgb.dimensions())),
        Err(e) => {
            tracing::warn!("cover: can't re-encode the image ({e}), keeping it as it is");
            (bytes, size)
        }
    }
}

/// The best front cover for this album from every source. `release` adds the Cover Art Archive;
/// without one (a gnudb or CD-Text name) Apple Music and Deezer are still asked.
pub async fn find(
    http: &reqwest::Client,
    artist: &str,
    album: &str,
    release: Option<&Release>,
) -> Option<Cover> {
    let all = candidates(http, artist, album, release).await;
    let chosen = choose(&all);
    let list: Vec<String> = all
        .iter()
        .map(|c| match reject_reason(c) {
            Some(why) => format!("{} (rejected: {why})", c.describe()),
            None => c.describe(),
        })
        .collect();
    tracing::info!(
        "cover candidates: {}; chosen: {}",
        if list.is_empty() { "none".into() } else { list.join(", ") },
        chosen.map(|i| all[i].label.as_str()).unwrap_or("none")
    );
    let winner = all.into_iter().nth(chosen?)?;
    let (ow, oh) = winner.size()?;
    let (bytes, stored) = tokio::task::spawn_blocking(move || cap(winner.bytes)).await.ok()?;
    let size = match stored {
        Some((w, h)) if (w, h) != (ow, oh) => format!("{ow}×{oh} → {w}×{h}"),
        _ => format!("{ow}×{oh}"),
    };
    Some(Cover { bytes, source: format!("{} ({size})", winner.label) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    /// A PNG header claiming `w`×`h`, padded to `len` bytes.
    fn png(w: u32, h: u32, len: usize) -> Vec<u8> {
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        b.extend_from_slice(&w.to_be_bytes());
        b.extend_from_slice(&h.to_be_bytes());
        b.resize(len, 0);
        b
    }

    fn cand(source: Source, w: u32, h: u32) -> Candidate {
        Candidate { source, label: format!("{source:?}"), bytes: png(w, h, 200 * 1024) }
    }

    #[test]
    fn picks_the_matching_itunes_album() {
        let body = serde_json::json!({"results": [
            {"artistName": "Someone Else", "collectionName": "REVIVE+", "artworkUrl100": "https://x/a/100x100bb.jpg"},
            {"artistName": "IVE", "collectionName": "REVIVE+ - EP", "artworkUrl100": "https://x/b/100x100bb.jpg"}
        ]});
        assert_eq!(itunes_pick(&body, "IVE", "REVIVE+").as_deref(), Some("https://x/b/10000x10000bb.jpg"));
        assert_eq!(itunes_pick(&body, "IVE", "Other"), None);
    }

    #[test]
    fn itunes_answer_fixture() {
        let body = fixture("itunes_search_armageddon.json");
        let url = itunes_pick(&body, "aespa", "Armageddon").unwrap();
        assert!(url.contains("00888735949869_Cover.jpg/10000x10000bb.jpg"), "{url}");
        assert!(!url.contains("bb-"), "no quality suffix: {url}");
        assert_eq!(itunes_pick(&body, "aespa", "Drama"), None);
    }

    #[test]
    fn deezer_answer_fixture_skips_the_wrong_artist() {
        let body = fixture("deezer_search_armageddon.json");
        assert_eq!(
            deezer_pick(&body, "aespa", "Armageddon").as_deref(),
            Some(
                "https://cdn-images.dzcdn.net/images/cover/40c12f3b196ddad8dd006f430e586613/1000x1000-000000-80-0-0.jpg"
            )
        );
        // "Aeswa – Speedcore Armageddon" is in the answer, but it isn't this record.
        assert_eq!(deezer_pick(&body, "Aeswa", "Armageddon"), None);
        assert_eq!(deezer_pick(&body, "", "Armageddon"), None);
    }

    #[test]
    fn caa_listing_fixture_takes_the_front_not_the_placeholder() {
        let (original, thumb) = caa_front(&fixture("caa_release_group_revive.json")).unwrap();
        assert!(original.ends_with("/44431520000.jpg"), "{original}");
        assert!(thumb.unwrap().ends_with("/44431520000-1200.jpg"));
        assert_eq!(caa_front(&serde_json::json!({"images": [{"front": false, "image": "x"}]})), None);
    }

    #[test]
    fn pixel_size_of_jpeg_and_png() {
        let jpeg = include_bytes!("../tests/fixtures/cover.jpg");
        let (w, h) = pixel_size(jpeg).expect("jpeg size");
        let decoded = image::load_from_memory(jpeg).unwrap();
        assert_eq!((w, h), (decoded.width(), decoded.height()));
        assert_eq!(pixel_size(&png(1400, 1300, 64)), Some((1400, 1300)));
        // A progressive JPEG (SOF2) after an APP0 segment, with fill bytes before the marker.
        let mut prog =
            vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00, 0xFF, 0xFF, 0xC2, 0x00, 0x11, 0x08];
        prog.extend_from_slice(&[0x05, 0x78, 0x0B, 0xB8]); // height 1400, width 3000
        assert_eq!(pixel_size(&prog), Some((3000, 1400)));
        assert_eq!(pixel_size(b"GIF89a...."), None);
        assert_eq!(pixel_size(&[0xFF, 0xD8, 0xFF]), None);
        assert_eq!(pixel_size(b"\x89PNG\r\n\x1a\n"), None);
    }

    #[test]
    fn apple_wins_a_near_tie() {
        let c = [
            cand(Source::CoverArtArchive, 1500, 1500),
            cand(Source::Apple, 1400, 1400),
            cand(Source::Deezer, 1000, 1000),
        ];
        assert_eq!(choose(&c), Some(1));
        // Same size from both: Apple.
        let c = [cand(Source::CoverArtArchive, 3000, 3000), cand(Source::Apple, 3000, 3000)];
        assert_eq!(choose(&c), Some(1));
    }

    #[test]
    fn a_much_larger_original_wins_outright() {
        let c = [
            cand(Source::Apple, 1400, 1400),
            cand(Source::CoverArtArchive, 3000, 3000),
            cand(Source::Deezer, 1000, 1000),
        ];
        assert_eq!(choose(&c), Some(1));
        // Deezer only wins when it is clearly the largest.
        let c = [cand(Source::Apple, 600, 600), cand(Source::Deezer, 1000, 1000)];
        assert_eq!(choose(&c), Some(1));
    }

    #[test]
    fn undersized_non_square_and_tiny_candidates_are_rejected() {
        let small = cand(Source::Apple, 248, 250);
        let back = cand(Source::CoverArtArchive, 3000, 1500);
        let tiny = Candidate { source: Source::Apple, label: "x".into(), bytes: png(1400, 1400, 5000) };
        let gif = Candidate { source: Source::Apple, label: "x".into(), bytes: vec![b'G'; 50_000] };
        assert_eq!(reject_reason(&small), Some("smaller than 500×500"));
        assert_eq!(reject_reason(&back), Some("not square"));
        assert_eq!(reject_reason(&tiny), Some("under 10 KB"));
        assert_eq!(reject_reason(&gif), Some("not a JPEG or PNG"));
        // 1002×893 is within 1:1.15.
        assert_eq!(reject_reason(&cand(Source::CoverArtArchive, 1002, 893)), None);
        let deezer = cand(Source::Deezer, 1000, 1000);
        assert_eq!(choose(&[small.clone(), back.clone(), tiny.clone(), deezer]), Some(3));
        assert_eq!(choose(&[small, back, tiny, gif]), None);
    }

    #[test]
    fn caps_a_large_cover() {
        let big =
            image::RgbImage::from_fn(3200, 3100, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, 7]));
        let mut png_bytes = Vec::new();
        image::DynamicImage::ImageRgb8(big)
            .write_to(&mut std::io::Cursor::new(&mut png_bytes), image::ImageFormat::Png)
            .unwrap();
        let (out, size) = cap(png_bytes);
        assert_eq!(size, Some((3000, 2906)));
        assert!(out.starts_with(&[0xFF, 0xD8, 0xFF]));
        assert_eq!(pixel_size(&out), Some((3000, 2906)));
        // Within the limits: untouched.
        let jpeg = include_bytes!("../tests/fixtures/cover.jpg").to_vec();
        assert_eq!(cap(jpeg.clone()).0, jpeg);
        // Too large but undecodable: kept rather than lost.
        let broken = png(4000, 4000, 20_000);
        assert_eq!(cap(broken.clone()), (broken, Some((4000, 4000))));
    }

    /// Live (`--ignored`): REVIVE+ has no release cover, its release group has a 3000 px one,
    /// and Apple has the same 3000 px artwork, which wins the tie.
    #[tokio::test]
    #[ignore = "live: musicbrainz.org, the Cover Art Archive, Apple and Deezer"]
    async fn live_cover_chain() {
        let rs = crate::musicbrainz::parse_lookup(
            &serde_json::from_str(include_str!("../tests/fixtures/discid_revive.json")).unwrap(),
        );
        let r = &rs[0];
        let http = crate::musicbrainz::http_client();
        let all = candidates(&http, &r.artist(), &r.title, Some(r)).await;
        let labels: Vec<String> = all.iter().map(Candidate::describe).collect();
        assert!(
            labels.iter().any(|l| l.starts_with("Cover Art Archive release group original")),
            "{labels:?}"
        );
        let cover = find(&http, &r.artist(), &r.title, Some(r)).await.expect("a cover");
        assert!(cover.is_jpeg());
        assert_eq!(cover.source, "Apple Music (3000×3000)");
    }
}
