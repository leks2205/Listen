//! Export (Feishin's lyrics export form): `.lrc` for synchronized lyrics, `.txt` for plain.

use listen_core::model::Lyrics;
use std::time::Duration;

fn stamp(d: Duration, sep: char) -> String {
    let cs = d.as_millis() / 10;
    format!("{:02}:{:02}{sep}{:02}", cs / 6000, cs / 100 % 60, cs % 100)
}

/// LRC text; `offset_ms` goes into the `[offset:]` tag. Word timings are kept as
/// enhanced-LRC stamps and translations as a second line at the same timestamp.
pub fn to_lrc(lyrics: &Lyrics, name: &str, artist: &str, offset_ms: i32) -> String {
    let mut out = format!("[ar:{artist}]\n[ti:{name}]\n[offset:{offset_ms}]\n");
    for line in &lyrics.lines {
        let Some(start) = line.start else {
            out.push_str(&line.text);
            out.push('\n');
            continue;
        };
        out.push_str(&format!("[{}]", stamp(start, '.')));
        if line.words.is_empty() {
            out.push_str(&line.text);
        } else {
            for w in &line.words {
                out.push_str(&format!("<{}>{}", stamp(w.start, '.'), w.text));
            }
            if let Some(last) = line.words.last() {
                out.push_str(&format!("<{}>", stamp(last.end, '.')));
            }
        }
        out.push('\n');
        if let Some(t) = &line.translation {
            out.push_str(&format!("[{}]{t}\n", stamp(start, '.')));
        }
    }
    out
}

/// The lines as plain text.
pub fn to_txt(lyrics: &Lyrics) -> String {
    let mut out = lyrics.lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n");
    out.push('\n');
    out
}

/// "Artist - Title.lrc" with path separators and other trouble removed.
pub fn file_name(artist: &str, title: &str, synced: bool) -> String {
    let clean = |s: &str| -> String {
        s.chars()
            .map(|c| {
                if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() {
                    '_'
                } else {
                    c
                }
            })
            .collect::<String>()
            .trim()
            .trim_matches('.')
            .to_string()
    };
    let (artist, title) = (clean(artist), clean(title));
    let base = match (artist.is_empty(), title.is_empty()) {
        (_, true) => "lyrics".to_string(),
        (true, false) => title,
        (false, false) => format!("{artist} - {title}"),
    };
    format!("{base}.{}", if synced { "lrc" } else { "txt" })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_lrc;

    #[test]
    fn lrc_round_trips() {
        let src = "[00:01.50]Hello_BREAK_Hallo\n[01:02.00]<01:02.00>Wor<01:02.40>ld<01:03.00>";
        let l = parse_lrc(src);
        let out = to_lrc(&l, "Song", "Artist", 150);
        assert!(
            out.starts_with("[ar:Artist]\n[ti:Song]\n[offset:150]\n[00:01.50]Hello\n[00:01.50]Hallo\n"),
            "{out}"
        );
        assert!(out.contains("[01:02.00]<01:02.00>Wor<01:02.40>ld<01:03.00>\n"), "{out}");
        // Reading it back (offset aside) keeps lines, translation and words.
        let back = parse_lrc(&out.replace("[offset:150]", "[offset:0]"));
        assert_eq!(back.lines.len(), 2);
        assert_eq!(back.lines[0].translation.as_deref(), Some("Hallo"));
        assert_eq!(back.lines[1].words.len(), 2);
    }

    #[test]
    fn txt_is_the_plain_lines() {
        let l = parse_lrc("[00:01.00]a\n[00:02.00]b");
        assert_eq!(to_txt(&l), "a\nb\n");
    }

    #[test]
    fn unsynced_lines_are_written_bare() {
        let l = parse_lrc("one\ntwo");
        assert!(to_lrc(&l, "t", "a", 0).ends_with("one\ntwo\n"));
    }

    #[test]
    fn file_names_are_safe() {
        assert_eq!(file_name("AC/DC", "Back: In Black?", true), "AC_DC - Back_ In Black_.lrc");
        assert_eq!(file_name("", "Solo", false), "Solo.txt");
        assert_eq!(file_name("A", "", true), "lyrics.lrc");
        assert_eq!(file_name("..", "..", true), "lyrics.lrc");
    }
}
