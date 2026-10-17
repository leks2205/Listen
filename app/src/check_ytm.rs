//! `listen --check-ytm-session [--write]`: exercises the stored YouTube Music session's write paths
//! (like/unlike, a throwaway playlist) with a read-back after each step, restoring everything it changes.
//! Without `--write` it only prints the plan. Prints ✓ / ✗ / – per step; non-zero exit on failure.
//! Paced like a person (random pauses, few read-backs) since it runs against a real account.

use listen_core::model::{ServerId, Song, SourceKind};
use listen_source::{Capabilities, MusicSource, SongQuery};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Pass,
    Fail(String),
    /// Not applicable for this source (e.g. it cannot remove songs from playlists).
    Skip(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub name: String,
    pub outcome: Outcome,
}

impl Step {
    fn line(&self) -> String {
        match &self.outcome {
            Outcome::Pass => format!("✓ {}", self.name),
            Outcome::Fail(why) => format!("✗ {}: {why}", self.name),
            Outcome::Skip(why) => format!("– {} (skipped: {why})", self.name),
        }
    }
}

/// What `--write` does, one line per step.
pub const PLAN: &[&str] = &[
    "pick a library song that is not liked (none → the like steps are skipped)",
    "like it, read the first page of Liked Music back",
    "unlike it again, read its like status in the library back",
    "create the playlist “Listen check <timestamp>”, read the playlist list back",
    "add the song to it, read the playlist back",
    "remove the song again (skipped if the source cannot), read the playlist back",
    "delete the playlist, read the playlist list back",
];

/// Read-back retries and pause ranges between steps (YouTube shows its own writes with a delay).
#[derive(Debug, Clone, Copy)]
pub struct Settle {
    pub tries: u32,
    pub wait: (Duration, Duration),
    pub pause: (Duration, Duration),
}

impl Settle {
    pub const LIVE: Settle = Settle {
        // A fresh playlist or an unlike can take over 15 s to show.
        tries: 5,
        wait: (Duration::from_secs(6), Duration::from_secs(12)),
        pause: (Duration::from_secs(4), Duration::from_secs(10)),
    };
}

/// Sleeps a random time within `range`.
async fn breathe(range: (Duration, Duration)) {
    let (lo, hi) = range;
    if hi.is_zero() {
        return;
    }
    let ms = rand::random_range(lo.as_millis() as u64..=hi.as_millis().max(lo.as_millis()) as u64);
    tokio::time::sleep(Duration::from_millis(ms)).await;
}

async fn eventually<F, Fut>(settle: Settle, mut check: F) -> Result<bool, String>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<bool, String>>,
{
    for attempt in 0..settle.tries.max(1) {
        if attempt > 0 {
            breathe(settle.wait).await;
        }
        if check().await? {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn is_liked(source: &dyn MusicSource, id: &str) -> Result<bool, String> {
    let liked = source
        .songs(&SongQuery { favorites_only: true, limit: 100, ..Default::default() })
        .await
        .map_err(|e| format!("reading the liked songs: {e}"))?;
    Ok(liked.items.iter().any(|s| s.id == id))
}

/// The song's like status in the library listing (updated at once, unlike Liked Music).
async fn liked_in_library(source: &dyn MusicSource, id: &str) -> Result<bool, String> {
    let lib = source
        .songs(&SongQuery { limit: 100_000, ..Default::default() })
        .await
        .map_err(|e| format!("reading the library: {e}"))?;
    Ok(lib.items.iter().any(|s| s.id == id && s.favorite))
}

async fn playlist_has_song(source: &dyn MusicSource, playlist: &str, id: &str) -> Result<bool, String> {
    let songs = source.playlist_songs(playlist).await.map_err(|e| format!("reading the playlist: {e}"))?;
    Ok(songs.iter().any(|s| s.id == id))
}

async fn playlist_listed(source: &dyn MusicSource, playlist: &str) -> Result<bool, String> {
    let all = source.playlists().await.map_err(|e| format!("reading the playlists: {e}"))?;
    Ok(all.iter().any(|p| p.id == playlist))
}

fn step(name: &str, outcome: Outcome) -> Step {
    Step { name: name.into(), outcome }
}

fn res(name: &str, r: Result<(), String>) -> Step {
    step(name, r.map_or_else(Outcome::Fail, |()| Outcome::Pass))
}

/// Runs the write checks against `source`, reporting each step; restores everything, also on failure.
pub async fn run(
    source: &dyn MusicSource,
    playlist_name: &str,
    settle: Settle,
    mut report: impl FnMut(&Step),
) -> Vec<Step> {
    let mut steps: Vec<Step> = Vec::new();
    let mut push = |s: Step, steps: &mut Vec<Step>| {
        report(&s);
        steps.push(s);
    };

    // Pick a song that isn't liked, so existing likes are never removed.
    let page = match source.songs(&SongQuery { limit: 50, ..Default::default() }).await {
        Ok(page) if !page.items.is_empty() => page.items,
        Ok(_) => {
            push(step("pick a song", Outcome::Fail("the library has no songs".into())), &mut steps);
            return steps;
        }
        Err(e) => {
            push(step("pick a song", Outcome::Fail(e.to_string())), &mut steps);
            return steps;
        }
    };
    let song: Song = page.iter().find(|s| !s.favorite).unwrap_or(&page[0]).clone();
    if song.favorite {
        push(
            step("like / unlike", Outcome::Skip("no unliked song on the first library page".into())),
            &mut steps,
        );
    } else {
        push(step(&format!("picked “{}” (not liked)", song.title), Outcome::Pass), &mut steps);
        breathe(settle.pause).await;
        let liked = async {
            source.set_favorite(std::slice::from_ref(&song.id), true).await.map_err(|e| e.to_string())?;
            if eventually(settle, || is_liked(source, &song.id)).await? {
                Ok(())
            } else {
                Err("not in Liked Music after liking it".to_string())
            }
        }
        .await;
        let did_like = liked.is_ok();
        push(res("like", liked), &mut steps);
        breathe(settle.pause).await;
        // Unlike also after a failed read-back: the like itself may have worked.
        let r = async {
            source.set_favorite(std::slice::from_ref(&song.id), false).await.map_err(|e| e.to_string())?;
            if eventually(settle, || async { Ok(!liked_in_library(source, &song.id).await?) }).await? {
                Ok(())
            } else {
                Err("still in Liked Music after unliking it".into())
            }
        }
        .await;
        push(res(if did_like { "unlike again (restore)" } else { "unlike (cleanup)" }, r), &mut steps);
    }
    breathe(settle.pause).await;

    // Throwaway playlist.
    let created = source.create_playlist(playlist_name, &[]).await;
    let id = match created {
        Ok(p) => p.id,
        Err(e) => {
            push(step("create the playlist", Outcome::Fail(e.to_string())), &mut steps);
            return steps;
        }
    };
    // Open the new playlist directly: the library list shows it only later.
    let r = match eventually(settle, || async { Ok(source.playlist_songs(&id).await.is_ok()) }).await {
        Ok(true) => Ok(()),
        Ok(false) => Err("the new playlist does not open".into()),
        Err(e) => Err(e),
    };
    push(res(&format!("create “{playlist_name}”"), r), &mut steps);
    breathe(settle.pause).await;

    let r = async {
        source.add_to_playlist(&id, std::slice::from_ref(&song.id)).await.map_err(|e| e.to_string())?;
        if eventually(settle, || playlist_has_song(source, &id, &song.id)).await? {
            Ok(())
        } else {
            Err("the song is not in the playlist".into())
        }
    }
    .await;
    push(res("add the song", r), &mut steps);
    breathe(settle.pause).await;

    if source.capabilities().contains(Capabilities::PLAYLIST_REMOVE) {
        let r = async {
            let songs = source.playlist_songs(&id).await.map_err(|e| e.to_string())?;
            let at = songs.iter().position(|s| s.id == song.id).ok_or("the song is not in the playlist")?;
            source.remove_from_playlist(&id, &[at]).await.map_err(|e| e.to_string())?;
            if eventually(settle, || async { Ok(!playlist_has_song(source, &id, &song.id).await?) }).await? {
                Ok(())
            } else {
                Err("the song is still in the playlist".into())
            }
        }
        .await;
        push(res("remove the song", r), &mut steps);
    } else {
        push(
            step("remove the song", Outcome::Skip("this source cannot remove songs from a playlist".into())),
            &mut steps,
        );
    }

    breathe(settle.pause).await;
    let r = async {
        source.delete_playlist(&id).await.map_err(|e| e.to_string())?;
        if eventually(settle, || async { Ok(!playlist_listed(source, &id).await?) }).await? {
            Ok(())
        } else {
            Err("the playlist is still listed".into())
        }
    }
    .await;
    push(res("delete the playlist", r), &mut steps);
    steps
}

/// True when no step failed.
pub fn ok(steps: &[Step]) -> bool {
    steps.iter().all(|s| !matches!(s.outcome, Outcome::Fail(_)))
}

/// Entry point; returns the process exit code.
pub fn session(write: bool) -> i32 {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    rt.block_on(session_async(write)).unwrap_or_else(|e| {
        eprintln!("✗ {e}");
        1
    })
}

async fn session_async(write: bool) -> Result<i32, String> {
    let paths = listen_core::config::Paths::discover().map_err(|e| e.to_string())?;
    let config = listen_core::config::Config::load(&paths.config_file()).map_err(|e| e.to_string())?;
    let is_ytm = |s: &&listen_core::config::ServerConfig| s.kind == SourceKind::YouTubeMusic;
    let server = config
        .active()
        .filter(is_ytm)
        .or_else(|| config.servers.iter().find(is_ytm))
        .ok_or("no YouTube Music account configured (sign in once in Listen)")?;
    println!("account: {}", server.name);
    if !write {
        println!("dry run (add --write to run it against this account):");
        for line in PLAN {
            println!("  would {line}");
        }
        return Ok(0);
    }
    let secret = {
        let (dir, id) = (paths.config_dir.clone(), server.id.clone());
        tokio::task::spawn_blocking(move || crate::secrets::Secrets::new(&dir).get(&id))
            .await
            .map_err(|e| e.to_string())?
            .ok_or("no stored YouTube Music session (sign in once in Listen)")?
    };
    let yt = listen_ytmusic::YouTubeMusic::connect(ServerId(server.id.0.clone()), &secret)
        .await
        .map_err(|e| format!("sign-in: {e}"))?;
    if yt.is_guest() {
        return Err("the stored session is a guest session; sign in to YouTube Music in Listen first".into());
    }
    yt.ping().await.map_err(|e| format!("signed-in request: {e}"))?;
    println!("✓ signed in");
    let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    let name = format!("Listen check {stamp}");
    let steps = run(&yt, &name, Settle::LIVE, |s| println!("{}", s.line())).await;
    let failed = steps.iter().filter(|s| matches!(s.outcome, Outcome::Fail(_))).count();
    if failed == 0 {
        println!("all write checks passed");
        Ok(0)
    } else {
        println!("{failed} step(s) failed");
        Ok(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use listen_core::model::{Album, Artist, ImageRef, Page, Playlist, SearchResults};
    use listen_source::{AlbumQuery, ListQuery, Result as SrcResult, SourceError, StreamSource};
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Mutex;

    #[derive(Default)]
    struct State {
        liked: BTreeSet<String>,
        playlists: BTreeMap<String, (String, Vec<String>)>,
        next: u32,
    }

    /// An in-memory account. `lie_about_likes`: likes are accepted but never show up.
    struct Fake {
        state: Mutex<State>,
        can_remove: bool,
        lie_about_likes: bool,
        songs: Vec<Song>,
    }

    fn song(id: &str) -> Song {
        let mut s = crate::demo::album_songs(0).remove(0);
        s.id = id.into();
        s.title = format!("Song {id}");
        s
    }

    fn fake(can_remove: bool, lie: bool, liked: &[&str]) -> Fake {
        Fake {
            state: Mutex::new(State {
                liked: liked.iter().map(|s| s.to_string()).collect(),
                ..Default::default()
            }),
            can_remove,
            lie_about_likes: lie,
            songs: vec![song("a"), song("b")],
        }
    }

    fn page<T>(items: Vec<T>) -> Page<T> {
        Page { total: Some(items.len()), items, start_index: 0 }
    }

    #[async_trait]
    impl MusicSource for Fake {
        fn kind(&self) -> SourceKind {
            SourceKind::YouTubeMusic
        }
        fn capabilities(&self) -> Capabilities {
            if self.can_remove {
                Capabilities::PLAYLIST_EDIT | Capabilities::PLAYLIST_REMOVE
            } else {
                Capabilities::PLAYLIST_EDIT
            }
        }
        async fn ping(&self) -> SrcResult<()> {
            Ok(())
        }
        async fn albums(&self, _: &AlbumQuery) -> SrcResult<Page<Album>> {
            Ok(page(vec![]))
        }
        async fn album(&self, _: &str) -> SrcResult<Album> {
            Err(SourceError::NotFound)
        }
        async fn artists(&self, _: &ListQuery) -> SrcResult<Page<Artist>> {
            Ok(page(vec![]))
        }
        async fn artist(&self, _: &str) -> SrcResult<Artist> {
            Err(SourceError::NotFound)
        }
        async fn artist_albums(&self, _: &str) -> SrcResult<Vec<Album>> {
            Ok(vec![])
        }
        async fn top_songs(&self, _: &Artist, _: usize) -> SrcResult<Vec<Song>> {
            Ok(vec![])
        }
        async fn songs(&self, q: &SongQuery) -> SrcResult<Page<Song>> {
            let st = self.state.lock().unwrap();
            let items = self
                .songs
                .iter()
                .filter(|s| !q.favorites_only || st.liked.contains(&s.id))
                // Like YouTube Music: library songs carry their like status.
                .map(|s| Song { favorite: st.liked.contains(&s.id), ..s.clone() })
                .collect();
            Ok(page(items))
        }
        async fn random_songs(&self, _: usize, _: Option<&str>) -> SrcResult<Vec<Song>> {
            Ok(vec![])
        }
        async fn search(&self, _: &str, _: usize) -> SrcResult<SearchResults> {
            Ok(SearchResults::default())
        }
        async fn playlists(&self) -> SrcResult<Vec<Playlist>> {
            let st = self.state.lock().unwrap();
            Ok(st
                .playlists
                .iter()
                .map(|(id, (name, _))| Playlist {
                    id: id.clone(),
                    server_id: ServerId("t".into()),
                    source: SourceKind::YouTubeMusic,
                    name: name.clone(),
                    description: None,
                    owner: None,
                    image: None,
                    song_count: None,
                    duration: Duration::ZERO,
                    public: false,
                    editable: true,
                })
                .collect())
        }
        async fn playlist_songs(&self, id: &str) -> SrcResult<Vec<Song>> {
            let st = self.state.lock().unwrap();
            let (_, ids) = st.playlists.get(id).ok_or(SourceError::NotFound)?;
            Ok(ids.iter().map(|i| song(i)).collect())
        }
        async fn create_playlist(&self, name: &str, songs: &[String]) -> SrcResult<Playlist> {
            let id = {
                let mut st = self.state.lock().unwrap();
                st.next += 1;
                let id = format!("PL{}", st.next);
                st.playlists.insert(id.clone(), (name.into(), songs.to_vec()));
                id
            };
            Ok(self.playlists().await?.into_iter().find(|p| p.id == id).unwrap())
        }
        async fn add_to_playlist(&self, id: &str, songs: &[String]) -> SrcResult<()> {
            let mut st = self.state.lock().unwrap();
            st.playlists.get_mut(id).ok_or(SourceError::NotFound)?.1.extend(songs.iter().cloned());
            Ok(())
        }
        async fn remove_from_playlist(&self, id: &str, indices: &[usize]) -> SrcResult<()> {
            if !self.can_remove {
                return Err(SourceError::Unsupported);
            }
            let mut st = self.state.lock().unwrap();
            let list = &mut st.playlists.get_mut(id).ok_or(SourceError::NotFound)?.1;
            for i in indices.iter().rev() {
                list.remove(*i);
            }
            Ok(())
        }
        async fn delete_playlist(&self, id: &str) -> SrcResult<()> {
            self.state.lock().unwrap().playlists.remove(id).map(|_| ()).ok_or(SourceError::NotFound)
        }
        async fn set_favorite(&self, ids: &[String], favorite: bool) -> SrcResult<()> {
            if self.lie_about_likes {
                return Ok(());
            }
            let mut st = self.state.lock().unwrap();
            for id in ids {
                if favorite {
                    st.liked.insert(id.clone());
                } else {
                    st.liked.remove(id);
                }
            }
            Ok(())
        }
        async fn stream(&self, _: &Song) -> SrcResult<StreamSource> {
            Err(SourceError::Unsupported)
        }
        fn image_url(&self, _: &ImageRef, _: u32) -> Option<String> {
            None
        }
    }

    const QUICK: Settle =
        Settle { tries: 2, wait: (Duration::ZERO, Duration::ZERO), pause: (Duration::ZERO, Duration::ZERO) };

    #[tokio::test]
    async fn every_step_passes_and_the_account_ends_as_it_started() {
        for liked in [&["a"][..], &[][..]] {
            let f = fake(true, false, liked);
            let steps = run(&f, "Listen check test", QUICK, |_| {}).await;
            assert!(ok(&steps), "{steps:#?}");
            assert!(steps.iter().all(|s| s.outcome == Outcome::Pass), "{steps:#?}");
            let st = f.state.lock().unwrap();
            assert!(st.playlists.is_empty(), "the throwaway playlist is deleted");
            assert!(!st.liked.contains("b"), "the test like is undone");
            assert_eq!(st.liked.contains("a"), !liked.is_empty(), "liked state unchanged");
        }
    }

    #[tokio::test]
    async fn existing_likes_are_never_touched() {
        let f = fake(true, false, &["a", "b"]);
        let steps = run(&f, "Listen check test", QUICK, |_| {}).await;
        assert!(ok(&steps), "{steps:#?}");
        let skipped = steps.iter().find(|s| s.name == "like / unlike").unwrap();
        assert!(matches!(skipped.outcome, Outcome::Skip(_)));
        let st = f.state.lock().unwrap();
        assert!(st.liked.contains("a") && st.liked.contains("b"));
    }

    #[tokio::test]
    async fn a_source_that_cannot_remove_skips_that_step_and_still_deletes() {
        let f = fake(false, false, &[]);
        let steps = run(&f, "Listen check test", QUICK, |_| {}).await;
        assert!(ok(&steps), "{steps:#?}");
        let removed = steps.iter().find(|s| s.name == "remove the song").unwrap();
        assert!(matches!(removed.outcome, Outcome::Skip(_)));
        assert!(f.state.lock().unwrap().playlists.is_empty());
    }

    #[tokio::test]
    async fn a_like_that_does_not_show_up_fails_the_run() {
        let f = fake(true, true, &[]);
        let steps = run(&f, "Listen check test", QUICK, |_| {}).await;
        assert!(!ok(&steps));
        let like = steps.iter().find(|s| s.name == "like").unwrap();
        assert!(matches!(&like.outcome, Outcome::Fail(why) if why.contains("not in Liked Music")));
        // Later steps still ran and cleaned up.
        assert!(f.state.lock().unwrap().playlists.is_empty());
    }

    #[test]
    fn step_lines_use_check_marks() {
        let s = |o| Step { name: "x".into(), outcome: o }.line();
        assert_eq!(s(Outcome::Pass), "✓ x");
        assert!(s(Outcome::Fail("why".into())).starts_with("✗ x: why"));
        assert!(s(Outcome::Skip("n/a".into())).starts_with("– x"));
    }
}
