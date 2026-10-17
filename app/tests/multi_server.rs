//! Several servers at once: switching keeps connections and the queue;
//! removing the active server falls back to another one.

use listen::app::{Listen, Message, Route, SourceHandle};
use listen::{Args, demo};
use listen_core::config::ServerConfig;
use listen_core::model::{ServerId, SourceKind};
use listen_source::MusicSource;
use std::sync::Arc;

fn server(id: &str, name: &str) -> ServerConfig {
    ServerConfig {
        id: ServerId(id.into()),
        name: name.into(),
        kind: SourceKind::Subsonic,
        url: format!("https://{id}.example"),
        username: "me".into(),
    }
}

fn source(id: &str) -> Arc<dyn MusicSource> {
    // No network needed: constructing a Subsonic client doesn't connect.
    Arc::new(
        listen_subsonic::Subsonic::new(ServerId(id.into()), &format!("https://{id}.example"), "me", "pw")
            .unwrap(),
    )
}

#[test]
fn switch_add_remove() {
    let (mut state, _) = Listen::boot(&Args { ephemeral: true, ..Args::default() });
    state.config.servers = vec![server("nd", "Navidrome"), server("yt", "YouTube Music")];
    let (nd, yt) = (source("nd"), source("yt"));

    let _ = state.update(Message::Connected(ServerId("nd".into()), Ok(SourceHandle(nd.clone()))));
    assert_eq!(state.config.active_server, Some(ServerId("nd".into())));
    assert!(!state.needs_login());

    // Something from Navidrome is queued…
    let mut songs = demo::album_songs(0);
    for s in &mut songs {
        s.server_id = ServerId("nd".into());
    }
    state.queue.set(songs, 0);

    // A background connection doesn't take over browsing…
    let _ = state.update(Message::Connected(ServerId("yt".into()), Ok(SourceHandle(yt.clone()))));
    assert_eq!(state.config.active_server, Some(ServerId("nd".into())));
    assert!(Arc::ptr_eq(state.source.as_ref().unwrap(), &nd));
    // …switching to it is instant.
    let _ = state.update(Message::SwitchServer(ServerId("yt".into())));
    assert!(!state.connecting);
    assert_eq!(state.config.active_server, Some(ServerId("yt".into())));
    assert_eq!(state.sources.len(), 2, "both stay connected");
    assert_eq!(state.queue.len(), 6, "queue survives the switch");
    assert_eq!(state.route, Route::Home);

    // Switching back reuses the existing connection (no sign-in).
    let _ = state.update(Message::SwitchServer(ServerId("nd".into())));
    assert!(Arc::ptr_eq(state.source.as_ref().unwrap(), &nd));
    assert!(!state.connecting);

    // "Add server" shows the sign-in page without dropping anything.
    let _ = state.update(Message::AddServer);
    assert!(state.needs_login());
    let _ = state.update(Message::CancelAddServer);
    assert!(!state.needs_login());
    assert_eq!(state.sources.len(), 2);

    // Removing the browsed server drops its songs and falls back to the other.
    let _ = state.update(Message::RemoveServer(ServerId("nd".into())));
    assert_eq!(state.config.servers.len(), 1);
    assert!(state.queue.is_empty(), "Navidrome songs removed from the queue");
    assert_eq!(state.config.active_server, Some(ServerId("yt".into())));
    assert!(Arc::ptr_eq(state.source.as_ref().unwrap(), &yt));
}

/// Switching back to a server shows its library from memory, on the page you
/// left; a result that arrives for a server you've left is dropped.
#[test]
fn switching_back_keeps_the_library() {
    use listen_source::HomeShelf;
    let (mut state, _) = Listen::boot(&Args { ephemeral: true, ..Args::default() });
    state.config.servers = vec![server("nd", "Navidrome"), server("yt", "YouTube Music")];
    let _ = state.update(Message::Connected(ServerId("yt".into()), Ok(SourceHandle(source("yt")))));
    let _ = state.update(Message::Connected(ServerId("nd".into()), Ok(SourceHandle(source("nd")))));
    let _ = state.update(Message::SwitchServer(ServerId("yt".into())));

    let shelf = |title: &str| HomeShelf {
        title: title.into(),
        albums: demo::albums(),
        playlists: vec![],
        songs: vec![],
    };
    let _ = state.update(Message::ForServer(
        ServerId("yt".into()),
        Box::new(Message::HomeLoaded(vec![shelf("Quick picks")])),
    ));
    let _ = state.update(Message::Navigate(Route::Albums));
    state.library.albums = demo::albums();
    state.library.albums_done = true;

    // To Navidrome: a late YouTube Music result must not land there.
    let _ = state.update(Message::SwitchServer(ServerId("nd".into())));
    assert!(state.library.home.is_empty() && state.library.albums.is_empty());
    let _ = state.update(Message::ForServer(
        ServerId("yt".into()),
        Box::new(Message::HomeLoaded(vec![shelf("Late")])),
    ));
    assert!(state.library.home.is_empty(), "late result for the other server is dropped");

    // Back to YouTube Music: everything is there at once, on the same page.
    let _ = state.update(Message::SwitchServer(ServerId("yt".into())));
    assert_eq!(state.route, Route::Albums);
    assert!(state.library.home_loaded);
    assert_eq!(state.library.home[0].title, "Quick picks");
    assert_eq!(state.library.albums.len(), demo::albums().len());
}
