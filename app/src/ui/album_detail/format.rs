//! Album page text: dates, sizes, the metadata line, release types and external links
//! (Feishin `utils/format.tsx`, `album-detail-header.tsx`, `album-detail-content.tsx`).

use listen_core::model::Album;
use listen_core::settings::ExternalLinksSettings;
use std::time::Duration;

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// `2001-03-12` as "Mar 12, 2001", `2001-03` as "March 2001", `2001` as "2001";
/// anything else as it is.
pub fn partial_date(s: &str) -> String {
    let s = s.trim();
    let parts: Vec<&str> = s.split('-').collect();
    let num = |p: &str| p.parse::<u32>().ok();
    match parts.as_slice() {
        [y] if y.len() == 4 && num(y).is_some() => (*y).to_string(),
        [y, m] if y.len() == 4 => match num(m).filter(|m| (1..=12).contains(m)) {
            Some(m) => format!("{} {y}", MONTHS[m as usize - 1]),
            None => s.to_string(),
        },
        [y, m, d] if y.len() == 4 => {
            match (num(m).filter(|m| (1..=12).contains(m)), num(&d[..d.len().min(2)])) {
                (Some(m), Some(d)) => format!("{} {d}, {y}", &MONTHS[m as usize - 1][..3]),
                _ => s.to_string(),
            }
        }
        _ => s.to_string(),
    }
}

/// "1.50 MiB" (Feishin `formatSizeString`).
pub fn size_string(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let (mut size, mut unit) = (bytes as f64, 0);
    while size > 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    format!("{size:.2} {}", UNITS[unit])
}

/// "1h 3m 45s" (Feishin `formatDurationString`).
pub fn duration_words(d: Duration) -> String {
    let s = d.as_secs();
    let (h, m, sec) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 {
        format!("{h}h {m}m {sec}s")
    } else if m > 0 {
        format!("{m}m {sec}s")
    } else {
        format!("{sec}s")
    }
}

/// The release type as Feishin names it ("ep" is "EP", "dj-mix" is "DJ Mix").
pub fn release_type_label(raw: &str) -> String {
    match raw.trim().to_lowercase().as_str() {
        "ep" => "EP".into(),
        "dj-mix" => "DJ Mix".into(),
        "audio drama" => "Audio Drama".into(),
        "field recording" => "Field Recording".into(),
        "mixtape/street" => "Mixtape".into(),
        "spokenword" => "Spoken Word".into(),
        other => {
            let mut c = other.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        }
    }
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 { format!("1 {word}") } else { format!("{n} {word}s") }
}

/// The year of the release date, else the album's year.
pub fn release_year(album: &Album) -> Option<i32> {
    album
        .info
        .release_date
        .as_deref()
        .and_then(|d| d.get(..4))
        .and_then(|y| y.parse().ok())
        .or(album.year)
        .filter(|y| *y > 0)
}

/// Items of the line under the title: original date (if different), release date or year,
/// tracks, duration, explicit, size and plays.
pub fn meta_items(album: &Album) -> Vec<String> {
    let info = &album.info;
    let mut items = Vec::new();
    if let Some(original) = info.original_date.as_deref().filter(|o| info.release_date.as_deref() != Some(o))
    {
        items.push(partial_date(original));
    }
    let prefix = if items.is_empty() { "" } else { "Released " };
    if let Some(date) = &info.release_date {
        items.push(format!("{prefix}{}", partial_date(date)));
    } else if let Some(year) = album.year.filter(|y| *y > 0) {
        items.push(format!("{prefix}{year}"));
    }
    let tracks = album.song_count.map(|n| n as usize).unwrap_or(album.songs.len());
    items.push(plural(tracks, "track"));
    items.push(duration_words(album.duration));
    if info.explicit {
        items.push("Explicit".into());
    }
    let size = info.size.or_else(|| {
        let sizes: Vec<u64> = album.songs.iter().filter_map(|s| s.size).collect();
        (!sizes.is_empty()).then(|| sizes.iter().sum())
    });
    if let Some(size) = size.filter(|s| *s > 0) {
        items.push(size_string(size));
    }
    if let Some(plays) = album.play_count.filter(|p| *p > 0) {
        items.push(plural(plays as usize, "play"));
    }
    items
}

/// The album's comment: its own, else the first track's (Navidrome's album comment is that tag).
pub fn comment(album: &Album) -> Option<String> {
    album
        .info
        .comment
        .clone()
        .or_else(|| album.songs.iter().find_map(|s| s.comment.clone()))
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
}

/// Height of the comment block: 75 px when collapsed, else the estimated text height.
pub fn comment_height(text: &str, open: bool, width: f32) -> f32 {
    const LINE: f32 = 20.0;
    const COLLAPSED: f32 = 75.0;
    if !open {
        return COLLAPSED.min(estimated_text_height(text, width, LINE));
    }
    estimated_text_height(text, width, LINE)
}

/// Whether the comment is taller than the collapsed 75 px.
pub fn comment_is_long(text: &str, width: f32) -> bool {
    estimated_text_height(text, width, 20.0) > 75.0
}

fn estimated_text_height(text: &str, width: f32, line: f32) -> f32 {
    let per_line = (width / 7.2).max(10.0) as usize;
    let lines: usize = text.lines().map(|l| l.chars().count().div_ceil(per_line).max(1)).sum();
    lines as f32 * line
}

/// Percent-encodes everything but unreserved characters.
pub fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// A link to another site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalLink {
    pub label: &'static str,
    pub url: String,
}

/// Last.fm, MusicBrainz (needs an id), ListenBrainz, Qobuz and Spotify, as the
/// per-site switches allow (Feishin `AlbumMetadataExternalLinks`).
pub fn external_links(album: &Album, sites: &ExternalLinksSettings) -> Vec<ExternalLink> {
    let mut links = Vec::new();
    if !sites.enabled {
        return links;
    }
    let artist = album.artist_line();
    let name = &album.name;
    let search = url_encode(format!("{artist} {name}").trim());
    if sites.last_fm {
        links.push(ExternalLink {
            label: "Last.fm",
            url: format!("https://www.last.fm/music/{}/{}", url_encode(&artist), url_encode(name)),
        });
    }
    if let (true, Some(mbid)) = (sites.music_brainz, &album.info.musicbrainz_id) {
        links.push(ExternalLink {
            label: "MusicBrainz",
            url: format!("https://musicbrainz.org/release/{mbid}"),
        });
    }
    if sites.listen_brainz {
        links.push(ExternalLink {
            label: "ListenBrainz",
            url: format!("https://listenbrainz.org/search/?search_term={search}"),
        });
    }
    if sites.qobuz {
        links.push(ExternalLink {
            label: "Qobuz",
            url: format!("https://www.qobuz.com/us-en/search/albums/{search}"),
        });
    }
    if sites.spotify {
        // "Open in the Spotify app" uses the spotify: scheme.
        let url = if sites.native_spotify {
            format!("spotify:search:{search}")
        } else {
            format!("https://open.spotify.com/search/{search}")
        };
        links.push(ExternalLink { label: "Spotify", url });
    }
    links
}

#[cfg(test)]
mod tests {
    use super::*;

    fn album() -> Album {
        let mut a = crate::demo::albums().remove(0);
        a.info = Default::default();
        a
    }

    #[test]
    fn partial_dates() {
        assert_eq!(partial_date("2001"), "2001");
        assert_eq!(partial_date("2001-03"), "March 2001");
        assert_eq!(partial_date("2001-03-12"), "Mar 12, 2001");
        assert_eq!(partial_date("soon"), "soon");
    }

    #[test]
    fn sizes_and_durations() {
        assert_eq!(size_string(512), "512.00 B");
        assert_eq!(size_string(3 * 1024 * 1024), "3.00 MiB");
        assert_eq!(duration_words(Duration::from_secs(3825)), "1h 3m 45s");
        assert_eq!(duration_words(Duration::from_secs(225)), "3m 45s");
    }

    #[test]
    fn release_types() {
        assert_eq!(release_type_label("ep"), "EP");
        assert_eq!(release_type_label("dj-mix"), "DJ Mix");
        assert_eq!(release_type_label("album"), "Album");
    }

    #[test]
    fn meta_line_follows_feishin() {
        let mut a = album();
        a.year = Some(2001);
        a.info.explicit = true;
        a.info.size = Some(5 * 1024 * 1024);
        a.play_count = Some(1);
        let items = meta_items(&a);
        assert_eq!(items[0], "2001");
        assert!(items.contains(&"Explicit".to_string()) && items.contains(&"5.00 MiB".to_string()));
        assert_eq!(items.last().unwrap(), "1 play");
        // An original date that differs comes first and the release gets a prefix.
        a.info.original_date = Some("1999-05-01".into());
        a.info.release_date = Some("2001-03-12".into());
        let items = meta_items(&a);
        assert_eq!(&items[..2], ["May 1, 1999", "Released Mar 12, 2001"]);
        // The same date twice shows once.
        a.info.original_date = Some("2001-03-12".into());
        assert_eq!(meta_items(&a)[0], "Mar 12, 2001");
    }

    #[test]
    fn links_encode_names_and_need_an_id_for_musicbrainz() {
        let mut a = album();
        a.name = "A & B".into();
        let links = external_links(&a, &ExternalLinksSettings::default());
        assert!(links.iter().all(|l| l.label != "MusicBrainz"));
        assert!(links[0].url.ends_with("/A%20%26%20B"));
        a.info.musicbrainz_id = Some("abc".into());
        assert!(
            external_links(&a, &ExternalLinksSettings::default())
                .iter()
                .any(|l| l.url == "https://musicbrainz.org/release/abc")
        );
    }

    #[test]
    fn each_site_has_its_own_switch() {
        let mut a = album();
        a.info.musicbrainz_id = Some("abc".into());
        let labels = |s: &ExternalLinksSettings| -> Vec<&'static str> {
            external_links(&a, s).iter().map(|l| l.label).collect()
        };
        let mut s = ExternalLinksSettings::default();
        assert_eq!(labels(&s), ["Last.fm", "MusicBrainz", "ListenBrainz", "Qobuz", "Spotify"]);
        s.last_fm = false;
        s.qobuz = false;
        assert_eq!(labels(&s), ["MusicBrainz", "ListenBrainz", "Spotify"]);
        s.native_spotify = true;
        assert!(external_links(&a, &s).last().unwrap().url.starts_with("spotify:search:"));
        s.enabled = false;
        assert!(labels(&s).is_empty(), "the master switch hides all");
    }

    #[test]
    fn comment_collapses_only_when_long() {
        assert!(!comment_is_long("short", 600.0));
        assert!(comment_is_long(&"word ".repeat(400), 600.0));
        assert_eq!(comment_height(&"word ".repeat(400), false, 600.0), 75.0);
    }
}
