//! Heart-rate tempo mode with a fake heart-rate source (no Bluetooth): the queue re-ranks when the target
//! moves, Auto-DJ fills take songs that fit (topped up from the analysis store), the playing song never
//! stops, Off / Shuffle switch to Auto-DJ, songs the user queued stay, and the heart button toggles and
//! shows the target BPM.

mod common;

use iced_test::simulator::Simulator;
use listen::app::heart_rate::{HeartRateMsg, HrStatus};
use listen::app::play_mode::PlayMode;
use listen::app::{Listen, Message};
use listen::heart_rate::HrEvent;
use listen::{Args, demo};
use listen_core::model::Song;
use listen_core::queue::WhyCode;
use std::sync::Arc;

fn hr(m: HeartRateMsg) -> Message {
    Message::HeartRate(m)
}

fn sample(bpm: u16) -> Message {
    hr(HeartRateMsg::Source(HrEvent::Sample(bpm)))
}

/// Demo state in Off mode with an empty history and only the first song of the first album queued (and
/// playing).
fn rig() -> Listen {
    let (mut s, _) = Listen::boot(&Args { demo: true, ..Args::default() });
    s.history.entries.clear();
    s.config.settings.auto_dj.enabled = false;
    s.queue.set(demo::album_songs(0)[..1].to_vec(), 0);
    s.analysis = Some(listen::app::analysis::AnalysisHandle::default());
    s
}

/// A record saying `song` runs at `bpm`.
fn analysed(song: &Song, bpm: f32) -> Arc<listen_core::analysis::TrackAnalysis> {
    use listen_core::analysis::*;
    let period = 60.0 / bpm;
    let beats: Vec<f32> = (0..400).map(|i| 0.1 + i as f32 * period).collect();
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
            audio_end: song.duration.as_secs_f32().max(200.0),
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
            voiced: None,
        })),
    })
}

fn analyse(s: &Listen, song: &Song, bpm: f32) {
    s.analysis.as_ref().unwrap().insert([analysed(song, bpm)]);
}

/// Describe-it tags in `now`'s scene (its genre and decade) for `songs`, plus `other` for `others`: the scene
/// index the heart-rate top-up's scene gate reads.
fn tag_scenes(s: &Listen, now: &Song, songs: &[Song], others: &[(Song, &str)]) {
    use listen_core::scene::{SceneIndex, SceneRecord};
    let decade = now.year.map(|y| format!("{}s", y / 10 * 10)).unwrap_or_default();
    let tags = format!("synthwave, electronic, {decade}, instrumental");
    let rec = |x: &Song, t: &str| SceneRecord {
        song_id: listen_analysis::record_key(x),
        title_hint: format!("{} – {}", x.artist_line(), x.title),
        tags_en: t.into(),
        audio: None,
        at: 0,
    };
    let records = std::iter::once(rec(now, &tags))
        .chain(songs.iter().map(|x| rec(x, &tags)))
        .chain(others.iter().map(|(x, t)| rec(x, t)));
    s.analysis.as_ref().unwrap().set_scenes(SceneIndex::from_records(records));
}

/// A distinct song (own id, title and artist) from the demo library.
fn song(i: usize) -> Song {
    let mut s = demo::album_songs(i / 8)[i % 8 % demo::album_songs(i / 8).len()].clone();
    s.id = format!("hr-song-{i}");
    s.title = format!("Tempo song {i}");
    s.artists[0].name = format!("Tempo artist {i}");
    s
}

fn titles(s: &Listen) -> Vec<String> {
    s.queue.iter().map(|i| i.song.title.clone()).collect()
}

#[test]
fn off_and_shuffle_switch_to_auto_dj_and_off_keeps_the_mode() {
    let mut s = rig();
    assert_eq!(s.play_mode(), PlayMode::Off);
    let _ = s.update(hr(HeartRateMsg::Toggle));
    assert!(s.hr_on());
    assert_eq!(s.play_mode(), PlayMode::AutoDj, "Off → Auto-DJ");
    assert_eq!(s.hr_status(), HrStatus::Searching);
    let _ = s.update(hr(HeartRateMsg::Toggle));
    assert!(!s.hr_on());
    assert_eq!(s.hr_status(), HrStatus::Off);
    assert_eq!(s.play_mode(), PlayMode::AutoDj, "switching off leaves the play mode");

    let _ = s.update(Message::SetPlayMode(PlayMode::Shuffle));
    assert_eq!(s.play_mode(), PlayMode::Shuffle);
    let _ = s.update(hr(HeartRateMsg::Set(true)));
    assert_eq!(s.play_mode(), PlayMode::AutoDj, "Shuffle → Auto-DJ");
    assert!(!s.queue.shuffle());
}

#[test]
fn the_ai_dj_stays_the_ai_dj() {
    let mut s = rig();
    s.config.settings.ai_dj.enabled = true;
    assert_eq!(s.play_mode(), PlayMode::AiDj);
    let _ = s.update(hr(HeartRateMsg::Set(true)));
    assert!(s.hr_on());
    assert_eq!(s.play_mode(), PlayMode::AiDj);
}

#[test]
fn toggling_mid_song_never_interrupts_the_song() {
    let mut s = rig();
    s.queue.set(demo::album_songs(0), 2);
    s.playback.playing = true;
    s.playback.position = std::time::Duration::from_secs(42);
    let current = s.queue.current().unwrap().uid;
    for m in [
        hr(HeartRateMsg::Toggle),
        sample(150),
        sample(90),
        hr(HeartRateMsg::Toggle),
        hr(HeartRateMsg::Toggle),
    ] {
        let _ = s.update(m);
        assert_eq!(s.queue.current().map(|i| i.uid), Some(current));
        assert_eq!(s.queue.current_position(), Some(2));
        assert!(s.playback.playing);
        assert_eq!(s.playback.position, std::time::Duration::from_secs(42));
    }
}

#[test]
fn the_rerank_takes_out_misfit_dj_picks_and_never_touches_the_users_songs() {
    use listen_core::queue::{Picker, Why};
    let mut s = rig();
    let now = s.queue.current().unwrap().song.clone();
    analyse(&s, &now, 150.0);
    let why = || Why::new(Picker::AutoDj, WhyCode::NewToYou);
    // DJ picks that fit / don't, and songs the user added (a misfit at the end of the DJ picks, and one
    // queued with "Play next", which becomes the item after the current one).
    let next = song(1);
    let fit = song(2);
    let miss = song(3);
    let half = song(4);
    let unknown = song(5);
    for (x, bpm) in [(&next, 100.0), (&fit, 148.0), (&miss, 120.0), (&half, 76.0)] {
        analyse(&s, x, bpm);
    }
    s.queue.add_picked(vec![(next.clone(), why()), (fit.clone(), why()), (miss.clone(), why())]);
    let mine = song(6);
    analyse(&s, &mine, 60.0);
    s.queue.add_last(vec![mine.clone()], false);
    s.queue.add_picked(vec![(half.clone(), why()), (unknown.clone(), why())]);
    let queued = song(7);
    analyse(&s, &queued, 99.0);
    s.queue.enqueue_next(vec![queued.clone()]);
    let before = titles(&s);

    let _ = s.update(hr(HeartRateMsg::Set(true)));
    assert_eq!(s.play_mode(), PlayMode::AutoDj);
    assert_eq!(s.hr.reranks, 0, "no reading yet");
    let _ = s.update(sample(150));
    assert_eq!(s.hr.reranks, 1);
    assert_eq!(s.hr_target().map(|t| t.bpm), Some(150.0));
    assert_eq!(s.hr_status(), HrStatus::On);
    let after = titles(&s);
    for kept in [&now, &queued, &fit, &mine, &half] {
        assert!(after.contains(&kept.title), "{} kept: {after:?}", kept.title);
    }
    for gone in [&next, &miss, &unknown] {
        assert!(!after.contains(&gone.title), "{} taken out: {after:?}", gone.title);
    }
    // The user's songs keep their places relative to the queue's start.
    let at = |v: &[String], x: &Song| v.iter().position(|t| *t == x.title);
    assert_eq!(at(&after, &queued), at(&before, &queued));
    assert_eq!(at(&after, &queued), Some(1));
    assert_eq!(at(&after, &now), Some(0));
    // Closest tempo first in the DJ's slots: 148 (2 off), then 76 (half time, 2 off at the faster pulse).
    assert!(at(&after, &fit) < at(&after, &half));

    // A small move (152 smoothed over 30 s) doesn't re-rank; a big one does.
    let _ = s.hr_sample_at(152.0, 5.0);
    assert_eq!(s.hr.reranks, 1);
    let _ = s.hr_sample_at(100.0, 200.0);
    assert_eq!(s.hr.reranks, 2);
    let after = titles(&s);
    assert!(after.contains(&mine.title) && after.contains(&queued.title), "{after:?}");
}

#[test]
fn auto_dj_fills_take_fitting_songs_topped_up_from_the_store_and_off_is_normal_again() {
    let mut s = rig();
    s.config.settings.mixing.flow_select = true;
    let now = s.queue.current().unwrap().song.clone();
    analyse(&s, &now, 140.0);
    // The fetched candidates: one fits 150 (half time 75), the rest sit at 110–125.
    let fetched: Vec<Song> = (10..18).map(song).collect();
    analyse(&s, &fetched[0], 75.0);
    for (i, x) in fetched.iter().enumerate().skip(1) {
        analyse(&s, x, 108.0 + 2.0 * i as f32);
    }
    // The listening history holds analysed songs that fit (and one that doesn't).
    let store: Vec<Song> = (20..28).map(song).collect();
    for (i, x) in store.iter().enumerate() {
        analyse(&s, x, if i == 0 { 120.0 } else { 146.0 + i as f32 });
        s.history.entries.push(listen_core::history::Entry::new(x, 1_000 + i as i64));
    }
    // A history song that fits the tempo and is synthwave too, but from the 1990s: more than a decade away.
    let kpop = song(28);
    analyse(&s, &kpop, 150.0);
    s.history.entries.push(listen_core::history::Entry::new(&kpop, 2_000));
    tag_scenes(&s, &now, &store, &[(kpop.clone(), "synthwave, electronic, 1990s, instrumental")]);
    let _ = s.update(hr(HeartRateMsg::Set(true)));
    let _ = s.update(sample(150));
    let before = s.queue.len();
    let _ = s.update(Message::AutoDjLoaded(Ok(fetched.clone())));
    let added: Vec<_> = s.queue.iter().skip(before).collect();
    let n = s.config.settings.auto_dj.item_count as usize;
    assert_eq!(added.len(), n);
    let target = s.hr_target().unwrap();
    let lookup = |x: &Song| s.analysis.as_ref().unwrap().flow(x).and_then(|f| f.bpm).unwrap();
    for item in &added {
        assert!(item.auto);
        let bpm = lookup(&item.song);
        assert!(listen_mix::tempo::fits(&target, bpm), "{} at {bpm}", item.song.title);
        assert!(
            matches!(item.why.as_ref().map(|w| &w.code), Some(WhyCode::MatchesTempo { .. })),
            "{:?}",
            item.why
        );
    }
    assert!(added.iter().any(|i| i.song.id == fetched[0].id), "the fitting fetched song");
    assert!(!added.iter().any(|i| i.song.id == store[0].id), "the store's misfit stays out");
    assert!(!added.iter().any(|i| i.song.id == kpop.id), "another era stays out");
    let fill = s.hr.last_fill.clone().unwrap();
    assert_eq!(fill["padded"], 0);
    assert_eq!(fill["from_store"], (n - 1) as u64);
    assert_eq!(fill["other_era"], 1, "{fill}");
    assert!(
        fill["scene_note"].as_str().unwrap().contains("[session scene instrumental · electronic"),
        "{fill}"
    );
    let tempo_picks: Vec<u64> = added.iter().map(|i| i.uid).collect();

    // Off: the next fill is an ordinary one; the tempo picks already queued stay.
    let _ = s.update(hr(HeartRateMsg::Set(false)));
    let before = s.queue.len();
    let _ = s.update(Message::AutoDjLoaded(Ok(fetched[1..].to_vec())));
    let added: Vec<_> = s.queue.iter().skip(before).collect();
    assert!(!added.is_empty());
    assert!(
        added.iter().all(|i| !matches!(i.why.as_ref().map(|w| &w.code), Some(WhyCode::MatchesTempo { .. })))
    );
    assert!(tempo_picks.iter().all(|uid| s.queue.iter().any(|i| i.uid == *uid)));
}

#[test]
fn a_short_store_pads_with_the_nearest_misfits() {
    let mut s = rig();
    let now = s.queue.current().unwrap().song.clone();
    analyse(&s, &now, 140.0);
    let fetched: Vec<Song> = (30..38).map(song).collect();
    for (i, x) in fetched.iter().enumerate() {
        analyse(&s, x, 100.0 + i as f32);
    }
    let _ = s.update(hr(HeartRateMsg::Set(true)));
    let _ = s.update(sample(70));
    let before = s.queue.len();
    let _ = s.update(Message::AutoDjLoaded(Ok(fetched)));
    assert_eq!(
        s.queue.len() - before,
        s.config.settings.auto_dj.item_count as usize,
        "the DJ never runs dry"
    );
    let fill = s.hr.last_fill.clone().unwrap();
    assert_eq!(fill["fitting"], 0);
    assert!(fill["note"].as_str().unwrap().starts_with("heart-rate tempo: only 0 songs fit 70"));
}

#[test]
fn the_source_status_and_the_remembered_sensor() {
    let mut s = rig();
    // Events while off are ignored.
    let _ = s.update(sample(120));
    assert!(s.hr_target().is_none() && s.hr.last_bpm.is_none());
    let _ = s.update(hr(HeartRateMsg::Set(true)));
    let _ = s.update(hr(HeartRateMsg::Source(HrEvent::NotFound("no Bluetooth adapter".into()))));
    assert_eq!(s.hr_status(), HrStatus::NotFound);
    assert_eq!(s.hr_status().text(), "No heart-rate source found");
    let _ = s.update(hr(HeartRateMsg::Source(HrEvent::Searching)));
    assert_eq!(s.hr_status(), HrStatus::Searching);
    // The fake source is never remembered; a real sensor is.
    let fake = HrEvent::Connected { id: "fake".into(), name: "Test heart-rate source".into() };
    let _ = s.update(hr(HeartRateMsg::Source(fake)));
    assert_eq!(s.hr_status(), HrStatus::On);
    assert!(s.config.settings.playback.heart_rate_sensor.is_none());
    let strap = HrEvent::Connected { id: "AA:BB:CC:DD:EE:FF".into(), name: "Strap 1234".into() };
    let _ = s.update(hr(HeartRateMsg::Source(strap)));
    assert_eq!(
        s.config.settings.playback.heart_rate_sensor.as_ref().map(|x| x.name.as_str()),
        Some("Strap 1234")
    );
    let _ = s.update(hr(HeartRateMsg::Source(HrEvent::Lost)));
    assert_eq!(s.hr_status(), HrStatus::NotFound);
    let generation = s.hr.generation;
    let _ = s.update(hr(HeartRateMsg::Forget));
    assert!(s.config.settings.playback.heart_rate_sensor.is_none());
    assert!(s.hr.generation > generation, "the source starts over");
    // Implausible readings don't set a target.
    let _ = s.update(sample(10));
    assert!(s.hr_target().is_none());
    let _ = s.update(sample(200));
    assert_eq!(s.hr_target().map(|t| t.bpm), Some(180.0), "clamped");
}

#[test]
fn the_hotkey_and_mcp_switch_it() {
    let mut s = rig();
    let m = s.hotkey_message("toggleHeartRate").expect("bound to a message");
    assert!(matches!(m, Message::HeartRate(HeartRateMsg::Toggle)));
    let _ = s.update(m);
    assert!(s.hr_on());
    assert!(listen_core::settings::hotkey_action("toggleHeartRate").is_some());
    let json = s.hr_json();
    assert_eq!(json["on"], true);
    assert_eq!(json["status"], "searching");
}

/// Clicks the widget with the id `id` in the current view and feeds the messages back.
fn click(s: &mut Listen, id: &'static str) {
    let messages: Vec<Message> = {
        let _gpu = common::gpu();
        let mut ui = Simulator::with_size(common::iced_settings(), s.window_size, s.view());
        ui.click(iced::widget::Id::new(id)).unwrap_or_else(|e| panic!("no {id}: {e:?}"));
        ui.into_messages().collect()
    };
    for m in messages {
        let _ = s.update(m);
    }
}

fn shows(s: &Listen, text: &str) -> bool {
    let _gpu = common::gpu();
    let mut ui = Simulator::with_size(common::iced_settings(), s.window_size, s.view());
    ui.find(text).is_ok()
}

#[test]
fn the_heart_button_toggles_and_shows_the_target_bpm() {
    let mut s = rig();
    s.queue.set(demo::album_songs(0), 0);
    let _ = s.update(Message::WindowResized(iced::Size::new(1600.0, 900.0)));
    let id = listen::ui::player_bar::HEART_ID;
    click(&mut s, id);
    assert!(s.hr_on(), "the button switched it on");
    assert_eq!(s.play_mode(), PlayMode::AutoDj);
    assert!(!shows(&s, "137"), "no target before a reading");
    let _ = s.update(sample(137));
    assert!(shows(&s, "137"), "the target BPM is on the button");
    click(&mut s, id);
    assert!(!s.hr_on(), "the button switched it off");
    assert!(!shows(&s, "137"));
}
