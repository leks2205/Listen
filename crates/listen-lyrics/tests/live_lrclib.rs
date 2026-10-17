//! Opt-in: `LISTEN_LYRICS_LIVE=1`. Looks up a well-known song on lrclib.net.
use listen_core::model::{ArtistRef, GainInfo, ServerId, Song, SourceKind};
use std::time::Duration;

#[tokio::test]
async fn finds_synced_lyrics() {
    if std::env::var("LISTEN_LYRICS_LIVE").is_err() {
        return;
    }
    let song = Song {
        mbz: Default::default(),
        extra: Default::default(),
        id: "x".into(),
        server_id: ServerId("t".into()),
        source: SourceKind::Subsonic,
        title: "Never Gonna Give You Up".into(),
        artists: vec![ArtistRef { id: None, name: "Rick Astley".into() }],
        album_artists: vec![],
        album: Some("Whenever You Need Somebody".into()),
        album_id: None,
        image: None,
        duration: Duration::from_secs(213),
        track_number: None,
        disc_number: None,
        year: None,
        genres: vec![],
        codec: None,
        bit_rate: None,
        sample_rate: None,
        bit_depth: None,
        bpm: None,
        favorite: false,
        rating: None,
        play_count: None,
        gain: GainInfo::default(),
        explicit: false,
        path: None,
        size: None,
        suffix: None,
        channels: None,
        created: None,
        comment: None,
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
