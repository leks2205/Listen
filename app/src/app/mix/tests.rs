//! The transition engine's control flow without mpv: inert decks, deck events and task results fed in as
//! messages, positions set by hand. Timing against real audio is `--check-decks`.

use super::*;
use crate::app::SourceHandle;
use crate::app::dj::Break;
use crate::playback::PlayerMsg;
use crate::{Args, demo};
use listen_core::analysis::{ANALYZER_VERSION, BarCurves, Camelot, FileId, Mood, Outcome, TrackAnalysis};
use listen_core::model::{
    Album, Artist, ImageRef, Page, Playlist, SearchResults, ServerId, Song, SourceKind,
};
use listen_core::queue::Repeat;
use listen_player::Player;
use listen_source::{AlbumQuery, Capabilities, ListQuery, SongQuery, SourceError, StreamSource};
use std::path::PathBuf;
use std::sync::Mutex;

const SERVER: &str = "nd";
const LEN_S: u64 = 240;

/// Song `i` of demo album `album`, as a 4-minute Navidrome song.
fn song(album: usize, i: usize) -> Song {
    let mut s = demo::album_songs(album)[i].clone();
    s.server_id = ServerId(SERVER.into());
    s.duration = Duration::from_secs(LEN_S);
    s
}

const YT: &str = "yt";

/// Song `i` of demo album `album` on the fake YouTube Music server, labelled `kind`, listed as 0 s long.
fn yt_song(album: usize, i: usize, kind: SourceKind) -> Song {
    let mut s = demo::album_songs(album)[i].clone();
    s.server_id = ServerId(YT.into());
    s.id = format!("vid{album}x{i}");
    s.source = kind;
    s.album_id = None;
    s.track_number = None;
    s.duration = Duration::ZERO;
    s
}

/// A YouTube Music-like source: `mix_input` "fetches" an empty local file
/// (or fails), and pins are recorded.
struct FakeYtm {
    dir: PathBuf,
    fail: bool,
    /// (song id, pinned) in call order.
    pins: Mutex<Vec<(String, bool)>>,
}

impl FakeYtm {
    fn new(fail: bool) -> Arc<Self> {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("listen-mix-test-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        Arc::new(Self { dir, fail, pins: Mutex::new(Vec::new()) })
    }

    fn file(&self, song: &Song) -> PathBuf {
        self.dir.join(format!("{}.opus", song.id))
    }

    /// Songs pinned now.
    fn pinned(&self) -> Vec<String> {
        let mut now: Vec<String> = Vec::new();
        for (id, on) in self.pins.lock().unwrap().iter() {
            if *on {
                assert!(!now.contains(id), "{id} pinned twice");
                now.push(id.clone());
            } else {
                assert!(now.contains(id), "{id} unpinned without a pin");
                now.retain(|x| x != id);
            }
        }
        now
    }
}

impl Drop for FakeYtm {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[async_trait::async_trait]
impl MusicSource for FakeYtm {
    fn kind(&self) -> SourceKind {
        SourceKind::YouTubeMusic
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::empty()
    }
    async fn ping(&self) -> listen_source::Result<()> {
        Ok(())
    }
    async fn albums(&self, _: &AlbumQuery) -> listen_source::Result<Page<Album>> {
        Err(SourceError::Unsupported)
    }
    async fn album(&self, _: &str) -> listen_source::Result<Album> {
        Err(SourceError::Unsupported)
    }
    async fn artists(&self, _: &ListQuery) -> listen_source::Result<Page<Artist>> {
        Err(SourceError::Unsupported)
    }
    async fn artist(&self, _: &str) -> listen_source::Result<Artist> {
        Err(SourceError::Unsupported)
    }
    async fn artist_albums(&self, _: &str) -> listen_source::Result<Vec<Album>> {
        Err(SourceError::Unsupported)
    }
    async fn top_songs(&self, _: &Artist, _: usize) -> listen_source::Result<Vec<Song>> {
        Err(SourceError::Unsupported)
    }
    async fn songs(&self, _: &SongQuery) -> listen_source::Result<Page<Song>> {
        Err(SourceError::Unsupported)
    }
    async fn random_songs(&self, _: usize, _: Option<&str>) -> listen_source::Result<Vec<Song>> {
        Err(SourceError::Unsupported)
    }
    async fn search(&self, _: &str, _: usize) -> listen_source::Result<SearchResults> {
        Err(SourceError::Unsupported)
    }
    async fn playlists(&self) -> listen_source::Result<Vec<Playlist>> {
        Err(SourceError::Unsupported)
    }
    async fn playlist_songs(&self, _: &str) -> listen_source::Result<Vec<Song>> {
        Err(SourceError::Unsupported)
    }
    async fn set_favorite(&self, _: &[String], _: bool) -> listen_source::Result<()> {
        Err(SourceError::Unsupported)
    }
    async fn stream(&self, song: &Song) -> listen_source::Result<StreamSource> {
        Ok(StreamSource::direct(format!("https://yt.example/{}", song.id)))
    }
    fn image_url(&self, _: &ImageRef, _: u32) -> Option<String> {
        None
    }
    fn analysable(&self) -> bool {
        true
    }
    async fn mix_input(&self, song: &Song) -> listen_source::Result<Option<AudioInput>> {
        if self.fail {
            return Err(SourceError::Network("edge node refused".into()));
        }
        let path = self.file(song);
        std::fs::write(&path, b"").map_err(|e| SourceError::Other(e.to_string()))?;
        Ok(Some(AudioInput::File(path)))
    }
    fn pin_mix(&self, song: &Song, pinned: bool) {
        self.pins.lock().unwrap().push((song.id.clone(), pinned));
    }
}

/// A steady 124 bpm 8A record covering the whole song (beat-mixable).
fn record(song: &Song) -> Arc<TrackAnalysis> {
    let bpm = 124.0_f32;
    let period = 60.0 / bpm;
    // YouTube Music listings may say 0 s; the analysis knows the real length.
    let len = if song.duration.is_zero() { LEN_S as f32 } else { song.duration.as_secs_f32() };
    let n = (len / period) as usize - 1;
    let beats: Vec<f32> = (0..n).map(|i| 0.1 + i as f32 * period).collect();
    Arc::new(TrackAnalysis {
        version: ANALYZER_VERSION,
        analyzed_at: 1,
        machine: "test".into(),
        decoder: "test".into(),
        song_id: listen_analysis::record_key(song),
        server_hint: String::new(),
        file: FileId::of(song),
        title_hint: song.title.clone(),
        outcome: Outcome::Ok(Box::new(Features {
            audio_start: 0.1,
            audio_end: len,
            grid: Some(BeatGrid {
                beats,
                downbeat_phase: 0,
                beats_per_bar: 4,
                bpm,
                confidence: 0.9,
                stability: 0.99,
            }),
            key: Some((Camelot { number: 8, minor: true }, 0.9)),
            lufs: -9.0,
            lra: 5.0,
            bars: BarCurves::default(),
            phrase_bars: 8,
            phrase_offset: 0,
            sections: vec![],
            intro_instrumental_bars: 8,
            outro_instrumental_bars: 8,
            timbre: vec![],
            mood: Mood { energy: 0.6, danceability: 0.7, brightness: 0.5 },
        })),
    })
}

/// Auto-DJ and mixing on, Navidrome connected, two inert decks, `current` playing before `upcoming`
/// (`auto`: added by the Auto-DJ). Everything analysed, nothing planned.
fn rig(current: Song, upcoming: Vec<Song>, auto: bool) -> Listen {
    rig_with(current, upcoming, auto, None)
}

/// [`rig`] plus a fake YouTube Music server (`yt`) if given.
fn rig_with(current: Song, upcoming: Vec<Song>, auto: bool, ytm: Option<Arc<FakeYtm>>) -> Listen {
    let (mut s, _) = Listen::boot(&Args { ephemeral: true, ..Args::default() });
    let set = &mut s.config.settings;
    set.mixing.enabled = true;
    set.auto_dj.enabled = true;
    set.playback.track_fade_secs = 3.0;
    let source = listen_subsonic::Subsonic::new(ServerId(SERVER.into()), "https://nd.example", "me", "pw")
        .expect("client");
    let _ = s.update(Message::Connected(ServerId(SERVER.into()), Ok(SourceHandle(Arc::new(source)))));
    if let Some(ytm) = ytm {
        let _ = s.update(Message::Connected(ServerId(YT.into()), Ok(SourceHandle(ytm))));
    }
    for deck in 0..2 {
        let _ = s.update(Message::Deck(deck, PlayerMsg::Ready(Player::inert())));
    }
    let handle = crate::app::analysis::AnalysisHandle::default();
    handle.insert(std::iter::once(&current).chain(&upcoming).map(record).collect::<Vec<_>>());
    s.analysis = Some(handle);
    s.queue.set(vec![current], 0);
    s.queue.add_last(upcoming, auto);
    s.playback.playing = true;
    s.playback.volume = 100;
    s
}

/// Auto-DJ picks from three different albums.
fn auto_rig() -> Listen {
    rig(song(0, 0), vec![song(1, 0), song(2, 0)], true)
}

fn uid(s: &Listen, offset: usize) -> u64 {
    let at = s.queue.current_position().unwrap() + offset;
    s.queue.iter().nth(at).unwrap().uid
}

/// A position report from `deck` (routed like mpv's).
fn pos(s: &mut Listen, deck: usize, secs: f64) {
    let ev = PlayerEvent::Position(Duration::from_secs_f64(secs));
    let _ = s.update(Message::Deck(deck as u8, PlayerMsg::Event(ev)));
}

fn event(s: &mut Listen, deck: usize, ev: PlayerEvent) {
    let _ = s.update(Message::Deck(deck as u8, PlayerMsg::Event(ev)));
}

fn tick(s: &mut Listen) {
    let _ = s.update(Message::FadeTick);
}

fn plan_of(s: &Listen) -> TransitionPlan {
    s.mix.plan().expect("a plan").clone()
}

/// Outgoing position at which the incoming deck starts.
fn start_out(s: &Listen) -> f64 {
    let p = plan_of(s);
    p.out_mix_start_s - (p.in_mix_start_s - p.in_load_at_s) / p.in_speed * s.mix.speed[s.mix.out_deck]
}

/// Plans the mix, asserts it is a real one, and delivers the incoming deck's input.
fn plan(s: &mut Listen) {
    plan_unfetched(s);
    fetch(s);
    assert_eq!(s.mix.phase, Phase::Planned);
}

/// [`plan`] without the fetch result (it is still under way).
fn plan_unfetched(s: &mut Listen) {
    let _ = s.refresh_next();
    assert_eq!(s.mix.phase, Phase::Planned, "an eligible, analysed pair gets a plan");
    assert_ne!(plan_of(s).kind, Kind::Gapless);
    assert_eq!(s.mix.fetch.busy, Some(s.mix.in_uid), "the fetch starts with the plan");
}

/// Runs the incoming song's real `mix_input` and feeds the result back.
fn fetch(s: &mut Listen) {
    let uid = s.mix.in_uid;
    let song = s.queue.iter().find(|i| i.uid == uid).expect("incoming song").song.clone();
    let source = s.sources.get(&song.server_id).expect("its source").clone();
    let rt = tokio::runtime::Builder::new_current_thread().build().expect("runtime");
    let r = rt.block_on(fetch_input(source, song));
    let _ = s.update(Message::Mix(MixMsg::Input(uid, r)));
}

/// Moves the outgoing deck to where the incoming one is preloaded.
fn to_preload(s: &mut Listen) {
    let (out, start) = (s.mix.out_deck, start_out(s));
    pos(s, out, start - PRELOAD_LEAD_S + 0.5);
    tick(s);
}

/// Walks a planned mix to `Running` the way mpv's replies would.
fn run(s: &mut Listen) {
    let (out, inn) = (s.mix.out_deck, s.mix.in_deck);
    let start = start_out(s);
    pos(s, out, start - PRELOAD_LEAD_S - 1.0);
    tick(s);
    assert_eq!(s.mix.phase, Phase::Planned, "not loaded before the lead time");
    pos(s, out, start - PRELOAD_LEAD_S + 0.5);
    tick(s);
    assert_eq!(s.mix.phase, Phase::Preloading);
    // A restart before mpv accepted the load is the deck's previous file.
    event(s, inn, PlayerEvent::Restarted);
    assert_eq!(s.mix.phase, Phase::Preloading);
    let token = s.mix.token;
    let _ = s.update(Message::Mix(MixMsg::Loaded(token, Ok(()))));
    event(s, inn, PlayerEvent::Restarted);
    assert_eq!(s.mix.phase, Phase::Ready);
    pos(s, out, start - 1.0);
    tick(s);
    assert_eq!(s.mix.phase, Phase::Starting);
    let token = s.mix.token;
    let _ = s.update(Message::Mix(MixMsg::Started(token, Ok(Instant::now()))));
    assert_eq!(s.mix.phase, Phase::Running);
}

/// Idle, the pair remembered as gapless, mpv's next slot handed back.
fn assert_cancelled(s: &mut Listen, pair: (u64, u64)) {
    assert_eq!(s.mix.phase, Phase::Idle);
    assert!(!s.mix.owns_next());
    assert_eq!(s.mix.skip, Some(pair), "the pair isn't tried again");
    assert_eq!(s.mix.gain(0), 1.0);
    assert_eq!(s.mix.gain(1), 1.0);
    let _ = s.refresh_next();
    assert_eq!(s.mix.phase, Phase::Idle, "the gapless slot stays");
    assert!(matches!(s.next_slot(), Some(crate::app::dj::Slot::Song(_))));
}

#[test]
fn plans_auto_dj_picks() {
    let mut s = auto_rig();
    plan(&mut s);
    assert_eq!(s.mix.pair(), Some((uid(&s, 0), uid(&s, 1))));
    assert!(s.mix.owns_next());
}

#[test]
fn no_plan_for_ineligible_pairs() {
    // An album played by hand: consecutive tracks stay gapless…
    let mut s = rig(song(0, 0), vec![song(0, 1)], false);
    let _ = s.refresh_next();
    assert_eq!(s.mix.phase, Phase::Idle, "hand-started album");
    // …and so do they when the Auto-DJ added them.
    let mut s = rig(song(0, 0), vec![song(0, 1)], true);
    let _ = s.refresh_next();
    assert_eq!(s.mix.phase, Phase::Idle, "same-album consecutive tracks");

    // A "between songs" DJ clip before the next song.
    let mut s = auto_rig();
    s.config.settings.ai_dj.enabled = true;
    s.dj.brk = Break::Armed {
        before_uid: uid(&s, 1),
        clip: Some("clip.mp3".into()),
        line: String::new(),
        mode: listen_dj::BreakMode::Between,
        clip_secs: 4.0,
    };
    let _ = s.refresh_next();
    assert_eq!(s.mix.phase, Phase::Idle, "between-songs DJ break");

    // Repeat one.
    let mut s = auto_rig();
    s.queue.repeat = Repeat::One;
    let _ = s.refresh_next();
    assert_eq!(s.mix.phase, Phase::Idle, "repeat one");

    // Mixing off.
    let mut s = auto_rig();
    s.config.settings.mixing.enabled = false;
    let _ = s.refresh_next();
    assert_eq!(s.mix.phase, Phase::Idle, "mixing off");
}

#[test]
fn a_between_break_armed_later_cancels_the_plan() {
    let mut s = auto_rig();
    plan(&mut s);
    let pair = s.mix.pair().unwrap();
    s.config.settings.ai_dj.enabled = true;
    s.dj.brk = Break::Armed {
        before_uid: pair.1,
        clip: Some("clip.mp3".into()),
        line: String::new(),
        mode: listen_dj::BreakMode::Between,
        clip_secs: 4.0,
    };
    tick(&mut s);
    assert_eq!(s.mix.phase, Phase::Idle);
    // Not planned again while the break waits (the pair isn't eligible).
    let _ = s.refresh_next();
    assert_eq!(s.mix.pair(), None);
    let _ = pair;
}

#[test]
fn refresh_while_planned_keeps_the_plan() {
    let mut s = auto_rig();
    plan(&mut s);
    let token = s.mix.token;
    for _ in 0..3 {
        let task = s.refresh_next();
        // Only the "clear mpv's next slot" command; no new plan, no resolve.
        assert_eq!(task.units(), 1);
        assert_eq!(s.mix.phase, Phase::Planned);
        assert_eq!(s.mix.token, token, "not re-planned");
    }
}

/// User actions drop the mix but not the pair: a seek re-plans it, and so does play after pause/stop.
#[test]
fn user_actions_cancel_and_the_pair_is_planned_again() {
    type Action = fn(&mut Listen);
    let cases: [(&str, Action); 5] = [
        ("seek", |s| drop(s.update(Message::Seek(30.0)))),
        ("stop", |s| drop(s.update(Message::Stop))),
        ("pause", |s| drop(s.update(Message::TogglePlay))),
        ("start current", |s| drop(s.start_current())),
        ("song ended early", |s| event(s, 0, PlayerEvent::Ended)),
    ];
    for (name, action) in cases {
        for running in [false, true] {
            let mut s = auto_rig();
            plan(&mut s);
            if running {
                run(&mut s);
            }
            let (pair, token) = (s.mix.pair().unwrap(), s.mix.token);
            action(&mut s);
            assert_eq!(s.decks.active, 0, "{name}: the active deck keeps playing");
            assert_eq!(s.mix.entered_uid, None, "{name}");
            assert_ne!(s.mix.skip, Some(pair), "{name}: not given up");
            assert!(!matches!(
                s.mix.phase,
                Phase::Preloading | Phase::Ready | Phase::Starting | Phase::Running
            ));
            match name {
                "song ended early" => assert_eq!(s.queue.current().unwrap().uid, pair.1, "{name}"),
                _ => assert_eq!(s.queue.current().unwrap().uid, pair.0, "{name}"),
            }
            match name {
                "seek" => {
                    assert_eq!(s.mix.pair(), Some(pair), "{name}: planned again");
                    assert!(s.mix.token > token, "{name}: a new plan");
                }
                "stop" | "pause" => {
                    assert_eq!(s.mix.phase, Phase::Idle, "{name}: nothing planned while stopped");
                    assert_eq!((s.mix.gain(0), s.mix.gain(1)), (1.0, 1.0));
                    let _ = s.update(Message::TogglePlay);
                    assert_eq!(s.mix.pair(), Some(pair), "{name}: planned again on play");
                }
                _ => {}
            }
        }
    }
}

/// An incoming deck not ready in time: the pair plays gapless and isn't tried again.
#[test]
fn a_late_incoming_deck_falls_back_to_gapless() {
    let mut s = auto_rig();
    plan(&mut s);
    let (out, pair) = (s.mix.out_deck, s.mix.pair().unwrap());
    let start = start_out(&s);
    pos(&mut s, out, start - PRELOAD_LEAD_S + 0.5);
    tick(&mut s);
    assert_eq!(s.mix.phase, Phase::Preloading);
    let deadline = plan_of(&s).out_mix_start_s - READY_DEADLINE_S;
    pos(&mut s, out, deadline + 0.1);
    tick(&mut s);
    assert_cancelled(&mut s, pair);
}

#[test]
fn a_queue_change_cancels_and_replans_the_new_pair() {
    let mut s = auto_rig();
    plan(&mut s);
    let pair = s.mix.pair().unwrap();
    s.queue.remove(pair.1);
    tick(&mut s);
    // The new next song (another Auto-DJ pick) gets its own plan.
    assert_eq!(s.mix.pair(), Some((pair.0, uid(&s, 1))));
}

#[test]
fn incoming_deck_is_silent_until_it_starts_then_follows_the_envelope() {
    let mut s = auto_rig();
    plan(&mut s);
    let (out, inn) = (s.mix.out_deck, s.mix.in_deck);
    let start = start_out(&s);
    // Before the start only the outgoing deck sounds, at full level (no fade-out of its own).
    s.playback.position = Duration::from_secs(LEN_S - 1);
    assert_eq!(s.deck_volume(out), 100.0);
    s.playback.position = Duration::ZERO;
    pos(&mut s, out, start - PRELOAD_LEAD_S + 0.5);
    tick(&mut s);
    let token = s.mix.token;
    let _ = s.update(Message::Mix(MixMsg::Loaded(token, Ok(()))));
    event(&mut s, inn, PlayerEvent::Restarted);
    for phase in [Phase::Ready, Phase::Starting] {
        if phase == Phase::Starting {
            pos(&mut s, out, start - 1.0);
            tick(&mut s);
        }
        assert_eq!(s.mix.phase, phase);
        assert_eq!(s.deck_volume(inn), 0.0, "{phase:?}: incoming silent");
        assert_eq!(s.deck_volume(out), 100.0, "{phase:?}: outgoing full");
    }
    let token = s.mix.token;
    let _ = s.update(Message::Mix(MixMsg::Started(token, Ok(Instant::now()))));

    let p = plan_of(&s);
    for frac in [0.1, 0.5, 0.9] {
        let out_t = p.out_mix_start_s + frac * p.blend_s * s.mix.speed[out];
        pos(&mut s, out, out_t);
        tick(&mut s);
        if s.mix.handed_over {
            break; // the active deck changed; covered by the handover test
        }
        let t = (out_t - p.out_mix_start_s) / s.mix.speed[out];
        let (g_out, g_in) = p.gain.gains_at(t);
        let trim = if g_out > 0.0 && g_in > 0.0 { 0.85 } else { 1.0 };
        for (deck, g) in [(out, g_out), (inn, g_in)] {
            let want = 100.0 * (g * trim).cbrt();
            // Positions are extrapolated by a few µs between report and tick.
            assert!(
                (s.deck_volume(deck) - want).abs() < 0.5,
                "deck {deck} at {frac}: {} vs {want}",
                s.deck_volume(deck)
            );
        }
    }
}

#[test]
fn handover_switches_queue_and_deck_once() {
    let mut s = auto_rig();
    plan(&mut s);
    run(&mut s);
    let (out, inn) = (s.mix.out_deck, s.mix.in_deck);
    let (out_uid, in_uid) = s.mix.pair().unwrap();
    let p = plan_of(&s);
    s.playback.scrobbled = true;
    pos(&mut s, inn, p.in_mix_start_s + 5.0);
    pos(&mut s, out, p.handover_out_s + 0.01);
    tick(&mut s);
    assert!(s.mix.handed_over);
    assert_eq!(s.queue.current().unwrap().uid, in_uid);
    assert_eq!(s.decks.active, inn);
    assert_eq!(s.mix.entered_uid, Some(in_uid));
    assert!(!s.playback.scrobbled, "the incoming song scrobbles on its own");
    assert!(s.playback.position.as_secs_f64() >= p.in_mix_start_s, "position is the incoming deck's");
    assert!(!s.mix.owns_next(), "mpv's next slot is the new song's again");

    // More ticks and reports don't advance the queue again.
    for dt in [0.1, 0.2] {
        pos(&mut s, out, p.handover_out_s + dt);
        tick(&mut s);
    }
    assert_eq!(s.queue.current().unwrap().uid, in_uid);
    assert_eq!(s.queue.iter().position(|i| i.uid == out_uid), Some(0));
    // The outgoing deck runs out and stops. Without a tempo settle the mix finishes in this tick
    // and the next pair is planned.
    pos(&mut s, out, p.out_stop_s + 0.01);
    tick(&mut s);
    assert!(s.mix.out_stopped || s.mix.pair() == Some((in_uid, uid(&s, 1))), "{:?}", s.mix.phase);
    assert_eq!(s.decks.active, inn);
    assert_eq!(s.queue.current().unwrap().uid, in_uid);
    assert_eq!(s.deck_volume(inn), 100.0);
    assert_eq!(s.deck_volume(out), 0.0);
}

#[test]
fn no_plan_while_a_between_clip_airs() {
    let mut s = auto_rig();
    s.config.settings.ai_dj.enabled = true;
    s.dj.brk = Break::Airing { clip: "clip.mp3".into() };
    let _ = s.mix_consider();
    assert_eq!(s.mix.phase, Phase::Idle);
}

#[test]
fn outgoing_tail_ending_after_handover_stops_it() {
    let mut s = auto_rig();
    plan(&mut s);
    run(&mut s);
    let (out, inn) = (s.mix.out_deck, s.mix.in_deck);
    let p = plan_of(&s);
    pos(&mut s, out, p.handover_out_s + 0.01);
    tick(&mut s);
    assert_eq!(s.decks.active, inn);
    let current = s.queue.current().unwrap().uid;
    event(&mut s, out, PlayerEvent::Ended);
    assert_eq!(s.mix.phase, Phase::Settling);
    assert_eq!(s.queue.current().unwrap().uid, current, "the old deck's end isn't the queue's");
}

#[test]
fn next_during_the_blend_jumps_into_the_incoming_song() {
    let mut s = auto_rig();
    plan(&mut s);
    run(&mut s);
    let inn = s.mix.in_deck;
    let in_uid = s.mix.pair().unwrap().1;
    let p = plan_of(&s);
    pos(&mut s, inn, p.in_mix_start_s + 0.5);
    let _ = s.update(Message::Next);
    assert_eq!(s.queue.current().unwrap().uid, in_uid, "exactly one song on");
    assert_eq!(s.decks.active, inn, "it keeps playing on its deck");
    assert_eq!(s.mix.entered_uid, Some(in_uid));
    assert_eq!(s.deck_volume(inn), 100.0, "full at once (and no fade-in of its own)");
    // The finished mix looks at the next pair straight away.
    assert_eq!(s.mix.pair(), Some((in_uid, uid(&s, 1))));
    assert_eq!(s.mix.out_deck, inn);
}

#[test]
fn next_before_the_blend_skips_as_usual() {
    let mut s = auto_rig();
    plan(&mut s);
    let pair = s.mix.pair().unwrap();
    let _ = s.update(Message::Next);
    assert_eq!(s.queue.current().unwrap().uid, pair.1);
    assert_eq!(s.decks.active, 0);
    assert_eq!(s.mix.entered_uid, None);
}

#[test]
fn mixed_in_song_gets_no_fade_in() {
    let mut s = auto_rig();
    plan(&mut s);
    run(&mut s);
    let inn = s.mix.in_deck;
    let in_uid = s.mix.pair().unwrap().1;
    let _ = s.update(Message::Next);
    assert_eq!(s.mix.entered_uid, Some(in_uid));
    // Early in the song, where a started song would still be fading in.
    s.playback.position = Duration::from_millis(500);
    assert_eq!(s.deck_volume(inn), 100.0);
    s.mix.entered_uid = None;
    assert!(s.deck_volume(inn) < 70.0, "control: a started song fades in");
}

/// A mix cancelled before its handover leaves the mixed-in current song's "no fade-in".
#[test]
fn cancel_keeps_the_current_songs_mixed_in_mark() {
    let mut s = auto_rig();
    plan(&mut s);
    run(&mut s);
    let inn = s.mix.in_deck;
    let _ = s.update(Message::Next);
    assert_eq!(s.mix.phase, Phase::Planned, "next pair planned");
    s.playback.position = Duration::from_millis(500);
    let _ = s.mix_cancel("the queue changed");
    assert_eq!(s.deck_volume(inn), 100.0, "no dip into a fade-in");
}

/// Planning right after a song change ignores the previous song's stale position.
#[test]
fn plan_after_a_song_change_ignores_the_old_position() {
    let mut s = rig(song(0, 0), vec![song(1, 0), song(2, 0), song(3, 0)], true);
    pos(&mut s, 0, 230.0);
    let _ = s.update(Message::Next); // nothing planned yet: a plain skip
    assert_eq!(s.playback.position, Duration::ZERO);
    plan(&mut s);
    tick(&mut s);
    assert_eq!(s.mix.phase, Phase::Planned, "not preloaded (or given up) on a stale position");
}

#[test]
fn repeat_one_switched_on_mid_mix_cancels_before_the_handover() {
    let mut s = auto_rig();
    plan(&mut s);
    run(&mut s);
    let pair = s.mix.pair().unwrap();
    s.queue.repeat = Repeat::One;
    tick(&mut s);
    assert_eq!(s.mix.phase, Phase::Idle);
    assert_eq!(s.queue.current().unwrap().uid, pair.0, "the song repeats");
    assert_eq!(s.decks.active, 0);
}

/// Navidrome → two YouTube Music picks (labelled Subsonic so the lookup finds their records).
fn ytm_rig(fail: bool) -> (Listen, Arc<FakeYtm>) {
    let ytm = FakeYtm::new(fail);
    let next = vec![yt_song(1, 0, SourceKind::Subsonic), yt_song(2, 0, SourceKind::Subsonic)];
    (rig_with(song(0, 0), next, true, Some(ytm.clone())), ytm)
}

fn id_of(s: &Listen, uid: u64) -> String {
    s.queue.iter().find(|i| i.uid == uid).unwrap().song.id.clone()
}

#[test]
fn the_incoming_deck_plays_the_fetched_file_which_stays_pinned_while_on_a_deck() {
    let (mut s, ytm) = ytm_rig(false);
    plan_unfetched(&mut s);
    let (out_uid, in_uid) = s.mix.pair().unwrap();
    let in_id = id_of(&s, in_uid);
    assert!(ytm.pinned().is_empty(), "nothing fetched yet");
    fetch(&mut s);
    let file = ytm.file(&s.queue.iter().find(|i| i.uid == in_uid).unwrap().song);
    assert_eq!(s.mix.fetch.input(in_uid), Some(&AudioInput::File(file.clone())));
    assert_eq!(ytm.pinned(), std::slice::from_ref(&in_id), "pinned from the fetch on");

    // A seek re-plans the pair and reuses the file (no second fetch).
    let _ = s.update(Message::Seek(30.0));
    assert_eq!(s.mix.pair(), Some((out_uid, in_uid)));
    assert_eq!(s.mix.fetch.busy, None);
    assert_eq!(ytm.pinned(), std::slice::from_ref(&in_id));

    run(&mut s);
    assert_eq!(s.mix.in_loaded, Some(AudioInput::File(file)), "the deck loads the local copy");

    // Handover: no listed or reported duration, so the analysis' length is shown.
    let (out, p) = (s.mix.out_deck, plan_of(&s));
    pos(&mut s, out, p.handover_out_s + 0.01);
    tick(&mut s);
    assert_eq!(s.queue.current().unwrap().uid, in_uid);
    assert_eq!(s.duration(), Duration::from_secs(LEN_S));
    pos(&mut s, out, p.out_stop_s + 0.01);
    tick(&mut s);
    if s.mix.phase == Phase::Settling {
        // Fast-forward the tempo settle and the last EQ ramp.
        s.mix.settle_started = Some(Instant::now() - Duration::from_secs(60));
        s.mix.ramps.clear();
        tick(&mut s);
    }
    // It plays on from the file: still pinned, next to the next pick's file.
    assert_eq!(s.mix.pair().map(|p| p.0), Some(in_uid), "the next pair is planned");
    assert_eq!(ytm.pinned(), std::slice::from_ref(&in_id));
    fetch(&mut s);
    let next_id = id_of(&s, s.mix.in_uid);
    assert_eq!(ytm.pinned(), [in_id, next_id.clone()]);

    // Skipping before that blend leaves nothing pinned.
    let _ = s.update(Message::Next);
    assert_eq!(s.queue.current().unwrap().song.id, next_id);
    assert!(ytm.pinned().is_empty(), "{:?}", ytm.pinned());
}

/// A fetch still under way when the incoming deck must start: the pair plays gapless.
#[test]
fn a_fetch_not_ready_in_time_falls_back_to_gapless() {
    let (mut s, ytm) = ytm_rig(false);
    plan_unfetched(&mut s);
    let pair = s.mix.pair().unwrap();
    // Inside the preload window a running fetch still gets time...
    to_preload(&mut s);
    assert_eq!(s.mix.phase, Phase::Planned, "a running fetch may still finish");
    // ...until just before the incoming deck would have to start.
    let (out, start) = (s.mix.out_deck, start_out(&s));
    pos(&mut s, out, start - FETCH_GRACE_S + 0.5);
    tick(&mut s);
    assert_cancelled(&mut s, pair);
    // The fetch finishing late changes nothing (and pins nothing).
    let song = s.queue.iter().find(|i| i.uid == pair.1).unwrap().song.clone();
    let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
    let r = rt.block_on(fetch_input(ytm.clone(), song));
    let _ = s.update(Message::Mix(MixMsg::Input(pair.1, r)));
    assert_eq!(s.mix.phase, Phase::Idle);
    assert!(ytm.pinned().is_empty());
}

/// A plan made inside the preload window whose fetch arrives a moment later still blends.
#[test]
fn a_fetch_finishing_inside_the_preload_window_still_blends() {
    let (mut s, _ytm) = ytm_rig(false);
    plan_unfetched(&mut s);
    to_preload(&mut s);
    assert_eq!(s.mix.phase, Phase::Planned);
    fetch(&mut s);
    tick(&mut s);
    assert_eq!(s.mix.phase, Phase::Preloading, "the incoming deck loads once the file is there");
}

#[test]
fn a_failed_fetch_gives_the_pair_up() {
    let (mut s, ytm) = ytm_rig(true);
    plan_unfetched(&mut s);
    let pair = s.mix.pair().unwrap();
    fetch(&mut s);
    assert_cancelled(&mut s, pair);
    assert!(ytm.pinned().is_empty());
}

/// A cancelled mix releases the incoming file.
#[test]
fn a_cancel_unpins_the_incoming_file() {
    let (mut s, ytm) = ytm_rig(false);
    plan(&mut s);
    run(&mut s);
    assert_eq!(ytm.pinned().len(), 1);
    let _ = s.update(Message::Stop);
    assert_eq!(s.mix.phase, Phase::Idle);
    assert!(ytm.pinned().is_empty());
}

/// YouTube Music → YouTube Music and Navidrome → YouTube Music, found under `ytm:<videoId>`.
#[test]
fn youtube_music_pairs_are_planned() {
    for current in [song(0, 0), yt_song(0, 1, SourceKind::YouTubeMusic)] {
        let ytm = FakeYtm::new(false);
        let next = vec![yt_song(1, 0, SourceKind::YouTubeMusic), yt_song(2, 0, SourceKind::YouTubeMusic)];
        let mut s = rig_with(current.clone(), next, true, Some(ytm.clone()));
        let (cur, nxt) = (s.queue.current().unwrap().clone(), s.queue.peek_next().unwrap().clone());
        assert!(listen_mix::plan::mix_eligible(&cur, &nxt, true, false, &s.config.settings.mixing));
        let analysis = s.analysis.clone().unwrap();
        assert!(analysis.lookup(&cur.song).is_some() && analysis.lookup(&nxt.song).is_some());
        plan(&mut s);
        run(&mut s);
        assert_eq!(s.mix.in_loaded, Some(AudioInput::File(ytm.file(&nxt.song))), "{}", current.title);
        let (out, p) = (s.mix.out_deck, plan_of(&s));
        pos(&mut s, out, p.handover_out_s + 0.01);
        tick(&mut s);
        assert_eq!(s.queue.current().unwrap().uid, nxt.uid);
        assert_eq!(s.duration(), Duration::from_secs(LEN_S));
    }
}
