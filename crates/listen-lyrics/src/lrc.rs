//! LRC parsing: plain text, `[mm:ss.xx]` lines (several stamps per line), enhanced word stamps
//! (`<mm:ss.xx>word`), `[offset:]`, NetEase's `[ms,dur]` karaoke format, and translations (NetEase's
//! `_BREAK_` merge, or a second line at the same timestamp).

use listen_core::model::{LyricLine, Lyrics, WordCue};
use std::time::Duration;

/// Separates the original and the translated text of a merged line (Feishin's NetEase merge).
pub const BREAK: &str = "_BREAK_";

/// A word whose end is unknown (the last one of a line without a following line).
const LAST_WORD: Duration = Duration::from_millis(2000);

struct RawLine {
    start: Option<i64>,
    text: String,
    /// (start ms, text); the end comes from the next word.
    words: Vec<(i64, String)>,
    /// End stamp of the last word, when the line has one (`<..>` after it).
    end: Option<i64>,
}

/// Parses LRC text. Lines without timestamps make the result unsynced.
pub fn parse_lrc(text: &str) -> Lyrics {
    let mut raw: Vec<RawLine> = Vec::new();
    let mut offset_ms: i64 = 0;
    let mut any_stamp = false;
    for line in text.lines() {
        let mut rest = line.trim();
        let mut stamps = Vec::new();
        while let Some(inner) = rest.strip_prefix('[') {
            let Some(end) = inner.find(']') else { break };
            let tag = &inner[..end];
            if let Some(off) = tag.strip_prefix("offset:") {
                offset_ms = off.trim().parse().unwrap_or(0);
            } else if let Some(t) = parse_stamp(tag) {
                stamps.push(t);
            }
            rest = inner[end + 1..].trim_start();
        }
        if stamps.is_empty() {
            if !rest.is_empty() && !line.trim_start().starts_with('[') {
                raw.push(RawLine { start: None, text: rest.to_string(), words: vec![], end: None });
            }
            continue;
        }
        any_stamp = true;
        let (text, words, end) = split_words(rest);
        for t in stamps {
            raw.push(RawLine { start: Some(t), text: text.clone(), words: words.clone(), end });
        }
    }
    if !any_stamp && let Some(lines) = parse_netease_karaoke(text) {
        return finish(lines, true, 0);
    }
    finish(raw, any_stamp, offset_ms)
}

fn finish(mut raw: Vec<RawLine>, synced: bool, offset_ms: i64) -> Lyrics {
    if synced {
        raw.retain(|l| l.start.is_some());
        raw.sort_by_key(|l| l.start);
    }
    let shift = |ms: i64| Duration::from_millis((ms - offset_ms).max(0) as u64);
    let mut lines: Vec<LyricLine> = raw
        .iter()
        .map(|l| {
            let mut line = LyricLine::new(l.start.map(shift), l.text.clone());
            if let Some((a, b)) = l.text.split_once(BREAK) {
                line.text = a.trim().to_string();
                line.translation = Some(b.trim().to_string()).filter(|t| !t.is_empty());
            }
            line.words = l
                .words
                .iter()
                .enumerate()
                .map(|(i, (start, text))| {
                    let end = match l.words.get(i + 1) {
                        Some((next, _)) => shift(*next),
                        None => l.end.map(shift).unwrap_or(shift(*start) + LAST_WORD),
                    };
                    WordCue { start: shift(*start), end: end.max(shift(*start)), text: text.clone() }
                })
                .collect();
            line
        })
        .collect();
    if synced {
        // The last word of a line ends at the latest when the next line starts.
        for i in 0..lines.len().saturating_sub(1) {
            let next = lines[i + 1].start;
            if let (Some(next), Some(w)) = (next, lines[i].words.last_mut())
                && w.end > next
                && next >= w.start
            {
                w.end = next;
            }
        }
        merge_same_stamp_translations(&mut lines);
    }
    Lyrics { provider: "lrc".into(), synced, lines, matched: None }
}

/// Makes the second of two lines with the same timestamp and different text the first's translation.
fn merge_same_stamp_translations(lines: &mut Vec<LyricLine>) {
    let mut out: Vec<LyricLine> = Vec::with_capacity(lines.len());
    for line in lines.drain(..) {
        if let Some(prev) = out.last_mut()
            && prev.start.is_some()
            && prev.start == line.start
            && prev.translation.is_none()
            && !prev.text.is_empty()
            && !line.text.is_empty()
            && prev.text != line.text
            && line.translation.is_none()
        {
            prev.translation = Some(line.text);
            continue;
        }
        out.push(line);
    }
    *lines = out;
}

/// `mm:ss.xx` (also `mm:ss,xx` and `mm:ss`) in milliseconds.
fn parse_stamp(tag: &str) -> Option<i64> {
    let (m, s) = tag.split_once(':')?;
    let minutes: i64 = m.trim().parse().ok()?;
    let seconds: f64 = s.trim().replace(',', ".").parse().ok()?;
    if !(0.0..1000.0).contains(&seconds) {
        return None;
    }
    Some(minutes * 60_000 + (seconds * 1000.0).round() as i64)
}

/// Splits `text <t1>a <t2>b <t3>` into the plain text, the timed words and the end
/// stamp of the last word (a trailing `<t>` without text).
fn split_words(rest: &str) -> (String, Vec<(i64, String)>, Option<i64>) {
    enum Piece {
        Stamp(i64),
        Text(String),
    }
    if !rest.contains('<') {
        return (rest.to_string(), vec![], None);
    }
    let mut pieces: Vec<Piece> = Vec::new();
    let mut buf = String::new();
    let mut s = rest;
    while let Some(i) = s.find('<') {
        let (before, after) = s.split_at(i);
        buf.push_str(before);
        match after.find('>').and_then(|j| parse_stamp(&after[1..j]).map(|t| (t, j))) {
            Some((t, j)) => {
                if !buf.is_empty() {
                    pieces.push(Piece::Text(std::mem::take(&mut buf)));
                }
                pieces.push(Piece::Stamp(t));
                s = &after[j + 1..];
            }
            None => {
                buf.push('<');
                s = &after[1..];
            }
        }
    }
    buf.push_str(s);
    if !buf.is_empty() {
        pieces.push(Piece::Text(buf));
    }
    let mut prefix = String::new();
    let mut words: Vec<(i64, String)> = Vec::new();
    let mut cur: Option<i64> = None;
    for piece in pieces {
        match piece {
            // Two stamps in a row: the first one only closed the previous word.
            Piece::Stamp(t) => cur = Some(t),
            Piece::Text(text) => match cur.take() {
                Some(t) if !text.trim().is_empty() => words.push((t, text)),
                Some(_) => {
                    // Whitespace only: it belongs to the previous word.
                    if let Some(last) = words.last_mut() {
                        last.1.push_str(&text);
                    }
                }
                None if words.is_empty() => prefix.push_str(&text),
                None => {
                    if let Some(last) = words.last_mut() {
                        last.1.push_str(&text);
                    }
                }
            },
        }
    }
    if words.is_empty() {
        return (strip_stamps(rest), vec![], None);
    }
    // A dangling stamp after the last word is its end.
    let end = cur;
    let text: String = format!("{prefix}{}", words.iter().map(|(_, w)| w.as_str()).collect::<String>());
    if let Some(last) = words.last_mut() {
        last.1 = last.1.trim_end().to_string();
    }
    (text.trim().to_string(), words, end)
}

/// `s` without its `<mm:ss.xx>` stamps.
fn strip_stamps(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('<') {
        let tail = &rest[i..];
        match tail.find('>').filter(|j| parse_stamp(&tail[1..*j]).is_some()) {
            Some(j) => {
                out.push_str(&rest[..i]);
                rest = &tail[j + 1..];
            }
            None => {
                out.push_str(&rest[..=i]);
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out.trim().to_string()
}

/// NetEase's `[start_ms,duration_ms]text(start,duration)...` karaoke lines.
fn parse_netease_karaoke(text: &str) -> Option<Vec<RawLine>> {
    let mut out = Vec::new();
    for line in text.lines() {
        let Some(inner) = line.trim().strip_prefix('[') else { continue };
        let Some(end) = inner.find(']') else { continue };
        let Some((start, dur)) = inner[..end].split_once(',') else { continue };
        let (Ok(start), Ok(_)) = (start.trim().parse::<i64>(), dur.trim().parse::<i64>()) else { continue };
        let body = strip_pairs(&inner[end + 1..]);
        if body.trim().is_empty() {
            continue;
        }
        out.push(RawLine { start: Some(start), text: body, words: vec![], end: None });
    }
    (!out.is_empty()).then_some(out)
}

/// Removes `(123,456)` pairs and the spaces they leave before punctuation.
fn strip_pairs(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('(') {
        let tail = &rest[i + 1..];
        match tail.find(')') {
            Some(j) if tail[..j].split_once(',').is_some_and(|(a, b)| is_num(a) && is_num(b)) => {
                out.push_str(&rest[..i]);
                rest = &tail[j + 1..];
            }
            _ => {
                out.push_str(&rest[..=i]);
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out.replace(" ,", ",").replace(" .", ".").trim().to_string()
}

fn is_num(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// Index of the line being sung at `position` (with a lead time so the
/// highlight arrives slightly early, Feishin's `lineLeadTimeMs`).
pub fn active_line(lyrics: &Lyrics, position: Duration, lead: Duration) -> Option<usize> {
    if !lyrics.synced {
        return None;
    }
    let t = position + lead;
    lyrics.lines.iter().rposition(|l| l.start.is_some_and(|s| s <= t))
}

/// Whether any line carries word timings (the karaoke view applies).
pub fn has_word_sync(lyrics: &Lyrics) -> bool {
    lyrics.synced && lyrics.lines.iter().any(|l| !l.words.is_empty())
}

/// Whether any line has a translation.
pub fn has_translation(lyrics: &Lyrics) -> bool {
    lyrics.lines.iter().any(|l| l.translation.is_some())
}

/// Merges NetEase's original and translated LRC the way Feishin does: a translated
/// line with the same timestamp follows the original, joined by `_BREAK_`.
pub fn merge_translation(original: &str, translated: &str) -> String {
    let mut map = std::collections::HashMap::new();
    for line in translated.lines() {
        if let Some((stamp, text)) = split_stamp_line(line)
            && !text.trim().is_empty()
        {
            map.insert(stamp.to_string(), text.trim().to_string());
        }
    }
    if map.is_empty() {
        return original.to_string();
    }
    original
        .lines()
        .map(|line| match split_stamp_line(line) {
            Some((stamp, text)) if !text.trim().is_empty() => match map.get(stamp) {
                Some(tr) => format!("[{stamp}]{}{BREAK}{tr}", text.trim()),
                None => line.to_string(),
            },
            _ => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `[mm:ss.xx]text` -> (`mm:ss.xx`, `text`).
fn split_stamp_line(line: &str) -> Option<(&str, &str)> {
    let inner = line.strip_prefix('[')?;
    let end = inner.find(']')?;
    let stamp = &inner[..end];
    parse_stamp(stamp)?;
    Some((stamp, &inner[end + 1..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn parses_synced_lrc() {
        let l = parse_lrc("[ar: Someone]\n[00:12.50]First line\n[00:15.00][01:02.00]Chorus\n\n[00:20]Last");
        assert!(l.synced);
        let texts: Vec<&str> = l.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["First line", "Chorus", "Last", "Chorus"]);
        assert_eq!(l.lines[0].start, Some(ms(12_500)));
        assert_eq!(l.lines[3].start, Some(Duration::from_secs(62)));
    }

    #[test]
    fn plain_text_is_unsynced() {
        let l = parse_lrc("Line one\nLine two");
        assert!(!l.synced);
        assert_eq!(l.lines.len(), 2);
        assert_eq!(active_line(&l, Duration::from_secs(5), Duration::ZERO), None);
    }

    #[test]
    fn offset_and_active_line() {
        let l = parse_lrc("[offset:500]\n[00:01.00]a\n[00:03.00]b\n[00:05.00]c");
        assert_eq!(l.lines[0].start, Some(ms(500)));
        let lead = ms(800);
        assert_eq!(active_line(&l, ms(0), lead), Some(0));
        assert_eq!(active_line(&l, ms(1800), lead), Some(1));
        assert_eq!(active_line(&l, Duration::from_secs(60), lead), Some(2));
        let early = parse_lrc("[00:10.00]x");
        assert_eq!(active_line(&early, Duration::from_secs(1), lead), None);
    }

    #[test]
    fn offset_tag_after_the_lines_still_counts() {
        let l = parse_lrc("[00:02.00]a\n[offset:1000]");
        assert_eq!(l.lines[0].start, Some(ms(1000)));
    }

    #[test]
    fn offset_never_goes_negative() {
        let l = parse_lrc("[offset:5000]\n[00:01.00]a");
        assert_eq!(l.lines[0].start, Some(Duration::ZERO));
    }

    #[test]
    fn enhanced_lrc_word_stamps() {
        let l =
            parse_lrc("[00:10.00]<00:10.00>Hello <00:10.50>big <00:11.00>world<00:11.80>\n[00:13.00]next");
        assert!(l.synced);
        assert!(has_word_sync(&l));
        let first = &l.lines[0];
        assert_eq!(first.text, "Hello big world");
        let w: Vec<(&str, u64, u64)> = first
            .words
            .iter()
            .map(|w| (w.text.as_str(), w.start.as_millis() as u64, w.end.as_millis() as u64))
            .collect();
        assert_eq!(w, [("Hello ", 10_000, 10_500), ("big ", 10_500, 11_000), ("world", 11_000, 11_800)]);
        assert!(l.lines[1].words.is_empty());
    }

    #[test]
    fn last_word_ends_at_the_next_line() {
        let l = parse_lrc("[00:10.00]<00:10.00>a <00:10.50>b\n[00:11.00]<00:11.00>c");
        assert_eq!(l.lines[0].words[1].end, ms(11_000));
        // No following line: a fixed tail.
        assert_eq!(l.lines[1].words[0].end, ms(13_000));
    }

    #[test]
    fn word_stamps_respect_offset() {
        let l = parse_lrc("[offset:500]\n[00:10.00]<00:10.00>a <00:11.00>b");
        assert_eq!(l.lines[0].words[0].start, ms(9_500));
        assert_eq!(l.lines[0].words[1].start, ms(10_500));
    }

    #[test]
    fn angle_brackets_that_are_not_stamps_stay_text() {
        let l = parse_lrc("[00:01.00]a <3 b");
        assert_eq!(l.lines[0].text, "a <3 b");
        assert!(l.lines[0].words.is_empty());
    }

    #[test]
    fn netease_break_translation() {
        let l = parse_lrc("[00:01.00]Hello_BREAK_Hallo\n[00:02.00]Bye");
        assert_eq!(l.lines[0].text, "Hello");
        assert_eq!(l.lines[0].translation.as_deref(), Some("Hallo"));
        assert_eq!(l.lines[1].translation, None);
        assert!(has_translation(&l));
    }

    #[test]
    fn merge_translation_like_feishin() {
        let merged =
            merge_translation("[00:01.00]Hello\n[00:02.00]Bye\n[00:03.00]", "[00:01.00]Hallo\n[00:09.00]x");
        assert_eq!(merged, "[00:01.00]Hello_BREAK_Hallo\n[00:02.00]Bye\n[00:03.00]");
        assert_eq!(merge_translation("[00:01.00]a", ""), "[00:01.00]a");
        let l = parse_lrc(&merged);
        assert_eq!(l.lines[0].translation.as_deref(), Some("Hallo"));
    }

    #[test]
    fn same_timestamp_second_line_is_a_translation() {
        let l = parse_lrc("[00:01.00]Hello\n[00:01.00]Hallo\n[00:02.00]Same\n[00:02.00]Same");
        assert_eq!(l.lines.len(), 3);
        assert_eq!(l.lines[0].translation.as_deref(), Some("Hallo"));
        // A repeated identical line is just a repeat.
        assert_eq!(l.lines[1].translation, None);
    }

    #[test]
    fn netease_karaoke_format() {
        let l = parse_lrc("[1500,2000]Hel(1500,300)lo (1800,300)world(2100,100) ,\n[4000,1000]Bye");
        assert!(l.synced);
        assert_eq!(l.lines[0].start, Some(ms(1500)));
        assert_eq!(l.lines[0].text, "Hello world,");
        assert_eq!(l.lines[1].start, Some(ms(4000)));
    }

    #[test]
    fn empty_stamped_line_is_a_break() {
        let l = parse_lrc("[00:01.00]a\n[00:05.00]\n[00:06.00]b");
        assert_eq!(l.lines.len(), 3);
        assert_eq!(l.lines[1].text, "");
    }
}
