//! Spelling variants of a song's names. Different sources write the same song differently (YouTube Music
//! "Ditto (Official Audio)" by "NewJeans", a tag "Ditto" by "뉴진스 (NewJeans)", album "OMG - Single"), and
//! Last.fm counts each spelling as its own track. These helpers list the plausible spellings; the
//! resolver asks Last.fm which one is the most common (see [`crate::resolve`]).

/// Words that mark a bracketed part as a video/upload label rather than part of the title.
const UPLOAD_WORDS: &[&str] = &[
    "official",
    "mv",
    "m/v",
    "music video",
    "video",
    "audio",
    "lyric",
    "lyrics",
    "visualizer",
    "visualiser",
    "performance",
    "color coded",
    "colour coded",
    "4k",
    "hd",
    "hq",
    "teaser",
    "live clip",
    "special clip",
];

/// Album suffixes that only describe the release format.
const FORMAT_SUFFIXES: &[&str] =
    &[" - single", " - ep", " (single)", " (ep)", " [single]", " [ep]", " - the 1st single"];

fn has_hangul_or_kana(s: &str) -> bool {
    s.chars().any(|c| matches!(c as u32, 0xAC00..=0xD7A3 | 0x1100..=0x11FF | 0x3130..=0x318F | 0x3040..=0x30FF | 0x4E00..=0x9FFF))
}

/// Splits "A (B)" / "A [B]" into the outer text and every bracketed part, in order.
fn brackets(s: &str) -> (String, Vec<String>) {
    let mut outer = String::new();
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut cur = String::new();
    for c in s.chars() {
        match c {
            '(' | '[' | '（' | '【' => {
                if depth == 0 {
                    cur.clear();
                } else {
                    cur.push(c);
                }
                depth += 1;
            }
            ')' | ']' | '）' | '】' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    parts.push(cur.trim().to_string());
                } else {
                    cur.push(c);
                }
            }
            _ if depth > 0 => cur.push(c),
            _ => outer.push(c),
        }
    }
    (collapse(&outer), parts)
}

fn collapse(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches(|c: char| c == '-' || c == '|' || c.is_whitespace())
        .to_string()
}

fn is_upload_label(part: &str) -> bool {
    let p = part.to_lowercase();
    UPLOAD_WORDS.iter().any(|w| {
        p == *w
            || p.split(|c: char| !c.is_alphanumeric() && c != '/').any(|t| t == *w)
            || p.contains(w) && w.len() > 4
    })
}

fn is_feat(part: &str) -> bool {
    let p = part.to_lowercase();
    p.starts_with("feat") || p.starts_with("ft.") || p.starts_with("with ") || p.starts_with("prod")
}

/// The title without upload labels: "Ditto (Official Audio)" → "Ditto", "Hype Boy [MV]" → "Hype Boy".
/// Remix, version and feature credits stay.
pub fn clean_title(title: &str) -> String {
    let mut out = title.to_string();
    let (_, parts) = brackets(title);
    for p in parts.iter().filter(|p| is_upload_label(p)) {
        for (o, c) in [('(', ')'), ('[', ']'), ('（', '）'), ('【', '】')] {
            out = out.replace(&format!("{o}{p}{c}"), "");
        }
    }
    // "Artist - Title (Official Video)" style uploads: a trailing " | M/V" or " MV".
    let lower = out.to_lowercase();
    for tail in [" | mv", " | m/v", " m/v", " mv", " official mv"] {
        if lower.ends_with(tail) {
            out.truncate(out.len() - tail.len());
        }
    }
    let c = collapse(&out);
    if c.is_empty() { collapse(title) } else { c }
}

fn push(v: &mut Vec<String>, s: String) {
    let s = collapse(&s);
    if !s.is_empty() && !v.iter().any(|x| x.eq_ignore_ascii_case(&s)) {
        v.push(s);
    }
}

/// Plausible spellings of a title, the given one first.
pub fn title_variants(title: &str) -> Vec<String> {
    let mut v = Vec::new();
    push(&mut v, title.to_string());
    let clean = clean_title(title);
    push(&mut v, clean.clone());
    let (outer, parts) = brackets(&clean);
    // Without feature credits: "Song (feat. X)" → "Song".
    if parts.iter().any(|p| is_feat(p)) {
        let mut s = clean.clone();
        for p in parts.iter().filter(|p| is_feat(p)) {
            s = s.replace(&format!("({p})"), "").replace(&format!("[{p}]"), "");
        }
        push(&mut v, s);
    }
    // A translated title: "사랑 안해 (I Don't Love You)" → both halves.
    if has_hangul_or_kana(&outer) || parts.iter().any(|p| has_hangul_or_kana(p)) {
        push(&mut v, outer);
        for p in parts.iter().filter(|p| !is_feat(p)) {
            push(&mut v, p.clone());
        }
    }
    v.truncate(4);
    v
}

/// Plausible spellings of the artist: the first artist, the whole credit, and both halves of
/// "뉴진스 (NewJeans)".
pub fn artist_variants(first: &str, line: &str) -> Vec<String> {
    let mut v = Vec::new();
    push(&mut v, first.to_string());
    for s in [first, line] {
        let (outer, parts) = brackets(s);
        if !parts.is_empty() {
            push(&mut v, outer);
            for p in parts {
                push(&mut v, p);
            }
        }
    }
    push(&mut v, line.to_string());
    v.truncate(4);
    v
}

/// Plausible spellings of an album: as given, without " - Single" / " - EP", without upload labels.
pub fn album_variants(album: &str) -> Vec<String> {
    let mut v = Vec::new();
    push(&mut v, album.to_string());
    let lower = album.to_lowercase();
    for suf in FORMAT_SUFFIXES {
        if lower.ends_with(suf) {
            push(&mut v, album[..album.len() - suf.len()].to_string());
        }
    }
    push(&mut v, clean_title(album));
    let (outer, parts) = brackets(album);
    if has_hangul_or_kana(&outer) || parts.iter().any(|p| has_hangul_or_kana(p)) {
        push(&mut v, outer);
    }
    v.truncate(3);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_labels_go_credits_stay() {
        assert_eq!(clean_title("Ditto (Official Audio)"), "Ditto");
        assert_eq!(clean_title("Hype Boy [MV]"), "Hype Boy");
        assert_eq!(clean_title("Supernova (Official Music Video)"), "Supernova");
        assert_eq!(clean_title("Love Dive (Remix)"), "Love Dive (Remix)");
        assert_eq!(clean_title("Spicy (feat. X)"), "Spicy (feat. X)");
        // A title that is only a label stays as it is (never empty).
        assert_eq!(clean_title("(Official Audio)"), "(Official Audio)");
        assert_eq!(clean_title("Lemonade"), "Lemonade");
    }

    #[test]
    fn variants() {
        assert_eq!(title_variants("Ditto (Official Audio)"), ["Ditto (Official Audio)", "Ditto"]);
        assert_eq!(title_variants("Spicy (feat. X)"), ["Spicy (feat. X)", "Spicy"]);
        assert_eq!(
            title_variants("사랑 안해 (I Don't Love You)"),
            ["사랑 안해 (I Don't Love You)", "사랑 안해", "I Don't Love You"]
        );
        assert_eq!(
            artist_variants("뉴진스 (NewJeans)", "뉴진스 (NewJeans)"),
            ["뉴진스 (NewJeans)", "뉴진스", "NewJeans"]
        );
        assert_eq!(artist_variants("aespa", "aespa & X"), ["aespa", "aespa & X"]);
        assert_eq!(album_variants("OMG - Single"), ["OMG - Single", "OMG"]);
    }
}
