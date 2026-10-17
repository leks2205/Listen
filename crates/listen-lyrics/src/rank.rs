//! Ranking search results against the song being looked up (Feishin's
//! `orderSearchResults`): the score is 0 for a perfect match and 1 for none; the
//! name and the artist both have to fit, so the worse of the two counts.

use crate::providers::{Query, SearchHit};

/// Results scoring above this are not trusted for the automatic lookup.
pub const AUTO_THRESHOLD: f32 = 0.55;

/// Lowercase, letters and digits only, single spaces.
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = true;
    for c in s.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            out.push(c);
            space = false;
        } else if !space {
            out.push(' ');
            space = true;
        }
    }
    out.trim_end().to_string()
}

fn levenshtein(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur.push((prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// How badly `candidate` fits `wanted` (0 = same).
pub fn distance(wanted: &str, candidate: &str) -> f32 {
    let (w, c) = (normalize(wanted), normalize(candidate));
    if w.is_empty() {
        return 0.0;
    }
    if c.is_empty() {
        return 1.0;
    }
    if w == c {
        return 0.0;
    }
    let (wl, cl) = (w.chars().count() as f32, c.chars().count() as f32);
    // One contains the other as whole words ("Song" in "Song (Remastered)").
    let padded = |s: &str| format!(" {s} ");
    if padded(&c).contains(&padded(&w)) || padded(&w).contains(&padded(&c)) {
        return 0.05 + 0.4 * (1.0 - wl.min(cl) / wl.max(cl));
    }
    let (a, b): (Vec<char>, Vec<char>) = (w.chars().collect(), c.chars().collect());
    levenshtein(&a, &b) as f32 / a.len().max(b.len()) as f32
}

/// The score of one hit for the query.
pub fn score(q: &Query, hit: &SearchHit) -> f32 {
    let name = distance(&q.name, &hit.name);
    let artist = if hit.artist.is_empty() { 0.0 } else { distance(&q.artist, &hit.artist) };
    name.max(artist).clamp(0.0, 1.0)
}

/// Scores the hits and orders them: synchronized lyrics first, then by score.
pub fn order(q: &Query, mut hits: Vec<SearchHit>) -> Vec<SearchHit> {
    for h in &mut hits {
        h.score = score(q, h);
    }
    hits.sort_by(|a, b| {
        (b.synced == Some(true))
            .cmp(&(a.synced == Some(true)))
            .then(a.score.partial_cmp(&b.score).unwrap_or(std::cmp::Ordering::Equal))
    });
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::Source;

    fn hit(name: &str, artist: &str, synced: Option<bool>) -> SearchHit {
        SearchHit {
            source: Source::Lrclib,
            id: name.into(),
            artist: artist.into(),
            name: name.into(),
            synced,
            score: 1.0,
        }
    }

    fn q(name: &str, artist: &str) -> Query {
        Query { name: name.into(), artist: artist.into(), ..Query::default() }
    }

    #[test]
    fn normalizes() {
        assert_eq!(normalize("  Don't Stop (Me) Now! "), "don t stop me now");
        assert_eq!(normalize("夜に駆ける"), "夜に駆ける");
    }

    #[test]
    fn exact_and_contained_names_score_low() {
        assert_eq!(distance("Song", "song"), 0.0);
        assert!(distance("Song", "Song (Remastered 2011)") < 0.45);
        assert!(distance("Song", "Totally Different") > 0.55);
        assert_eq!(distance("", "anything"), 0.0);
    }

    #[test]
    fn the_worse_of_name_and_artist_counts() {
        let good = hit("Never Gonna Give You Up", "Rick Astley", Some(false));
        let wrong_artist = hit("Never Gonna Give You Up", "Cover Band X", Some(false));
        let query = q("Never Gonna Give You Up", "Rick Astley");
        assert!(score(&query, &good) < 0.01);
        assert!(score(&query, &wrong_artist) > AUTO_THRESHOLD);
    }

    #[test]
    fn synced_first_then_score() {
        let query = q("Song", "Artist");
        let hits = vec![
            hit("Song", "Artist", Some(false)),
            hit("Song (live)", "Artist", Some(true)),
            hit("Song", "Artist", None),
        ];
        let ordered = order(&query, hits);
        assert_eq!(ordered[0].synced, Some(true));
        assert_eq!(ordered[1].synced, Some(false));
        assert!(ordered[1].score <= ordered[2].score + f32::EPSILON);
    }
}
