//! The "Add to playlist" page of the context menu: a sticky fuzzy search, the
//! playlist songs were added to last pinned on top (Feishin
//! `use-recent-playlists`), and a "Skip duplicates" switch that adds only the
//! songs a playlist does not have yet (`add-to-playlist-action.tsx`).

use super::*;
use listen_core::model::SourceKind;

/// How well `query` matches `text` as a fuzzy (in-order) match; `None` if it does not.
/// Higher is better: consecutive letters, word starts and an early start score more.
pub fn fuzzy_score(query: &str, text: &str) -> Option<i32> {
    let q: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect();
    if q.is_empty() {
        return Some(0);
    }
    let t: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
    let mut score = 0;
    let mut at = 0;
    let mut previous: Option<usize> = None;
    for c in q {
        let found = (at..t.len()).find(|&i| t[i] == c)?;
        score += 10;
        if previous.is_some_and(|p| p + 1 == found) {
            score += 15;
        }
        if found == 0 || !t[found - 1].is_alphanumeric() {
            score += 8;
        }
        if previous.is_none() {
            score -= found.min(20) as i32;
        }
        previous = Some(found);
        at = found + 1;
    }
    Some(score)
}

/// The playlists the picker shows: those matching `query` (best first), with the
/// `recent` playlist on top when it matches.
pub fn ordered<'a>(playlists: &'a [Playlist], query: &str, recent: Option<&str>) -> Vec<&'a Playlist> {
    let mut scored: Vec<(i32, usize, &Playlist)> = playlists
        .iter()
        .enumerate()
        .filter_map(|(i, p)| fuzzy_score(query, &p.name).map(|s| (s, i, p)))
        .collect();
    if !query.trim().is_empty() {
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    }
    let mut out: Vec<&Playlist> = scored.into_iter().map(|(_, _, p)| p).collect();
    if let Some(i) = recent.and_then(|r| out.iter().position(|p| p.id == r)) {
        let p = out.remove(i);
        out.insert(0, p);
    }
    out
}

/// `ids` without those in `existing` and without repeats, in order.
pub fn new_songs(ids: &[String], existing: &[String]) -> Vec<String> {
    let mut seen: std::collections::HashSet<&str> = existing.iter().map(String::as_str).collect();
    ids.iter().filter(|i| seen.insert(i.as_str())).cloned().collect()
}

/// `songs` whose id is in `ids`, in `songs` order (for the "Added N songs" text).
fn shown_songs(songs: &[Song], ids: &[String]) -> Vec<Song> {
    let have: std::collections::HashSet<&str> = ids.iter().map(String::as_str).collect();
    songs.iter().filter(|s| have.contains(s.id.as_str())).cloned().collect()
}

/// "“Title”" or "N songs".
fn what(songs: &[Song], n: usize) -> String {
    if n == 1 {
        songs.first().map_or_else(|| "1 song".into(), |s| format!("“{}”", s.title))
    } else {
        format!("{n} songs")
    }
}

impl Listen {
    /// Demo mode has no server: tells what would have happened.
    fn demo_added(&mut self, server: &ServerId, id: &str, name: &str, n: usize) -> Task<Message> {
        let created = id.is_empty().then(|| format!("demo-new-{}", self.library.playlists.len()));
        if let Some(new_id) = &created {
            let mut p = demo::navidrome_playlist().playlist;
            p.id = new_id.clone();
            p.name = name.to_string();
            p.song_count = Some(n as u32);
            self.library.playlists.push(p);
        }
        self.playlist_page.recent.insert(server.clone(), created.clone().unwrap_or_else(|| id.to_string()));
        let text = match created {
            Some(_) => format!("Created “{name}” with {n} songs."),
            None => format!("Added {n} songs to “{name}”."),
        };
        self.toast_success(text);
        Task::none()
    }

    /// Adds `songs` to playlist `id` (an empty id makes a new playlist called `name`),
    /// remembers it as the most recent one and tells how it went.
    pub(in crate::app) fn add_songs_to_playlist(
        &mut self,
        id: String,
        name: String,
        songs: Vec<Song>,
    ) -> Task<Message> {
        let server = songs[0].server_id.clone();
        let Some(source) = self.sources.get(&server).cloned() else {
            if self.demo {
                // Nothing to send; the list of playlists is local.
                return self.demo_added(&server, &id, &name, songs.len());
            }
            // Never a success notice for an add that reached no server.
            let server_name = self
                .config
                .servers
                .iter()
                .find(|s| s.id == server)
                .map_or_else(|| "the server".to_string(), |s| format!("“{}”", s.name));
            self.toast_error(format!("Not connected to {server_name} — try again in a moment."));
            return Task::none();
        };
        let ytm = songs[0].source == SourceKind::YouTubeMusic;
        let skip = self.config.settings.lists.skip_duplicates && !ytm && !id.is_empty();
        let browsing = self.config.active_server.as_ref() == Some(&server);
        // Adding to the playlist on screen: its rows and song count changed, so the page is
        // loaded again afterwards (dropping the detail without that left the page empty).
        let reopen = browsing && self.library.playlist.as_ref().is_some_and(|p| p.playlist.id == id);
        let reopen_id = id.clone();
        if browsing && !reopen {
            self.library.playlist = self.library.playlist.take().filter(|p| p.playlist.id != id);
        }
        if !id.is_empty() {
            self.playlist_page.recent.insert(server.clone(), id.clone());
        }
        let ids: Vec<String> = songs.iter().map(|s| s.id.clone()).collect();
        let add = Task::perform(
            async move {
                if id.is_empty() {
                    let n = ids.len();
                    return source
                        .create_playlist(&name, &ids)
                        .await
                        .map(|p| (Some(p.id), format!("Created “{name}” with {}.", what(&songs, n))))
                        .map_err(|e| format!("Could not add to the playlist: {e}"));
                }
                let mut ids = ids;
                if skip {
                    let have: Vec<String> = source
                        .playlist_songs(&id)
                        .await
                        .map_err(|e| format!("Could not read “{name}”: {e}"))?
                        .into_iter()
                        .map(|s| s.id)
                        .collect();
                    ids = new_songs(&ids, &have);
                    if ids.is_empty() {
                        return Ok((None, format!("Nothing to add: “{name}” has these songs already.")));
                    }
                }
                let n = ids.len();
                let shown = shown_songs(&songs, &ids);
                source
                    .add_to_playlist(&id, &ids)
                    .await
                    .map(|_| (None, format!("Added {} to “{name}”.", what(&shown, n))))
                    .map_err(|e| format!("Could not add to the playlist: {e}"))
            },
            move |r: Result<(Option<String>, String), String>| {
                let (created, text) = match r {
                    Ok((created, text)) => (created, Ok(text)),
                    Err(e) => (None, Err(e)),
                };
                Message::PlaylistPage(PlaylistPageMsg::Added(server.clone(), created, text))
            },
        );
        if reopen {
            Task::batch([add, self.open_playlist(reopen_id), self.load_playlists()])
        } else if browsing {
            add.chain(self.load_playlists())
        } else {
            add
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playlist(id: &str, name: &str) -> Playlist {
        let mut p = crate::demo::navidrome_playlist().playlist;
        p.id = id.into();
        p.name = name.into();
        p
    }

    fn names<'a>(l: &[&'a Playlist]) -> Vec<&'a str> {
        l.iter().map(|p| p.name.as_str()).collect()
    }

    #[test]
    fn fuzzy_matches_in_order_and_prefers_tight_matches() {
        assert!(fuzzy_score("rdt", "Road Trip").is_some());
        assert!(fuzzy_score("xyz", "Road Trip").is_none());
        assert!(fuzzy_score("rt", "Rtrip").unwrap() > fuzzy_score("rt", "Road Trip").unwrap() - 1);
        // Case and spaces do not matter; consecutive letters beat scattered ones.
        assert!(fuzzy_score("road", "ROAD TRIP").unwrap() > fuzzy_score("road", "r o a d").unwrap());
        assert_eq!(fuzzy_score("", "anything"), Some(0));
        assert_eq!(fuzzy_score("  ", "anything"), Some(0));
    }

    #[test]
    fn the_recent_playlist_is_pinned_and_the_best_match_comes_first() {
        let l = vec![
            playlist("1", "Gym"),
            playlist("2", "Road Trip"),
            playlist("3", "Chill"),
            playlist("4", "Gym 2"),
        ];
        assert_eq!(names(&ordered(&l, "", None)), ["Gym", "Road Trip", "Chill", "Gym 2"]);
        assert_eq!(names(&ordered(&l, "", Some("3"))), ["Chill", "Gym", "Road Trip", "Gym 2"]);
        assert_eq!(names(&ordered(&l, "gym", None)), ["Gym", "Gym 2"]);
        // The pinned playlist only stays on top while it matches the search.
        assert_eq!(names(&ordered(&l, "gym", Some("3"))), ["Gym", "Gym 2"]);
        assert_eq!(names(&ordered(&l, "gym", Some("4"))), ["Gym 2", "Gym"]);
        assert!(ordered(&l, "zzz", None).is_empty());
    }

    #[test]
    fn duplicates_are_skipped_once() {
        let ids: Vec<String> = ["a", "b", "a", "c"].map(String::from).to_vec();
        let have: Vec<String> = ["c"].map(String::from).to_vec();
        assert_eq!(new_songs(&ids, &have), ["a", "b"]);
        assert!(new_songs(&ids, &ids).is_empty());
    }

    /// The old per-song `Vec::contains` scan, kept as the equivalence reference.
    fn shown_songs_old(songs: &[Song], ids: &[String]) -> Vec<Song> {
        songs.iter().filter(|s| ids.contains(&s.id)).cloned().collect()
    }

    #[test]
    fn shown_songs_matches_the_linear_scan_order_and_message() {
        let mut songs: Vec<Song> = (0..6)
            .map(|i| {
                let mut s = crate::demo::album_songs(0).remove(0);
                s.id = format!("id-{i}");
                s.title = format!("Title {i}");
                s
            })
            .collect();
        songs[2].id = "id-1".into(); // duplicate id inside `songs`
        // Selection with repeats and ids that are not in `songs`.
        let ids: Vec<String> =
            ["id-1", "id-4", "missing", "id-2", "id-1", "id-9", "id-0"].map(String::from).to_vec();
        let new = shown_songs(&songs, &ids);
        let old = shown_songs_old(&songs, &ids);
        assert_eq!(
            new.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            old.iter().map(|s| s.id.as_str()).collect::<Vec<_>>()
        );
        assert_eq!(new.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["id-0", "id-1", "id-1", "id-4"]);
        assert_eq!(what(&new, ids.len()), what(&old, ids.len()));
        assert_eq!(what(&new, 7), "7 songs");
    }
}
