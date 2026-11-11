//! What the text model is asked (seed, what played, the last line, the
//! candidate pool) and how its answer is checked.

use crate::familiarity::{Familiarity, NEW_TAG};
use crate::persona::Persona;
use listen_core::model::Song;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fmt::Write;

/// A song that played since the DJ last talked.
#[derive(Debug, Clone)]
pub struct Played {
    pub song: Song,
    /// Skipped by the user before it finished.
    pub skipped: bool,
    /// Queued by the user rather than by the DJ.
    pub by_user: bool,
    /// How the mix engine brought it in ("16-bar beat blend + bass swap from Teardrop"); `None` when it
    /// simply started. Lets the DJ talk about its mixes.
    pub mixed_in: Option<String>,
}

/// How a break sits between the songs. Chosen at random per break; the model
/// is told which one so the line fits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakMode {
    /// In the silence between the song playing now and the new set.
    Between,
    /// Over the last seconds of the song playing now (turned down); the new
    /// set starts when the DJ is done.
    Outro,
    /// Over the start of the new set's first song (turned down, then back up).
    Intro,
    /// Starts over the end of the song playing now and carries on over the
    /// start of the next one.
    Across,
}

impl BreakMode {
    pub const ALL: [BreakMode; 4] =
        [BreakMode::Between, BreakMode::Outro, BreakMode::Intro, BreakMode::Across];

    /// Whether the voice plays over music (the second player, ducking).
    pub fn over_music(self) -> bool {
        self != BreakMode::Between
    }

    /// Longest line (seconds spoken) that suits this mode.
    pub fn max_secs(self) -> u32 {
        match self {
            BreakMode::Between => 20,
            BreakMode::Outro | BreakMode::Intro => 10,
            BreakMode::Across => 14,
        }
    }

    fn instruction(self) -> &'static str {
        match self {
            BreakMode::Between => {
                "This break airs in the silence after the song playing now ends, before your new set starts."
            }
            BreakMode::Outro => {
                "You talk OVER THE LAST SECONDS of the song playing now: you turn its volume down and \
                 speak while it plays quietly under you and ends; your new set starts right after you finish. \
                 It's fine to react to the song that's fading under you. If the set's first song is \
                 mixed in, the music under your voice is already a blend of both songs."
            }
            BreakMode::Intro => {
                "The song playing now ends normally. Then the FIRST SONG OF YOUR NEW SET starts quietly under \
                 you: you talk over its intro and turn it back up when you finish. You may name that first \
                 song as it starts (e.g. \"…and this is …\")."
            }
            BreakMode::Across => {
                "You start talking OVER THE END of the song playing now (you turn it down), keep talking as \
                 it ends and the FIRST SONG OF YOUR NEW SET starts quietly under you, and turn that one up \
                 when you finish. The line can bridge from the old song to the new one. If the new song \
                 is mixed in, the music under your voice is the blend of both songs."
            }
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            BreakMode::Between => "between songs",
            BreakMode::Outro => "over the outro",
            BreakMode::Intro => "over the intro",
            BreakMode::Across => "across the change",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlanRequest<'a> {
    pub persona: &'a Persona,
    /// The song the DJ was started from.
    pub seed: &'a Song,
    /// What the Auto-DJ would have played after the seed: more of the mood.
    pub seed_context: &'a [Song],
    /// Songs the listener queued themselves ("Add to queue"), oldest first:
    /// they steer the session together with the seed.
    pub listener_picks: &'a [Song],
    /// The mood the DJ settled on in the last set (None on the first set).
    pub mood: Option<&'a str>,
    /// The mood of the first set, from the seed: the anchor for the session.
    pub seed_mood: Option<&'a str>,
    /// Finished songs since the last break, oldest first.
    pub played: &'a [Played],
    /// Playing now; the break comes right after it.
    pub current: Option<&'a Song>,
    /// What the DJ said last time.
    pub last_line: Option<&'a str>,
    /// Songs to choose from.
    pub candidates: &'a [Song],
    /// Audio analysis per song id ("124 bpm, 8A, energy 7/10"), where known.
    /// Candidates may end in "; mix from now: …": how the mix engine would
    /// bring them in after the song playing now.
    pub notes: &'a HashMap<String, String>,
    /// What the listener knows of a song, by id ("new to you", "plays 12, last played 3 days ago"); from
    /// the listening history (`familiarity::Listening::tags`). Shown for candidates.
    pub familiar: &'a HashMap<String, String>,
    pub set_min: usize,
    pub set_max: usize,
    /// First set after switching the DJ on (the line is an intro).
    pub first: bool,
    /// Where the break sits.
    pub mode: BreakMode,
    /// Two-host personas: the co-host joins this break.
    pub duo: bool,
    /// Language, home town and the listener's own notes.
    pub show: Show<'a>,
    /// What the listener told the DJ ("Tell the DJ"): outranks the seed's mood.
    pub requests: &'a crate::steer::Requests,
    /// A request changed since the last set: the break acknowledges it.
    pub requests_fresh: bool,
}

/// What the listener set up for the show (Settings → AI DJ). Every field may be empty.
#[derive(Debug, Clone, Copy, Default)]
pub struct Show<'a> {
    /// The language the hosts speak, in English ("German"); empty = English.
    pub language: &'a str,
    /// Where the show airs.
    pub location: &'a str,
    /// Free text: names, likes, things to avoid.
    pub notes: &'a str,
}

/// One speaker's part of a break.
#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    /// The speaker's name (empty = the host).
    pub speaker: String,
    pub text: String,
    /// Delivery direction for the speech model.
    pub style: String,
}

/// The model's answer, validated.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// Indices into the candidate pool, in play order.
    pub picks: Vec<usize>,
    pub mood: String,
    /// The break: one turn, or a short exchange between two hosts.
    pub turns: Vec<Turn>,
    /// Title of the new-to-you song the break introduces (asked for when the pool has one), as answered.
    pub introduces: String,
    /// Per pick (same order and length as `picks`): why it fits now, at most [`NOTE_MAX`] characters;
    /// empty when the model gave none (filled-in picks have none).
    pub notes: Vec<String>,
}

/// Longest per-song note ("Why this song?") kept from a plan, in characters.
pub const NOTE_MAX: usize = 80;

impl Plan {
    /// The break as text (with names when two people talk).
    pub fn line(&self) -> String {
        line_of(&self.turns)
    }
}

/// The break as text (with names when two people talk).
pub fn line_of(turns: &[Turn]) -> String {
    let named = turns.iter().any(|t| t.speaker != turns[0].speaker);
    turns
        .iter()
        .map(|t| if named { format!("{}: {}", t.speaker, t.text) } else { t.text.clone() })
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Deserialize)]
struct RawPlan {
    #[serde(default)]
    picks: Vec<Value>,
    #[serde(default)]
    mood: String,
    #[serde(default)]
    line: String,
    #[serde(default)]
    style: String,
    #[serde(default)]
    turns: Vec<RawTurn>,
    #[serde(default)]
    introduces: String,
    #[serde(default)]
    notes: Vec<Value>,
}

#[derive(Debug, Deserialize)]
struct RawTurn {
    #[serde(default)]
    speaker: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    style: String,
}

/// Where the show airs (the listener's own setting; empty = nowhere in particular).
fn location_brief(location: &str) -> String {
    let location = location.trim();
    if location.is_empty() {
        return "The show has no particular home town. Don't name or greet any city or region as \
                if the show aired there."
            .into();
    }
    format!(
        "The show airs in {location}. That is where the listener is, whatever city a persona is \
         from. If you mention a place, it's this one; never greet another city as if the show aired \
         there. Most breaks don't mention the location at all."
    )
}

pub fn system_instruction(persona: &Persona, show: &Show) -> String {
    let duo = match &persona.cohost {
        Some(c) => format!(
            "\nCo-host in the studio: {brief}\n\
             Two-host breaks are a short, natural exchange written as `turns`: {host} always opens; \
             {co} reacts, adds a fact or teases him; at most 4 turns, each 1–2 short sentences. They \
             talk to each other and to the listener like a real radio duo — no interview, no \
             repeating each other. When the prompt says {co} doesn't join, write a single turn for \
             {host}. Every turn gets its own `style` (English) for that speaker's voice.\n",
            brief = c.brief,
            host = persona.host,
            co = c.name
        ),
        None => String::new(),
    };
    let location = location_brief(show.location);
    let language = if show.language.trim().is_empty() { "English" } else { show.language.trim() };
    let notes = match show.notes.trim() {
        "" => String::new(),
        n => format!("\nNotes from the listener (follow them unless they break the rules below): {n}\n"),
    };
    format!(
        "You are the host of a personal radio show inside a music player.\n\
         Persona: {brief}\n{duo}\n\
         Location: {location}\n{notes}\n\
         Your job each time:\n\
         1. Pick the next set of songs ONLY from the numbered candidate list. The set must keep \
         the mood of the seed song (energy, tempo, feeling, era, genre) while flowing naturally \
         from what just played. The seed (and what the listener's Auto-DJ would have played after \
         it) defines the session: small variations are welcome, but never drift into a different \
         genre or energy over the session — if recent sets have wandered away from the seed's mood, \
         steer back towards it with this set. Order the set so it flows. Avoid the same artist twice in a set \
         and avoid artists that just played, unless the pool leaves no choice. If the user \
         skipped songs, steer away from what they skipped. Songs the listener queued \
         themselves show what they want right now: lean the mood towards them. When a song \
         is in the pool as a radio edit (or single/radio version), prefer that version over \
         the album, extended or live one; never pick two versions of the same song. Where \
         songs list a measured tempo, Camelot key and energy, let them flow like a DJ set: \
         neighbouring tempos (within ~6 %, or half/double time), the same or a neighbouring key \
         on the Camelot wheel (e.g. 8A → 8A, 7A, 9A, 8B) and energy that builds or eases \
         gradually instead of jumping. You mix the songs yourself: a candidate marked \
         \"mix from now\" says how it would join the song playing now (a beat blend is \
         smoothest, then a short blend or a filter-out; a fade means tempo or beat don't fit). \
         When the mood allows, open the set with a song that blends well and order the rest so \
         neighbours mix smoothly (close tempo and key).\n\
         2. Write the spoken break that airs right after the song playing now, before your new \
         set. Speak {language} (natural, spoken, informal — in German the du-Form). Keep song titles and artist names in their \
         original language. Refer back to one or two songs that just played and tease the new set \
         (e.g. the first artist or the vibe) without reading out a list. 1–3 short sentences; \
         the prompt says how long it may be and how it sits against the music. No greeting on every break, no station names, no made-up \
         facts about artists — if unsure, talk about the sound and mood instead. Never repeat \
         the idea or phrasing of your previous line. Plain text only: no emojis, markdown or \
         stage directions; you may use at most one of the tags <laugh>, <short pause> or <breath> \
         where it feels natural. Like a real DJ you know where the songs change and time your \
         talking to the music, but you never talk about the mixing itself (no blends, cuts, \
         fades, tempo or keys in your line — that's your craft, not your topic). When your line \
         airs over the start of your set's first song, keep it shorter than that song's \
         instrumental intro (\"intro … s\" in its note) so you never talk over the singer.\n\
         What the listener knows: candidates may carry a tag from their listening history — \
         \"{NEW_TAG}\" (never played here; \"new artist\" = no song of that artist either) or \
         \"plays N, last played …\" (\"favourite\" = played a lot). Mix a few familiar songs with new \
         ones. When your set has a new-to-you song and you speak, introduce one of them with one short \
         story line (where it comes from, what it is about, why it fits now) so it lands; use only facts \
         you are sure of — if you know nothing certain about it, describe its sound and mood instead. The \
         year in a song's line is its release year: use it rather than guessing an era. \
         Never read out play counts; \"one you haven't played in a while\" is fine. A candidate's \
         \"from now:\" states how it fits the song playing now (keys, tempo, energy): trust it.\n\
         3. `style`: a short English direction for the voice (emotion, pace) that suits this line \
         and the persona.\n\
         4. `mood`: a few words describing the mood you are keeping.\n\
         5. `notes`: one per pick, in the same order — a short reason the listener sees next to the song \
         in the queue (why it fits right now: its sound, how it follows the previous song, that it is new \
         to them or one they haven't played in a while). Speak {language}, at most 80 characters, no song \
         title, no made-up facts.",
        brief = persona.brief,
        duo = duo
    )
}

/// Starts the mix preview in a candidate's note.
pub const MIX_PREVIEW: &str = "mix from now:";

/// One song with year, genre and its note. The mix preview is kept only for candidates (`mix` = true).
fn song_line(out: &mut String, song: &Song, notes: &HashMap<String, String>, mix: bool) {
    let _ = write!(out, "{} – {}", song.title, song.artist_line());
    let mut extra = Vec::new();
    if let Some(y) = song.year {
        extra.push(y.to_string());
    }
    if !song.genres.is_empty() {
        extra.push(song.genres.iter().take(2).cloned().collect::<Vec<_>>().join("/"));
    }
    let note = notes.get(&song.id).map(|n| match n.find(MIX_PREVIEW) {
        Some(at) if !mix => n[..at].trim_end_matches([' ', ';']),
        _ => n.as_str(),
    });
    // Measured tempo/key/energy (audio analysis) beat the server's bpm tag.
    match note.filter(|n| !n.is_empty()) {
        Some(note) => extra.push(note.to_string()),
        None => {
            if let Some(bpm) = song.bpm.filter(|b| *b > 0) {
                extra.push(format!("{bpm} bpm"));
            }
        }
    }
    if !extra.is_empty() {
        let _ = write!(out, " ({})", extra.join(", "));
    }
}

/// Some candidate is tagged new to the listener (the break should introduce one, the schema asks which).
pub fn offers_new(r: &PlanRequest<'_>) -> bool {
    r.candidates.iter().any(|s| r.familiar.get(&s.id).is_some_and(|t| Familiarity::is_new_tag(t)))
}

/// The per-set prompt.
pub fn user_prompt(r: &PlanRequest<'_>) -> String {
    let mut p = String::new();
    p.push_str("Seed song (the mood to keep): ");
    song_line(&mut p, r.seed, r.notes, false);
    p.push('\n');
    if !r.seed_context.is_empty() {
        p.push_str(
            "What the listener's Auto-DJ would have played after it (the same mood; use it to \
             understand the vibe, not as a playlist to copy):\n",
        );
        for song in r.seed_context {
            p.push_str("- ");
            song_line(&mut p, song, r.notes, false);
            p.push('\n');
        }
    }
    if !r.listener_picks.is_empty() {
        p.push_str(
            "Songs the listener queued themselves this session (they steer the mood together \
             with the seed; most recent last):\n",
        );
        for song in r.listener_picks {
            p.push_str("- ");
            song_line(&mut p, song, r.notes, false);
            p.push('\n');
        }
    }
    if let Some(mood) = r.seed_mood.filter(|m| !m.is_empty()) {
        let _ = writeln!(p, "Original mood of this session (from the seed — stay close to it): {mood}");
    }
    if let Some(mood) = r.mood.filter(|m| !m.is_empty() && Some(*m) != r.seed_mood) {
        let _ = writeln!(p, "Mood of your last set: {mood}");
    }
    p.push_str(&r.requests.prompt(r.requests_fresh));
    if r.first {
        p.push_str(
            "\nThis is the first break: the listener just switched you on. Briefly say hi and \
             connect the seed song to what's coming.\n",
        );
    }
    let _ = writeln!(
        p,
        "\nHow this break airs: {} Keep the line under {} seconds spoken.",
        r.mode.instruction(),
        r.mode.max_secs()
    );
    if let Some(co) = &r.persona.cohost {
        if r.duo {
            let _ =
                writeln!(p, "This break: {} joins in — write 2–4 turns ({} first).", co.name, r.persona.host);
        } else {
            let _ = writeln!(p, "This break: only {} talks — one turn.", r.persona.host);
        }
    }
    if !r.played.is_empty() {
        p.push_str("\nPlayed since your last break (oldest first):\n");
        for played in r.played {
            p.push_str("- ");
            song_line(&mut p, &played.song, r.notes, false);
            if let Some(how) = played.mixed_in.as_deref().filter(|h| !h.is_empty()) {
                let _ = write!(p, " [mixed in: {how}]");
            }
            match (played.skipped, played.by_user) {
                (true, _) => p.push_str(" [skipped by the listener]"),
                (false, true) => p.push_str(" [chosen by the listener]"),
                _ => {}
            }
            p.push('\n');
        }
    }
    if let Some(current) = r.current {
        p.push_str("Playing now (your break airs right after it): ");
        song_line(&mut p, current, r.notes, false);
        p.push('\n');
    }
    match r.last_line.filter(|l| !l.is_empty()) {
        Some(line) => {
            let _ = writeln!(p, "\nYour previous break was: \"{line}\"");
        }
        None if !r.first => p.push_str("\nYou haven't said anything yet.\n"),
        None => {}
    }
    let _ = writeln!(
        p,
        "\nCandidates (pick {}–{} by number):",
        r.set_min.min(r.candidates.len()),
        r.set_max.min(r.candidates.len())
    );
    for (i, song) in r.candidates.iter().enumerate() {
        let _ = write!(p, "{i}. ");
        song_line(&mut p, song, r.notes, true);
        if let Some(tag) = r.familiar.get(&song.id) {
            let _ = write!(p, " [{tag}]");
        }
        p.push('\n');
    }
    let new: Vec<String> = r
        .candidates
        .iter()
        .enumerate()
        .filter(|(_, s)| r.familiar.get(&s.id).is_some_and(|t| Familiarity::is_new_tag(t)))
        .map(|(i, _)| i.to_string())
        .collect();
    if !new.is_empty() {
        let _ = writeln!(
            p,
            "\nNew to the listener: candidates {}. Put at least one of them in your set. Your break MUST \
             introduce one of THESE (not a familiar song): name it and give one short, concrete line about it \
             — where it comes from, what it is about, or (when you aren't sure of facts) how it sounds — not \
             just its title. Write its title in `introduces`. Keep it within the time limit.",
            new.join(", ")
        );
    }
    p
}

/// JSON schema of the answer (two-host personas answer in `turns`).
/// `new` adds the required `introduces` field (the new-to-you song the break introduces; see [`offers_new`]).
pub fn response_schema(persona: &Persona, set_min: usize, set_max: usize, new: bool) -> Value {
    let mut schema = base_schema(persona, set_min, set_max);
    if new {
        schema["properties"]["introduces"] = json!({
            "type": "string",
            "description": "Title of the new-to-you song your break introduces"
        });
        if let Some(req) = schema["required"].as_array_mut() {
            req.push(json!("introduces"));
        }
    }
    schema
}

fn base_schema(persona: &Persona, set_min: usize, set_max: usize) -> Value {
    // The picks and their notes are the same with and without a co-host; only
    // the spoken part differs (one line vs. turns between two voices).
    let picks = json!({
        "type": "array",
        "items": { "type": "integer" },
        "minItems": set_min,
        "maxItems": set_max,
        "description": "Candidate numbers in play order"
    });
    let notes = json!({
        "type": "array",
        "items": { "type": "string" },
        "minItems": set_min,
        "maxItems": set_max,
        "description": "One note per pick, same order: why this song fits right now, in the show's language, at most 80 characters (no song title)"
    });
    if let Some(co) = &persona.cohost {
        return json!({
            "type": "object",
            "properties": {
                "picks": picks,
                "notes": notes,
                "mood": { "type": "string" },
                "turns": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 4,
                    "items": {
                        "type": "object",
                        "properties": {
                            "speaker": { "type": "string", "enum": [persona.host, co.name] },
                            "text": { "type": "string", "description": "What they say, in the show's language" },
                            "style": { "type": "string", "description": "Voice direction, in English" }
                        },
                        "required": ["speaker", "text", "style"]
                    }
                }
            },
            "required": ["picks", "notes", "mood", "turns"]
        });
    }
    json!({
        "type": "object",
        "properties": {
            "picks": picks,
            "notes": notes,
            "mood": { "type": "string" },
            "line": { "type": "string", "description": "The spoken break, in the show's language" },
            "style": { "type": "string", "description": "Voice direction for the speech model, in English" }
        },
        "required": ["picks", "notes", "mood", "line", "style"]
    })
}

/// Parses the model's JSON and repairs what it can: drops out-of-range and
/// repeated picks, trims to `set_max`, and tops up to `set_min` from the
/// pool (in pool order) so a sloppy answer still yields a usable set.
pub fn parse_plan(text: &str, pool: usize, set_min: usize, set_max: usize) -> Result<Plan, String> {
    let body = text.trim().trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```");
    let raw: RawPlan = serde_json::from_str(body.trim()).map_err(|e| format!("bad plan JSON: {e}"))?;
    let set_max = set_max.max(1).min(pool);
    let set_min = set_min.max(1).min(set_max);
    let mut picks: Vec<usize> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    for (k, v) in raw.picks.iter().enumerate() {
        let n = v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().trim_start_matches('#').parse().ok()));
        if let Some(n) = n.map(|n| n as usize)
            && n < pool
            && !picks.contains(&n)
        {
            picks.push(n);
            notes.push(raw.notes.get(k).and_then(Value::as_str).map(clip_note).unwrap_or_default());
        }
    }
    picks.truncate(set_max);
    notes.truncate(set_max);
    let mut filler = 0;
    while picks.len() < set_min && filler < pool {
        if !picks.contains(&filler) {
            picks.push(filler);
            notes.push(String::new());
        }
        filler += 1;
    }
    if picks.is_empty() {
        return Err("the plan picked no songs".into());
    }
    let mut turns: Vec<Turn> = raw
        .turns
        .iter()
        .map(|t| Turn {
            speaker: t.speaker.trim().to_string(),
            text: clean_line(&t.text),
            style: t.style.trim().to_string(),
        })
        .filter(|t| !t.text.is_empty())
        .take(4)
        .collect();
    if turns.is_empty() && !raw.line.trim().is_empty() {
        turns.push(Turn {
            speaker: String::new(),
            text: clean_line(&raw.line),
            style: raw.style.trim().to_string(),
        });
    }
    Ok(Plan {
        picks,
        mood: raw.mood.trim().to_string(),
        turns,
        introduces: raw.introduces.trim().to_string(),
        notes,
    })
}

/// A per-song note cleaned up and cut to [`NOTE_MAX`] characters (at a word, with "…").
pub fn clip_note(note: &str) -> String {
    let note = clean_line(note);
    if note.chars().count() <= NOTE_MAX {
        return note;
    }
    let cut: String = note.chars().take(NOTE_MAX - 1).collect();
    // `rfind` gives a byte offset; the half-a-note rule counts characters (Hangul is 3 bytes each).
    let cut = match cut.rfind(' ') {
        Some(i) if cut[..i].chars().count() > NOTE_MAX / 2 => &cut[..i],
        _ => cut.as_str(),
    };
    format!("{}…", cut.trim_end_matches([',', ';', ':', '.', ' ', '-']))
}

/// Strips markdown/emoji-ish leftovers and collapses whitespace.
fn clean_line(line: &str) -> String {
    let no_md: String = line.chars().filter(|c| !matches!(c, '*' | '#' | '`' | '_')).collect();
    no_md.split_whitespace().collect::<Vec<_>>().join(" ").trim_matches('"').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persona::PERSONAS;
    use listen_core::model::{ArtistRef, ServerId, SourceKind};
    use std::time::Duration;

    fn song(title: &str, artist: &str, year: i32, genre: &str) -> Song {
        Song {
            artists: vec![ArtistRef { id: None, name: artist.into() }],
            duration: Duration::from_secs(200),
            year: Some(year),
            genres: vec![genre.into()],
            ..Song::new(title.to_lowercase(), ServerId("s".into()), SourceKind::Subsonic, title)
        }
    }

    #[test]
    fn clip_note_counts_characters_not_bytes_for_hangul() {
        // The last space sits after 20 characters (60 bytes): backing up to it would drop most of the note.
        let note = format!("{} {}", "가".repeat(20), "하".repeat(80));
        let clipped = clip_note(&note);
        assert_eq!(clipped.chars().count(), NOTE_MAX, "{clipped}");
        assert!(clipped.ends_with('…'));
        // A space late in the note is still a word boundary to cut at.
        let note = format!("{} {}", "가".repeat(60), "하".repeat(40));
        assert_eq!(clip_note(&note), format!("{}…", "가".repeat(60)));
    }

    /// A plain request over `pool`; the first song is the seed.
    fn bare_request<'a>(
        pool: &'a [Song],
        familiar: &'a HashMap<String, String>,
        notes: &'a HashMap<String, String>,
        requests: &'a crate::steer::Requests,
    ) -> PlanRequest<'a> {
        PlanRequest {
            persona: &PERSONAS[0],
            seed: &pool[0],
            seed_context: &[],
            listener_picks: &[],
            mood: None,
            seed_mood: None,
            played: &[],
            current: Some(&pool[0]),
            last_line: None,
            candidates: pool,
            notes,
            familiar,
            set_min: 2,
            set_max: 5,
            first: false,
            mode: BreakMode::Intro,
            duo: false,
            show: Show::default(),
            requests,
            requests_fresh: false,
        }
    }

    #[test]
    fn offers_new_follows_the_familiarity_tag() {
        let pool = vec![
            song("Teardrop", "Massive Attack", 1998, "Trip-Hop"),
            song("Roads", "Portishead", 1994, "Trip-Hop"),
        ];
        let (notes, requests) = (HashMap::new(), crate::steer::Requests::default());
        for new_artist in [false, true] {
            let tag = Familiarity { new_to_you: true, new_artist, ..Familiarity::default() }.tag().unwrap();
            let familiar = HashMap::from([(pool[1].id.clone(), tag)]);
            let req = bare_request(&pool, &familiar, &notes, &requests);
            assert!(offers_new(&req), "{familiar:?}");
            assert!(user_prompt(&req).contains("New to the listener: candidates 1."), "{familiar:?}");
        }
        let known =
            Familiarity { plays: 12, last_played_days: Some(3), ..Familiarity::default() }.tag().unwrap();
        let familiar = HashMap::from([(pool[1].id.clone(), known)]);
        assert!(!offers_new(&bare_request(&pool, &familiar, &notes, &requests)));
    }

    #[test]
    fn prompt_has_context_and_numbered_pool() {
        let seed = song("Teardrop", "Massive Attack", 1998, "Trip-Hop");
        let played = vec![
            Played {
                song: song("Glory Box", "Portishead", 1994, "Trip-Hop"),
                skipped: true,
                by_user: false,
                mixed_in: None,
            },
            Played {
                song: song("Sour Times", "Portishead", 1994, "Trip-Hop"),
                skipped: false,
                by_user: false,
                mixed_in: Some("8-bar beat blend + bass swap from Glory Box".into()),
            },
        ];
        let pool = vec![
            song("Roads", "Portishead", 1994, "Trip-Hop"),
            song("Angel", "Massive Attack", 1998, "Trip-Hop"),
        ];
        let notes = HashMap::from([
            ("roads".to_string(), "76 bpm, 4A, energy 3/10, dark".to_string()),
            ("angel".to_string(), "78 bpm, 4A, energy 4/10; mix from now: 16-bar beat blend".to_string()),
        ]);
        let req = PlanRequest {
            persona: &PERSONAS[0],
            seed: &seed,
            seed_context: std::slice::from_ref(&pool[1]),
            listener_picks: std::slice::from_ref(&pool[0]),
            mood: Some("dunkel, langsam"),
            seed_mood: Some("dunkel, schwer"),
            played: &played,
            current: Some(&seed),
            last_line: Some("Das war Teardrop."),
            candidates: &pool,
            notes: &notes,
            familiar: &HashMap::from([
                ("roads".to_string(), "new to you".to_string()),
                ("angel".to_string(), "plays 4, last played 3 days ago".to_string()),
            ]),
            set_min: 2,
            set_max: 5,
            first: false,
            mode: BreakMode::Intro,
            duo: false,
            show: Show { language: "German", ..Show::default() },
            requests: &crate::steer::Requests::default(),
            requests_fresh: false,
        };
        let p = user_prompt(&req);
        assert!(!p.contains("requests right now"));
        let mut asked = crate::steer::Requests::default();
        asked.add(crate::steer::Steer::Chip(crate::steer::Chip::Calmer));
        let p2 = user_prompt(&PlanRequest { requests: &asked, requests_fresh: true, ..req.clone() });
        assert!(p2.contains("requests right now") && p2.contains("Calmer:") && p2.contains("acknowledge"));
        assert!(p.contains("Seed song (the mood to keep): Teardrop – Massive Attack (1998, Trip-Hop)"));
        assert!(p.contains("Auto-DJ would have played after it"));
        assert!(
            p.contains("Original mood of this session (from the seed — stay close to it): dunkel, schwer")
        );
        assert!(p.contains("Mood of your last set: dunkel, langsam"));
        assert!(
            system_instruction(&PERSONAS[0], &Show { language: "German", ..Show::default() })
                .contains("steer back")
        );
        assert!(p.contains("- Angel – Massive Attack (1998, Trip-Hop, 78 bpm, 4A, energy 4/10)\n"));
        assert!(p.contains("queued themselves this session"));
        assert!(p.contains("- Roads – Portishead (1994, Trip-Hop, 76 bpm"));
        assert!(
            system_instruction(&PERSONAS[0], &Show { language: "German", ..Show::default() })
                .contains("radio edit")
        );
        assert!(p.contains("Glory Box – Portishead (1994, Trip-Hop) [skipped by the listener]"));
        assert!(p.contains("Your previous break was: \"Das war Teardrop.\""));
        assert!(p.contains("pick 2–2 by number"));
        assert!(p.contains(
            "0. Roads – Portishead (1994, Trip-Hop, 76 bpm, 4A, energy 3/10, dark) [new to you]\n"
        ));
        assert!(p.contains("16-bar beat blend) [plays 4, last played 3 days ago]\n"));
        assert!(offers_new(&req) && p.contains("New to the listener: candidates 0. Put at least one"));
        assert!(!p.contains("- Roads – Portishead (1994, Trip-Hop, 76 bpm, 4A, energy 3/10, dark) [new"));
        assert!(
            system_instruction(&PERSONAS[0], &Show { language: "German", ..Show::default() })
                .contains("Camelot")
        );
        assert!(p.contains(
            "1. Angel – Massive Attack (1998, Trip-Hop, 78 bpm, 4A, energy 4/10; mix from now: 16-bar beat blend)"
        ));
        assert!(p.contains(
            "- Sour Times – Portishead (1994, Trip-Hop) [mixed in: 8-bar beat blend + bass swap from Glory Box]"
        ));
        let sys = system_instruction(&PERSONAS[0], &Show { language: "German", ..Show::default() });
        assert!(sys.contains("mix from now") && sys.contains("never talk about the mixing"));
        assert!(sys.contains("instrumental intro"));
        assert!(sys.contains("introduce one of them with one short story line") && sys.contains("from now:"));
        assert!(
            system_instruction(&PERSONAS[0], &Show { language: "German", ..Show::default() })
                .contains("German")
        );
        for persona in &PERSONAS {
            let show = Show { location: "Kiel", notes: "My name is Sam.", ..Show::default() };
            let sys = system_instruction(persona, &show);
            assert!(
                sys.contains("airs in Kiel")
                    && sys.contains("My name is Sam.")
                    && sys.contains("Speak English")
            );
            let sys = system_instruction(persona, &Show { location: " ", ..Show::default() });
            assert!(sys.contains("no particular home town") && !sys.contains("Notes from the listener"));
            assert!(!persona.sample.contains("Hamburg"));
        }
        assert!(p.contains("FIRST SONG OF YOUR NEW SET starts quietly"));
        assert!(p.contains("under 10 seconds"));
    }

    #[test]
    fn parse_repairs_sloppy_answers() {
        let text = r#"```json
{"picks": [3, 3, "1", 99, 0, 4, 2], "mood": " ruhig ", "line": "**Das** war  schön.", "style": "calm"}
```"#;
        let plan = parse_plan(text, 5, 2, 3).unwrap();
        assert_eq!(plan.picks, vec![3, 1, 0]);
        assert_eq!(plan.mood, "ruhig");
        assert_eq!(plan.line(), "Das war schön.");
        assert_eq!(plan.introduces, "");
        let intro = parse_plan(r#"{"picks": [0], "line": "x", "introduces": " Roads "}"#, 2, 1, 2).unwrap();
        assert_eq!(intro.introduces, "Roads");
        let noted = parse_plan(
            r#"{"picks": [2, 2, 9, 0], "notes": ["  Same **key**, a bit faster ", "dup", "bad", "x"], "line": "x"}"#,
            4,
            3,
            5,
        )
        .unwrap();
        assert_eq!(noted.picks, vec![2, 0, 1]);
        assert_eq!(noted.notes, vec!["Same key, a bit faster".to_string(), "x".into(), String::new()]);
        assert_eq!(intro.notes, vec![String::new()]);

        let few = parse_plan(r#"{"picks": [2], "mood": "", "line": "x", "style": ""}"#, 4, 3, 5).unwrap();
        assert_eq!(few.picks, vec![2, 0, 1]);

        let tiny_pool = parse_plan(r#"{"picks": [0, 1, 2], "line": "x"}"#, 1, 2, 5).unwrap();
        assert_eq!(tiny_pool.picks, vec![0]);

        assert!(parse_plan("not json", 5, 2, 5).is_err());
    }

    #[test]
    fn schema_bounds() {
        let s = response_schema(&PERSONAS[0], 2, 5, false);
        assert!(s["properties"].get("introduces").is_none());
        let n = response_schema(&PERSONAS[0], 2, 5, true);
        assert_eq!(n["properties"]["introduces"]["type"], "string");
        assert!(n["required"].as_array().unwrap().contains(&json!("introduces")));
        assert_eq!(s["properties"]["picks"]["minItems"], 2);
        assert_eq!(s["properties"]["picks"]["maxItems"], 5);
        assert_eq!(s["properties"]["notes"]["maxItems"], 5);
        assert!(s["required"].as_array().unwrap().contains(&json!("notes")));
        let duo = crate::persona::Persona::by_id("hamburg_duo");
        let s = response_schema(duo, 2, 5, true);
        assert!(s["required"].as_array().unwrap().contains(&json!("introduces")));
        assert_eq!(
            s["properties"]["turns"]["items"]["properties"]["speaker"]["enum"],
            json!(["Hauke", "Merle"])
        );
    }

    #[test]
    fn two_host_turns() {
        let text = r#"{"picks": [0, 1], "mood": "wach", "turns": [
            {"speaker": "Hauke", "text": "Moin, das war **stark**!", "style": "excited"},
            {"speaker": "Merle", "text": "Und jetzt wird's noch besser.", "style": "teasing"},
            {"speaker": "Merle", "text": "  ", "style": ""}]}"#;
        let plan = parse_plan(text, 3, 2, 5).unwrap();
        assert_eq!(plan.turns.len(), 2);
        assert_eq!(plan.line(), "Hauke: Moin, das war stark! Merle: Und jetzt wird's noch besser.");
        let solo = parse_plan(
            r#"{"picks": [0, 1], "turns": [{"speaker": "Hauke", "text": "Moin!", "style": ""}]}"#,
            3,
            2,
            5,
        )
        .unwrap();
        assert_eq!(solo.line(), "Moin!");
        let duo = crate::persona::Persona::by_id("hamburg_duo");
        assert!(system_instruction(duo, &Show::default()).contains("Merle"));
        assert!(!system_instruction(duo, &Show::default()).contains("mentions a mix"));
        assert!(BreakMode::Across.instruction().contains("blend of both songs"));
    }
}
