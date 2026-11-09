//! The scene of what plays in the AI DJ's prompt (desktop and phone say the same): the session's scene
//! (language, genre, era of the current song and the ones before it) goes into the listener notes as a hard
//! rule, and each candidate's note names its scene, so the model doesn't follow a 1980s ballad with a
//! 2020s idol song even if one slipped into the pool.

use listen_core::scene::{Scene, era_label};

/// A language code as the prompt names it.
fn language(code: &str) -> &str {
    match code {
        "ko" => "Korean",
        "en" => "English",
        "ja" => "Japanese",
        "zh" => "Chinese",
        "pt" => "Portuguese",
        "es" => "Spanish",
        "fr" => "French",
        "de" => "German",
        other => other,
    }
}

/// "Korean · pop · 1980s", "instrumental · electronic · 2020s"; `None` when nothing is known.
pub fn label(s: &Scene) -> Option<String> {
    if !s.is_known() && s.era.is_none() {
        return None;
    }
    let mut parts: Vec<String> = Vec::new();
    match (s.instrumental, s.lang) {
        (true, _) => parts.push("instrumental".into()),
        (false, Some(l)) => parts.push(language(l).into()),
        (false, None) => {}
    }
    if !s.families.is_empty() {
        parts.push(s.families.label());
    }
    if let Some(e) = s.era {
        parts.push(era_label(e));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// What a candidate's note says about its scene: "scene: Korean · pop · 1990s".
pub fn note(s: &Scene) -> Option<String> {
    label(s).map(|l| format!("scene: {l}"))
}

/// Appends [`note`] to a candidate's note (or starts it).
pub fn add_note(note_text: &mut String, s: &Scene) {
    if let Some(n) = note(s) {
        *note_text = if note_text.is_empty() { n } else { format!("{note_text}; {n}") };
    }
}

/// The listener notes with the scene rule added (`scene`: the session's, `None` mixed or unknown: notes
/// unchanged).
pub fn rule(notes: &str, scene: Option<&Scene>) -> String {
    let Some(label) = scene.and_then(label) else { return notes.to_string() };
    let era = scene
        .and_then(|s| s.era)
        .map(|e| {
            format!(
                " Stay within a decade of the {}: never jump from an older song to a much newer one or back.",
                era_label(e)
            )
        })
        .unwrap_or_default();
    let rule = format!(
        "SCENE RULE: what plays now is {label}. Every song of the set MUST be from this scene: the same language \
         and genre.{era} Read each candidate's \"scene:\" note; never pick one from another scene, even if it \
         would flow better."
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
    fn the_rule_names_language_genre_and_era() {
        let s = Scene::from_tags("Korean pop, ballad, 1980s, South Korea, Korean, male solo");
        let r = rule("No ballads.", Some(&s));
        assert!(r.starts_with("No ballads.\nSCENE RULE: what plays now is Korean · pop · 1980s."), "{r}");
        assert!(r.contains("within a decade of the 1980s"), "{r}");
        assert_eq!(rule("x", None), "x");
        assert_eq!(rule("x", Some(&Scene::default())), "x");
    }

    #[test]
    fn candidate_notes_name_the_scene() {
        let mut n = "keys compatible".to_string();
        add_note(&mut n, &Scene::from_tags("K-pop, dance-pop, 2020s, Korean, girl group"));
        assert_eq!(n, "keys compatible; scene: Korean · pop · 2020s");
        let mut n = String::new();
        add_note(&mut n, &Scene::from_tags("tech house, electronic, 2020s, instrumental"));
        assert_eq!(n, "scene: instrumental · electronic · 2020s");
        let mut n = String::new();
        add_note(&mut n, &Scene::default());
        assert!(n.is_empty());
    }
}
