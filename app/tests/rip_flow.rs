//! The rip panel's state machine in the app (no drive needed): picker,
//! choosing a version, finishing, failing, dismissing; plus a snapshot of
//! the panel with a picker and a finished rip.

use iced_test::simulator::Simulator;
use listen::app::rip::RipMsg;
use listen::app::{INTER, Listen, Message};
use listen::{Args, FONTS};
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

#[test]
fn snapshot_rip_panel() {
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

    let settings = iced::Settings {
        fonts: FONTS.iter().map(|f| std::borrow::Cow::Borrowed(*f)).collect(),
        default_font: INTER,
        ..Default::default()
    };
    let theme = s.theme();
    let mut ui = Simulator::with_size(settings, iced::Size::new(1600.0, 950.0), s.view());
    let snapshot = ui.snapshot(&theme).expect("snapshot renders");
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/snapshots");
    std::fs::create_dir_all(&dir).unwrap();
    for entry in std::fs::read_dir(&dir).unwrap().flatten() {
        if entry.file_name().to_string_lossy().starts_with("rip-panel-") {
            std::fs::remove_file(entry.path()).unwrap();
        }
    }
    assert!(snapshot.matches_image(dir.join("rip-panel")).expect("png written"));
}
