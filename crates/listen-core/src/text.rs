//! Text matching helpers shared by every crate that compares names (versions, substitutes,
//! release radar, lyrics lookups). Look here before writing another "lower-case, letters
//! and digits only" helper.

/// Folds `s` to its letters and digits (any script), lower-cased, so case, punctuation and
/// spacing don't matter when two names are compared: `"Love Dive!"` and `"love-dive"` both
/// give `"lovedive"`. Hangul, kana and other scripts are kept as they are.
///
/// It does not remove accents or bracketed extras; `versions::song_key`, `substitute` and
/// `releases::release_key` do that themselves before folding.
pub fn fold(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

#[cfg(test)]
mod tests {
    use super::fold;

    #[test]
    fn fold_keeps_letters_and_digits_of_any_script_in_lower_case() {
        assert_eq!(fold("Love Dive!"), "lovedive");
        assert_eq!(fold("love-dive"), "lovedive");
        assert_eq!(fold("아이유 (IU) 2024"), "아이유iu2024");
        assert_eq!(fold(" - ,. "), "");
    }
}
