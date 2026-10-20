use super::*;

fn file(
    rel: &str,
    title: &str,
    artist: &str,
    album: &str,
    album_artist: Option<&str>,
    track: u32,
) -> Scanned {
    Scanned {
        rel: rel.into(),
        path: PathBuf::from("/music").join(rel),
        title: title.into(),
        artists: vec![artist.into()],
        album: Some(album.into()),
        album_artist: album_artist.map(String::from),
        track: Some(track),
        genres: vec!["Rock".into()],
        duration: Duration::from_secs(100),
        ..Default::default()
    }
}

/// The pre-HashMap `Index::build` (linear album lookups), kept verbatim as the
/// equivalence reference the new implementation is tested against.
#[cfg(test)]
mod reference {
    use super::*;

    pub fn build(server: &ServerId, files: Vec<Scanned>) -> Index {
        // Group into albums: same album name and album artist; without an album
        // artist, tracks of the same folder (so unrelated "Greatest Hits" stay apart).
        let mut groups: HashMap<(String, String, String), Vec<usize>> = HashMap::new();
        for (i, f) in files.iter().enumerate() {
            let Some(album) = &f.album else { continue };
            let (artist, dir) = match &f.album_artist {
                Some(a) => (a.to_lowercase(), String::new()),
                None => (
                    String::new(),
                    f.path.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
                ),
            };
            groups.entry((album.to_lowercase(), artist, dir)).or_default().push(i);
        }

        let mut album_of: HashMap<usize, String> = HashMap::new();
        let mut album_cover: HashMap<String, Option<String>> = HashMap::new();
        let mut entries: Vec<(Album, Vec<usize>)> = Vec::new();
        let mut group_list: Vec<_> = groups.into_iter().collect();
        group_list.sort_by(|a, b| a.0.cmp(&b.0));
        for ((_, _, _), mut idx) in group_list {
            idx.sort_by_key(|&i| {
                (files[i].disc.unwrap_or(1), files[i].track.unwrap_or(0), files[i].rel.clone())
            });
            let first = &files[idx[0]];
            let album_name = first.album.clone().unwrap_or_default();
            let album_artist = match &first.album_artist {
                Some(a) => a.clone(),
                None => {
                    let distinct: HashSet<String> = idx
                        .iter()
                        .map(|&i| files[i].artists.first().cloned().unwrap_or_else(|| UNKNOWN_ARTIST.into()))
                        .collect();
                    if distinct.len() > 1 {
                        VARIOUS_ARTISTS.to_string()
                    } else {
                        distinct.into_iter().next().unwrap_or_else(|| UNKNOWN_ARTIST.into())
                    }
                }
            };
            let id = format!(
                "al-{:016x}",
                fnv1a(&format!(
                    "{}\u{1}{}\u{1}{}",
                    album_name.to_lowercase(),
                    album_artist.to_lowercase(),
                    first.path.parent().map(|p| p.to_string_lossy()).unwrap_or_default()
                ))
            );
            let cover = idx
                .iter()
                .find_map(|&i| files[i].folder_art.as_ref())
                .or_else(|| idx.iter().find(|&&i| files[i].embedded_art).map(|&i| &files[i].path))
                .and_then(|p| file_url(p));
            let mut genres: Vec<String> = Vec::new();
            for &i in &idx {
                for g in &files[i].genres {
                    if !genres.contains(g) {
                        genres.push(g.clone());
                    }
                }
            }
            for &i in &idx {
                album_of.insert(i, id.clone());
            }
            album_cover.insert(id.clone(), cover.clone());
            let album = Album {
                release_types: Vec::new(),
                id,
                server_id: server.clone(),
                source: SourceKind::Local,
                name: album_name,
                album_artists: refs(&[album_artist]),
                image: cover.map(ImageRef::Url),
                year: idx.iter().find_map(|&i| files[i].year),
                release_type: None,
                genres,
                song_count: Some(idx.len() as u32),
                duration: idx.iter().map(|&i| files[i].duration).sum(),
                favorite: false,
                rating: None,
                play_count: None,
                songs: Vec::new(),
                info: Default::default(),
            };
            entries.push((album, idx));
        }

        // Tracks, in file order; `files` index -> tracks index is the identity.
        let mut tracks: Vec<Track> = Vec::with_capacity(files.len());
        for (i, f) in files.iter().enumerate() {
            let album_id = album_of.get(&i).cloned();
            let album_artists = album_id
                .as_ref()
                .and_then(|id| entries.iter().find(|(a, _)| &a.id == id))
                .map(|(a, _)| a.album_artists.clone())
                .unwrap_or_default();
            let artist_names: Vec<String> =
                if f.artists.is_empty() { vec![UNKNOWN_ARTIST.into()] } else { f.artists.clone() };
            let image = album_id
                .as_ref()
                .and_then(|id| album_cover.get(id).cloned().flatten())
                .or_else(|| {
                    f.folder_art.as_ref().or(f.embedded_art.then_some(&f.path)).and_then(|p| file_url(p))
                })
                .map(ImageRef::Url);
            let song = Song {
                mbz: Default::default(),
                extra: Default::default(),
                id: format!("{:016x}", fnv1a(&f.rel)),
                server_id: server.clone(),
                source: SourceKind::Local,
                title: f.title.clone(),
                artists: refs(&artist_names),
                album_artists,
                album: f.album.clone(),
                album_id,
                image,
                duration: f.duration,
                track_number: f.track,
                disc_number: f.disc,
                year: f.year,
                genres: f.genres.clone(),
                codec: f.codec.clone(),
                bit_rate: f.bit_rate,
                sample_rate: f.sample_rate,
                bit_depth: f.bit_depth,
                bpm: f.bpm,
                favorite: false,
                rating: None,
                play_count: None,
                gain: f.gain.clone(),
                explicit: false,
                path: None,
                size: None,
                suffix: None,
                channels: None,
                created: None,
                comment: None,
            };
            let haystack =
                format!("{} {} {}", song.title, artist_names.join(" "), f.album.clone().unwrap_or_default())
                    .to_lowercase();
            tracks.push(Track {
                song,
                path: f.path.clone(),
                added: f.modified.unwrap_or(SystemTime::UNIX_EPOCH),
                haystack,
            });
        }
        let by_id: HashMap<String, usize> =
            tracks.iter().enumerate().map(|(i, t)| (t.song.id.clone(), i)).collect();

        let mut albums: Vec<Entry> = entries
            .into_iter()
            .map(|(album, idx)| {
                let mut artist_ids: HashSet<String> =
                    album.album_artists.iter().filter_map(|a| a.id.clone()).collect();
                for &i in &idx {
                    artist_ids.extend(tracks[i].song.artists.iter().filter_map(|a| a.id.clone()));
                }
                let added = idx.iter().map(|&i| tracks[i].added).max().unwrap_or(SystemTime::UNIX_EPOCH);
                Entry { album, added, tracks: idx, artist_ids }
            })
            .collect();
        albums.sort_by_key(|e| e.album.name.to_lowercase());
        let album_by_id = albums.iter().enumerate().map(|(i, e)| (e.album.id.clone(), i)).collect();

        // Artists: every name that is a track artist or album artist.
        let mut names: HashMap<String, String> = HashMap::new();
        let mut song_counts: HashMap<String, u32> = HashMap::new();
        for t in &tracks {
            for a in &t.song.artists {
                if let Some(id) = &a.id {
                    names.entry(id.clone()).or_insert_with(|| a.name.clone());
                    *song_counts.entry(id.clone()).or_default() += 1;
                }
            }
        }
        for e in &albums {
            for a in &e.album.album_artists {
                if let Some(id) = &a.id {
                    names.entry(id.clone()).or_insert_with(|| a.name.clone());
                }
            }
        }
        let mut artists: Vec<Artist> = names
            .into_iter()
            .map(|(id, name)| Artist {
                album_count: Some(albums.iter().filter(|e| e.artist_ids.contains(&id)).count() as u32),
                song_count: song_counts.get(&id).copied(),
                id,
                server_id: server.clone(),
                source: SourceKind::Local,
                name,
                image: None,
                favorite: false,
                rating: None,
                biography: None,
                similar: Vec::new(),
            })
            .collect();
        artists.sort_by_key(|a| a.name.to_lowercase());
        let artist_by_id = artists.iter().enumerate().map(|(i, a)| (a.id.clone(), i)).collect();

        let mut genre_map: HashMap<String, (HashSet<String>, u32)> = HashMap::new();
        for t in &tracks {
            for g in &t.song.genres {
                let e = genre_map.entry(g.clone()).or_default();
                e.1 += 1;
                if let Some(a) = &t.song.album_id {
                    e.0.insert(a.clone());
                }
            }
        }
        let mut genres: Vec<Genre> = genre_map
            .into_iter()
            .map(|(name, (albums, songs))| Genre {
                id: name.clone(),
                name,
                album_count: Some(albums.len() as u32),
                song_count: Some(songs),
            })
            .collect();
        genres.sort_by_key(|g| g.name.to_lowercase());

        Index { tracks, by_id, albums, album_by_id, artists, artist_by_id, genres }
    }
}

use std::time::Duration;

fn folder(files: Vec<Scanned>) -> LocalFolder {
    let server_id = ServerId("local-test".into());
    let index = Index::build(&server_id, files);
    LocalFolder { server_id, root: PathBuf::from("/music"), index: Arc::new(index) }
}

#[tokio::test]
async fn groups_albums_and_artists() {
    let src = folder(vec![
        file("a/2.flac", "Two", "Alpha", "First", Some("Alpha"), 2),
        file("a/1.flac", "One", "Alpha", "First", Some("Alpha"), 1),
        file("b/1.flac", "Solo", "Beta", "Second", Some("Beta"), 1),
    ]);
    let albums = src.albums(&AlbumQuery::default()).await.unwrap();
    assert_eq!(albums.total, Some(2));
    assert_eq!(albums.items[0].name, "First");
    assert_eq!(albums.items[0].song_count, Some(2));
    let first = src.album(&albums.items[0].id).await.unwrap();
    assert_eq!(first.songs.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(), ["One", "Two"]);
    let artists = src.artists(&ListQuery::default()).await.unwrap();
    assert_eq!(artists.items.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["Alpha", "Beta"]);
    let alpha = src.artist_albums(&artists.items[0].id).await.unwrap();
    assert_eq!(alpha.len(), 1);
}

#[tokio::test]
async fn compilation_without_album_artist_is_various() {
    let src = folder(vec![
        file("c/1.mp3", "A", "X", "Hits", None, 1),
        file("c/2.mp3", "B", "Y", "Hits", None, 2),
        file("d/1.mp3", "C", "Z", "Hits", None, 1),
    ]);
    let albums = src.albums(&AlbumQuery::default()).await.unwrap().items;
    assert_eq!(albums.len(), 2, "same album name in different folders stays apart");
    assert!(albums.iter().any(|a| a.artist_line() == "Various Artists"));
    assert!(albums.iter().any(|a| a.artist_line() == "Z"));
}

#[tokio::test]
async fn search_genres_and_paging() {
    let src = folder(vec![
        file("a/1.flac", "Hello World", "Alpha", "First", Some("Alpha"), 1),
        file("a/2.flac", "Other", "Alpha", "First", Some("Alpha"), 2),
    ]);
    let found = src.search("hello", 10).await.unwrap();
    assert_eq!(found.songs.len(), 1);
    assert_eq!(found.albums.len(), 0);
    assert_eq!(src.search("first", 10).await.unwrap().albums.len(), 1);
    assert_eq!(src.genres().await.unwrap()[0].song_count, Some(2));
    let page = src.songs(&SongQuery { limit: 1, start: 1, ..Default::default() }).await.unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.total, Some(2));
    assert_eq!(src.random_songs(5, Some("rock")).await.unwrap().len(), 2);
    assert!(src.random_songs(5, Some("jazz")).await.unwrap().is_empty());
}

#[tokio::test]
async fn scans_a_real_folder_and_streams_files() {
    let dir = std::env::temp_dir().join(format!("listen-local-test-{}", std::process::id()));
    let album_dir = dir.join("Artist").join("Album");
    std::fs::create_dir_all(&album_dir).unwrap();
    // 0.1 s of 8-bit mono silence.
    let samples = vec![128u8; 800];
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + samples.len() as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&8000u32.to_le_bytes());
    wav.extend_from_slice(&8000u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&8u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(samples.len() as u32).to_le_bytes());
    wav.extend_from_slice(&samples);
    std::fs::write(album_dir.join("01 Song.wav"), &wav).unwrap();
    std::fs::write(album_dir.join("notes.txt"), "x").unwrap();
    std::fs::write(album_dir.join("Folder.jpg"), [0xff, 0xd8, 0xff]).unwrap();

    let src = LocalFolder::connect(ServerId("l".into()), dir.to_str().unwrap()).await.unwrap();
    assert_eq!(src.song_count(), 1);
    let songs = src.songs(&SongQuery::default()).await.unwrap().items;
    assert_eq!(songs[0].title, "01 Song");
    assert!(songs[0].duration > Duration::ZERO);
    let stream = src.stream(&songs[0]).await.unwrap();
    assert!(stream.url.starts_with("file://") && stream.url.ends_with("01%20Song.wav"));
    assert!(LocalFolder::connect(ServerId("l".into()), "/definitely/not/here").await.is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn folder_art_is_served_as_bytes() {
    let dir = std::env::temp_dir().join(format!("listen-local-art-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("cover.png"), [1u8, 2, 3]).unwrap();
    let url = file_url(&dir.join("cover.png")).unwrap();
    assert_eq!(load_cover(&url), Some(vec![1, 2, 3]));
    assert_eq!(load_cover("file:///no/such.png"), None);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn reconnecting_picks_up_added_and_removed_files() {
    let dir = std::env::temp_dir().join(format!("listen-local-rescan-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36u32 + 800).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    for v in [16u32, 0x0001_0001, 8000, 8000, 0x0008_0001] {
        wav.extend_from_slice(&v.to_le_bytes());
    }
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&800u32.to_le_bytes());
    wav.extend_from_slice(&[128u8; 800]);
    std::fs::write(dir.join("a.wav"), &wav).unwrap();
    let id = ServerId("r".into());
    assert_eq!(LocalFolder::connect(id.clone(), dir.to_str().unwrap()).await.unwrap().song_count(), 1);
    std::fs::write(dir.join("b.wav"), &wav).unwrap();
    assert_eq!(LocalFolder::connect(id.clone(), dir.to_str().unwrap()).await.unwrap().song_count(), 2);
    std::fs::remove_file(dir.join("a.wav")).unwrap();
    assert_eq!(LocalFolder::connect(id, dir.to_str().unwrap()).await.unwrap().song_count(), 1);
    std::fs::remove_dir_all(&dir).unwrap();
}

fn sc(
    rel: &str,
    title: &str,
    artists: &[&str],
    album: Option<&str>,
    album_artist: Option<&str>,
    track: u32,
) -> Scanned {
    Scanned {
        rel: rel.into(),
        path: PathBuf::from("/music").join(rel),
        title: title.into(),
        artists: artists.iter().map(|a| (*a).to_string()).collect(),
        album: album.map(String::from),
        album_artist: album_artist.map(String::from),
        track: Some(track),
        ..Default::default()
    }
}

/// Compilations, shared album artists, repeated artist ids, no-album tracks, case folds.
fn build_fixture() -> Vec<Scanned> {
    vec![
        // A compilation from one folder: becomes one "Various Artists" album.
        sc("comp/01.wav", "One", &["Alice"], Some("Hits"), None, 1),
        sc("comp/02.wav", "Two", &["Bob"], Some("Hits"), None, 2),
        // The same album name in another folder stays a separate album.
        sc("other/01.wav", "Three", &["Carol"], Some("Hits"), None, 1),
        // Shared album artist, one guest track artist.
        sc("band/01.wav", "Four", &["The Band"], Some("Shared"), Some("The Band"), 1),
        sc("band/02.wav", "Five", &["Guest"], Some("Shared"), Some("The Band"), 2),
        // Repeated artist ids and the same artist across two albums.
        sc("dup/01.wav", "Six", &["Dup", "Dup"], Some("Duo"), Some("Dup"), 1),
        sc("dup/02.wav", "Seven", &["Dup"], Some("Solo"), Some("Dup"), 1),
        // A track without an album.
        sc("loose/one.wav", "Eight", &["Eve"], None, None, 1),
        // Case-folded album name/artist: the old grouping merges these.
        sc("case/01.wav", "Nine", &["F"], Some("Loud"), Some("F"), 1),
        sc("case/02.wav", "Ten", &["F"], Some("loud"), Some("f"), 2),
        // No artist at all -> Unknown Artist.
        sc("unknown/01.wav", "Eleven", &[], Some("Nameless"), None, 1),
    ]
}

#[test]
fn index_build_matches_the_old_linear_lookups() {
    let server = ServerId("eq".into());
    let files = build_fixture();
    let old = reference::build(&server, files.clone());
    let new = Index::build(&server, files.clone());

    assert_eq!(new.tracks.len(), old.tracks.len());
    for (a, b) in new.tracks.iter().zip(&old.tracks) {
        assert_eq!(a.song.id, b.song.id);
        assert_eq!(a.song.title, b.song.title);
        assert_eq!(a.song.album_id, b.song.album_id);
        assert_eq!(a.song.album_artists, b.song.album_artists);
        assert_eq!(a.song.artists, b.song.artists);
    }
    assert_eq!(
        new.albums.iter().map(|e| (&e.album.id, &e.album.name)).collect::<Vec<_>>(),
        old.albums.iter().map(|e| (&e.album.id, &e.album.name)).collect::<Vec<_>>()
    );
    assert_eq!(new.artists.len(), old.artists.len());
    for (a, b) in new.artists.iter().zip(&old.artists) {
        assert_eq!((&a.id, &a.name), (&b.id, &b.name));
        assert_eq!(a.album_count, b.album_count, "album_count of {}", a.name);
        assert_eq!(a.song_count, b.song_count, "song_count of {}", a.name);
    }
    assert_eq!(
        new.genres.iter().map(|g| (&g.id, g.album_count, g.song_count)).collect::<Vec<_>>(),
        old.genres.iter().map(|g| (&g.id, g.album_count, g.song_count)).collect::<Vec<_>>()
    );

    // The fixture really exercises the interesting shapes.
    let dup = new.artists.iter().find(|a| a.name == "Dup").unwrap();
    assert_eq!((dup.album_count, dup.song_count), (Some(2), Some(3)));
    let various = new.albums.iter().filter(|e| e.album.artist_line() == "Various Artists").count();
    assert_eq!(various, 1);
    assert_eq!(new.tracks.iter().filter(|t| t.song.album_id.is_none()).count(), 1);
}
