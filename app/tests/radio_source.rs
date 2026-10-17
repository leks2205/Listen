//! Radio, Related and the jukebox against a fake server (not demo mode):
//! what the app asks the source and what it does with the answers.

use async_trait::async_trait;
use iced::Task;
use listen::app::radio::RadioMsg;
use listen::app::{Listen, Message, Route, SourceHandle};
use listen::{Args, demo};
use listen_core::config::ServerConfig;
use listen_core::model::{
    Album, Artist, ImageRef, Page, Playlist, SearchResults, ServerId, Song, SourceKind,
};
use listen_core::settings::AudioPlayer;
use listen_source::{
    AlbumQuery, Capabilities, InternetRadio, JukeboxCmd, JukeboxStatus, ListQuery, MusicSource, Result,
    SongQuery, SourceError, StreamSource,
};
use std::sync::{Arc, Mutex};

#[derive(Default, Debug)]
struct Calls {
    radio: Vec<String>,
    similar: Vec<(String, usize)>,
    jukebox: Vec<JukeboxCmd>,
}

struct Fake {
    caps: Capabilities,
    calls: Arc<Mutex<Calls>>,
    stations: Mutex<Vec<InternetRadio>>,
    jukebox_ok: bool,
}

#[async_trait]
impl MusicSource for Fake {
    fn kind(&self) -> SourceKind {
        SourceKind::Subsonic
    }
    fn capabilities(&self) -> Capabilities {
        self.caps
    }
    async fn ping(&self) -> Result<()> {
        Ok(())
    }
    async fn albums(&self, _: &AlbumQuery) -> Result<Page<Album>> {
        Err(SourceError::Unsupported)
    }
    async fn album(&self, _: &str) -> Result<Album> {
        Err(SourceError::Unsupported)
    }
    async fn artists(&self, _: &ListQuery) -> Result<Page<Artist>> {
        Err(SourceError::Unsupported)
    }
    async fn artist(&self, _: &str) -> Result<Artist> {
        Err(SourceError::Unsupported)
    }
    async fn artist_albums(&self, _: &str) -> Result<Vec<Album>> {
        Ok(vec![])
    }
    async fn top_songs(&self, _: &Artist, _: usize) -> Result<Vec<Song>> {
        Ok(vec![])
    }
    async fn songs(&self, _: &SongQuery) -> Result<Page<Song>> {
        Err(SourceError::Unsupported)
    }
    async fn random_songs(&self, _: usize, _: Option<&str>) -> Result<Vec<Song>> {
        Ok(vec![])
    }
    async fn search(&self, _: &str, _: usize) -> Result<SearchResults> {
        Ok(SearchResults::default())
    }
    async fn playlists(&self) -> Result<Vec<Playlist>> {
        Ok(vec![])
    }
    async fn playlist_songs(&self, _: &str) -> Result<Vec<Song>> {
        Ok(vec![])
    }
    async fn set_favorite(&self, _: &[String], _: bool) -> Result<()> {
        Ok(())
    }
    async fn stream(&self, _: &Song) -> Result<StreamSource> {
        Err(SourceError::Unsupported)
    }
    fn image_url(&self, _: &ImageRef, _: u32) -> Option<String> {
        None
    }
    async fn similar_songs(&self, song: &Song, count: usize) -> Result<Vec<Song>> {
        self.calls.lock().unwrap().similar.push((song.id.clone(), count));
        // The server returns the song itself among the results.
        Ok(songs(4))
    }
    async fn internet_radios(&self) -> Result<Vec<InternetRadio>> {
        self.calls.lock().unwrap().radio.push("list".into());
        Ok(self.stations.lock().unwrap().clone())
    }
    async fn create_internet_radio(&self, r: &InternetRadio) -> Result<InternetRadio> {
        self.calls.lock().unwrap().radio.push(format!("create {}", r.name));
        let made = InternetRadio { id: "r9".into(), ..r.clone() };
        self.stations.lock().unwrap().push(made.clone());
        Ok(made)
    }
    async fn update_internet_radio(&self, r: &InternetRadio) -> Result<()> {
        self.calls.lock().unwrap().radio.push(format!("update {} {}", r.id, r.name));
        Ok(())
    }
    async fn delete_internet_radio(&self, id: &str) -> Result<()> {
        self.calls.lock().unwrap().radio.push(format!("delete {id}"));
        self.stations.lock().unwrap().retain(|s| s.id != id);
        Ok(())
    }
    async fn jukebox(&self, cmd: &JukeboxCmd) -> Result<JukeboxStatus> {
        self.calls.lock().unwrap().jukebox.push(cmd.clone());
        if !self.jukebox_ok {
            return Err(SourceError::Server { code: 0, message: "Jukebox is disabled".into() });
        }
        Ok(JukeboxStatus {
            current_index: 0,
            playing: !matches!(cmd, JukeboxCmd::Stop),
            gain: 1.0,
            position_secs: 0,
        })
    }
}

fn songs(n: usize) -> Vec<Song> {
    demo::album_songs(0)
        .into_iter()
        .take(n)
        .map(|mut s| {
            s.server_id = ServerId("srv".into());
            s.source = SourceKind::Subsonic;
            s
        })
        .collect()
}

fn app(caps: Capabilities, jukebox_ok: bool) -> (Listen, Arc<Mutex<Calls>>) {
    let (mut s, _) =
        Listen::boot(&Args { ephemeral: true, theme: Some("tokyoNight".into()), ..Args::default() });
    let id = ServerId("srv".into());
    s.config.servers = vec![ServerConfig {
        id: id.clone(),
        name: "srv".into(),
        kind: SourceKind::Subsonic,
        url: "https://x.example".into(),
        username: "me".into(),
    }];
    let calls = Arc::new(Mutex::new(Calls::default()));
    let fake = Fake {
        caps,
        calls: calls.clone(),
        stations: Mutex::new(vec![InternetRadio {
            id: "r1".into(),
            name: "Jazz".into(),
            stream_url: "http://x/jazz".into(),
            home_page_url: None,
            image_id: None,
        }]),
        jukebox_ok,
    };
    let _ = s.update(Message::Connected(id, Ok(SourceHandle(Arc::new(fake)))));
    (s, calls)
}

/// Runs a task and returns the messages it produced.
fn run(task: Task<Message>) -> Vec<Message> {
    use iced::futures::StreamExt;
    let Some(mut stream) = iced_runtime::task::into_stream(task) else { return vec![] };
    tokio::runtime::Runtime::new().unwrap().block_on(async move {
        let mut out = Vec::new();
        while let Some(action) = stream.next().await {
            if let iced_runtime::Action::Output(m) = action {
                out.push(m);
            }
        }
        out
    })
}

/// Sends `m`, then feeds the answers back (two rounds).
fn settle(s: &mut Listen, m: Message) {
    let mut pending = run(s.update(m));
    for _ in 0..3 {
        let next: Vec<Message> = pending.drain(..).flat_map(|m| run(s.update(m))).collect();
        pending = next;
    }
}

const RADIO: Capabilities = Capabilities::INTERNET_RADIO.union(Capabilities::SIMILAR_SONGS);

#[test]
fn the_radio_page_loads_and_writes_through_the_source() {
    let (mut s, calls) = app(RADIO, false);
    settle(&mut s, Message::Navigate(Route::Radio));
    assert_eq!(s.radio.stations.len(), 1);

    settle(&mut s, Message::Radio(RadioMsg::OpenCreate));
    for m in [
        RadioMsg::FormName("Rock".into()),
        RadioMsg::FormStream("https://x/rock".into()),
        RadioMsg::FormSubmit,
    ] {
        settle(&mut s, Message::Radio(m));
    }
    assert_eq!(s.radio.stations.len(), 2, "{:?}", calls.lock().unwrap());
    assert!(s.radio.form.is_none());

    settle(&mut s, Message::Radio(RadioMsg::OpenEdit("r9".into())));
    settle(&mut s, Message::Radio(RadioMsg::FormName("Rock 2".into())));
    settle(&mut s, Message::Radio(RadioMsg::FormSubmit));
    settle(&mut s, Message::Radio(RadioMsg::Delete("r1".into())));
    let radio = calls.lock().unwrap().radio.clone();
    assert!(radio.contains(&"create Rock".to_string()), "{radio:?}");
    assert!(radio.contains(&"update r9 Rock 2".to_string()), "{radio:?}");
    assert!(radio.contains(&"delete r1".to_string()), "{radio:?}");
    assert!(s.radio.stations.iter().all(|x| x.id != "r1"));
}

#[test]
fn radio_is_hidden_where_the_source_has_none() {
    let (s, _) = app(Capabilities::SIMILAR_SONGS, false);
    assert!(!s.supports_route(&Route::Radio));
    let (s, _) = app(RADIO, false);
    assert!(s.supports_route(&Route::Radio));
}

#[test]
fn related_asks_the_source_and_drops_the_song_itself() {
    let (mut s, calls) = app(RADIO, false);
    let list = songs(4);
    s.queue.set(list.clone(), 0);
    settle(&mut s, Message::Radio(RadioMsg::RelatedRefresh));
    assert_eq!(calls.lock().unwrap().similar, [(list[0].id.clone(), 50)]);
    let shown: Vec<_> = s.radio.related.songs.iter().map(|x| x.id.clone()).collect();
    assert_eq!(shown, [list[1].id.clone(), list[2].id.clone(), list[3].id.clone()]);
    assert_eq!(s.fs.related_for.as_deref(), Some(list[0].id.as_str()), "the full-screen tab got it");
    assert_eq!(s.fs.related.len(), 3);
}

#[test]
fn related_says_so_when_the_source_cannot() {
    let (mut s, calls) = app(Capabilities::INTERNET_RADIO, false);
    s.queue.set(songs(2), 0);
    settle(&mut s, Message::Radio(RadioMsg::RelatedRefresh));
    assert!(calls.lock().unwrap().similar.is_empty());
    assert!(s.radio.related.error.is_some());
}

#[test]
fn the_jukebox_takes_over_playback_when_chosen() {
    let (mut s, calls) = app(Capabilities::JUKEBOX, true);
    settle(&mut s, Message::Radio(RadioMsg::JukeboxProbe));
    assert_eq!(s.radio.jukebox.available, Some(true));
    assert!(s.jukebox_offered());
    settle(&mut s, Message::Radio(RadioMsg::SetAudioPlayer(AudioPlayer::Jukebox)));
    assert!(s.jukebox_active());

    let list = songs(3);
    s.queue.set(list.clone(), 1);
    settle(&mut s, Message::Radio(RadioMsg::JukeboxPoll));
    calls.lock().unwrap().jukebox.clear();
    // Play from the second song: the server gets the queue from there.
    let task = s.update(Message::Next);
    let _ = run(task);
    let cmds = calls.lock().unwrap().jukebox.clone();
    assert_eq!(cmds[0], JukeboxCmd::Set(vec![list[2].id.clone()]), "{cmds:?}");
    assert_eq!(cmds[1], JukeboxCmd::Start);

    calls.lock().unwrap().jukebox.clear();
    settle(&mut s, Message::TogglePlay);
    assert_eq!(calls.lock().unwrap().jukebox, [JukeboxCmd::Stop]);
    settle(&mut s, Message::Volume(40.0));
    assert!(calls.lock().unwrap().jukebox.contains(&JukeboxCmd::SetGain(0.4)));
}

#[test]
fn a_server_without_jukebox_is_not_offered_and_falls_back_to_mpv() {
    let (mut s, _) = app(Capabilities::JUKEBOX, false);
    settle(&mut s, Message::Radio(RadioMsg::JukeboxProbe));
    assert_eq!(s.radio.jukebox.available, Some(false));
    assert!(!s.jukebox_offered(), "hidden when unsupported");
    assert!(!s.jukebox_active());
    // A saved Jukebox choice on a server that refuses goes back to mpv.
    s.config.settings.playback.audio_player = AudioPlayer::Jukebox;
    settle(&mut s, Message::Radio(RadioMsg::JukeboxProbed(false)));
    assert_eq!(s.config.settings.playback.audio_player, AudioPlayer::Mpv);
}
