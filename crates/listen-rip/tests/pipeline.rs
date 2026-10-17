//! The whole rip pipeline with a mock drive (sine-wave FLACs and a
//! whipper-style log) into a temporary "NAS" folder. Needs `flac` and
//! `metaflac` on PATH (skipped otherwise).

use listen_rip::drive::DriveInfo;
use listen_rip::musicbrainz::{Release, parse_lookup};
use listen_rip::pipeline::{self, Candidate, Command, JobConfig, Lookup, Stage, Update};
use listen_rip::ripper::{MockRipper, Ripper};
use listen_rip::toc::Toc;
use listen_rip::whipper::Ar;
use listen_rip::{flac, publish};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

const COVER: &[u8] = include_bytes!("fixtures/cover.jpg");

fn tools() -> bool {
    let ok = ["flac", "metaflac"]
        .iter()
        .all(|t| std::process::Command::new(t).arg("--version").output().is_ok_and(|o| o.status.success()));
    if !ok {
        eprintln!("flac/metaflac not installed; skipping");
    }
    ok
}

fn revive_toc() -> Toc {
    Toc::from_offsets(
        1,
        154_800,
        &[150, 14625, 27975, 40575, 53175, 66750, 79425, 92325, 105150, 117975, 130200, 139500],
    )
}

fn revive_releases() -> Vec<Release> {
    parse_lookup(&serde_json::from_str(include_str!("fixtures/discid_revive.json")).unwrap())
}

struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("listen-rip-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("nas/Music")).unwrap();
        Self { root }
    }
    fn dest(&self) -> PathBuf {
        self.root.join("nas/Music")
    }
    fn cfg(&self, toc: Toc, lookup: Lookup) -> JobConfig {
        JobConfig {
            drive: DriveInfo {
                device: "/dev/sr-mock".into(),
                vendor: "HL-DT-ST".into(),
                model: "DVDRAM GU70N".into(),
            },
            toc,
            offset: 48,
            destination: self.dest(),
            work_dir: self.root.join("work"),
            log_dir: self.root.join("logs"),
            replay_gain: true,
            eject: false,
            picker_timeout: Duration::from_millis(800),
            stamp: "2026-10-01 14.32".into(),
            lookup,
            destination_retry: Duration::from_millis(100),
        }
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn mock(tracks: usize) -> MockRipper {
    MockRipper { track_secs: vec![1.0; tracks], ..Default::default() }
}

/// Runs a job; `react` may answer updates with commands.
async fn drive_job(
    cfg: JobConfig,
    ripper: MockRipper,
    mut react: impl FnMut(&Update) -> Option<Command>,
) -> (Vec<Update>, Stage) {
    let (tx, mut updates) = mpsc::unbounded_channel();
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let job = tokio::spawn(pipeline::run(cfg, Ripper::Mock(ripper), reqwest::Client::new(), tx, cmd_rx));
    let mut seen = Vec::new();
    let end = loop {
        let u = tokio::time::timeout(Duration::from_secs(120), updates.recv()).await.expect("job stalled");
        let Some(u) = u else { panic!("job ended without a final stage") };
        if let Some(c) = react(&u) {
            let _ = cmd_tx.send(c);
        }
        if let Update::Stage(s) = &u
            && s.finished()
        {
            break s.clone();
        }
        seen.push(u);
    };
    job.await.unwrap();
    (seen, end)
}

/// Watches the destination (ignoring dot folders) while a job runs and
/// records every FLAC it sees with its size: nothing half-written may show up.
fn watch_dest(dest: PathBuf) -> (Arc<AtomicBool>, std::thread::JoinHandle<HashMap<PathBuf, Vec<u64>>>) {
    let stop = Arc::new(AtomicBool::new(false));
    let s = stop.clone();
    let h = std::thread::spawn(move || {
        let mut seen: HashMap<PathBuf, Vec<u64>> = HashMap::new();
        fn walk(dir: &Path, seen: &mut HashMap<PathBuf, Vec<u64>>) {
            for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                let p = e.path();
                if p.is_dir() {
                    walk(&p, seen);
                } else if name.ends_with(".flac")
                    && let Ok(m) = p.metadata()
                {
                    seen.entry(p).or_default().push(m.len());
                }
            }
        }
        while !s.load(Ordering::Relaxed) {
            walk(&dest, &mut seen);
            std::thread::sleep(Duration::from_millis(2));
        }
        seen
    });
    (stop, h)
}

fn tags_of(map: &[(String, String)], key: &str) -> Vec<String> {
    map.iter().filter(|(k, _)| k == key).map(|(_, v)| v.clone()).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn musicbrainz_album_with_picker() {
    if !tools() {
        return;
    }
    let sb = Sandbox::new("mb");
    // Something the user already has next to it must stay untouched.
    std::fs::create_dir_all(sb.dest().join("IVE/Other")).unwrap();
    std::fs::write(sb.dest().join("IVE/Other/01 Keep.flac"), b"not really flac").unwrap();

    let (stop, watcher) = watch_dest(sb.dest());
    let cfg =
        sb.cfg(revive_toc(), Lookup::Given { releases: revive_releases(), cover: Some(COVER.to_vec()) });
    let candidates: Arc<Mutex<Vec<Candidate>>> = Arc::default();
    let c2 = candidates.clone();
    let (updates, end) = drive_job(cfg, mock(12), move |u| match u {
        Update::Candidates { candidates, chosen, .. } => {
            assert_eq!(*chosen, 0);
            *c2.lock().unwrap() = candidates.clone();
            Some(Command::Choose(2))
        }
        _ => None,
    })
    .await;
    stop.store(true, Ordering::Relaxed);
    let seen = watcher.join().unwrap();
    assert_eq!(end, Stage::Done, "{updates:?}");

    let candidates = candidates.lock().unwrap().clone();
    assert_eq!(candidates.len(), 5, "all five REVIVE+ versions offered");
    let report = updates
        .iter()
        .find_map(|u| if let Update::Finished(r) = u { Some(r.clone()) } else { None })
        .expect("a report");
    let album = sb.dest().join("IVE/REVIVE+");
    assert_eq!(report.folder, album);
    assert_eq!(report.accuraterip.verified, 12);
    assert_eq!(report.accuraterip_line(), "AccurateRip: 12/12 tracks verified");

    // Files: MusicBee's naming, cover.jpg, the rip log; no cue/m3u, no leftovers.
    let mut names: Vec<String> = std::fs::read_dir(&album)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names.len(), 14, "{names:?}");
    assert_eq!(names[0], "01 BLACKHOLE.flac");
    assert!(names.contains(&"03 숨바꼭질 (Hush).flac".to_string()), "{names:?}");
    assert!(names.contains(&"cover.jpg".to_string()) && names.contains(&"REVIVE+.log".to_string()));
    assert!(!sb.dest().join(publish::INCOMING).exists(), "staging folder removed once empty");
    assert!(!sb.root.join("work").exists(), "local work folder cleaned up");
    assert_eq!(std::fs::read(sb.dest().join("IVE/Other/01 Keep.flac")).unwrap(), b"not really flac");

    // Never a half-written file where Navidrome looks.
    for (path, sizes) in &seen {
        let fin = path.metadata().map(|m| m.len()).unwrap_or(0);
        assert!(sizes.iter().all(|s| *s == fin), "{} seen at sizes {sizes:?}, final {fin}", path.display());
    }

    // Tags, as a tag reader sees them.
    let f = album.join("03 숨바꼭질 (Hush).flac");
    let tags = flac::read_tags(&f).await.unwrap();
    assert_eq!(tags_of(&tags, "TITLE"), ["숨바꼭질 (Hush)"]);
    assert_eq!(tags_of(&tags, "ALBUMARTIST"), ["IVE"]);
    assert_eq!(tags_of(&tags, "ALBUM"), ["REVIVE+"]);
    assert_eq!(tags_of(&tags, "TRACKNUMBER"), ["3"]);
    assert_eq!(tags_of(&tags, "TRACKTOTAL"), ["12"]);
    assert_eq!(tags_of(&tags, "DISCNUMBER"), ["1"]);
    assert_eq!(tags_of(&tags, "MUSICBRAINZ_ALBUMID"), [candidates[2].id.clone()], "the picked version");
    assert_eq!(tags_of(&tags, "MUSICBRAINZ_DISCID"), ["7rEapOzTbtsuZwvusWs2r_d8duc-"]);
    assert_eq!(tags_of(&tags, "MUSICBRAINZ_TRACKID").len(), 1);
    assert_eq!(tags_of(&tags, "MUSICBRAINZ_ALBUMARTISTID").len(), 1);
    assert!(tags_of(&tags, "DATE")[0].starts_with("2026"));
    assert_eq!(tags_of(&tags, "REPLAYGAIN_TRACK_GAIN").len(), 1, "{tags:?}");
    assert_eq!(tags_of(&tags, "REPLAYGAIN_ALBUM_GAIN").len(), 1);
    assert!(flac::has_front_cover(&f).await, "cover embedded");
    // Audio intact after the level-8 re-encode.
    let md5 = flac::md5(&f).await.unwrap();
    assert!(!md5.chars().all(|c| c == '0'));
    let test = tokio::process::Command::new("flac").args(["-t", "--silent"]).arg(&f).status().await.unwrap();
    assert!(test.success());
    assert_eq!(flac::sample_count(&f).await.unwrap(), 44_100);

    // A second reader (FFmpeg) agrees, if installed.
    if let Ok(out) = std::process::Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "format_tags", "-of", "json"])
        .arg(&f)
        .output()
    {
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
        let t = &v["format"]["tags"];
        let get = |k: &str| {
            t.as_object()
                .and_then(|o| o.iter().find(|(key, _)| key.eq_ignore_ascii_case(k)))
                .and_then(|(_, v)| v.as_str())
                .unwrap_or("")
                .to_string()
        };
        assert_eq!(get("title"), "숨바꼭질 (Hush)");
        assert_eq!(get("album_artist"), "IVE");
        assert_eq!(get("MUSICBRAINZ_DISCID"), "7rEapOzTbtsuZwvusWs2r_d8duc-");
    }

    // LISTEN_RIP_KEEP=<dir>: keep the album for checks with other tag readers.
    if let Ok(keep) = std::env::var("LISTEN_RIP_KEEP") {
        let to = PathBuf::from(keep);
        std::fs::create_dir_all(&to).unwrap();
        for e in std::fs::read_dir(&album).unwrap().flatten() {
            std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn picker_times_out_to_the_best_match() {
    if !tools() {
        return;
    }
    let sb = Sandbox::new("timeout");
    let cfg = sb.cfg(revive_toc(), Lookup::Given { releases: revive_releases(), cover: None });
    let best: Arc<Mutex<String>> = Arc::default();
    let b2 = best.clone();
    let (_, end) = drive_job(cfg, mock(12), move |u| {
        if let Update::Candidates { candidates, .. } = u {
            *b2.lock().unwrap() = candidates[0].id.clone();
        }
        None
    })
    .await;
    assert_eq!(end, Stage::Done);
    let tags = flac::read_tags(&sb.dest().join("IVE/REVIVE+/01 BLACKHOLE.flac")).await.unwrap();
    assert_eq!(tags_of(&tags, "MUSICBRAINZ_ALBUMID"), [best.lock().unwrap().clone()]);
}

#[tokio::test(flavor = "multi_thread")]
async fn unknown_disc_uses_cd_text_and_is_flagged() {
    if !tools() {
        return;
    }
    let sb = Sandbox::new("cdtext");
    let toc = Toc::from_offsets(1, 30_000, &[150, 10_000, 20_000]);
    let ripper = MockRipper {
        toc_text: Some(
            "CD_DA\nCD_TEXT {\n  LANGUAGE 0 {\n    TITLE \"Mixtape\"\n    PERFORMER \"Band\"\n  }\n}\nTRACK AUDIO\nCD_TEXT {\n  LANGUAGE 0 {\n    TITLE \"Intro\"\n  }\n}\n"
                .into(),
        ),
        ..mock(3)
    };
    let (updates, end) = drive_job(sb.cfg(toc, Lookup::Offline), ripper, |_| None).await;
    assert_eq!(end, Stage::Done);
    assert!(updates.iter().any(|u| matches!(u, Update::Warning(w) if w.contains("CD-Text"))));
    let dir = sb.dest().join("Band/Mixtape");
    let tags = flac::read_tags(&dir.join("01 Intro.flac")).await.unwrap();
    assert_eq!(tags_of(&tags, "LISTEN_NEEDS_FIX"), ["1"]);
    assert!(dir.join("02 Track 02.flac").exists());
    assert!(!dir.join("cover.jpg").exists());

    // No CD-Text at all: "Unknown Artist/Unknown Album (…)".
    let sb2 = Sandbox::new("unknown");
    let toc = Toc::from_offsets(1, 30_000, &[150, 10_000, 20_000]);
    let (_, end) = drive_job(sb2.cfg(toc, Lookup::Offline), mock(3), |_| None).await;
    assert_eq!(end, Stage::Done);
    assert!(sb2.dest().join("Unknown Artist/Unknown Album (2026-10-01 14.32)/01 Track 01.flac").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn unreadable_track_waits_for_a_decision() {
    if !tools() {
        return;
    }
    let toc = || Toc::from_offsets(1, 30_000, &[150, 10_000, 20_000]);
    // Publish anyway.
    let sb = Sandbox::new("skip-publish");
    let ripper = MockRipper { skip_track: Some(2), ..mock(3) };
    let (_, end) = drive_job(sb.cfg(toc(), Lookup::Offline), ripper.clone(), |u| match u {
        Update::Stage(Stage::NeedsDecision(why)) => {
            assert!(why.contains("[2]"), "{why}");
            Some(Command::PublishAnyway)
        }
        _ => None,
    })
    .await;
    assert_eq!(end, Stage::Done);
    let dir = sb.dest().join("Unknown Artist/Unknown Album (2026-10-01 14.32)");
    assert!(dir.join("01 Track 01.flac").exists() && !dir.join("02 Track 02.flac").exists());

    // Discard: nothing reaches the NAS.
    let sb = Sandbox::new("skip-discard");
    let (_, end) = drive_job(sb.cfg(toc(), Lookup::Offline), ripper, |u| {
        matches!(u, Update::Stage(Stage::NeedsDecision(_))).then_some(Command::Discard)
    })
    .await;
    assert_eq!(end, Stage::Cancelled);
    assert_eq!(std::fs::read_dir(sb.dest()).unwrap().count(), 0);
    assert!(!sb.root.join("work").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn disc_removed_or_cancelled_mid_rip() {
    if !tools() {
        return;
    }
    let toc = || Toc::from_offsets(1, 30_000, &[150, 10_000, 20_000]);
    // whipper fails because the disc is gone.
    let sb = Sandbox::new("gone");
    let (_, end) =
        drive_job(sb.cfg(toc(), Lookup::Offline), MockRipper { fail_track: Some(2), ..mock(3) }, |_| None)
            .await;
    assert!(matches!(&end, Stage::Failed(r) if r.contains("no CD")), "{end:?}");
    assert_eq!(std::fs::read_dir(sb.dest()).unwrap().count(), 0);

    // Listen notices the removal first (UDisks2).
    let sb = Sandbox::new("removed");
    let slow = MockRipper { step: Duration::from_millis(200), ..mock(3) };
    let (_, end) = drive_job(sb.cfg(toc(), Lookup::Offline), slow.clone(), |u| {
        matches!(u, Update::Progress { track: 2, .. }).then_some(Command::DiscRemoved)
    })
    .await;
    assert_eq!(end, Stage::Failed("the disc was removed".into()));
    assert!(!sb.root.join("work").exists());

    // Cancel.
    let sb = Sandbox::new("cancel");
    let (_, end) = drive_job(sb.cfg(toc(), Lookup::Offline), slow, |u| {
        matches!(u, Update::Progress { track: 1, percent: 50, .. }).then_some(Command::Cancel)
    })
    .await;
    assert_eq!(end, Stage::Cancelled);
    assert_eq!(std::fs::read_dir(sb.dest()).unwrap().count(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn waits_for_the_nas_and_never_merges_albums() {
    if !tools() {
        return;
    }
    let sb = Sandbox::new("wait");
    let toc = Toc::from_offsets(1, 30_000, &[150, 10_000, 20_000]);
    let mut cfg = sb.cfg(toc, Lookup::Offline);
    cfg.destination = sb.root.join("not-mounted-yet/Music");
    let dest = cfg.destination.clone();
    let mut waited = false;
    let (updates, end) = drive_job(cfg, mock(3), |u| match u {
        Update::Stage(Stage::WaitingForDestination(_)) if !waited => {
            waited = true;
            std::fs::create_dir_all(&dest).unwrap();
            // An album with the same name is already there.
            std::fs::create_dir_all(dest.join("Unknown Artist/Unknown Album (2026-10-01 14.32)")).unwrap();
            std::fs::write(dest.join("Unknown Artist/Unknown Album (2026-10-01 14.32)/mine.txt"), b"x")
                .unwrap();
            Some(Command::RetryPublish)
        }
        _ => None,
    })
    .await;
    assert_eq!(end, Stage::Done);
    assert!(waited);
    let second = dest.join("Unknown Artist/Unknown Album (2026-10-01 14.32) (2)");
    assert!(second.join("01 Track 01.flac").exists());
    assert_eq!(
        std::fs::read_dir(dest.join("Unknown Artist/Unknown Album (2026-10-01 14.32)")).unwrap().count(),
        1
    );
    assert!(updates.iter().any(|u| matches!(u, Update::Warning(w) if w.contains("already existed"))));
}

#[tokio::test(flavor = "multi_thread")]
async fn accuraterip_mismatch_keeps_the_files_and_warns() {
    if !tools() {
        return;
    }
    let sb = Sandbox::new("ar");
    let toc = Toc::from_offsets(1, 30_000, &[150, 10_000, 20_000]);
    let ripper =
        MockRipper { accuraterip: vec![Ar::Match { confidence: 3 }, Ar::Mismatch, Ar::NotInDb], ..mock(3) };
    let (updates, end) = drive_job(sb.cfg(toc, Lookup::Offline), ripper, |_| None).await;
    assert_eq!(end, Stage::Done);
    let report = updates
        .iter()
        .find_map(|u| if let Update::Finished(r) = u { Some(r.clone()) } else { None })
        .unwrap();
    assert_eq!(
        report.accuraterip_line(),
        "AccurateRip: 1/3 tracks verified, 1 mismatch, 1 not in the AccurateRip database"
    );
    assert!(report.warnings.iter().any(|w| w.contains("didn't match (files kept)")));
    assert_eq!(
        std::fs::read_dir(&report.folder)
            .unwrap()
            .filter(|e| { e.as_ref().is_ok_and(|e| e.file_name().to_string_lossy().ends_with(".flac")) })
            .count(),
        3
    );
}
