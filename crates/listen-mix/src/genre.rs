//! Genre rule for vocal transition edges: club music is mixed over vocal samples, other music is not.

use listen_core::settings::TransitionEdges;

/// Genre terms (lower case, words separated by one space) where vocal samples over the beat are normal.
/// Matched on whole words in order, so "house" hits "Deep House" but not "Greenhouse".
pub const CLUB_GENRE_TERMS: &[&str] = &[
    "garage",
    "uk garage",
    "speed garage",
    "2 step",
    "2step",
    "two step",
    "bassline",
    "grime",
    "house",
    "techno",
    "trance",
    "psytrance",
    "goa",
    "drum and bass",
    "dnb",
    "jungle",
    "dubstep",
    "brostep",
    "breakbeat",
    "breaks",
    "big beat",
    "electro",
    "electronic",
    "electronica",
    "edm",
    "dance",
    "eurodance",
    "club",
    "rave",
    "amapiano",
    "hardstyle",
    "gabber",
    "happy hardcore",
    "hi nrg",
    "italo",
    "nu disco",
    "disco house",
    "future bass",
    "uk bass",
    "bass music",
    "moombahton",
    "juke",
    "footwork",
    "baile funk",
];

/// A genre part with one of these words is a band or vocal genre even if it names a club term
/// ("Garage Rock", "Dance-Pop").
const NOT_CLUB_WORDS: &[&str] = &["rock", "punk", "metal", "pop", "folk", "country"];

/// Lower-case words of `s` joined by single spaces, with `&` read as "and".
fn words(s: &str) -> String {
    let lower = s.to_lowercase().replace('&', " and ");
    let parts: Vec<&str> = lower.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    parts.join(" ")
}

/// Whether one genre tag is a club genre. Tags like "Electronic/Dance" are checked part by part.
fn is_club_tag(tag: &str) -> bool {
    tag.split([';', ',', '/', '|']).map(words).any(|part| {
        let padded = format!(" {part} ");
        !part.split(' ').any(|w| NOT_CLUB_WORDS.contains(&w))
            && CLUB_GENRE_TERMS.iter().any(|t| padded.contains(&format!(" {t} ")))
    })
}

/// Whether any of `genres` is a club genre.
pub fn is_club(genres: &[String]) -> bool {
    genres.iter().any(|g| is_club_tag(g))
}

/// Whether vocals at a song's edge may be overlapped, given the setting and that song's genres.
pub fn relax_vocal_edge(mode: TransitionEdges, genres: &[String]) -> bool {
    match mode {
        TransitionEdges::Always => true,
        TransitionEdges::Instrumental => false,
        TransitionEdges::Smart => is_club(genres),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(tags: &[&str]) -> Vec<String> {
        tags.iter().map(|t| (*t).to_string()).collect()
    }

    #[test]
    fn club_genres_match_in_any_spelling() {
        for tag in [
            "UK Garage",
            "garage",
            "Garage House",
            "Speed Garage",
            "2-Step",
            "2 Step Garage",
            "Deep House",
            "Tech-House",
            "Progressive House",
            "Techno",
            "TRANCE",
            "Drum & Bass",
            "Drum and Bass",
            "DnB",
            "Jungle",
            "Dubstep",
            "Breakbeat",
            "Electro",
            "EDM",
            "Electronic/Dance",
            "Dance",
            "Club",
            "Bassline",
            "Grime",
            "Amapiano",
            "Hardstyle",
            "Afro House",
        ] {
            assert!(is_club(&g(&[tag])), "{tag}");
        }
        assert!(is_club(&g(&["Pop", "UK Garage"])));
    }

    #[test]
    fn other_genres_do_not_match() {
        for tag in [
            "Pop",
            "K-Pop",
            "Rock",
            "Garage Rock",
            "Dance-Pop",
            "Dancehall",
            "Greenhouse",
            "Hip-Hop",
            "Classical",
            "Soundtrack",
            "Metal",
            "Singer/Songwriter",
            "",
        ] {
            assert!(!is_club(&g(&[tag])), "{tag}");
        }
        assert!(!is_club(&[]));
    }

    #[test]
    fn decision_table() {
        use TransitionEdges::*;
        let club = g(&["UK Garage"]);
        let pop = g(&["Pop"]);
        let none: Vec<String> = Vec::new();
        for (mode, genres, relaxed) in [
            (Always, &club, true),
            (Always, &pop, true),
            (Always, &none, true),
            (Instrumental, &club, false),
            (Instrumental, &pop, false),
            (Instrumental, &none, false),
            (Smart, &club, true),
            (Smart, &pop, false),
            (Smart, &none, false),
        ] {
            assert_eq!(relax_vocal_edge(mode, genres), relaxed, "{mode:?} {genres:?}");
        }
    }
}
