//! Romaji for Japanese lyrics (Feishin's "romaji" option): kana to modified Hepburn, without a
//! dictionary (unlike Feishin's Kuroshiro), so kanji stay as they are.

/// Whether `text` has any kana.
pub fn has_kana(text: &str) -> bool {
    text.chars().any(is_kana)
}

fn is_kana(c: char) -> bool {
    matches!(c as u32, 0x3041..=0x3096 | 0x30A1..=0x30FA | 0x31F0..=0x31FF | 0xFF66..=0xFF9D)
}

/// Katakana to hiragana (so the table only has one script).
fn to_hira(c: char) -> char {
    match c as u32 {
        0x30A1..=0x30F6 => char::from_u32(c as u32 - 0x60).unwrap_or(c),
        _ => c,
    }
}

fn base(c: char) -> Option<&'static str> {
    Some(match c {
        'あ' => "a",
        'い' => "i",
        'う' => "u",
        'え' => "e",
        'お' => "o",
        'か' => "ka",
        'き' => "ki",
        'く' => "ku",
        'け' => "ke",
        'こ' => "ko",
        'が' => "ga",
        'ぎ' => "gi",
        'ぐ' => "gu",
        'げ' => "ge",
        'ご' => "go",
        'さ' => "sa",
        'し' => "shi",
        'す' => "su",
        'せ' => "se",
        'そ' => "so",
        'ざ' => "za",
        'じ' => "ji",
        'ず' => "zu",
        'ぜ' => "ze",
        'ぞ' => "zo",
        'た' => "ta",
        'ち' => "chi",
        'つ' => "tsu",
        'て' => "te",
        'と' => "to",
        'だ' => "da",
        'ぢ' => "ji",
        'づ' => "zu",
        'で' => "de",
        'ど' => "do",
        'な' => "na",
        'に' => "ni",
        'ぬ' => "nu",
        'ね' => "ne",
        'の' => "no",
        'は' => "ha",
        'ひ' => "hi",
        'ふ' => "fu",
        'へ' => "he",
        'ほ' => "ho",
        'ば' => "ba",
        'び' => "bi",
        'ぶ' => "bu",
        'べ' => "be",
        'ぼ' => "bo",
        'ぱ' => "pa",
        'ぴ' => "pi",
        'ぷ' => "pu",
        'ぺ' => "pe",
        'ぽ' => "po",
        'ま' => "ma",
        'み' => "mi",
        'む' => "mu",
        'め' => "me",
        'も' => "mo",
        'や' => "ya",
        'ゆ' => "yu",
        'よ' => "yo",
        'ら' => "ra",
        'り' => "ri",
        'る' => "ru",
        'れ' => "re",
        'ろ' => "ro",
        'わ' => "wa",
        'ゐ' => "i",
        'ゑ' => "e",
        'を' => "o",
        'ゔ' => "vu",
        'ぁ' => "a",
        'ぃ' => "i",
        'ぅ' => "u",
        'ぇ' => "e",
        'ぉ' => "o",
        'ゃ' => "ya",
        'ゅ' => "yu",
        'ょ' => "yo",
        'ゎ' => "wa",
        _ => return None,
    })
}

/// Consonant stem of a kana in the i-row, for the small ya/yu/yo digraphs.
fn digraph(first: char, small: char) -> Option<&'static str> {
    let b = base(first)?;
    let vowel = match small {
        'ゃ' => 'a',
        'ゅ' => 'u',
        'ょ' => 'o',
        _ => return None,
    };
    Some(match (b, vowel) {
        ("shi", 'a') => "sha",
        ("shi", 'u') => "shu",
        ("shi", 'o') => "sho",
        ("chi", 'a') => "cha",
        ("chi", 'u') => "chu",
        ("chi", 'o') => "cho",
        ("ji", 'a') => "ja",
        ("ji", 'u') => "ju",
        ("ji", 'o') => "jo",
        ("ki", 'a') => "kya",
        ("ki", 'u') => "kyu",
        ("ki", 'o') => "kyo",
        ("gi", 'a') => "gya",
        ("gi", 'u') => "gyu",
        ("gi", 'o') => "gyo",
        ("ni", 'a') => "nya",
        ("ni", 'u') => "nyu",
        ("ni", 'o') => "nyo",
        ("hi", 'a') => "hya",
        ("hi", 'u') => "hyu",
        ("hi", 'o') => "hyo",
        ("bi", 'a') => "bya",
        ("bi", 'u') => "byu",
        ("bi", 'o') => "byo",
        ("pi", 'a') => "pya",
        ("pi", 'u') => "pyu",
        ("pi", 'o') => "pyo",
        ("mi", 'a') => "mya",
        ("mi", 'u') => "myu",
        ("mi", 'o') => "myo",
        ("ri", 'a') => "rya",
        ("ri", 'u') => "ryu",
        ("ri", 'o') => "ryo",
        _ => return None,
    })
}

fn macron(v: char) -> char {
    match v {
        'a' => 'ā',
        'i' => 'ī',
        'u' => 'ū',
        'e' => 'ē',
        'o' => 'ō',
        other => other,
    }
}

/// Romanizes the kana in `text`; `None` when there is none (nothing to show).
pub fn romanize(text: &str) -> Option<String> {
    if !has_kana(text) {
        return None;
    }
    let chars: Vec<char> = text.chars().map(to_hira).collect();
    let mut out = String::new();
    let mut i = 0;
    let mut double_next = false;
    while i < chars.len() {
        let c = chars[i];
        let mut piece: Option<String> = None;
        let mut used = 1;
        if c == 'っ' {
            double_next = true;
            i += 1;
            continue;
        }
        if c == 'ん' {
            // n before a vowel or y is written n'.
            let next = chars.get(i + 1).and_then(|n| base(*n)).and_then(|b| b.chars().next());
            piece = Some(if next.is_some_and(|n| "aiueoy".contains(n)) { "n'".into() } else { "n".into() });
        } else if c == 'ー' {
            // A long vowel: lengthen the previous one.
            if let Some(last) = out.pop() {
                out.push(macron(last));
                i += 1;
                continue;
            }
        } else if let Some(small) = chars.get(i + 1).filter(|s| matches!(s, 'ゃ' | 'ゅ' | 'ょ'))
            && let Some(d) = digraph(c, *small)
        {
            piece = Some(d.into());
            used = 2;
        }
        if piece.is_none() {
            piece = base(c).map(String::from);
        }
        match piece {
            Some(mut p) => {
                if double_next {
                    // tt, kk, ss ... (tch before "ch")
                    if p.starts_with("ch") {
                        p.insert(0, 't');
                    } else if let Some(f) = p.chars().next() {
                        p.insert(0, f);
                    }
                    double_next = false;
                }
                out.push_str(&p);
            }
            None => {
                if double_next {
                    double_next = false;
                }
                out.push(chars[i]);
            }
        }
        i += used;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hiragana_and_katakana() {
        assert_eq!(romanize("こんにちは").as_deref(), Some("konnichiha"));
        assert_eq!(romanize("ありがとう").as_deref(), Some("arigatou"));
        assert_eq!(romanize("カタカナ").as_deref(), Some("katakana"));
    }

    #[test]
    fn digraphs_small_tsu_and_n() {
        assert_eq!(romanize("きょう").as_deref(), Some("kyou"));
        assert_eq!(romanize("しゃしん").as_deref(), Some("shashin"));
        assert_eq!(romanize("ざっし").as_deref(), Some("zasshi"));
        assert_eq!(romanize("まっちゃ").as_deref(), Some("matcha"));
        assert_eq!(romanize("ほんや").as_deref(), Some("hon'ya"));
        assert_eq!(romanize("ほん").as_deref(), Some("hon"));
    }

    #[test]
    fn long_vowel_mark_and_mixed_text() {
        assert_eq!(romanize("ラーメン").as_deref(), Some("rāmen"));
        assert_eq!(romanize("夜に駆ける きっと").as_deref(), Some("夜ni駆keru kitto"));
    }

    #[test]
    fn nothing_without_kana() {
        assert_eq!(romanize("Hello"), None);
        assert_eq!(romanize("夜に駆ける".replace(['に', 'け', 'る'], "").as_str()), None);
        assert!(!has_kana("中文歌词"));
    }
}
