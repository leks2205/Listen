//! "Why this song?": the reason shown next to a song a DJ picked (see [`listen_core::queue::Why`]).
//! One reason per song, chosen by priority so it says something the listener can check.

use crate::familiarity::{FAVOURITE_PLAYS, FORGOTTEN_DAYS, Familiarity};
use listen_core::queue::{Picker, Why, WhyCode};

const DAY: i64 = 86_400;

/// A favourite not heard for a while: its last play in Unix seconds.
fn forgotten(f: &Familiarity, now: i64) -> Option<i64> {
    let days = f.last_played_days?;
    (f.plays >= FAVOURITE_PLAYS && days >= FORGOTTEN_DAYS).then(|| now - i64::from(days) * DAY)
}

/// The Auto-DJ's reason: a forgotten favourite, else similar to `similar_to` (the song whose similar
/// songs it came from), else new to you, else a random pick from the library.
pub fn auto_dj(f: &Familiarity, similar_to: Option<&str>, now: i64) -> Why {
    let code = if let Some(last_played) = forgotten(f, now) {
        WhyCode::ForgottenFavourite { last_played }
    } else if let Some(seed) = similar_to.filter(|s| !s.trim().is_empty()) {
        WhyCode::SimilarTo { seed: seed.trim().to_string() }
    } else if f.new_to_you {
        WhyCode::NewToYou
    } else {
        WhyCode::Random
    };
    Why::new(Picker::AutoDj, code)
}

/// The AI DJ's reason: its own note for the pick; without one, a forgotten favourite or new to you,
/// else "Picked by the AI DJ".
pub fn ai_dj(note: &str, f: &Familiarity, now: i64) -> Why {
    let note = crate::prompt::clip_note(note);
    let code = if !note.is_empty() {
        WhyCode::AiDj { note }
    } else if let Some(last_played) = forgotten(f, now) {
        WhyCode::ForgottenFavourite { last_played }
    } else if f.new_to_you {
        WhyCode::NewToYou
    } else {
        WhyCode::AiDj { note: String::new() }
    };
    Why::new(Picker::AiDj, code)
}

/// A DJ bridge between the songs titled `a` and `b`.
pub fn bridge(a: &str, b: &str) -> Why {
    Why::new(Picker::Bridge, WhyCode::BridgeBetween { a: a.to_string(), b: b.to_string() })
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000;

    fn fam(plays: u32, days: Option<u32>, new: bool) -> Familiarity {
        Familiarity { plays, last_played_days: days, new_to_you: new, new_artist: false }
    }

    #[test]
    fn the_auto_dj_names_the_most_telling_reason() {
        let old_fav = fam(14, Some(120), false);
        assert_eq!(
            auto_dj(&old_fav, Some("Roads"), NOW).code,
            WhyCode::ForgottenFavourite { last_played: NOW - 120 * DAY }
        );
        assert_eq!(
            auto_dj(&fam(3, Some(2), false), Some(" Roads "), NOW).code,
            WhyCode::SimilarTo { seed: "Roads".into() }
        );
        assert_eq!(auto_dj(&fam(0, None, true), None, NOW).code, WhyCode::NewToYou);
        assert_eq!(auto_dj(&fam(0, None, false), Some(""), NOW).code, WhyCode::Random);
        assert_eq!(auto_dj(&fam(14, Some(10), false), None, NOW).code, WhyCode::Random);
        assert_eq!(auto_dj(&old_fav, None, NOW).picker, Picker::AutoDj);
    }

    #[test]
    fn the_ai_dj_note_wins_and_is_kept_short() {
        let w = ai_dj("Gleiche Tonart, legt beim Tempo etwas zu", &fam(0, None, true), NOW);
        assert_eq!(w.picker, Picker::AiDj);
        assert_eq!(w.code, WhyCode::AiDj { note: "Gleiche Tonart, legt beim Tempo etwas zu".into() });
        let long = "word ".repeat(40);
        let WhyCode::AiDj { note } = ai_dj(&long, &fam(0, None, false), NOW).code else { panic!() };
        assert!(note.chars().count() <= 80 && note.ends_with('…'), "{note}");
        assert_eq!(ai_dj(" ", &fam(0, None, true), NOW).code, WhyCode::NewToYou);
        assert_eq!(ai_dj("", &fam(1, Some(1), false), NOW).code, WhyCode::AiDj { note: String::new() });
        assert_eq!(bridge("A", "B").picker, Picker::Bridge);
    }
}
