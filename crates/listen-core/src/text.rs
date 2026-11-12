//! Text matching helpers shared by every crate that compares names (song versions, substitutes,
//! release radar, headphone models, Last.fm and lyrics titles, CD covers). Look here before writing
//! another "lower-case, letters and digits only" helper; `letters_and_digits_folds_use_text_fold`
//! (app) fails on a copy. Cutting `(…)`/`[…]`/`{…}` parts out of a title is [`without_brackets`].

use unicode_normalization::UnicodeNormalization;

/// Folds `s` to its letters and digits (any script), lower-cased, so case, punctuation and
/// spacing don't matter when two names are compared: `"Love Dive!"` and `"love-dive"` both
/// give `"lovedive"`. Hangul, kana and other scripts are kept as they are.
///
/// The input is NFC-normalised first, so a name decomposed by its tagger (NFD, as macOS writes it)
/// folds to the same key as the composed one: `"Cafe\u{301}"` gives `"café"`, conjoining jamo give
/// Hangul syllables. Keys of NFC text are unchanged by this.
///
/// It does not remove accents or bracketed extras: `versions::song_key` and `releases::release_key`
/// cut bracketed extras with [`without_brackets`] before folding, `substitute` also folds accents
/// first (`plain`).
pub fn fold(s: &str) -> String {
    s.nfc().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// `s` without its bracketed parts: everything inside `(…)`, `[…]` or `{…}`, nested ones included,
/// is dropped with the brackets; a closing bracket without an opener is dropped alone. Nothing else
/// changes (no case or space folding), so `"Song (Live) [2011]"` gives `"Song  "`. The version and
/// release keys use it; `substitute::split_extras` keeps the bracketed parts as extras instead.
pub(crate) fn without_brackets(s: &str) -> String {
    let mut base = String::with_capacity(s.len());
    let mut depth = 0usize;
    for c in s.chars() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            _ if depth == 0 => base.push(c),
            _ => {}
        }
    }
    base
}

#[cfg(test)]
mod tests {
    use super::{fold, without_brackets};
    use unicode_normalization::UnicodeNormalization;

    #[test]
    fn fold_keeps_letters_and_digits_of_any_script_in_lower_case() {
        assert_eq!(fold("Love Dive!"), "lovedive");
        assert_eq!(fold("love-dive"), "lovedive");
        assert_eq!(fold("아이유 (IU) 2024"), "아이유iu2024");
        assert_eq!(fold(" - ,. "), "");
    }

    #[test]
    fn fold_is_normalisation_independent() {
        for name in ["아이유", "Café", "Beyoncé – Déjà Vu", "ポップ"] {
            let nfd: String = name.nfd().collect();
            assert_ne!(nfd, name, "the test needs a decomposable name: {name}");
            assert_eq!(fold(&nfd), fold(name), "{name}");
        }
        assert_eq!(fold("Cafe\u{301}"), "café");
        assert_eq!(fold("아이유"), "아이유", "NFC keys are unchanged");
    }

    #[test]
    fn without_brackets_drops_every_bracketed_part_and_keeps_the_rest_as_is() {
        assert_eq!(without_brackets("Song (Live) [2011]"), "Song  ");
        assert_eq!(without_brackets("a (b [c] d) e {f}g"), "a  e g");
        assert_eq!(without_brackets("closer) only"), "closer only");
        assert_eq!(without_brackets("open (never closed"), "open ");
        assert_eq!(without_brackets("러브 다이브 (Love Dive)"), "러브 다이브 ");
    }
}
