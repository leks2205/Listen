//! Heart-rate mode in the AI DJ's prompt (desktop and phone say the same): the target tempo goes into the
//! listener notes as a hard rule, and each candidate's note says whether its tempo fits.

/// What a candidate's note says about its tempo: `Some(true)` fits, `Some(false)` does not, `None` unknown.
pub fn fit_note(fits: Option<bool>) -> &'static str {
    match fits {
        Some(true) => "tempo fits the heart rate",
        Some(false) => "tempo does NOT fit the heart rate, avoid",
        None => "tempo unknown, avoid",
    }
}

/// Appends [`fit_note`] to a candidate's note (or starts it).
pub fn add_fit_note(note: &mut String, fits: Option<bool>) {
    let fit = fit_note(fits);
    *note = if note.is_empty() { fit.to_string() } else { format!("{note}; {fit}") };
}

/// The listener notes with the heart-rate rule added: the target tempo (`bpm` ± `tol`) is a hard
/// requirement.
pub fn rule(notes: &str, bpm: f32, tol: f32) -> String {
    let (bpm, tol) = (bpm.round(), tol.round());
    let rule = format!(
        "HEART-RATE MODE IS ON. The tempo is a hard requirement, not a mood hint: every song of the set MUST be at \
         {bpm} BPM ± {tol} (half or double time counts). Pick only candidates marked \"{}\"; \
         never pick one marked \"avoid\" while a fitting one is left, even if it would flow better.",
        fit_note(Some(true))
    );
    match notes.trim() {
        "" => rule,
        n => format!("{n}\n{rule}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rule_follows_the_notes_and_names_the_target() {
        let r = rule("  ", 142.4, 8.0);
        assert!(r.starts_with("HEART-RATE MODE IS ON."));
        assert!(r.contains("142 BPM ± 8") && r.contains("\"tempo fits the heart rate\""));
        let r = rule("No ballads.", 70.0, 8.0);
        assert!(r.starts_with("No ballads.\nHEART-RATE MODE IS ON."));
    }

    #[test]
    fn fit_notes_append_to_the_candidate_note() {
        let mut n = String::new();
        add_fit_note(&mut n, Some(true));
        assert_eq!(n, "tempo fits the heart rate");
        let mut n = "keys compatible".to_string();
        add_fit_note(&mut n, Some(false));
        assert_eq!(n, "keys compatible; tempo does NOT fit the heart rate, avoid");
        assert_eq!(fit_note(None), "tempo unknown, avoid");
    }
}
