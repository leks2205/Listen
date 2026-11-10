//! Opt-in: `LISTEN_LYRICS_LIVE=1`. Looks up a well-known song on lrclib.net.
use listen_core::model::{ArtistRef, ServerId, Song, SourceKind};
use std::time::Duration;

#[tokio::test]
async fn finds_synced_lyrics() {
    if std::env::var("LISTEN_LYRICS_LIVE").is_err() {
        return;
    }
    let song = Song {
        artists: vec![ArtistRef { id: None, name: "Rick Astley".into() }],
        album: Some("Whenever You Need Somebody".into()),
        duration: Duration::from_secs(213),
        ..Song::new("x", ServerId("t".into()), SourceKind::Subsonic, "Never Gonna Give You Up")
    };
    let http =
        reqwest::Client::builder().user_agent("Listen (https://github.com/leks2205/Listen)").build().unwrap();
    let lyrics = listen_lyrics::lrclib(&http, &song).await.expect("lyrics found");
    assert!(
        lyrics.synced && lyrics.lines.len() > 20,
        "{} lines, synced={}",
        lyrics.lines.len(),
        lyrics.synced
    );
    println!("lrclib: {} synced lines, first: {:?}", lyrics.lines.len(), lyrics.lines[0]);
}
