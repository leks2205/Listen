//! Synced lyrics as a vocal map for DJ transitions: where the singing is, where lines end, and where
//! the chorus (the most repeated block of lines) comes back.

use listen_core::analysis::VocalMap;
use listen_core::model::Lyrics;

/// Fewest sung, timed lines for a usable map.
const MIN_LINES: usize = 4;
/// Lines closer than this are one sung stretch.
const MERGE_GAP_S: f32 = 1.0;
/// Seconds per syllable (Hangul, kana, Han) or word (other scripts) when a line's end is guessed.
const UNIT_S: f32 = 0.3;
/// Shortest and longest guessed line.
const MIN_LINE_S: f32 = 1.2;
const MAX_LINE_S: f32 = 8.0;
/// Shortest chorus (seconds of one occurrence).
const MIN_CHORUS_S: f32 = 8.0;

/// One timed, sung line.
struct Line {
    start: f32,
    /// Known end (word timings), if any.
    end: Option<f32>,
    text: String,
}

/// The vocal map of synced `lyrics` for a song of `duration` seconds, or `None` when the lyrics are
/// unsynced, too short, or run past the end (another version of the song).
pub fn vocal_map(lyrics: &Lyrics, duration: f32) -> Option<VocalMap> {
    if !lyrics.synced {
        return None;
    }
    // Every timed line marks the end of the one before it, sung or not.
    let mut stamps: Vec<(f32, Option<Line>)> = Vec::new();
    for l in &lyrics.lines {
        let Some(start) = l.start else { continue };
        let start = start.as_secs_f32();
        let sung = sung_text(&l.text).map(|text| Line {
            start,
            end: l.words.iter().map(|w| w.end.as_secs_f32()).reduce(f32::max).filter(|&e| e > start),
            text,
        });
        stamps.push((start, sung));
    }
    stamps.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut lines: Vec<(Line, f32)> = Vec::new(); // line and its end
    for (k, (start, line)) in stamps.iter().enumerate() {
        let Some(line) = line else { continue };
        let next = stamps[k + 1..].iter().map(|s| s.0).find(|&t| t > *start).unwrap_or(duration);
        let end = match line.end {
            Some(e) => e.min(next.max(*start + 0.1)),
            None => guess_end(*start, next, &line.text),
        };
        lines.push((Line { start: *start, end: line.end, text: line.text.clone() }, end));
    }
    if lines.len() < MIN_LINES || lines.iter().any(|(l, _)| l.start > duration + 2.0) {
        return None;
    }

    let mut sung: Vec<(f32, f32)> = Vec::new();
    for (l, end) in &lines {
        match sung.last_mut() {
            Some(last) if l.start - last.1 < MERGE_GAP_S => last.1 = last.1.max(*end),
            _ => sung.push((l.start, *end)),
        }
    }
    let line_ends = lines.iter().map(|(_, e)| *e).collect();
    Some(VocalMap { sung, line_ends, chorus: chorus(&lines) })
}

/// A line's end without word timings: right up to the next line when they follow closely, else
/// a length guessed from the syllables.
fn guess_end(start: f32, next: f32, text: &str) -> f32 {
    let gap = (next - start).max(0.1);
    let est = (UNIT_S * units(text) as f32 + 0.6).clamp(MIN_LINE_S, MAX_LINE_S);
    if gap <= est * 1.5 + 1.0 { next } else { start + est }
}

/// Syllables (Hangul, kana, Han) plus words in other scripts.
fn units(text: &str) -> usize {
    let mut n = 0;
    let mut in_word = false;
    for c in text.chars() {
        if is_syllabic(c) {
            n += 1;
            in_word = false;
        } else if c.is_alphanumeric() {
            if !in_word {
                n += 1;
            }
            in_word = true;
        } else {
            in_word = false;
        }
    }
    n
}

/// One sung syllable per character: Hangul syllables U+AC00-D7A3, kana U+3040-30FF, Han U+4E00-9FFF and
/// U+3400-4DBF; no jamo (parts of a syllable) and no rare Han planes (too rare to change a timing).
/// Its siblings have other ranges on purpose (pick by range, do not merge):
/// `listen_lyrics::gemini::is_hangul`, `listen_lyrics::furigana::is_kanji`, `listen_lastfm::names::has_cjk`,
/// `listen_core::substitute::script_of`, the app's `ui::is_hangul`.
fn is_syllabic(c: char) -> bool {
    matches!(c as u32, 0xAC00..=0xD7A3 | 0x3040..=0x30FF | 0x4E00..=0x9FFF | 0x3400..=0x4DBF)
}

/// The text of a sung line, or `None` for empty lines, credits ("作词 : …", "Composed by: …"),
/// site notes and lines with no letters (♪).
fn sung_text(text: &str) -> Option<String> {
    let t = text.trim();
    if t.is_empty() || !t.chars().any(char::is_alphanumeric) {
        return None;
    }
    let lower = t.to_lowercase();
    if t.starts_with('{') || ["http", "www.", ".com", "lyricsify"].iter().any(|w| lower.contains(w)) {
        return None;
    }
    if let Some(i) = t.find([':', '：']) {
        let key = t[..i].trim().to_lowercase();
        const CJK: [&str; 14] = [
            "作词", "作曲", "编曲", "制作", "混音", "母带", "和声", "录音", "监制", "出品", "词", "曲",
            "吉他", "贝斯",
        ];
        const LATIN: [&str; 16] = [
            "artist", "title", "album", "by", "lyrics", "lyricist", "composer", "composed", "arranged",
            "arranger", "producer", "produced", "written", "mixed", "mastered", "music",
        ];
        let latin = key.split_whitespace().count() <= 3 && key.split_whitespace().any(|w| LATIN.contains(&w));
        if CJK.iter().any(|w| key.contains(w)) || latin {
            return None;
        }
    }
    Some(t.to_string())
}

/// Lower-case letters and digits only, without parenthesised ad-libs ("(neon)"). Names without that
/// rule fold with `listen_core::text::fold`.
fn normalise(text: &str) -> String {
    let mut depth = 0i32;
    let mut out = String::new();
    for c in text.chars() {
        match c {
            '(' | '（' | '[' => depth += 1,
            ')' | '）' | ']' => depth = (depth - 1).max(0),
            _ if depth == 0 && c.is_alphanumeric() => out.extend(c.to_lowercase()),
            _ => {}
        }
    }
    out
}

/// Occurrences of the block of ≥ 2 consecutive lines that repeats and covers the most lines.
fn chorus(lines: &[(Line, f32)]) -> Vec<(f32, f32)> {
    let keys: Vec<String> = lines.iter().map(|(l, _)| normalise(&l.text)).collect();
    let n = keys.len();
    // run[i][j]: how many lines match from i and j on.
    let mut run = vec![vec![0usize; n + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (i + 1..n).rev() {
            if !keys[i].is_empty() && keys[i] == keys[j] {
                run[i][j] = run[i + 1][j + 1] + 1;
            }
        }
    }
    let occurrences = |i: usize, len: usize| {
        let mut at = Vec::new();
        let mut k = 0;
        while k + len <= n {
            if (0..len).all(|d| keys[k + d] == keys[i + d]) {
                at.push(k);
                k += len;
            } else {
                k += 1;
            }
        }
        at
    };
    let mut best: Option<(usize, Vec<usize>, usize)> = None; // (covered lines, starts, len)
    for (i, row) in run.iter().enumerate().take(n) {
        for (j, &matched) in row.iter().enumerate().take(n).skip(i + 1) {
            let len = matched.min(j - i);
            if len < 2 {
                continue;
            }
            let at = occurrences(i, len);
            let dur = lines[i + len - 1].1 - lines[i].0.start;
            if at.len() < 2 || dur < MIN_CHORUS_S {
                continue;
            }
            let covered = at.len() * len;
            if best.as_ref().is_none_or(|b| covered > b.0 || (covered == b.0 && len > b.2)) {
                best = Some((covered, at, len));
            }
        }
    }
    best.map(|(_, at, len)| at.iter().map(|&k| (lines[k].0.start, lines[k + len - 1].1)).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use listen_core::model::{LyricLine, WordCue};
    use std::time::Duration;

    fn lyrics(lines: &[(f32, &str)]) -> Lyrics {
        Lyrics {
            provider: "test".into(),
            synced: true,
            lines: lines.iter().map(|&(t, s)| LyricLine::new(Some(Duration::from_secs_f32(t)), s)).collect(),
            matched: None,
        }
    }

    #[test]
    fn unsynced_or_short_lyrics_give_no_map() {
        let mut l = lyrics(&[(10.0, "a b"), (12.0, "c d"), (14.0, "e f"), (16.0, "g h")]);
        assert!(vocal_map(&l, 200.0).is_some());
        l.synced = false;
        assert!(vocal_map(&l, 200.0).is_none());
        let short = lyrics(&[(10.0, "a b"), (12.0, "c d"), (14.0, "")]);
        assert!(vocal_map(&short, 200.0).is_none());
        // Lines far past the end: another version.
        let long = lyrics(&[(10.0, "a"), (12.0, "b"), (14.0, "c"), (250.0, "d")]);
        assert!(vocal_map(&long, 200.0).is_none());
    }

    #[test]
    fn credits_notes_and_empty_lines_are_not_sung() {
        let l = lyrics(&[
            (0.0, "作词 : 김영아"),
            (0.5, "作曲 : Someone / Other"),
            (1.0, "Composed by: A. Writer"),
            (20.0, "Hello there my friend"),
            (22.0, "How are you today"),
            (24.0, "Fine thank you"),
            (26.0, "Good to hear"),
            (28.0, ""),
            (60.0, "Back again we sing"),
            (195.0, "{more lyrics at lyricsify.com}"),
        ]);
        let m = vocal_map(&l, 200.0).unwrap();
        assert_eq!(m.sung.len(), 2, "{:?}", m.sung);
        assert_eq!(m.sung[0], (20.0, 28.0));
        assert!((m.sung[1].0 - 60.0).abs() < 1e-3);
        // "Back again we sing": 4 words → 1.8 s.
        assert!((m.sung[1].1 - 61.8).abs() < 1e-3, "{:?}", m.sung[1]);
        assert_eq!(m.line_ends.len(), 5);
        assert!((m.sung_within(0.0, 30.0) - 8.0).abs() < 1e-3);
    }

    #[test]
    fn guessed_ends_follow_the_syllables() {
        // Close lines run on; a long gap after a short line ends it early.
        assert_eq!(guess_end(10.0, 13.0, "one two three"), 13.0);
        let e = guess_end(10.0, 30.0, "사랑해 사랑해");
        assert!((e - (10.0 + 0.3 * 6.0 + 0.6)).abs() < 1e-4, "{e}");
        assert_eq!(guess_end(10.0, 40.0, "oh"), 10.0 + MIN_LINE_S);
        assert_eq!(units("Hello, 사랑 world!"), 4);
    }

    #[test]
    fn word_timings_give_the_end() {
        let mut l = lyrics(&[(10.0, "a b"), (20.0, "c d"), (30.0, "e f"), (40.0, "g h")]);
        l.lines[0].words = vec![
            WordCue { start: Duration::from_secs(10), end: Duration::from_millis(10_500), text: "a".into() },
            WordCue {
                start: Duration::from_millis(10_500),
                end: Duration::from_millis(11_200),
                text: "b".into(),
            },
        ];
        let m = vocal_map(&l, 200.0).unwrap();
        assert!((m.line_ends[0] - 11.2).abs() < 1e-4);
        assert_eq!(m.sung[0], (10.0, 11.2));
    }

    #[test]
    fn the_most_repeated_block_is_the_chorus() {
        let verse = ["walking down the road", "under the grey sky", "nothing left to say", "only the night"];
        let chorus =
            ["we are neon and beyond", "see the sparkles (neon)", "flashing neon yeah", "byeol gata"];
        let mut lines = Vec::new();
        let mut t = 10.0;
        let mut push = |text: &str, lines: &mut Vec<(f32, String)>| {
            lines.push((t, text.to_string()));
            t += 3.5;
        };
        for v in verse {
            push(v, &mut lines);
        }
        for c in chorus {
            push(c, &mut lines);
        }
        for v in ["second verse here", "other words now", "and some more", "before the hook"] {
            push(v, &mut lines);
        }
        for c in chorus {
            // Ad-libs differ between occurrences.
            push(&c.replace("(neon)", "(yeah)"), &mut lines);
        }
        push("bridge line", &mut lines);
        for c in chorus {
            push(c, &mut lines);
        }
        let refs: Vec<(f32, &str)> = lines.iter().map(|(t, s)| (*t, s.as_str())).collect();
        let m = vocal_map(&lyrics(&refs), 200.0).unwrap();
        assert_eq!(m.chorus.len(), 3, "{:?}", m.chorus);
        assert!((m.chorus[0].0 - 24.0).abs() < 1e-3);
        assert!((m.chorus[1].0 - 52.0).abs() < 1e-3);
        assert!((m.chorus[2].0 - 69.5).abs() < 1e-3);
        // Each occurrence spans four lines.
        assert!(m.chorus.iter().all(|c| c.1 - c.0 > 11.0), "{:?}", m.chorus);
    }

    #[test]
    fn no_repeats_no_chorus() {
        let l = lyrics(&[(10.0, "a b"), (12.0, "c d"), (14.0, "e f"), (16.0, "g h"), (18.0, "a b")]);
        assert!(vocal_map(&l, 200.0).unwrap().chorus.is_empty());
    }
}
