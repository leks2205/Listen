//! Furigana for Japanese lyrics (Feishin's "furigana" option): readings over the kanji, from the
//! IPADIC dictionary of the `furigana` cargo feature (lindera). Without it [`segments`] returns
//! `None`. Text without kana (Chinese) is left alone.

use crate::romaji::has_kana;

/// A piece of a line: the text and, for kanji, its reading in hiragana.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub text: String,
    pub ruby: Option<String>,
}

/// Whether this build can produce furigana.
pub const AVAILABLE: bool = cfg!(feature = "furigana");

/// Kanji (and the iteration marks that read like them).
pub fn is_kanji(c: char) -> bool {
    matches!(c as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F)
        || matches!(c, '々' | '〆')
}

fn to_hira(c: char) -> char {
    match c as u32 {
        0x30A1..=0x30F6 => char::from_u32(c as u32 - 0x60).unwrap_or(c),
        _ => c,
    }
}

pub fn katakana_to_hiragana(s: &str) -> String {
    s.chars().map(to_hira).collect()
}

/// Splits a token (`surface` with its `reading`) into kana and kanji with their part of the
/// reading; okurigana get no ruby (食べる / たべる gives 食 = た, べる).
pub fn align(surface: &str, reading: &str) -> Vec<Segment> {
    let plain = |t: &str| Segment { text: t.to_string(), ruby: None };
    if !surface.chars().any(is_kanji) {
        return vec![plain(surface)];
    }
    let reading = katakana_to_hiragana(reading);
    let s: Vec<char> = surface.chars().collect();
    let r: Vec<char> = reading.chars().collect();
    // Common kana at the end, then at the start.
    let mut tail = 0;
    while tail < s.len() - 1
        && tail < r.len().saturating_sub(1)
        && !is_kanji(s[s.len() - 1 - tail])
        && to_hira(s[s.len() - 1 - tail]) == r[r.len() - 1 - tail]
    {
        tail += 1;
    }
    let mut head = 0;
    while head + tail < s.len() - 1
        && head + tail < r.len().saturating_sub(1)
        && !is_kanji(s[head])
        && to_hira(s[head]) == r[head]
    {
        head += 1;
    }
    let mid_s: String = s[head..s.len() - tail].iter().collect();
    let mid_r: String = r[head..r.len() - tail].iter().collect();
    if mid_r.is_empty() || mid_r == mid_s {
        return vec![plain(surface)];
    }
    let mut out = Vec::new();
    if head > 0 {
        out.push(plain(&s[..head].iter().collect::<String>()));
    }
    out.push(Segment { text: mid_s, ruby: Some(mid_r) });
    if tail > 0 {
        out.push(plain(&s[s.len() - tail..].iter().collect::<String>()));
    }
    out
}

#[cfg(feature = "furigana")]
mod dict {
    use lindera::dictionary::load_dictionary;
    use lindera::mode::Mode;
    use lindera::segmenter::Segmenter;
    use lindera::tokenizer::Tokenizer;
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    struct Engine {
        tokenizer: Option<Tokenizer>,
        cache: HashMap<String, Vec<(String, Option<String>)>>,
    }

    fn engine() -> &'static Mutex<Engine> {
        static E: OnceLock<Mutex<Engine>> = OnceLock::new();
        E.get_or_init(|| {
            let tokenizer = load_dictionary("embedded://ipadic")
                .map(|d| Tokenizer::new(Segmenter::new(Mode::Normal, d, None)))
                .map_err(|e| tracing::warn!("furigana dictionary: {e}"))
                .ok();
            Mutex::new(Engine { tokenizer, cache: HashMap::new() })
        })
    }

    /// The tokens of `text` with their readings (katakana), cached per line.
    pub fn tokens(text: &str) -> Vec<(String, Option<String>)> {
        let mut e = engine().lock().unwrap_or_else(|p| p.into_inner());
        if let Some(hit) = e.cache.get(text) {
            return hit.clone();
        }
        let Some(tok) = e.tokenizer.as_ref() else { return Vec::new() };
        let mut out = Vec::new();
        if let Ok(mut list) = tok.tokenize(text) {
            for t in list.iter_mut() {
                let surface = t.surface.to_string();
                // IPADIC details: ..., base form, reading, pronunciation.
                let reading = t.details().get(7).filter(|r| **r != "*").map(|r| (*r).to_string());
                out.push((surface, reading));
            }
        }
        if e.cache.len() > 4096 {
            e.cache.clear();
        }
        e.cache.insert(text.to_string(), out.clone());
        out
    }
}

/// Loads the dictionary on a background thread so the first line doesn't stall the UI.
pub fn warm_up() {
    #[cfg(feature = "furigana")]
    {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            std::thread::spawn(|| {
                let _ = dict::tokens("あ");
            });
        });
    }
}

/// The line as segments with readings over the kanji; `None` when the line
/// has no kana (not Japanese), no kanji, or this build has no dictionary.
pub fn segments(text: &str) -> Option<Vec<Segment>> {
    if !has_kana(text) || !text.chars().any(is_kanji) {
        return None;
    }
    #[cfg(feature = "furigana")]
    {
        let mut out: Vec<Segment> = Vec::new();
        for (surface, reading) in dict::tokens(text) {
            match reading {
                Some(r) if surface.chars().any(is_kanji) => out.extend(align(&surface, &r)),
                _ => out.push(Segment { text: surface, ruby: None }),
            }
        }
        (out.iter().any(|s| s.ruby.is_some())).then_some(out)
    }
    #[cfg(not(feature = "furigana"))]
    None
}

/// The whole line read out in hiragana (kanji replaced by their reading), for
/// romanizing kanji; `None` without a dictionary.
pub fn reading(text: &str) -> Option<String> {
    #[cfg(feature = "furigana")]
    {
        let mut out = String::new();
        for (surface, reading) in dict::tokens(text) {
            match reading {
                Some(r) if surface.chars().any(is_kanji) => out.push_str(&katakana_to_hiragana(&r)),
                _ => out.push_str(&surface),
            }
        }
        (!out.is_empty()).then_some(out)
    }
    #[cfg(not(feature = "furigana"))]
    {
        let _ = text;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(t: &str, r: Option<&str>) -> Segment {
        Segment { text: t.into(), ruby: r.map(String::from) }
    }

    #[test]
    fn okurigana_stay_without_ruby() {
        assert_eq!(align("食べる", "タベル"), [seg("食", Some("た")), seg("べる", None)]);
        assert_eq!(align("お願い", "オネガイ"), [seg("お", None), seg("願", Some("ねが")), seg("い", None)]);
        assert_eq!(align("今日", "キョウ"), [seg("今日", Some("きょう"))]);
        assert_eq!(align("は", "ハ"), [seg("は", None)]);
    }

    #[test]
    fn non_japanese_lines_get_nothing() {
        assert_eq!(segments("hello world"), None);
        // Chinese: kanji but no kana.
        assert_eq!(segments("今天天气很好"), None);
    }

    #[cfg(feature = "furigana")]
    #[test]
    fn lines_get_their_readings() {
        let s = segments("今日は天気がいいですね").unwrap();
        assert!(s.contains(&seg("今日", Some("きょう"))));
        assert!(s.contains(&seg("天気", Some("てんき"))));
        let joined: String = s.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(joined, "今日は天気がいいですね");
        assert_eq!(reading("今日は").as_deref(), Some("きょうは"));
    }
}
