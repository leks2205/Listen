//! The rip panel's state machine in the app (no drive needed): picker,
//! choosing a version, finishing, failing, dismissing; plus a snapshot of
//! the panel with a picker and a finished rip.

mod common;

use iced_test::simulator::Simulator;
use listen::Args;
use listen::app::rip::RipMsg;
use listen::app::{Listen, Message};
use listen_rip::pipeline::{Candidate, Report, Stage, Update};
use listen_rip::tags::MetaSource;
use listen_rip::whipper::ArSummary;
use std::path::PathBuf;
use std::time::Duration;

fn demo() -> Listen {
    let args = Args { demo: true, theme: Some("tokyoNight".into()), ..Args::default() };
    Listen::boot(&args).0
}

fn up(state: &mut Listen, id: u64, u: Update) {
    let _ = state.update(Message::Rip(RipMsg::Update(id, u)));
}

fn candidates() -> Vec<Candidate> {
    ["BANGERS ver.", "CHALLENGERS ver.", "SPOILERS ver.", "LOVED IVE ver.", "DIGIPACK ver."]
        .iter()
        .map(|d| Candidate {
            title: "REVIVE+".into(),
            artist: "IVE".into(),
            details: format!("{d} · KR · 2026-02-23"),
            id: d.to_lowercase(),
        })
        .collect()
}

fn report() -> Box<Report> {
    Box::new(Report {
        artist: "IVE".into(),
        album: "REVIVE+".into(),
        folder: PathBuf::from("/mnt/nas/Music/IVE/REVIVE+"),
        accuraterip: ArSummary { tracks: 12, verified: 12, ..Default::default() },
        source: MetaSource::MusicBrainz,
        disc_id: "7rEapOzTbtsuZwvusWs2r_d8duc-".into(),
        submit_url: String::new(),
        warnings: Vec::new(),
    })
}

#[test]
fn picker_choice_and_finish() {
    let mut s = demo();
    let id = s.rip.test_job("/dev/sr0");
    up(&mut s, id, Update::Progress { track: 3, total: 12, percent: 40 });
    up(
        &mut s,
        id,
        Update::Candidates { candidates: candidates(), chosen: 0, timeout: Duration::from_secs(30) },
    );
    up(
        &mut s,
        id,
        Update::Identified { artist: "IVE".into(), album: "REVIVE+".into(), source: MetaSource::MusicBrainz },
    );
    let job = &s.rip.jobs[0];
    assert_eq!(job.title(), "IVE – REVIVE+");
    assert!(job.seconds_left().is_some_and(|l| l >= 29));
    assert!(s.rip.active());

    let _ = s.update(Message::Rip(RipMsg::Choose(id, 4)));
    assert_eq!(s.rip.jobs[0].chosen, 4);
    assert!(s.rip.jobs[0].picker_until.is_none(), "choosing stops the countdown");

    up(&mut s, id, Update::Stage(Stage::Tagging));
    up(&mut s, id, Update::Finished(report()));
    up(&mut s, id, Update::Stage(Stage::Done));
    assert!(!s.rip.active());
    assert!(s.rip.jobs[0].report.is_some());
    let _ = s.update(Message::Rip(RipMsg::Dismiss(id)));
    assert!(s.rip.jobs.is_empty());
}

#[test]
fn a_failed_rip_frees_the_queue() {
    let mut s = demo();
    let id = s.rip.test_job("/dev/sr0");
    up(&mut s, id, Update::Stage(Stage::Failed("the disc was removed".into())));
    assert!(!s.rip.active());
    // Dismissing an unfinished job does nothing.
    let id2 = s.rip.test_job("/dev/sr1");
    let _ = s.update(Message::Rip(RipMsg::Dismiss(id2)));
    assert_eq!(s.rip.jobs.len(), 2);
}

/// The panel mid-rip: a picker, a warning longer than the card, and a finished rip.
fn panel_state() -> Listen {
    let mut s = demo();
    let a = s.rip.test_job("/dev/sr0");
    up(&mut s, a, Update::Activity("Reading track 3 of 12".into()));
    up(&mut s, a, Update::Progress { track: 3, total: 12, percent: 40 });
    up(
        &mut s,
        a,
        Update::Candidates { candidates: candidates(), chosen: 0, timeout: Duration::from_secs(30) },
    );
    up(
        &mut s,
        a,
        Update::Identified { artist: "IVE".into(), album: "REVIVE+".into(), source: MetaSource::MusicBrainz },
    );
    up(
        &mut s,
        a,
        // Longer than the card is wide: it has to wrap, not run out of the card.
        Update::Warning(
            "MusicBrainz doesn't know this disc; Various Artists – GUMIN WORKS ORIGINAL SOUND \
             TRACK only matches its total length, not its tracks, and was not used"
                .into(),
        ),
    );
    let b = s.rip.test_job("/dev/sr1");
    up(
        &mut s,
        b,
        Update::Identified {
            artist: "IVE".into(),
            album: "I've IVE".into(),
            source: MetaSource::MusicBrainz,
        },
    );
    up(&mut s, b, Update::Finished(report()));
    up(&mut s, b, Update::Stage(Stage::Done));
    s
}

fn shoot(width: f32, height: f32, name: &str) {
    let s = panel_state();
    let settings = common::iced_settings();
    let theme = s.theme();
    let _gpu = common::gpu();
    let mut ui = Simulator::with_size(settings, iced::Size::new(width, height), s.view());
    let snapshot = ui.snapshot(&theme).expect("snapshot renders");
    common::save_png(&snapshot, name);
}

/// Wide, then narrow: the warning, the status line and the picker header are all longer than the card
/// and have to wrap instead of running over the Cancel button. One test: `save_png("rip-panel")` removes
/// every `rip-panel-*` picture first, the narrow one included.
#[test]
fn snapshot_rip_panel() {
    shoot(1600.0, 950.0, "rip-panel");
    shoot(900.0, 700.0, "rip-panel-narrow");
}

/// The rip card's Secure / Fast switch changes this rip's mode before its first track.
#[test]
fn rip_card_switches_the_mode() {
    use listen_core::settings::RipMode;
    let mut s = demo();
    let switch = listen_rip::ModeSwitch::new(RipMode::Fast);
    let id = s.rip.test_job_with_mode("/dev/sr0", switch.clone());
    up(&mut s, id, Update::Activity("Reading the disc".into()));
    let messages: Vec<Message> = {
        let _gpu = common::gpu();
        let mut ui = Simulator::with_size(common::iced_settings(), iced::Size::new(1600.0, 950.0), s.view());
        assert!(ui.find("Fast").is_ok());
        ui.click("Secure").expect("the Secure switch on the rip card");
        ui.into_messages().collect()
    };
    assert!(messages.iter().any(|m| matches!(m, Message::Rip(RipMsg::Mode(i, RipMode::Secure)) if *i == id)));
    for m in messages {
        let _ = s.update(m);
    }
    assert_eq!(switch.mode(), RipMode::Secure);
    let _ = s.update(Message::Rip(RipMsg::Mode(id, RipMode::Fast)));
    assert_eq!(switch.mode(), RipMode::Fast);
}

// ---- the rip window ------------------------------------------------------------------------------

use listen::app::Route;
use listen::app::rip::{TrackAr, TrackState};
use listen_rip::whipper::{Ar, TrackLog};

/// Twelve tracks, 3:00 to 4:50.
fn lengths() -> Vec<u64> {
    (0..12).map(|i| 180_000 + i * 10_000).collect()
}

fn titles() -> Vec<(u32, String)> {
    [
        "Opening Theme",
        "Paper Lanterns",
        "A Song That Has a Very Long Title, Longer Than the Column It Sits In",
        "Night Bus",
        "Glass",
        "Interlude",
        "Northbound",
        "Copper",
        "Low Tide",
        "Static",
        "Second Light",
        "Closing Theme",
    ]
    .iter()
    .enumerate()
    .map(|(i, t)| (i as u32 + 1, (*t).to_string()))
    .collect()
}

fn log(number: u32, v1: Ar) -> TrackLog {
    TrackLog { number, v1, status: "Copy OK".into(), ..Default::default() }
}

fn states(s: &Listen, job: usize) -> Vec<TrackState> {
    s.rip.jobs[job].tracks.iter().map(|t| t.state).collect()
}

/// Reads tracks 1..=`upto` (the last one to `percent`) as the pipeline reports it.
fn read_to(s: &mut Listen, id: u64, upto: u32, percent: u32) {
    for n in 1..=upto {
        up(s, id, Update::Activity(format!("Reading track {n} of 12")));
        up(s, id, Update::Progress { track: n, total: 12, percent: 0 });
        up(s, id, Update::Progress { track: n, total: 12, percent: if n == upto { percent } else { 100 } });
    }
}

#[test]
fn the_track_table_fills_in_as_tracks_progress() {
    use TrackState::*;
    let mut s = demo();
    let id = s.rip.test_job_with_tracks("/dev/sr0", &lengths());
    assert_eq!(states(&s, 0), vec![Waiting; 12], "every TOC track is there before the first is read");
    assert_eq!(s.rip.jobs[0].tracks[0].display_title(), "Track 01");
    up(&mut s, id, Update::Tracks(titles()));
    assert_eq!(s.rip.jobs[0].tracks[1].title, "Paper Lanterns");

    read_to(&mut s, id, 3, 40);
    let job = &s.rip.jobs[0];
    assert_eq!(&states(&s, 0)[..4], &[Encoding, Encoding, Reading, Waiting]);
    assert_eq!(job.tracks[2].percent, 40);
    assert_eq!(job.current, Some(2), "the row being read is highlighted");

    // Speed from what was measured: 3:00 + 3:10 + 40 % of 3:20 = 450 s of audio in 150 s = 3×.
    s.rip.jobs[0].clock.reading = Duration::from_secs(150);
    let job = &s.rip.jobs[0];
    assert_eq!(job.audio_read_ms(), 450_000);
    assert!((job.read_speed().unwrap() - 3.0).abs() < 0.01);
    let rest = (job.audio_total_ms() - 450_000) / 3;
    assert_eq!(job.time_left().unwrap().as_millis() as u64 / 1000, rest / 1000);

    read_to(&mut s, id, 12, 100);
    up(
        &mut s,
        id,
        Update::Read {
            tracks: (1..=12)
                .map(|n| log(n, if n == 7 { Ar::Mismatch } else { Ar::Match { confidence: 9 } }))
                .collect(),
            unread: Vec::new(),
        },
    );
    assert_eq!(states(&s, 0), vec![Encoding; 12]);
    assert_eq!(s.rip.jobs[0].current, None);
    assert_eq!(s.rip.jobs[0].tracks[0].accuraterip, Some(TrackAr::Verified(9)));
    assert_eq!(s.rip.jobs[0].tracks[6].accuraterip, Some(TrackAr::Mismatch));
    assert!(s.rip.jobs[0].time_left().is_none(), "nothing left to read");

    up(&mut s, id, Update::Stage(Stage::Tagging));
    up(&mut s, id, Update::Activity("Tagging track 5 of 12".into()));
    up(&mut s, id, Update::Progress { track: 5, total: 12, percent: 0 });
    assert_eq!(&states(&s, 0)[3..6], &[Tagged, Encoding, Encoding]);
    assert_eq!(s.rip.jobs[0].current, Some(4));
    up(&mut s, id, Update::Activity("ReplayGain".into()));
    assert_eq!(states(&s, 0), vec![Tagged; 12]);

    // The window shows the table and the log.
    let _ = s.update(Message::Navigate(Route::Rips));
    let _gpu = common::gpu();
    let mut ui = Simulator::with_size(common::iced_settings(), iced::Size::new(1600.0, 950.0), s.view());
    assert!(ui.find("Paper Lanterns").is_ok());
    assert!(ui.find("Mismatch").is_ok());
    assert!(ui.find("Tagging track 5 of 12").is_ok(), "the log");
}

#[test]
fn a_finished_rip_shows_its_folder_and_accuraterip_line() {
    let mut s = demo();
    let id = s.rip.test_job_with_tracks("/dev/sr0", &lengths());
    read_to(&mut s, id, 12, 100);
    up(
        &mut s,
        id,
        Update::Read {
            tracks: (1..=12).map(|n| log(n, Ar::Match { confidence: 4 })).collect(),
            unread: vec![],
        },
    );
    up(&mut s, id, Update::Stage(Stage::Tagging));
    up(&mut s, id, Update::Stage(Stage::Publishing));
    up(&mut s, id, Update::Warning("no cover found".into()));
    up(&mut s, id, Update::Finished(report()));
    up(&mut s, id, Update::Stage(Stage::Done));
    assert_eq!(states(&s, 0), vec![TrackState::Tagged; 12]);
    assert_eq!(s.rip.jobs[0].overall(), 1.0);

    let _ = s.update(Message::Navigate(Route::Rips));
    assert_eq!(s.route, Route::Rips);
    let _gpu = common::gpu();
    let mut ui = Simulator::with_size(common::iced_settings(), iced::Size::new(1600.0, 950.0), s.view());
    assert!(ui.find("Filed in /mnt/nas/Music/IVE/REVIVE+ · AccurateRip: 12/12 tracks verified").is_ok());
    assert!(ui.find("no cover found").is_ok(), "the warnings stay with the record");
    assert!(ui.find("Details").is_err(), "the window has no card above the player bar");
}

#[test]
fn a_track_that_could_not_be_read_shows_as_failed() {
    use TrackState::*;
    let mut s = demo();
    let id = s.rip.test_job_with_tracks("/dev/sr0", &lengths());
    read_to(&mut s, id, 12, 100);
    let mut logs: Vec<TrackLog> = (1..=12).map(|n| log(n, Ar::Match { confidence: 2 })).collect();
    logs[4].status = "Track skipped".into();
    up(&mut s, id, Update::Read { tracks: logs, unread: vec![5, 9] });
    assert_eq!(s.rip.jobs[0].tracks[4].state, Failed);
    assert_eq!(s.rip.jobs[0].tracks[8].state, Failed);
    assert_eq!(s.rip.jobs[0].tracks[4].accuraterip, None);

    // Tagging counts files: the 5th file is track 6.
    up(&mut s, id, Update::Stage(Stage::Tagging));
    up(&mut s, id, Update::Progress { track: 5, total: 10, percent: 0 });
    assert_eq!(s.rip.jobs[0].current, Some(5));
    assert_eq!(&states(&s, 0)[3..7], &[Tagged, Failed, Encoding, Encoding]);
    up(&mut s, id, Update::Stage(Stage::NeedsDecision("2 track(s) couldn't be read: [5, 9]".into())));
    assert_eq!(states(&s, 0).iter().filter(|t| **t == Failed).count(), 2);
    assert_eq!(states(&s, 0).iter().filter(|t| **t == Tagged).count(), 10);

    let _ = s.update(Message::Navigate(Route::Rips));
    {
        let _gpu = common::gpu();
        let mut ui = Simulator::with_size(common::iced_settings(), iced::Size::new(1600.0, 950.0), s.view());
        assert!(ui.find("Failed").is_ok());
        assert!(ui.find("Publish anyway").is_ok(), "the window has the card's actions");
    }

    // The disc leaves the drive during track 4: that one failed; nothing was filed, the rest were skipped.
    let id = s.rip.test_job_with_tracks("/dev/sr1", &lengths());
    read_to(&mut s, id, 4, 30);
    up(&mut s, id, Update::Stage(Stage::Failed("the disc was removed".into())));
    assert_eq!(&states(&s, 1)[..6], &[Skipped, Skipped, Skipped, Failed, Skipped, Skipped]);
}

#[test]
fn the_rip_window_shows_two_rips_at_once() {
    use TrackState::*;
    let mut s = demo();
    let a = s.rip.test_job_with_tracks("/dev/sr0", &lengths());
    let b = s.rip.test_job_with_tracks("/dev/sr1", &lengths()[..5]);
    up(
        &mut s,
        a,
        Update::Identified { artist: "IVE".into(), album: "REVIVE+".into(), source: MetaSource::MusicBrainz },
    );
    up(
        &mut s,
        b,
        Update::Identified { artist: "Someone".into(), album: "Other".into(), source: MetaSource::CdText },
    );
    read_to(&mut s, a, 6, 10);
    read_to(&mut s, b, 2, 80);
    up(&mut s, b, Update::Stage(Stage::Cancelled));
    assert_eq!(s.rip.jobs[0].current, Some(5));
    assert_eq!(states(&s, 1), vec![Skipped; 5], "a cancelled rip skips what wasn't tagged");
    assert_eq!(s.rip.jobs[0].tracks.len(), 12);

    let _ = s.update(Message::Navigate(Route::Rips));
    let _gpu = common::gpu();
    let mut ui = Simulator::with_size(common::iced_settings(), iced::Size::new(1600.0, 2400.0), s.view());
    assert!(ui.find("IVE – REVIVE+").is_ok());
    assert!(ui.find("Someone – Other").is_ok());
    assert!(ui.find("Metadata from CD-Text").is_ok());
}

/// "Details" on the rip card opens the rip window.
#[test]
fn details_on_the_card_opens_the_rip_window() {
    let mut s = demo();
    let _ = s.rip.test_job_with_tracks("/dev/sr0", &lengths());
    let messages: Vec<Message> = {
        let _gpu = common::gpu();
        let mut ui = Simulator::with_size(common::iced_settings(), iced::Size::new(1600.0, 950.0), s.view());
        ui.click("Details").expect("Details on the rip card");
        ui.into_messages().collect()
    };
    assert!(messages.iter().any(|m| matches!(m, Message::Navigate(Route::Rips))));
    for m in messages {
        let _ = s.update(m);
    }
    assert_eq!(s.route, Route::Rips);
}

/// Mid-rip: a picker, a warning, two tracks read, the third being read; and a finished rip below.
fn window_state() -> Listen {
    use listen_core::settings::RipMode;
    let mut s = demo();
    let a = s.rip.test_job_with_tracks("/dev/sr0", &lengths());
    {
        let j = &mut s.rip.jobs[0];
        j.mode = Some(listen_rip::ModeSwitch::new(RipMode::Fast));
        j.drive_label = "ASUS SDRW-08D2S-U".into();
        j.offset = Some((6, "AccurateRip drive database"));
        j.destination = PathBuf::from("/mnt/nas/Music");
    }
    let at = |s: &mut Listen, secs: u64| s.rip.jobs[0].clock.elapsed = Duration::from_secs(secs);
    up(&mut s, a, Update::Activity("Reading the disc".into()));
    at(&mut s, 4);
    up(
        &mut s,
        a,
        Update::Candidates { candidates: candidates(), chosen: 0, timeout: Duration::from_secs(30) },
    );
    up(
        &mut s,
        a,
        Update::Identified { artist: "IVE".into(), album: "REVIVE+".into(), source: MetaSource::MusicBrainz },
    );
    up(&mut s, a, Update::Tracks(titles()));
    for (n, secs) in [(1, 6), (2, 55), (3, 106)] {
        at(&mut s, secs);
        up(&mut s, a, Update::Activity(format!("Reading track {n} of 12")));
        up(&mut s, a, Update::Progress { track: n, total: 12, percent: 0 });
    }
    up(&mut s, a, Update::Log("track 3: a read error at 1:12; reading it again with paranoia".into()));
    up(&mut s, a, Update::Progress { track: 3, total: 12, percent: 40 });
    up(
        &mut s,
        a,
        Update::Warning(
            "MusicBrainz doesn't know this disc; Various Artists – GUMIN WORKS ORIGINAL SOUND \
             TRACK only matches its total length, not its tracks, and was not used"
                .into(),
        ),
    );
    {
        let j = &mut s.rip.jobs[0];
        j.clock.elapsed = Duration::from_secs(131);
        j.clock.reading = Duration::from_secs(125);
        // The countdown is wall-clock time: show the picker without it.
        j.picker_until = None;
    }
    let b = s.rip.test_job_with_tracks("/dev/sr1", &lengths()[..4]);
    up(
        &mut s,
        b,
        Update::Identified {
            artist: "IVE".into(),
            album: "I've IVE".into(),
            source: MetaSource::MusicBrainz,
        },
    );
    read_to(&mut s, b, 4, 100);
    up(
        &mut s,
        b,
        Update::Read {
            tracks: (1..=4).map(|n| log(n, Ar::Match { confidence: 12 })).collect(),
            unread: vec![],
        },
    );
    up(&mut s, b, Update::Stage(Stage::Tagging));
    up(&mut s, b, Update::Finished(report()));
    up(&mut s, b, Update::Stage(Stage::Done));
    s.rip.jobs[1].clock.elapsed = Duration::from_secs(312);
    let _ = s.update(Message::Navigate(Route::Rips));
    s
}

fn shoot_window(width: f32, height: f32, name: &str) {
    let mut s = window_state();
    // The page width (and with it the table's narrow layout) follows the window.
    s.window_size = iced::Size::new(width, height);
    let theme = s.theme();
    let _gpu = common::gpu();
    let mut ui = Simulator::with_size(common::iced_settings(), iced::Size::new(width, height), s.view());
    // 3:00 + 3:10 + 40 % of 3:20 = 450 s of audio in 125 s.
    assert!(ui.find("Elapsed 2:11 · 10:58 left · 3.6× real time").is_ok());
    let snapshot = ui.snapshot(&theme).expect("snapshot renders");
    let path = common::save_png(&snapshot, name);
    eprintln!("{}", path.display());
}

/// Wide, then narrow (900 px: the title column, the warning and the header lines wrap; nothing runs out
/// of the card). One test: `save_png("rip-window")` removes every `rip-window-*` picture first, the narrow
/// one included, so the two can't run in parallel.
#[test]
fn snapshot_rip_window() {
    shoot_window(1600.0, 950.0, "rip-window");
    shoot_window(900.0, 700.0, "rip-window-narrow");
}
