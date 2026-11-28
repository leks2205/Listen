//! The whole rip pipeline with a mock drive (sine-wave FLACs and a
//! whipper-style log) into a temporary "NAS" folder. Needs `flac` and
//! `metaflac` on PATH (skipped otherwise).

use listen_core::settings::RipMode;
use listen_rip::accuraterip::{self, DiscIds, Response};
use listen_rip::drive::DriveInfo;
use listen_rip::fast::{ArSource, FastRipper, Reader};
use listen_rip::gnudb;
use listen_rip::musicbrainz::{Credit, Medium, Release, Track, parse_lookup};
use listen_rip::pipeline::{self, Candidate, Command, JobConfig, Lookup, Stage, Update};
use listen_rip::ripper::{MockRipper, ModeSwitch, Ripper};
use listen_rip::tags::MetaSource;
use listen_rip::toc::Toc;
use listen_rip::whipper::{self, Ar};
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
        eprintln!(
            "SKIPPED {}: flac/metaflac not installed",
            std::thread::current().name().unwrap_or("pipeline")
        );
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

/// A one-CD release of `lengths` (ms) by "Band", with its own disc ID.
fn other_album(lengths: &[u64]) -> Release {
    Release {
        id: "e4e0b05a-01df-4e5d-af4e-d479cfc83780".into(),
        title: "Soundtrack".into(),
        credits: vec![Credit { name: "Band".into(), ..Default::default() }],
        media: vec![Medium {
            position: 1,
            format: "CD".into(),
            disc_ids: vec!["d2sA8pwMLzvc3mgfypRWeEcvXUQ-".into()],
            tracks: lengths
                .iter()
                .enumerate()
                .map(|(i, &ms)| Track {
                    position: i as u32 + 1,
                    title: format!("Song {}", i + 1),
                    length_ms: Some(ms),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }],
        ..Default::default()
    }
}

/// A 6-track CD (15:53) whose disc ID MusicBrainz does not have.
fn unknown_toc() -> Toc {
    Toc::from_offsets(1, 71_625, &[150, 12_675, 22_950, 35_175, 46_875, 60_225])
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
            gnudb_email: String::new(),
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
    react: impl FnMut(&Update) -> Option<Command>,
) -> (Vec<Update>, Stage) {
    run_job(cfg, Ripper::Mock(ripper), react).await
}

/// AccurateRip's answer for the mock's disc: one entry with every track's v1 checksum, one with its
/// v2 checksum (the database keeps both), `wrong` tracks (1-based) with a checksum that doesn't fit.
fn ar_bin(m: &MockRipper, toc: &Toc, confidence: u8, wrong: &[u32]) -> Vec<u8> {
    let n = m.track_secs.len() as u32;
    let sums: Vec<(u32, u32)> = (1..=n).map(|t| accuraterip::checksums(&m.pcm(t), t, n)).collect();
    let entry = |pick: fn(&(u32, u32)) -> u32| Response {
        ids: DiscIds::from_toc(toc),
        tracks: sums
            .iter()
            .enumerate()
            .map(|(i, s)| (confidence, if wrong.contains(&(i as u32 + 1)) { !pick(s) } else { pick(s) }))
            .collect(),
    };
    accuraterip::encode(&[entry(|s| s.0), entry(|s| s.1)])
}

fn fast(m: MockRipper, bin: Option<Vec<u8>>) -> Ripper {
    Ripper::Fast(FastRipper { reader: Reader::Mock(m), accuraterip: ArSource::Given(bin) })
}

async fn run_job(
    cfg: JobConfig,
    ripper: Ripper,
    mut react: impl FnMut(&Update) -> Option<Command>,
) -> (Vec<Update>, Stage) {
    let (tx, mut updates) = mpsc::unbounded_channel();
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let job = tokio::spawn(pipeline::run(cfg, ripper, reqwest::Client::new(), tx, cmd_rx));
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
    let cfg = sb.cfg(
        revive_toc(),
        Lookup::Given { releases: revive_releases(), cover: Some(COVER.to_vec()), exact: true, gnudb: None },
    );
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
    let cfg = sb.cfg(
        revive_toc(),
        Lookup::Given { releases: revive_releases(), cover: None, exact: true, gnudb: None },
    );
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

/// For a disc it doesn't know, MusicBrainz's fuzzy TOC search answers with
/// albums of the same total length. One whose tracks differ is a different
/// album (a 6-track CD came back as a Japanese OST) and may not tag the rip.
#[tokio::test(flavor = "multi_thread")]
async fn a_fuzzy_match_with_other_tracks_leaves_the_disc_untagged() {
    if !tools() {
        return;
    }
    let sb = Sandbox::new("fuzzy-wrong");
    // Same track count, total length within a second, two tracks 5.5 s and 8.2 s off.
    let other = other_album(&[172_520, 137_786, 162_093, 155_280, 169_786, 154_533]);
    let cfg = sb
        .cfg(unknown_toc(), Lookup::Given { releases: vec![other], cover: None, exact: false, gnudb: None });
    let (updates, end) = drive_job(cfg, mock(6), |_| None).await;
    assert_eq!(end, Stage::Done);
    assert!(updates.iter().any(|u| matches!(u, Update::Warning(w) if w.contains("not its tracks"))));
    assert!(!updates.iter().any(|u| matches!(u, Update::Candidates { .. })));
    assert!(!sb.dest().join("Band").exists(), "it was filed under the wrong album");
    assert!(sb.dest().join("Unknown Artist/Unknown Album (2026-10-01 14.32)/01 Track 01.flac").exists());
}

/// A fuzzy match whose every track fits is the disc (another pressing) and is used.
#[tokio::test(flavor = "multi_thread")]
async fn a_fuzzy_match_whose_tracks_fit_tags_the_disc() {
    if !tools() {
        return;
    }
    let sb = Sandbox::new("fuzzy-fits");
    let mut lengths = unknown_toc().track_lengths_ms();
    lengths[0] += 900;
    lengths[4] -= 1_100;
    let other = other_album(&lengths);
    let cfg = sb
        .cfg(unknown_toc(), Lookup::Given { releases: vec![other], cover: None, exact: false, gnudb: None });
    let (updates, end) = drive_job(cfg, mock(6), |_| None).await;
    assert_eq!(end, Stage::Done);
    assert!(updates.iter().any(|u| matches!(u, Update::Warning(w) if w.contains("every track length"))));
    assert!(sb.dest().join("Band/Soundtrack/01 Song 1.flac").exists());
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

/// The disc from the bug report: MusicBrainz 404s its disc ID, gnudb knows it. It is filed under
/// gnudb's name but stays flagged (still not on MusicBrainz); gnudb wins over CD-Text, is never
/// asked when MusicBrainz knows the disc, and never without an e-mail address.
#[tokio::test(flavor = "multi_thread")]
async fn unknown_disc_is_named_from_gnudb_and_still_flagged() {
    if !tools() {
        return;
    }
    let entry = || {
        Some(Box::new(
            gnudb::Recorded::new()
                .cddb(&gnudb::query_command(&unknown_toc()), include_str!("fixtures/gnudb_query_heart.txt"))
                .cddb("cddb read data 4603b98a", include_str!("fixtures/gnudb_read_heart.txt")),
        ))
    };
    let cd_text = "CD_DA\nCD_TEXT {\n  LANGUAGE 0 {\n    TITLE \"Mixtape\"\n    PERFORMER \"Band\"\n  }\n}\n";
    let ripper = || MockRipper { toc_text: Some(cd_text.into()), ..mock(6) };

    let sb = Sandbox::new("gnudb");
    let mut cfg = sb.cfg(
        unknown_toc(),
        Lookup::Given { releases: vec![], cover: Some(COVER.to_vec()), exact: false, gnudb: entry() },
    );
    cfg.gnudb_email = "someone@example.com".into();
    let (updates, end) = drive_job(cfg, ripper(), |_| None).await;
    assert_eq!(end, Stage::Done, "{updates:?}");
    assert!(updates.iter().any(
        |u| matches!(u, Update::Warning(w) if w == "MusicBrainz doesn't know this disc — tagged from gnudb")
    ));
    let report = report_of(&updates);
    assert_eq!(report.source, MetaSource::Gnudb);
    let dir = sb.dest().join("HEART OF WOMAN/ETERNAL FADE");
    assert_eq!(report.folder, dir);
    let tags = flac::read_tags(&dir.join("01 Footsteps.flac")).await.unwrap();
    assert_eq!(tags_of(&tags, "ALBUM"), ["ETERNAL FADE"]);
    assert_eq!(tags_of(&tags, "ALBUMARTIST"), ["HEART OF WOMAN"]);
    assert_eq!(tags_of(&tags, "ARTIST"), ["HEART OF WOMAN"]);
    assert_eq!(tags_of(&tags, "TITLE"), ["Footsteps"]);
    assert_eq!(tags_of(&tags, "DATE"), ["2026"]);
    assert_eq!(tags_of(&tags, "LISTEN_NEEDS_FIX"), ["1"]);
    assert!(tags_of(&tags, "MUSICBRAINZ_ALBUMID").is_empty());
    assert!(tags_of(&tags, "MUSICBRAINZ_TRACKID").is_empty());
    assert!(dir.join("05 LIT IT.flac").exists() && dir.join("06 Find U.flac").exists());
    assert!(!sb.dest().join("Band").exists(), "CD-Text only comes after gnudb");

    // MusicBrainz knows the disc: gnudb's answer is not used.
    let sb2 = Sandbox::new("gnudb-mb");
    let mut cfg = sb2.cfg(
        revive_toc(),
        Lookup::Given { releases: revive_releases(), cover: None, exact: true, gnudb: entry() },
    );
    cfg.gnudb_email = "someone@example.com".into();
    cfg.picker_timeout = Duration::from_millis(50);
    let (updates, end) = drive_job(cfg, mock(12), |_| None).await;
    assert_eq!(end, Stage::Done);
    assert_eq!(report_of(&updates).source, MetaSource::MusicBrainz);
    assert!(sb2.dest().join("IVE/REVIVE+/01 BLACKHOLE.flac").exists());
    assert!(!sb2.dest().join("HEART OF WOMAN").exists());

    // No address: gnudb is not asked; CD-Text names the disc.
    let sb3 = Sandbox::new("gnudb-noaddr");
    let cfg =
        sb3.cfg(unknown_toc(), Lookup::Given { releases: vec![], cover: None, exact: false, gnudb: entry() });
    assert!(cfg.gnudb_email.is_empty());
    let (updates, end) = drive_job(cfg, ripper(), |_| None).await;
    assert_eq!(end, Stage::Done);
    assert_eq!(report_of(&updates).source, MetaSource::CdText);
    assert!(!sb3.dest().join("HEART OF WOMAN").exists());
    assert!(sb3.dest().join("Band/Mixtape/01 Track 01.flac").exists());

    // gnudb has nothing either: CD-Text.
    let sb4 = Sandbox::new("gnudb-none");
    let mut cfg =
        sb4.cfg(unknown_toc(), Lookup::Given { releases: vec![], cover: None, exact: false, gnudb: None });
    cfg.gnudb_email = "someone@example.com".into();
    let (updates, end) = drive_job(cfg, ripper(), |_| None).await;
    assert_eq!(end, Stage::Done);
    assert_eq!(report_of(&updates).source, MetaSource::CdText);
}

/// RESCENE – PULSE (미니4집): gnudb refuses the disc's length, then lists two exact entries with the
/// same TOC (an Erik Satie tribute first). iTunes knows PULSE with the disc's track lengths and
/// nothing of the other one, so PULSE names the disc; with no outside source confirming either,
/// neither does.
#[tokio::test(flavor = "multi_thread")]
async fn colliding_gnudb_entries_need_an_outside_confirmation() {
    if !tools() {
        return;
    }
    let toc = || Toc::from_offsets(1, 71_175, &[150, 10_725, 25_125, 41_025, 54_675]);
    let gnudb = || {
        gnudb::Recorded::new()
            .cddb(
                "cddb query 3703b305 5 150 10725 25125 41025 54675 949",
                include_str!("fixtures/gnudb_query_pulse_refused.txt"),
            )
            .cddb(
                "cddb query 3703b305 5 150 10725 25125 41025 54675 947",
                include_str!("fixtures/gnudb_query_pulse.txt"),
            )
            .cddb("cddb read data 3703b39a", include_str!("fixtures/gnudb_read_fadladder.txt"))
            .cddb("cddb read data 3703b39c", include_str!("fixtures/gnudb_read_pulse.txt"))
    };
    let confirmed = gnudb()
        .json(
            "https://itunes.apple.com/search?term=RESCENE PULSE&entity=album&limit=10",
            include_str!("fixtures/itunes_search_pulse.json"),
        )
        .json(
            "https://itunes.apple.com/lookup?id=6828335122&entity=song",
            include_str!("fixtures/itunes_lookup_pulse.json"),
        )
        .json(
            "https://itunes.apple.com/search?term=Fadladder Fadladder Play Erik Satie&entity=album&limit=10",
            include_str!("fixtures/itunes_search_fadladder.json"),
        )
        .json(
            "https://api.deezer.com/search/album?q=Fadladder Fadladder Play Erik Satie",
            include_str!("fixtures/deezer_search_fadladder.json"),
        );

    let sb = Sandbox::new("gnudb-collide");
    let lookup = Lookup::Given {
        releases: vec![],
        cover: None,
        exact: false,
        gnudb: Some(Box::new(confirmed.clone())),
    };
    let mut cfg = sb.cfg(toc(), lookup);
    cfg.gnudb_email = "someone@example.com".into();
    let (updates, end) = drive_job(cfg, mock(5), |_| None).await;
    assert_eq!(end, Stage::Done, "{updates:?}");
    let report = report_of(&updates);
    assert_eq!(report.source, MetaSource::Gnudb);
    let dir = sb.dest().join("RESCENE/PULSE (미니4집)");
    assert_eq!(report.folder, dir);
    let tags = flac::read_tags(&dir.join("01 Finest Thing.flac")).await.unwrap();
    assert_eq!(tags_of(&tags, "ALBUM"), ["PULSE (미니4집)"]);
    assert_eq!(tags_of(&tags, "ALBUMARTIST"), ["RESCENE"]);
    assert_eq!(tags_of(&tags, "DATE"), ["2026"]);
    assert_eq!(tags_of(&tags, "LISTEN_NEEDS_FIX"), ["1"]);
    assert!(!sb.dest().join("Fadladder").exists());
    let asked = confirmed.asked();
    assert_eq!(
        asked[..2],
        [
            "cddb query 3703b305 5 150 10725 25125 41025 54675 949",
            "cddb query 3703b305 5 150 10725 25125 41025 54675 947"
        ]
    );

    // Neither iTunes nor Deezer knows either record: "Unknown Album", still flagged.
    let sb2 = Sandbox::new("gnudb-collide-none");
    let lookup =
        Lookup::Given { releases: vec![], cover: None, exact: false, gnudb: Some(Box::new(gnudb())) };
    let mut cfg = sb2.cfg(toc(), lookup);
    cfg.gnudb_email = "someone@example.com".into();
    let (updates, end) = drive_job(cfg, mock(5), |_| None).await;
    assert_eq!(end, Stage::Done, "{updates:?}");
    assert_eq!(report_of(&updates).source, MetaSource::Unknown);
    let dir = sb2.dest().join("Unknown Artist/Unknown Album (2026-10-01 14.32)");
    let tags = flac::read_tags(&dir.join("01 Track 01.flac")).await.unwrap();
    assert_eq!(tags_of(&tags, "LISTEN_NEEDS_FIX"), ["1"]);
    assert!(!sb2.dest().join("RESCENE").exists() && !sb2.dest().join("Fadladder").exists());
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

fn report_of(updates: &[Update]) -> Box<pipeline::Report> {
    updates
        .iter()
        .find_map(|u| if let Update::Finished(r) = u { Some(r.clone()) } else { None })
        .expect("a report")
}

/// File name → (tags without the ones that differ per run, audio MD5) of every FLAC in `dir`,
/// plus the parsed rip log.
async fn album_of(dir: &Path) -> (Vec<(String, Vec<(String, String)>, String)>, whipper::RipLog) {
    let mut files = Vec::new();
    let mut log = None;
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        if name.ends_with(".flac") {
            let tags = flac::read_tags(&p).await.unwrap();
            files.push((name, tags, flac::md5(&p).await.unwrap()));
        } else if name.ends_with(".log") {
            log = Some(whipper::parse_log(&std::fs::read_to_string(&p).unwrap()));
        }
    }
    (files, log.expect("the rip log is filed with the album"))
}

/// Fast and Secure file the same album: same names, tags, audio and AccurateRip result.
#[tokio::test(flavor = "multi_thread")]
async fn fast_rip_files_the_same_album_as_secure() {
    if !tools() {
        return;
    }
    let lookup = || Lookup::Given {
        releases: revive_releases(),
        cover: Some(COVER.to_vec()),
        exact: true,
        gnudb: None,
    };
    let secure = Sandbox::new("same-secure");
    let (s_updates, end) = drive_job(secure.cfg(revive_toc(), lookup()), mock(12), |_| None).await;
    assert_eq!(end, Stage::Done, "{s_updates:?}");

    let quick = Sandbox::new("same-fast");
    let m = mock(12);
    let bin = ar_bin(&m, &revive_toc(), 7, &[]);
    let (f_updates, end) = run_job(quick.cfg(revive_toc(), lookup()), fast(m, Some(bin)), |_| None).await;
    assert_eq!(end, Stage::Done, "{f_updates:?}");

    let (s_report, f_report) = (report_of(&s_updates), report_of(&f_updates));
    assert_eq!(f_report.accuraterip_line(), "AccurateRip: 12/12 tracks verified");
    assert_eq!(f_report.accuraterip_line(), s_report.accuraterip_line());
    assert_eq!(f_report.accuraterip, s_report.accuraterip);
    assert_eq!(
        f_report.folder.strip_prefix(quick.dest()).unwrap(),
        s_report.folder.strip_prefix(secure.dest()).unwrap()
    );

    let (s_files, s_log) = album_of(&s_report.folder).await;
    let (f_files, f_log) = album_of(&f_report.folder).await;
    assert_eq!(f_files.len(), 12);
    assert_eq!(f_files, s_files, "same file names, tags (ReplayGain included) and audio");
    assert_eq!(f_log.summary(), s_log.summary());
    for (f, s) in f_log.tracks.iter().zip(&s_log.tracks) {
        assert_eq!((f.number, f.v1, f.v2), (s.number, s.v1, s.v2));
        assert!(f.filename.ends_with(&format!("{:02}.flac", f.number)), "{}", f.filename);
    }
    assert!(f_report.folder.join("cover.jpg").exists());
    // Fast reads every track once, in order, with progress.
    let mut tracks: Vec<u32> = f_updates
        .iter()
        .take_while(|u| !matches!(u, Update::Stage(Stage::Tagging)))
        .filter_map(|u| if let Update::Progress { track, percent: 0, .. } = u { Some(*track) } else { None })
        .collect();
    tracks.dedup();
    assert_eq!(tracks, (1..=12).collect::<Vec<_>>());
    assert!(f_updates.iter().any(|u| matches!(u, Update::Activity(a) if a == "Reading track 12 of 12")));
    assert!(!f_updates.iter().any(|u| matches!(u, Update::Activity(a) if a == "Reading table")));
}

/// CD-Text, unreadable tracks and AccurateRip's verdicts in Fast mode.
#[tokio::test(flavor = "multi_thread")]
async fn fast_rip_cd_text_mismatch_and_unknown_discs() {
    if !tools() {
        return;
    }
    let toc = || Toc::from_offsets(1, 30_000, &[150, 10_000, 20_000]);
    let sb = Sandbox::new("fast-cdtext");
    let m = MockRipper {
        toc_text: Some(
            "CD_DA\nCD_TEXT {\n  LANGUAGE 0 {\n    TITLE \"Mixtape\"\n    PERFORMER \"Café Band\"\n  }\n}\nTRACK AUDIO\nCD_TEXT {\n  LANGUAGE 0 {\n    TITLE \"Intro\"\n  }\n}\n"
                .into(),
        ),
        ..mock(3)
    };
    let bin = ar_bin(&m, &toc(), 4, &[2]);
    let (updates, end) = run_job(sb.cfg(toc(), Lookup::Offline), fast(m, Some(bin)), |_| None).await;
    assert_eq!(end, Stage::Done, "{updates:?}");
    let report = report_of(&updates);
    assert_eq!(report.accuraterip_line(), "AccurateRip: 2/3 tracks verified, 1 mismatch");
    assert!(report.warnings.iter().any(|w| w.contains("didn't match (files kept)")));
    let dir = sb.dest().join("Café Band/Mixtape");
    let tags = flac::read_tags(&dir.join("01 Intro.flac")).await.unwrap();
    assert_eq!(tags_of(&tags, "ALBUMARTIST"), ["Café Band"]);
    assert_eq!(tags_of(&tags, "LISTEN_NEEDS_FIX"), ["1"]);
    assert!(dir.join("02 Track 02.flac").exists());

    // No CD-Text, not in AccurateRip: as today, an unknown album, nothing verified.
    let sb = Sandbox::new("fast-unknown");
    let (updates, end) = run_job(sb.cfg(toc(), Lookup::Offline), fast(mock(3), None), |_| None).await;
    assert_eq!(end, Stage::Done);
    assert_eq!(
        report_of(&updates).accuraterip_line(),
        "AccurateRip: 0/3 tracks verified, 3 not in the AccurateRip database"
    );
    assert!(sb.dest().join("Unknown Artist/Unknown Album (2026-10-01 14.32)/01 Track 01.flac").exists());

    // An unreadable track asks, like whipper's skipped ones.
    let sb = Sandbox::new("fast-skip");
    let (_, end) = run_job(
        sb.cfg(toc(), Lookup::Offline),
        fast(MockRipper { skip_track: Some(2), ..mock(3) }, None),
        |u| match u {
            Update::Stage(Stage::NeedsDecision(why)) => {
                assert!(why.contains("[2]"), "{why}");
                Some(Command::PublishAnyway)
            }
            _ => None,
        },
    )
    .await;
    assert_eq!(end, Stage::Done);
    let dir = sb.dest().join("Unknown Artist/Unknown Album (2026-10-01 14.32)");
    assert!(dir.join("01 Track 01.flac").exists() && !dir.join("02 Track 02.flac").exists());

    // The disc is gone, or the user cancels.
    let sb = Sandbox::new("fast-gone");
    let (_, end) = run_job(
        sb.cfg(toc(), Lookup::Offline),
        fast(MockRipper { fail_track: Some(2), ..mock(3) }, None),
        |_| None,
    )
    .await;
    assert!(matches!(&end, Stage::Failed(r) if r.contains("no CD")), "{end:?}");
    let sb = Sandbox::new("fast-cancel");
    let slow = MockRipper { step: Duration::from_millis(200), ..mock(3) };
    let (_, end) = run_job(sb.cfg(toc(), Lookup::Offline), fast(slow, None), |u| {
        matches!(u, Update::Progress { track: 1, percent: 50, .. }).then_some(Command::Cancel)
    })
    .await;
    assert_eq!(end, Stage::Cancelled);
    assert_eq!(std::fs::read_dir(sb.dest()).unwrap().count(), 0);
}

/// The rip card's Secure/Fast switch: while whipper still reads the table the rip starts over in
/// the other mode; once a track is read the mode stays.
#[tokio::test(flavor = "multi_thread")]
async fn rip_mode_switches_until_the_first_track() {
    if !tools() {
        return;
    }
    let toc = || Toc::from_offsets(1, 30_000, &[150, 10_000, 20_000]);
    let either = |switch: &ModeSwitch| Ripper::Either {
        mode: switch.clone(),
        fast: Box::new(fast(mock(3), None)),
        secure: Box::new(Ripper::Mock(MockRipper { table: Duration::from_secs(20), ..mock(3) })),
    };
    let sb = Sandbox::new("switch");
    let switch = ModeSwitch::new(RipMode::Secure);
    let s2 = switch.clone();
    let (updates, end) = run_job(sb.cfg(toc(), Lookup::Offline), either(&switch), move |u| {
        if matches!(u, Update::Activity(a) if a == "Reading table") {
            assert!(s2.set(RipMode::Fast), "switchable while the table is read");
        }
        None
    })
    .await;
    assert_eq!(end, Stage::Done, "{updates:?}");
    assert!(switch.locked());
    assert_eq!(switch.mode(), RipMode::Fast);
    assert!(!switch.set(RipMode::Secure), "locked once a track was read");
    let dir = sb.dest().join("Unknown Artist/Unknown Album (2026-10-01 14.32)");
    let log = std::fs::read_to_string(dir.join("Unknown Album (2026-10-01 14.32).log")).unwrap();
    assert!(log.contains("(Fast mode)"), "{log}");

    // Fast to Secure after the first track: ignored, the rip stays Fast.
    let sb = Sandbox::new("switch-late");
    let switch = ModeSwitch::new(RipMode::Fast);
    let s2 = switch.clone();
    let (_, end) = run_job(sb.cfg(toc(), Lookup::Offline), either(&switch), move |u| {
        if matches!(u, Update::Progress { track: 1, percent: 50, .. }) {
            assert!(!s2.set(RipMode::Secure));
        }
        None
    })
    .await;
    assert_eq!(end, Stage::Done);
    assert_eq!(switch.mode(), RipMode::Fast);

    // Cancelled during the table: nothing is left.
    let sb = Sandbox::new("switch-cancel");
    let switch = ModeSwitch::new(RipMode::Secure);
    let (_, end) = run_job(sb.cfg(toc(), Lookup::Offline), either(&switch), |u| {
        matches!(u, Update::Activity(a) if a == "Reading table").then_some(Command::Cancel)
    })
    .await;
    assert_eq!(end, Stage::Cancelled);
    assert!(!sb.root.join("work").exists());
}

/// A real Fast rip of the disc in the drive into a temporary folder, timed. Needs a CD and the
/// network (MusicBrainz, AccurateRip):
/// `LISTEN_RIP_DEVICE=/dev/sr0 cargo test -p listen-rip --test pipeline live_fast_rip -- --ignored --nocapture`
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: needs an audio CD in the drive"]
async fn live_fast_rip() {
    let device = PathBuf::from(std::env::var("LISTEN_RIP_DEVICE").unwrap_or_else(|_| "/dev/sr0".into()));
    let toc = listen_rip::drive::read_toc(&device).expect("an audio CD in the drive");
    let drive = listen_rip::drive::info(&device);
    let (offset, source) = whipper::resolve_offset(None, &drive).expect("the drive's read offset");
    let sb = Sandbox::new("live-fast");
    let mut cfg = sb.cfg(toc, Lookup::Online);
    cfg.drive = drive;
    cfg.offset = offset;
    cfg.eject = false;
    let http = listen_rip::musicbrainz::http_client();
    let started = std::time::Instant::now();
    let mut ripped_after = None;
    let (updates, end) = run_job(cfg, Ripper::Fast(FastRipper::drive(http)), |u| {
        if matches!(u, Update::DriveFree) {
            ripped_after = Some(started.elapsed());
        }
        if let Update::Activity(a) = u {
            eprintln!("{:>6.1} s  {a}", started.elapsed().as_secs_f32());
        }
        None
    })
    .await;
    let total = started.elapsed();
    assert_eq!(end, Stage::Done, "{updates:?}");
    let report = report_of(&updates);
    eprintln!("offset {offset:+} ({})", source.label());
    eprintln!(
        "disc read in {:.1} s, done (tagged and filed) after {:.1} s",
        ripped_after.unwrap_or_default().as_secs_f32(),
        total.as_secs_f32()
    );
    eprintln!("{}", report.accuraterip_line());
    eprintln!("{} – {} ({:?}); warnings: {:?}", report.artist, report.album, report.source, report.warnings);
}
