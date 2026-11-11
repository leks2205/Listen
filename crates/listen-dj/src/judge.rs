//! A second model call that grades a break, for `listen --check-ai-dj`: a regression check whenever the
//! prompt or the text model changes (LLM-as-a-judge agrees moderately with expert raters on music
//! recommendation explanations, RecSys 2026).

use crate::familiarity::NEW_TAG;
use serde::Deserialize;
use serde_json::{Value, json};
use std::fmt::Write;

/// One break to grade.
#[derive(Debug, Clone, Default)]
pub struct Case<'a> {
    /// The persona's description.
    pub persona: &'a str,
    /// The language the hosts should speak.
    pub language: &'a str,
    /// Who talks this break ("Only Hauke talks this break."), empty = not said.
    pub hosts: String,
    /// What played before the break, one prompt line each.
    pub played: Vec<String>,
    /// The new set, one prompt line each (with its tags: "[new to you]", "from now: …").
    pub set: Vec<String>,
    /// The break as spoken.
    pub line: &'a str,
    pub previous_line: Option<&'a str>,
}

impl Case<'_> {
    /// The set has a song tagged new to the listener, so the break should introduce one.
    pub fn has_new(&self) -> bool {
        self.set.iter().any(|s| s.contains(&format!("[{NEW_TAG}")))
    }
}

/// The judge's grades.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Verdict {
    /// No invented or unverifiable claims about artists or songs.
    #[serde(default)]
    pub grounded: bool,
    /// Sounds like the persona.
    #[serde(default)]
    pub persona: bool,
    /// In the show's language; titles and names unchanged.
    #[serde(default)]
    pub language: bool,
    /// Doesn't repeat the previous break's idea or phrasing.
    #[serde(default)]
    pub fresh: bool,
    /// Introduces a new-to-you song of the set (only graded when the set has one).
    #[serde(default)]
    pub introduces_new: bool,
    /// Doesn't talk about blends, keys, tempo or fades.
    #[serde(default)]
    pub no_mix_talk: bool,
    #[serde(default)]
    pub problems: String,
}

impl Verdict {
    pub fn passed(&self, case: &Case) -> bool {
        self.grounded
            && self.persona
            && self.language
            && self.fresh
            && self.no_mix_talk
            && (self.introduces_new || !case.has_new())
    }

    /// "grounded ✓ persona ✓ … " for the check's output.
    pub fn summary(&self, case: &Case) -> String {
        let mark = |b: bool| if b { "✓" } else { "✗" };
        let mut s = format!(
            "grounded {} persona {} language {} fresh {} no mix talk {}",
            mark(self.grounded),
            mark(self.persona),
            mark(self.language),
            mark(self.fresh),
            mark(self.no_mix_talk)
        );
        if case.has_new() {
            let _ = write!(s, " introduces new song {}", mark(self.introduces_new));
        }
        if !self.problems.trim().is_empty() {
            let _ = write!(s, " — {}", self.problems.trim());
        }
        s
    }
}

pub fn system_instruction() -> String {
    format!(
        "You grade one spoken break of an AI radio DJ inside a personal music player. Be strict and \
     literal. Answer only with the JSON object.\n\
     - grounded: false if the break states a fact about an artist or song (origin, members, history, \
     awards, chart places, what a lyric means) that is wrong or that you cannot confirm; talking about \
     the sound, mood or the listener is always grounded.\n\
     - persona: true if the tone matches the persona description. When the prompt says only one host \
     talks this break, the other host's absence is intended and is not a fault.\n\
     - language: true if it is spoken in the required language (song titles and artist names may stay \
     in their original language).\n\
     - fresh: false if it repeats the idea or phrasing of the previous break.\n\
     - introduces_new: true if the break names at least one song of the new set tagged \"{NEW_TAG}\" \
     (or its artist) and says anything about it — its origin, era, scene, what it is about, or how it \
     sounds or feels. False only if no new-to-you song is mentioned, or only its bare title.\n\
     - no_mix_talk: false only if it uses DJ craft terms: blend, crossfade, cut, fade, beat-matching, \
     BPM, tempo or key. Metaphors about mood or energy (\"shift down a gear\") are fine.\n\
     - problems: one short English sentence on what is wrong, empty if nothing."
    )
}

pub fn user_prompt(c: &Case) -> String {
    let mut p = String::new();
    let _ = writeln!(p, "Persona: {}", c.persona);
    if !c.hosts.is_empty() {
        let _ = writeln!(p, "{}", c.hosts);
    }
    let _ = writeln!(p, "Required language: {}", if c.language.is_empty() { "English" } else { c.language });
    if !c.played.is_empty() {
        p.push_str("Played before the break:\n");
        for s in &c.played {
            let _ = writeln!(p, "- {s}");
        }
    }
    p.push_str("New set after the break (tags show what the listener knows):\n");
    for s in &c.set {
        let _ = writeln!(p, "- {s}");
    }
    if let Some(prev) = c.previous_line.filter(|l| !l.is_empty()) {
        let _ = writeln!(p, "Previous break: \"{prev}\"");
    }
    let _ = writeln!(p, "Break to grade: \"{}\"", c.line);
    p
}

pub fn response_schema() -> Value {
    let flag = json!({ "type": "boolean" });
    json!({
        "type": "object",
        "properties": {
            "grounded": flag, "persona": flag, "language": flag, "fresh": flag,
            "introduces_new": flag, "no_mix_talk": flag,
            "problems": { "type": "string" }
        },
        "required": ["grounded", "persona", "language", "fresh", "introduces_new", "no_mix_talk", "problems"]
    })
}

pub fn parse(text: &str) -> Result<Verdict, String> {
    let body = text.trim().trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```");
    serde_json::from_str(body.trim()).map_err(|e| format!("bad verdict JSON: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grades_and_new_song_rule() {
        let case = Case {
            persona: "calm late-night host",
            language: "German",
            set: vec!["Roads – Portishead (1994) [new to you]".into(), "Angel – Massive Attack".into()],
            line: "Und jetzt Roads.",
            previous_line: Some("Das war Teardrop."),
            ..Case::default()
        };
        assert!(case.has_new());
        let p = user_prompt(&case);
        assert!(p.contains("Required language: German") && p.contains("[new to you]"));
        assert!(p.contains("Previous break: \"Das war Teardrop.\"") && p.contains("Break to grade"));
        let v = parse(
            r#"{"grounded":true,"persona":true,"language":true,"fresh":true,"introduces_new":false,
                "no_mix_talk":true,"problems":"only names Roads"}"#,
        )
        .unwrap();
        assert!(!v.passed(&case));
        assert!(v.summary(&case).contains("introduces new song ✗ — only names Roads"));
        let familiar = Case { set: vec!["Angel – Massive Attack [plays 4]".into()], ..case.clone() };
        assert!(v.passed(&familiar));
        assert!(!v.summary(&familiar).contains("introduces"));
        assert!(parse("nope").is_err());
        assert_eq!(response_schema()["required"].as_array().unwrap().len(), 7);
    }
}
