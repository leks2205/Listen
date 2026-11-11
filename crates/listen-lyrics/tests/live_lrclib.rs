//! Live (network): `cargo test -p listen-lyrics --test live_lrclib -- --ignored`. Looks up a
//! well-known song on lrclib.net.
use listen_core::model::{ArtistRef, ServerId, Song, SourceKind};
use std::time::Duration;

#[tokio::test]
#[ignore = "live: needs network (lrclib.net)"]
async fn finds_synced_lyrics() {
    let song = Song {
        artists: vec![ArtistRef { id: None, name: "Rick Astley".into() }],
        album: Some("Whenever You Need Somebody".into()),
        duration: Duration::from_secs(213),
        ..Song::new("x", ServerId("t".into()), SourceKind::Subsonic, "Never Gonna Give You Up")
    };
    let http = reqwest::Client::builder().user_agent(listen_core::USER_AGENT).build().unwrap();
    let lyrics = listen_lyrics::lrclib(&http, &song).await.expect("lyrics found");
    assert!(
        lyrics.synced && lyrics.lines.len() > 20,
        "{} lines, synced={}",
        lyrics.lines.len(),
        lyrics.synced
    );
    println!("lrclib: {} synced lines, first: {:?}", lyrics.lines.len(), lyrics.lines[0]);
}
