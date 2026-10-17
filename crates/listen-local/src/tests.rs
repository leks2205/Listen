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
