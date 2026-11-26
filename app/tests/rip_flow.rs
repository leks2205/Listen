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

#[test]
fn snapshot_rip_panel() {
    shoot(1600.0, 950.0, "rip-panel");
}

/// Narrow: the warning, the status line and the picker header are all longer
/// than the card and have to wrap instead of running over the Cancel button.
#[test]
fn snapshot_rip_panel_narrow() {
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
