//! Finding the same recording on a self-hosted server for a YouTube Music song: title and artist
//! are normalised, version tags (live, remix, instrumental, ...) must agree, durations must be
//! close. Pure functions; the lookups themselves live in the app.

use crate::analysis::Features;
use crate::model::{Song, SourceKind};
use std::collections::BTreeSet;
use unicode_normalization::UnicodeNormalization;

/// Durations this close (seconds) are the same recording.
const TOLERANCE_SECS: f64 = 3.0;
/// Longer songs may differ by this share of their length (encoder padding, silence).
const TOLERANCE_SHARE: f64 = 0.02;
const LONG_SECS: f64 = 150.0;

/// Words of a bracketed or dashed extra that say nothing about which recording it is.
const NOISE: &[&str] = &[
    "official",
    "video",
    "music",
    "mv",
    "m",
    "v",
    "audio",
    "lyric",
    "lyrics",
    "visualizer",
    "visualiser",
    "hd",
    "hq",
    "4k",
    "1080p",
    "720p",
    "deluxe",
    "edition",
    "expanded",
    "explicit",
    "clean",
    "bonus",
    "track",
    "album",
    "single",
    "radio",
    "edit",
    "original",
    "version",
    "ver",
    "soundtrack",
    "ost",
    "stereo",
    "mono",
    "full",
    "digital",
    "digitally",
    "anniversary",
    "the",
    "mix",
    "new",
    "standard",
    "lp",
    "main",
    "hidden",
];

/// Words before "mix" that name the same recording ("Radio Mix") rather than a remix.
const PLAIN_MIX: &[&str] = &[
    "original", "radio", "album", "single", "extended", "full", "main", "clean", "explicit", "lp", "stereo",
    "mono", "new", "standard", "video",
];

/// What a title says beyond its name.
#[derive(Debug, Default, PartialEq)]
struct Title {
    /// The name as written (lower case, plain), for searching.
    text: String,
    /// Folded name, plus its other-script aliases (`봄날 (Spring Day)`).
    keys: BTreeSet<String>,
    /// Version tags that make a different recording.
    tags: BTreeSet<&'static str>,
    /// Other extras, compared as written ("Taylor's Version", "Japanese Ver.").
    extras: BTreeSet<String>,
    /// Featured artists named in the title.
    feat: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Script {
    Latin,
    Hangul,
    Cjk,
    Other,
}

/// Lower case, compatibility forms (full-width letters) and accents folded, one kind of dash and quote.
fn plain(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.nfkc() {
        match c {
            '–' | '—' | '―' | '‒' | '−' => out.push('-'),
            '’' | '‘' | '´' | '`' => out.push('\''),
            '“' | '”' | '„' => out.push('"'),
            'ß' => out.push_str("ss"),
            'æ' | 'Æ' => out.push_str("ae"),
            'œ' | 'Œ' => out.push_str("oe"),
            'ø' | 'Ø' => out.push('o'),
            'đ' | 'Đ' => out.push('d'),
            'ł' | 'Ł' => out.push('l'),
            c if c < '\u{250}' => {
                // Latin letters lose their accents; other scripts stay as they are.
                for d in std::iter::once(c).nfd().filter(|d| !('\u{300}'..='\u{36f}').contains(d)) {
                    out.extend(d.to_lowercase());
                }
            }
            c => out.extend(c.to_lowercase()),
        }
    }
    out
}

/// Letters and digits only (any script), so punctuation and spacing don't matter.
fn fold(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).collect()
}

fn words(s: &str) -> Vec<&str> {
    s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect()
}

fn script_of(s: &str) -> Option<Script> {
    let (mut latin, mut hangul, mut cjk, mut other) = (0, 0, 0, 0);
    for c in s.chars().filter(|c| c.is_alphabetic()) {
        match c {
            'a'..='z' | '\u{c0}'..='\u{24f}' => latin += 1,
            '\u{1100}'..='\u{11ff}' | '\u{3130}'..='\u{318f}' | '\u{ac00}'..='\u{d7af}' => hangul += 1,
            '\u{3040}'..='\u{30ff}' | '\u{3400}'..='\u{9fff}' | '\u{f900}'..='\u{faff}' => cjk += 1,
            _ => other += 1,
        }
    }
    let top = latin.max(hangul).max(cjk).max(other);
    match top {
        0 => None,
        _ if top == latin => Some(Script::Latin),
        _ if top == hangul => Some(Script::Hangul),
        _ if top == cjk => Some(Script::Cjk),
        _ => Some(Script::Other),
    }
}

/// A year or an ordinal ("2011", "20th").
fn is_count(w: &str) -> bool {
    let digits = w.trim_end_matches(|c: char| c.is_alphabetic());
    !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) && w.len() - digits.len() <= 2
}

/// The version tags a bracketed or dashed extra carries.
fn tags_of(ws: &[&str]) -> BTreeSet<&'static str> {
    let mut tags = BTreeSet::new();
    for (i, w) in ws.iter().enumerate() {
        let prev = i.checked_sub(1).map(|p| ws[p]);
        let tag = match *w {
            "live" | "concert" => "live",
            "remix" | "remixed" | "rmx" | "bootleg" | "mashup" | "rework" | "reworked" | "refix" | "vip" => {
                "remix"
            }
            "mix" if prev.is_some_and(|p| !PLAIN_MIX.contains(&p)) => "remix",
            "instrumental" | "inst" | "karaoke" | "mr" | "backing" | "offvocal" => "instrumental",
            "vocal" if prev == Some("off") => "instrumental",
            "acapella" | "acappella" => "acapella",
            "cappella" if prev == Some("a") => "acapella",
            "acoustic" | "unplugged" | "stripped" => "acoustic",
            "piano" => "piano",
            "orchestral" | "orchestra" | "symphonic" => "orchestral",
            "cover" => "cover",
            "sped" | "nightcore" => "sped",
            "speed" if ws.get(i + 1) == Some(&"up") => "sped",
            "slowed" => "slowed",
            "reverb" => "reverb",
            "8d" => "8d",
            "demo" => "demo",
            "short" => "short",
            "extended" => "extended",
            _ => continue,
        };
        tags.insert(tag);
    }
    // "TV size" is a short cut.
    if ws.windows(2).any(|w| w == ["tv", "size"]) {
        tags.insert("short");
    }
    tags
}

/// Splits `(extras)` and ` - extras` off a title; returns the name and the extras.
fn split_extras(s: &str) -> (String, Vec<String>) {
    let (mut main, mut groups) = (String::new(), Vec::new());
    let (mut depth, mut cur) = (0usize, String::new());
    for c in s.chars() {
        match c {
            '(' | '[' | '{' | '【' | '〔' => {
                if depth > 0 {
                    cur.push(c);
                }
                depth += 1;
            }
            ')' | ']' | '}' | '】' | '〕' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    groups.push(std::mem::take(&mut cur));
                } else {
                    cur.push(c);
                }
            }
            _ if depth > 0 => cur.push(c),
            _ => main.push(c),
        }
    }
    if depth > 0 {
        groups.push(cur);
    }
    let main = main.replace(" | ", " - ");
    let mut parts = main.split(" - ").map(str::to_string);
    let name = parts.next().unwrap_or_default();
    groups.extend(parts);
    (name, groups)
}

/// Cuts "feat. X" off a name; returns the name and the featured part.
fn cut_feat(name: &str) -> (&str, Option<&str>) {
    let marks = [" feat. ", " feat ", " ft. ", " ft ", " featuring "];
    match marks.iter().filter_map(|p| name.find(p).map(|i| (i, p.len()))).min() {
        Some((i, len)) => (&name[..i], Some(&name[i + len..])),
        None => (name, None),
    }
}

fn artist_pieces(s: &str) -> Vec<&str> {
    let mut parts = vec![s];
    for sep in [
        ",",
        ";",
        "/",
        "、",
        "，",
        " & ",
        " and ",
        " x ",
        " × ",
        " feat. ",
        " ft. ",
        " featuring ",
        " with ",
        " vs. ",
    ] {
        parts = parts.into_iter().flat_map(|p| p.split(sep)).collect();
    }
    parts
}

/// Keys one credited artist stands for: the whole name, its parts, and bracketed aliases
/// (`BTS (방탄소년단)`), without "- Topic" / "VEVO" and a leading "the".
fn artist_keys_of(name: &str, out: &mut BTreeSet<String>) {
    let p = plain(name);
    let p = p.trim().trim_end_matches(" - topic").trim();
    let p = p.strip_suffix("vevo").map_or(p, str::trim);
    let (main, aliases) = split_extras(p);
    let mut add = |s: &str| {
        let k = fold(s);
        if let Some(rest) = k.strip_prefix("the")
            && rest.len() > 2
        {
            out.insert(rest.to_string());
        }
        if !k.is_empty() {
            out.insert(k);
        }
    };
    add(p);
    add(&main);
    for piece in artist_pieces(&main).into_iter().chain(aliases.iter().map(String::as_str)) {
        add(piece);
    }
}

fn artist_keys(song: &Song, feat: &[String]) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    for a in song.artists.iter().chain(&song.album_artists) {
        artist_keys_of(&a.name, &mut keys);
    }
    for f in feat {
        artist_keys_of(f, &mut keys);
    }
    keys
}

fn parse_title(raw: &str, artists: &BTreeSet<String>) -> Title {
    let p = plain(raw);
    let (mut name, mut groups) = split_extras(&p);
    // "Artist - Title" uploads: the artist part is no extra.
    if !groups.is_empty() && artists.contains(&fold(&name)) && !fold(&groups[0]).is_empty() {
        let (n, more) = split_extras(&groups.remove(0));
        name = n;
        groups.extend(more);
    }
    let mut title = Title::default();
    let (base, feat) = cut_feat(&name);
    if let Some(f) = feat {
        title.feat.push(f.to_string());
    }
    let base = base.replace(['&', '+'], " and ");
    title.text = base.split_whitespace().collect::<Vec<_>>().join(" ");
    let key = if fold(&title.text).is_empty() { fold(&p) } else { fold(&title.text) };
    let script = script_of(&title.text);
    title.keys.insert(key);
    for g in groups {
        let ws = words(&g);
        let Some(first) = ws.first() else { continue };
        if matches!(*first, "feat" | "ft" | "featuring" | "with") {
            title.feat.push(g.trim().split_once(' ').map_or(String::new(), |(_, r)| r.to_string()));
            continue;
        }
        // "(From the film ...)", "(Prod. by ...)".
        if matches!(*first, "from" | "prod" | "ost") {
            continue;
        }
        let tags = tags_of(&ws);
        if !tags.is_empty() {
            title.tags.extend(tags);
            continue;
        }
        if ws.iter().any(|w| w.starts_with("remaster")) || ws.iter().all(|w| NOISE.contains(w) || is_count(w))
        {
            continue;
        }
        let g_script = script_of(&g);
        let is_version = ws.iter().any(|w| matches!(*w, "ver" | "version"));
        if g_script.is_some() && g_script != script && !is_version {
            // A translation or transliteration of the name.
            title.keys.insert(fold(&g));
        } else {
            let ws: Vec<&str> = ws.iter().map(|w| if *w == "version" { "ver" } else { w }).collect();
            title.extras.insert(ws.concat());
        }
    }
    title
}

fn album_key(album: &str) -> String {
    fold(&split_extras(&plain(album)).0)
}

fn tolerance(longest: f64) -> f64 {
    if longest >= LONG_SECS { TOLERANCE_SECS.max(longest * TOLERANCE_SHARE) } else { TOLERANCE_SECS }
}

/// How well `candidate` is the same recording as `wanted`; `None` = it is not.
pub fn score(wanted: &Song, candidate: &Song) -> Option<i64> {
    if candidate.source == SourceKind::YouTubeMusic {
        return None;
    }
    // Artists first: they tell an "Artist - Title" upload from a title with a dash.
    let a = parse_title(&wanted.title, &artist_keys(wanted, &[]));
    let b = parse_title(&candidate.title, &artist_keys(candidate, &[]));
    if a.keys.is_disjoint(&b.keys) || a.tags != b.tags || a.extras != b.extras {
        return None;
    }
    if artist_keys(wanted, &a.feat).is_disjoint(&artist_keys(candidate, &b.feat)) {
        return None;
    }
    let mut score = 1000i64;
    let (x, y) = (wanted.duration.as_secs_f64(), candidate.duration.as_secs_f64());
    if x > 0.0 && y > 0.0 {
        let gap = (x - y).abs();
        if gap > tolerance(x.max(y)) {
            return None;
        }
        score -= (gap * 40.0) as i64;
    } else {
        // One side has no duration (some YouTube Music lists): the name has to carry it.
        score -= 300;
    }
    if let (Some(x), Some(y)) = (&wanted.album, &candidate.album) {
        let key = album_key(x);
        score += if !key.is_empty() && key == album_key(y) { 200 } else { -30 };
    }
    if fold(&plain(&wanted.title)) == fold(&plain(&candidate.title)) {
        score += 50;
    }
    let lossless = candidate.suffix.as_deref().or(candidate.codec.as_deref()).is_some_and(|s| {
        matches!(s.to_lowercase().as_str(), "flac" | "alac" | "wav" | "aiff" | "ape" | "wv")
    });
    score += if lossless { 20 } else { i64::from(candidate.bit_rate.unwrap_or(0).min(320) / 32) };
    Some(score)
}

/// The best candidate that is the same recording as `wanted` (the first of equals).
pub fn best_match<'a>(wanted: &Song, candidates: &'a [Song]) -> Option<&'a Song> {
    let mut best: Option<(i64, &Song)> = None;
    for c in candidates {
        if let Some(s) = score(wanted, c)
            && best.is_none_or(|(b, _)| s > b)
        {
            best = Some((s, c));
        }
    }
    best.map(|(_, c)| c)
}

/// The song name in a music video's title: the quoted part (`aespa 'Love Riot' MV`), else the
/// title without "Artist - " and video tags (`ILLIT - Magnetic (Official MV)`).
fn video_song_name(video: &Song) -> String {
    let t = video.title.as_str();
    for (open, close) in [('\'', '\''), ('‘', '’'), ('"', '"'), ('“', '”'), ('「', '」')] {
        if let Some(i) = t.find(open) {
            let rest = &t[i + open.len_utf8()..];
            if let Some(j) = rest.find(close)
                && j > 0
            {
                return rest[..j].to_string();
            }
        }
    }
    parse_title(t, &artist_keys(video, &[])).text
}

/// The song (art track) of a music video among search results: same name and artist, with an
/// album. Video tags ("MV", "Official Video") and the video's longer intro don't count against
/// it, unlike `score`, which looks for the same recording.
pub fn song_of_video<'a>(video: &Song, candidates: &'a [Song]) -> Option<&'a Song> {
    let name = fold(&plain(&video_song_name(video)));
    if name.is_empty() {
        return None;
    }
    let artists = artist_keys(video, &[]);
    let title = fold(&plain(&video.title));
    candidates.iter().find(|c| {
        if c.album.is_none() || c.id == video.id {
            return false;
        }
        let b = parse_title(&c.title, &artist_keys(c, &[]));
        if !b.keys.contains(&name) || !b.tags.is_empty() {
            return false;
        }
        // The artist is the uploader or is named in the video title (label channels upload MVs).
        let theirs = artist_keys(c, &b.feat);
        !artists.is_disjoint(&theirs) || theirs.iter().any(|a| !a.is_empty() && title.contains(a.as_str()))
    })
}

/// Search texts for `wanted`, best first: the name alone, then name and artist.
pub fn queries(wanted: &Song) -> Vec<String> {
    let title = parse_title(&wanted.title, &artist_keys(wanted, &[]));
    let mut out = Vec::new();
    if title.text.is_empty() {
        return out;
    }
    out.push(title.text.clone());
    if let Some(artist) = wanted.artists.first() {
        let (main, _) = split_extras(&plain(&artist.name));
        let main = main.trim().trim_end_matches(" - topic").trim();
        if !main.is_empty() {
            out.push(format!("{} {main}", title.text));
        }
    }
    out
}

/// Where the two files of a song line up: `ytm time = own time + shift`. From the beat grids
/// when both have a steady beat (moved by whole beats to where the sound starts), else from
/// where the sound starts. `None` when the analyses don't allow a sure answer.
pub fn time_shift(ytm: &Features, own: &Features) -> Option<f64> {
    const MAX_SHIFT: f64 = 10.0;
    let start_gap = f64::from(ytm.audio_start - own.audio_start);
    if start_gap.abs() > MAX_SHIFT {
        return None;
    }
    if let (Some(a), Some(b)) = (&ytm.grid, &own.grid)
        && a.confidence >= 0.4
        && b.confidence >= 0.4
        && (a.bpm / b.bpm - 1.0).abs() < 0.015
        && !a.beats.is_empty()
        && !b.beats.is_empty()
    {
        let period = f64::from(60.0_f32 / a.bpm);
        // Which beat of `ytm` is the first of `own`, judged by where the sound starts.
        let first = f64::from(a.beats[0] - b.beats[0]);
        let skip = ((start_gap - first) / period).round() as i64;
        let mut offsets: Vec<f64> = (0..16usize)
            .filter_map(|k| {
                let j = usize::try_from(k as i64 + skip).ok()?;
                Some(f64::from(*a.beats.get(j)? - *b.beats.get(k)?))
            })
            .collect();
        if offsets.len() >= 4 {
            offsets.sort_by(f64::total_cmp);
            let shift = offsets[offsets.len() / 2];
            return (shift.abs() <= MAX_SHIFT).then_some(shift);
        }
    }
    (start_gap.abs() >= 0.05).then_some(start_gap)
}

#[cfg(test)]
mod tests;
