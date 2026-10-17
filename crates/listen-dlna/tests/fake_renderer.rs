//! The client code against a fake renderer over loopback. No real device is involved.

use listen_dlna::soap::TrackMeta;
use listen_dlna::testing::FakeRenderer;
use listen_dlna::{Error, SoapError, TransportState, renderer};
use std::time::Duration;

async fn connect(f: &FakeRenderer) -> renderer::Renderer {
    let found = renderer::discover_at(f.ssdp, Duration::from_millis(600)).await;
    assert_eq!(found.len(), 1);
    found.into_iter().next().unwrap()
}

#[tokio::test]
async fn discovers_the_fake_renderer() {
    let f = FakeRenderer::start().await;
    let r = connect(&f).await;
    assert_eq!(r.name, "Fake Speaker");
    assert_eq!(r.udn, "uuid:fake-1");
    assert!(r.has_volume());
    assert_eq!(r.ip(), Some("127.0.0.1".parse().unwrap()));
}

#[tokio::test]
async fn play_pause_seek_and_position() {
    let f = FakeRenderer::start().await;
    let r = connect(&f).await;
    let meta = TrackMeta { title: "Song & Co".into(), artist: "Artist".into(), ..Default::default() };
    r.set_uri("http://192.168.1.2:9000/s/abc.mp3?x=1&y=2", "audio/mpeg", &meta).await.unwrap();
    r.play().await.unwrap();
    assert_eq!(r.transport_state().await.unwrap(), TransportState::Playing);
    r.seek(Duration::from_secs(75)).await.unwrap();
    let info = r.position_info().await.unwrap();
    assert_eq!(info.position, Some(Duration::from_secs(75)));
    assert_eq!(info.duration, Some(Duration::from_secs(205)));
    assert_eq!(info.uri, "http://192.168.1.2:9000/s/abc.mp3?x=1&y=2");
    r.pause().await.unwrap();
    assert_eq!(r.transport_state().await.unwrap(), TransportState::Paused);
    r.stop().await.unwrap();
    assert_eq!(r.transport_state().await.unwrap(), TransportState::Stopped);

    let d = f.state();
    let names: Vec<&str> = d.log.iter().map(|(a, _)| a.as_str()).collect();
    assert_eq!(
        names,
        [
            "SetAVTransportURI",
            "Play",
            "GetTransportInfo",
            "Seek",
            "GetPositionInfo",
            "Pause",
            "GetTransportInfo",
            "Stop",
            "GetTransportInfo"
        ]
    );
    assert_eq!(d.log[1].1["Speed"], "1");
    assert_eq!(d.log[3].1["Unit"], "REL_TIME");
    assert_eq!(d.log[3].1["Target"], "0:01:15");
    // The metadata the speaker received is DIDL with the escaped title.
    assert!(d.didl.contains("Song &amp; Co"), "{}", d.didl);
}

#[tokio::test]
async fn volume_and_mute() {
    let f = FakeRenderer::start().await;
    let r = connect(&f).await;
    assert_eq!(r.volume().await.unwrap(), 30);
    r.set_volume(65).await.unwrap();
    assert_eq!(r.volume().await.unwrap(), 65);
    r.set_volume(250).await.unwrap();
    assert_eq!(f.state().volume, 100, "clamped");
    assert!(!r.muted().await.unwrap());
    r.set_mute(true).await.unwrap();
    assert!(r.muted().await.unwrap());
}

#[tokio::test]
async fn upnp_faults_come_back_as_errors() {
    let f = FakeRenderer::start().await;
    let r = connect(&f).await;
    match r.play().await {
        Err(Error::Soap(SoapError::Fault { code: 701, description })) => {
            assert!(description.contains("Transition"))
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn a_dead_address_is_a_network_error_not_a_hang() {
    let f = FakeRenderer::start().await;
    let r = connect(&f).await;
    drop(f); // the listener task keeps running, so ask something that is not there
    let gone = renderer::Renderer::fetch("http://127.0.0.1:9/desc.xml").await;
    assert!(matches!(gone, Err(Error::Network(_))));
    let _ = r;
}
