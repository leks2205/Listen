//! Text matching helpers shared by every crate that compares names (song versions, substitutes,
//! release radar, headphone models, Last.fm and lyrics titles, CD covers). Look here before writing
//! another "lower-case, letters and digits only" helper; `letters_and_digits_folds_use_text_fold`
//! (app) fails on a copy.

use unicode_normalization::UnicodeNormalization;

/// Folds `s` to its letters and digits (any script), lower-cased, so case, punctuation and
/// spacing don't matter when two names are compared: `"Love Dive!"` and `"love-dive"` both
/// give `"lovedive"`. Hangul, kana and other scripts are kept as they are.
///
/// The input is NFC-normalised first, so a name decomposed by its tagger (NFD, as macOS writes it)
/// folds to the same key as the composed one: `"Cafe\u{301}"` gives `"café"`, conjoining jamo give
/// Hangul syllables. Keys of NFC text are unchanged by this.
///
/// It does not remove accents or bracketed extras; `versions::song_key`, `substitute` and
/// `releases::release_key` do that themselves before folding.
pub fn fold(s: &str) -> String {
    s.nfc().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

#[cfg(test)]
mod tests {
    use super::fold;
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
}
