//! DJ-mixing experiments with two mpv decks on one PipeWire graph: plays click tracks on two
//! [`Player`]s, records the sink's monitor with `pw-record` and compares it with what mpv
//! reported. Results: `docs/dj-transitions.md` ("S0b mpv deck prototype").
//!
//! Run against a private PipeWire with a null sink (never the desktop's):
//!
//! ```sh
//! tools/s0b-pipewire.sh start            # prints the XDG_RUNTIME_DIR to use
//! XDG_RUNTIME_DIR=/tmp/listen-pw cargo run --release -p listen-player --example twodeck -- all
//! tools/s0b-pipewire.sh stop
//! ```
//!
//! Experiments: `gen`, `pts`, `unpause`, `speed`, `cross`, `afcmd`, `codecs`, `align`, `all`.
//! Options: `--dir <work dir>` (default target/s0b), `--buffer <s>` (mpv
//! `--audio-buffer`, default 0.2), `--quantum <frames>` (default 256),
//! `--sink <node name>` (default s0b-sink), `--correct seek|nudge` (alignment, default nudge).

mod rec;
mod signal;

use listen_player::{MpvOptions, Player, PlayerEvent, PtsSample, Track};
use rec::{Recorder, Recording};
use signal::{BAR, BEAT, RATE, Stereo, fit, onsets, stats};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::sleep;

/// The mix filters of `listen_core::settings::deck_audio_filters`, without a tempo filter.
const MIX: &str = "@mix:lavfi=[bass@lo=g=0:f=120:w=0.7,highpass@hp=f=10,lowpass@lp=f=20000]";

/// The chain the app uses on a deck (no user EQ).
fn production() -> String {
    listen_core::settings::deck_audio_filters(&listen_core::settings::Playback::default())
}

struct Ctx {
    dir: PathBuf,
    sink: String,
    buffer: f64,
    quantum: u32,
    t0: Instant,
    /// Click onsets of the generated source (seconds into the file).
    src: Vec<f64>,
    correct: Correct,
}

/// How the alignment experiment removes the error left after a timed unpause.
#[derive(Clone, Copy, Debug)]
enum Correct {
    /// Exact relative seek on deck B (restarts its output).
    Seek,
    /// Run deck B [`NUDGE`] faster or slower until the error is gone.
    Nudge,
}

/// Speed offset for phase nudges (0.5 %).
const NUDGE: f64 = 0.005;

type Events = UnboundedReceiver<PlayerEvent>;

impl Ctx {
    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn track(&self, name: &str) -> Track {
        Track::url(self.path(name).to_string_lossy())
    }

    async fn deck(&self, name: &str, af: &str) -> (Player, Events) {
        let opts = MpvOptions {
            extra_args: vec!["--ao=pipewire".into(), "--volume=100".into()],
            audio_buffer: Some(self.buffer),
            client_name: name.into(),
            ..Default::default()
        };
        let (p, rx) = Player::spawn(&opts).await.expect("mpv starts");
        p.set_af(af).await.expect("filter chain accepted");
        (p, rx)
    }

    fn record(&self) -> Recorder {
        let r = Recorder::start(&self.sink, self.quantum, self.t0).expect("pw-record starts");
        r.wait_running();
        r
    }

    /// Stops a recording, keeps it as `<name>.wav` and reports the clock mapping's health.
    fn finish(&self, rec: Recorder, name: &str) -> Recording {
        let rec = rec.stop();
        rec.save(&self.path(&format!("{name}.wav")));
        rec.dump_offsets(&self.path(&format!("{name}_offsets.txt")));
        println!(
            "- (recording: {:.1} s, clock mapping spread {:.2} ms, {} jumps = lost cycles)",
            rec.audio.l.len() as f64 / RATE as f64,
            rec.offset_spread_ms(),
            rec.jumps()
        );
        rec
    }

    fn secs(&self, at: Instant) -> f64 {
        at.duration_since(self.t0).as_secs_f64()
    }

    /// The source click nearest to file position `pos`.
    fn click_near(&self, pos: f64) -> f64 {
        let i = self.src.partition_point(|&c| c < pos);
        let cand = [i.saturating_sub(1), i.min(self.src.len() - 1)];
        cand.iter().map(|&i| self.src[i]).min_by(|a, b| (a - pos).abs().total_cmp(&(b - pos).abs())).unwrap()
    }
}

async fn restarted(rx: &mut Events) -> Instant {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        match tokio::time::timeout_at(deadline, rx.recv()).await {
            Ok(Some(PlayerEvent::Restarted)) => return Instant::now(),
            Ok(Some(PlayerEvent::Error(e))) => panic!("mpv error: {e}"),
            Ok(Some(_)) => {}
            _ => panic!("no playback-restart"),
        }
    }
}

/// Drops pending events so a later `restarted` waits for a new one.
fn drain(rx: &mut Events) {
    while rx.try_recv().is_ok() {}
}

async fn pts(p: &Player) -> PtsSample {
    p.audio_pts().await.expect("ipc").expect("audio-pts")
}

async fn sleep_until(at: Instant) {
    tokio::time::sleep_until(tokio::time::Instant::from_std(at)).await;
}

/// Prediction error of `audio-pts`: recorded onset time minus the time the
/// nearest sample says that click is heard.
fn pts_errors(
    ctx: &Ctx,
    rec: &Recording,
    ch: &[f32],
    samples: &[(PtsSample, f64)],
    from: f64,
    to: f64,
) -> Vec<f64> {
    let (f0, f1) = (rec.frame_at(from), rec.frame_at(to));
    let mut errs = Vec::new();
    for onset in onsets(ch, f0.max(0.0) as usize) {
        if onset > f1 {
            break;
        }
        let h = rec.time_of(onset);
        let Some((s, speed)) = samples
            .iter()
            .filter(|(s, _)| ctx.secs(s.at) <= h + 0.5)
            .min_by(|a, b| (ctx.secs(a.0.at) - h).abs().total_cmp(&(ctx.secs(b.0.at) - h).abs()))
        else {
            continue;
        };
        let pos = s.pts + (h - ctx.secs(s.at)) * speed;
        let c = ctx.click_near(pos);
        errs.push(h - (ctx.secs(s.at) + (c - s.pts) / speed));
    }
    errs
}

// ---- gen ------------------------------------------------------------------------

fn generate(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    let secs = 300.0;
    let mut l = Stereo::silent(secs);
    signal::add_clicks(&mut l.l, true);
    signal::write_wav(&dir.join("clicks_L.wav"), &l).unwrap();
    let mut r = Stereo::silent(secs);
    signal::add_clicks(&mut r.r, true);
    signal::write_wav(&dir.join("clicks_R.wav"), &r).unwrap();
    let mut t = Stereo::silent(secs);
    signal::add_clicks(&mut t.l, true);
    signal::add_sine(&mut t.r, 440.0, 0.3);
    signal::write_wav(&dir.join("clicks_tone.wav"), &t).unwrap();
    let mut af = Stereo::silent(90.0);
    signal::add_sine(&mut af.l, 80.0, 0.5);
    signal::add_sine(&mut af.r, 6000.0, 0.3);
    signal::write_wav(&dir.join("af.wav"), &af).unwrap();
    for (ext, codec) in [
        ("flac", &["-c:a", "flac"][..]),
        ("mp3", &["-c:a", "libmp3lame", "-b:a", "192k"]),
        ("m4a", &["-c:a", "aac", "-b:a", "192k"]),
        ("opus", &["-c:a", "libopus", "-b:a", "128k"]),
    ] {
        let out = dir.join(format!("clicks_L.{ext}"));
        let ok = std::process::Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
            .arg(dir.join("clicks_L.wav"))
            .args(codec)
            .arg(&out)
            .status()
            .is_ok_and(|s| s.success());
        assert!(ok, "ffmpeg failed for {ext}");
    }
    println!("generated test files in {}", dir.display());
}

// ---- 1: audio-pts ---------------------------------------------------------------

async fn exp_pts(ctx: &Ctx, secs: f64) {
    println!("\n## 1. audio-pts behaviour, two decks for {secs:.0} s");
    let rec = ctx.record();
    let chain = production();
    let (a, mut ra) = ctx.deck("Deck A", &chain).await;
    let (b, mut rb) = ctx.deck("Deck B", &chain).await;
    a.load(&ctx.track("clicks_L.wav"), Some(Duration::ZERO), true).await.unwrap();
    b.load(&ctx.track("clicks_R.wav"), Some(Duration::ZERO), true).await.unwrap();
    restarted(&mut ra).await;
    restarted(&mut rb).await;
    a.set_paused(false).await.unwrap();
    sleep(Duration::from_millis(1137)).await;
    b.set_paused(false).await.unwrap();
    sleep(Duration::from_millis(500)).await;

    let start = Instant::now();
    let (mut sa, mut sb, mut tp) = (Vec::new(), Vec::new(), Vec::new());
    while start.elapsed().as_secs_f64() < secs {
        sa.push(pts(&a).await);
        sb.push(pts(&b).await);
        let t = Instant::now();
        let v = a.mpv().get_property("time-pos").await.unwrap().as_f64().unwrap();
        tp.push((t + t.elapsed() / 2, v));
        sleep(Duration::from_millis(20)).await;
    }
    let end = Instant::now();
    a.set_paused(true).await.unwrap();
    b.set_paused(true).await.unwrap();
    sleep(Duration::from_millis(300)).await;
    let rec = ctx.finish(rec, "rec_pts");

    let rtt: Vec<f64> = sa.iter().chain(&sb).map(|s| s.rtt.as_secs_f64()).collect();
    println!("- IPC round trip of get_property audio-pts: {}", stats(&rtt));
    let x: Vec<f64> = sa.iter().map(|s| ctx.secs(s.at)).collect();
    let y: Vec<f64> = sa.iter().map(|s| s.pts).collect();
    let (_, slope, res) = fit(&x, &y);
    println!(
        "- deck A audio-pts vs host clock: slope {slope:.6}, reading jitter (residuals) {}",
        stats(&res)
    );
    let steps: Vec<f64> = y.windows(2).map(|w| w[1] - w[0]).collect();
    let distinct = {
        let mut v = y.clone();
        v.dedup();
        v.len()
    };
    println!(
        "- deck A audio-pts: {} readings, {distinct} distinct values; step between readings {}",
        y.len(),
        stats(&steps)
    );
    let lead: Vec<f64> = tp.iter().zip(&sa).map(|((t, v), s)| v - s.pts_at(*t, 1.0)).collect();
    println!("- time-pos minus audio-pts (decoder lead = buffered audio): {}", stats(&lead));

    let wa: Vec<(PtsSample, f64)> = sa.iter().map(|s| (*s, 1.0)).collect();
    let wb: Vec<(PtsSample, f64)> = sb.iter().map(|s| (*s, 1.0)).collect();
    let (from, to) = (ctx.secs(start), ctx.secs(end));
    let ea = pts_errors(ctx, &rec, &rec.audio.l, &wa, from, to);
    let eb = pts_errors(ctx, &rec, &rec.audio.r, &wb, from, to);
    println!("- recorded click minus audio-pts prediction, deck A: {}", stats(&ea));
    println!("- recorded click minus audio-pts prediction, deck B: {}", stats(&eb));

    // Deck-to-deck offset in the recording vs what the two audio-pts say.
    let (f0, f1) = (rec.frame_at(from) as usize, rec.frame_at(to));
    let lo = onsets(&rec.audio.l, f0);
    let ro = onsets(&rec.audio.r, f0);
    let (mut t, mut d, mut err) = (Vec::new(), Vec::new(), Vec::new());
    for &r in ro.iter().filter(|&&r| r < f1) {
        // B started after A, so the matching L click is the latest one before this R click.
        let Some(&l) = lo.iter().rev().find(|&&l| l <= r) else { continue };
        let h = rec.time_of(r);
        let dr = (r - l) / RATE as f64;
        // audio-pts view at that moment: B is behind A by pa - pb.
        let s = sa
            .iter()
            .zip(&sb)
            .min_by(|x, y| (ctx.secs(x.0.at) - h).abs().total_cmp(&(ctx.secs(y.0.at) - h).abs()));
        if let Some((pa, pb)) = s {
            let behind = pa.pts - pb.pts_at(pa.at, 1.0);
            let wrapped = behind.rem_euclid(BEAT);
            let mut e = dr - wrapped;
            if e > BEAT / 2.0 {
                e -= BEAT;
            } else if e < -BEAT / 2.0 {
                e += BEAT;
            }
            err.push(e);
        }
        t.push(h);
        d.push(dr);
    }
    let dump: String = t.iter().zip(&d).map(|(t, d)| format!("{t:.3} {:.3}\n", d * 1000.0)).collect();
    let _ = std::fs::write(ctx.path("deck_offset_pts.txt"), dump);
    let (_, drift, res) = fit(&t, &d);
    println!(
        "- recorded B-after-A offset: mean {:.3} ms, drift {:.4} ms/min, residual {}",
        d.iter().sum::<f64>() / d.len() as f64 * 1000.0,
        drift * 60_000.0,
        stats(&res)
    );
    println!("- recorded deck offset minus the offset the two audio-pts imply: {}", stats(&err));
}

// ---- 2: unpause latency -----------------------------------------------------------

async fn exp_unpause(ctx: &Ctx, trials: usize) {
    println!("\n## 2. unpause-to-sound latency ({trials} trials, loaded paused 0.1 s before a click)");
    let rec = ctx.record();
    let (b, mut rb) = ctx.deck("Deck B", &production()).await;
    let mut runs = Vec::new();
    let mut ready = Vec::new();
    for i in 0..trials {
        let click = ctx.click_near(10.0 + 2.0 * i as f64);
        drain(&mut rb);
        let t_load = Instant::now();
        b.load(&ctx.track("clicks_R.wav"), Some(Duration::from_secs_f64(click - 0.1)), true).await.unwrap();
        ready.push(restarted(&mut rb).await.duration_since(t_load).as_secs_f64());
        let cued = pts(&b).await;
        sleep(Duration::from_millis(400)).await;
        let send = Instant::now();
        b.set_paused(false).await.unwrap();
        let ack = Instant::now();
        sleep(Duration::from_millis(250)).await;
        let s = pts(&b).await;
        sleep(Duration::from_millis(350)).await;
        b.set_paused(true).await.unwrap();
        sleep(Duration::from_millis(200)).await;
        runs.push((click, cued, send, ack, s));
    }
    let rec = ctx.finish(rec, "rec_unpause");
    let (mut lat, mut lat_bed, mut ack, mut cue, mut belief, mut pts_err) =
        (vec![], vec![], vec![], vec![], vec![], vec![]);
    for (click, cued, send, ackt, s) in &runs {
        let f = rec.frame_at(ctx.secs(*send)) as usize;
        let Some(&on) = onsets(&rec.audio.r, f).first() else { continue };
        let h = rec.time_of(on);
        lat.push(h - ctx.secs(*send) - 0.1);
        if let Some(bed) = signal::first_sound(&rec.audio.r, f, 0.004) {
            lat_bed.push(rec.time_of(bed as f64) - ctx.secs(*send));
        }
        ack.push(ackt.duration_since(*send).as_secs_f64());
        cue.push(cued.pts - (click - 0.1));
        // When mpv believes the first sample was heard, from audio-pts after starting.
        belief.push(ctx.secs(s.at) - (s.pts - (click - 0.1)) - ctx.secs(*send));
        pts_err.push(h - (ctx.secs(s.at) + (click - s.pts)));
    }
    let dump: String = lat
        .iter()
        .zip(&belief)
        .zip(&pts_err)
        .map(|((l, b), e)| format!("{:.3} {:.3} {:.3}\n", l * 1e3, b * 1e3, e * 1e3))
        .collect();
    let _ = std::fs::write(ctx.path("unpause_trials.txt"), dump);
    println!("- load (paused, start=) until playback-restart: {}", stats(&ready));
    println!("- audio-pts right after the paused load minus the requested start: {}", stats(&cue));
    println!("- set pause=no IPC reply time: {}", stats(&ack));
    println!("- unpause call to first click (minus the 0.1 s lead-in): {}", stats(&lat));
    println!("- unpause call to first bed sample (rough, 150 Hz bed): {}", stats(&lat_bed));
    println!("- unpause latency as audio-pts tells it afterwards: {}", stats(&belief));
    println!("- recorded click minus audio-pts prediction after unpausing: {}", stats(&pts_err));
}

// ---- 3: speed -------------------------------------------------------------------

/// Tempo filters under test (`None` = no tempo filter: mpv adds its own).
const TEMPO_FILTERS: [(&str, Option<&str>); 6] = [
    ("scaletempo2", Some("scaletempo2")),
    ("scaletempo2 search-interval=15", Some("scaletempo2=search-interval=15")),
    ("scaletempo", Some("scaletempo")),
    ("rubberband finer", Some("rubberband=engine=finer")),
    ("rubberband faster", Some("rubberband=engine=faster")),
    ("mpv auto", None),
];

fn tempo_chain(filter: Option<&str>) -> String {
    match filter {
        Some(f) => format!("{MIX},@ts:{f}"),
        None => MIX.to_string(),
    }
}

/// Frames in `range` at least 40 ms away from any click onset.
fn away_from_clicks(onsets: &[f64], range: std::ops::Range<usize>) -> impl Iterator<Item = usize> + '_ {
    let guard = RATE as f64 * 0.04;
    range.filter(move |&i| {
        let i = i as f64;
        let k = onsets.partition_point(|&o| o < i);
        let near = |j: usize| onsets.get(j).is_some_and(|&o| (o - i).abs() < guard);
        !near(k) && !(k > 0 && near(k - 1))
    })
}

/// Tone health in `range` away from clicks: (ms below half level, largest sample
/// step relative to a clean 0.3 × 440 Hz sine).
fn tone_glitches(rec: &Recording, l_onsets: &[f64], range: std::ops::Range<usize>) -> (f64, f64) {
    let r = &rec.audio.r;
    let clean_step = 0.3 * (440.0 * std::f64::consts::TAU / RATE as f64);
    let frames: Vec<usize> = away_from_clicks(l_onsets, range).collect();
    let step = frames.iter().filter(|&&i| i > 0).map(|&i| (r[i] - r[i - 1]).abs() as f64).fold(0.0, f64::max);
    // 2 ms windows (about one 440 Hz period) whose peak is under half the tone's.
    let win = RATE as usize / 500;
    let low = frames
        .chunks(win)
        .filter(|c| c.len() == win && c[win - 1] - c[0] == win - 1)
        .filter(|c| c.iter().map(|&i| r[i].abs()).fold(0.0f32, f32::max) < 0.15)
        .count();
    (low as f64 * 2.0, step / clean_step)
}

async fn exp_speed(ctx: &Ctx) {
    println!("\n## 3. speed: click timing and shape per tempo filter (each 4.5 s from 20 s)");
    let speeds = [1.0, 0.94, 0.97, 1.03, 1.06];
    let rec = ctx.record();
    let mut runs = Vec::new();
    for (name, filter) in TEMPO_FILTERS {
        let (p, mut rx) = ctx.deck("Deck A", &tempo_chain(filter)).await;
        for &speed in &speeds {
            drain(&mut rx);
            p.load(&ctx.track("clicks_tone.wav"), Some(Duration::from_secs(20)), true).await.unwrap();
            restarted(&mut rx).await;
            p.set_speed(speed).await.unwrap();
            let start = Instant::now();
            p.set_paused(false).await.unwrap();
            let mut samples = Vec::new();
            while start.elapsed() < Duration::from_millis(4500) {
                sleep(Duration::from_millis(100)).await;
                samples.push((pts(&p).await, speed));
            }
            let end = Instant::now();
            p.set_paused(true).await.unwrap();
            sleep(Duration::from_millis(150)).await;
            runs.push((name, speed, start, end, samples));
        }
        p.mpv().command(serde_json::json!(["quit"])).await.ok();
    }
    let rec = ctx.finish(rec, "rec_speed");
    println!(
        "| filter | speed | beat interval error sd (ms) | tempo error | audio-pts error median / sd (ms) | click peak | click spread (ms) | pre-echo | tone dropouts (ms) |"
    );
    println!("|---|---|---|---|---|---|---|---|---|");
    for (name, speed, start, end, samples) in &runs {
        let (f0, f1) = (rec.frame_at(ctx.secs(*start) + 0.3), rec.frame_at(ctx.secs(*end)));
        let on: Vec<f64> = onsets(&rec.audio.l, f0 as usize).into_iter().filter(|&o| o < f1).collect();
        let iv: Vec<f64> = on.windows(2).map(|w| (w[1] - w[0]) / RATE as f64 - BEAT / speed).collect();
        let ivs = stats(&iv);
        let tempo = -ivs.mean / (BEAT / speed) * 100.0;
        let e = stats(&pts_errors(ctx, &rec, &rec.audio.l, samples, ctx.secs(*start) + 0.3, ctx.secs(*end)));
        let (peak, spread, pre) = click_shape(&rec.audio.l, &on);
        let (low, _) = tone_glitches(&rec, &on, f0 as usize..f1 as usize);
        println!(
            "| {name} | {speed:.2} | {:.2} | {tempo:+.3} % | {:.1} / {:.1} | {peak:.2} | {spread:.1} | {pre:.3} | {low:.0} |",
            ivs.sd * 1000.0,
            e.median * 1000.0,
            e.sd * 1000.0,
        );
    }
}

/// Mean click peak, mean spread (first to last sample above 10 % of the peak, ms)
/// and pre-echo (energy in the 10 ms before the onset relative to the 10 ms after).
fn click_shape(ch: &[f32], onsets: &[f64]) -> (f64, f64, f64) {
    let (mut peak, mut spread, mut pre) = (0.0, 0.0, 0.0);
    let n = onsets.len().max(1) as f64;
    let w = RATE as usize / 100;
    for &o in onsets {
        let o = o as usize;
        let lo = o.saturating_sub(w);
        let hi = (o + 3 * w).min(ch.len());
        let p = ch[lo..hi].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let above: Vec<usize> = (lo..hi).filter(|&i| ch[i].abs() > 0.1 * p).collect();
        peak += p as f64;
        spread += (above.last().unwrap_or(&o) - above.first().unwrap_or(&o)) as f64 / RATE as f64 * 1000.0;
        let e = |r: std::ops::Range<usize>| ch[r].iter().map(|s| (*s as f64).powi(2)).sum::<f64>();
        pre += e(lo..o.saturating_sub(24)) / e(o..(o + w).min(ch.len())).max(1e-12);
    }
    (peak / n, spread / n, pre / n)
}

// ---- 3b: crossing 1.0 --------------------------------------------------------------

async fn exp_cross(ctx: &Ctx) {
    let schedule = [1.0, 0.97, 1.0, 1.03, 1.0, 0.995, 1.0, 1.005, 1.0, 0.98, 0.99, 1.0];
    println!("\n## 3b. speed changes while playing (every 0.6 s: {schedule:?})");
    let rec = ctx.record();
    let mut runs = Vec::new();
    for (name, filter) in TEMPO_FILTERS {
        let (p, mut rx) = ctx.deck("Deck A", &tempo_chain(filter)).await;
        p.load(&ctx.track("clicks_tone.wav"), Some(Duration::from_secs(30)), true).await.unwrap();
        restarted(&mut rx).await;
        let start = Instant::now();
        p.set_paused(false).await.unwrap();
        let mut samples = Vec::new();
        let mut changes = Vec::new();
        for &speed in &schedule {
            let at = Instant::now();
            p.set_speed(speed).await.unwrap();
            changes.push(at);
            while at.elapsed() < Duration::from_millis(600) {
                sleep(Duration::from_millis(50)).await;
                samples.push((pts(&p).await, speed));
            }
        }
        let end = Instant::now();
        p.set_paused(true).await.unwrap();
        sleep(Duration::from_millis(200)).await;
        p.mpv().command(serde_json::json!(["quit"])).await.ok();
        runs.push((name, start, end, samples, changes));
    }
    let rec = ctx.finish(rec, "rec_cross");
    println!(
        "| filter | changes with a dropout | dropout total (ms) | largest step vs clean sine | audio-pts error median / max (ms) |"
    );
    println!("|---|---|---|---|---|");
    for (name, start, end, samples, changes) in &runs {
        let (f0, f1) = (rec.frame_at(ctx.secs(*start) + 0.3) as usize, rec.frame_at(ctx.secs(*end)) as usize);
        let on: Vec<f64> = onsets(&rec.audio.l, f0).into_iter().filter(|&o| o < f1 as f64).collect();
        let (mut hit, mut total, mut worst) = (0, 0.0, 0.0f64);
        for c in changes.iter().skip(1) {
            let g0 = rec.frame_at(ctx.secs(*c)) as usize;
            let (low, step) = tone_glitches(&rec, &on, g0..(g0 + RATE as usize * 3 / 10).min(f1));
            hit += usize::from(low > 0.0);
            total += low;
            worst = worst.max(step);
        }
        let e = stats(&pts_errors(ctx, &rec, &rec.audio.l, samples, ctx.secs(*start) + 0.3, ctx.secs(*end)));
        println!(
            "| {name} | {hit}/{} | {total:.0} | {worst:.2}x | {:.1} / {:.1} |",
            changes.len() - 1,
            e.median * 1000.0,
            e.min.abs().max(e.max.abs()) * 1000.0
        );
    }
}

// ---- 4: af-command ------------------------------------------------------------------

async fn exp_afcmd(ctx: &Ctx, trials: usize) {
    println!("\n## 4. af-command latency and clicks");
    let rec = ctx.record();
    let (p, mut rx) = ctx.deck("Deck A", &production()).await;
    p.load(&ctx.track("af.wav"), None, false).await.unwrap();
    restarted(&mut rx).await;
    sleep(Duration::from_millis(500)).await;
    // Latency: lowpass from 20 kHz to 500 Hz on the 6 kHz tone (right channel).
    let mut sends = Vec::new();
    for _ in 0..trials {
        sleep(Duration::from_millis(800)).await;
        let t = Instant::now();
        p.af_command("mix", "f", "500", "lowpass@lp").await.unwrap();
        sends.push((t, Instant::now()));
        sleep(Duration::from_millis(800)).await;
        p.af_command("mix", "f", "20000", "lowpass@lp").await.unwrap();
    }
    // Clicks on the 80 Hz tone (left). Each case: setup commands, then a ramp of
    // (target, command, values) sent 25 ms apart, then a reset.
    type Case<'a> = (&'a str, Vec<(&'a str, &'a str, String)>, Vec<(&'a str, &'a str, String)>);
    let steps = |n: usize, from: f64, to: f64| (1..=n).map(move |i| from + (to - from) * i as f64 / n as f64);
    let cases: Vec<Case> = vec![
        ("bass g 0 → -24 dB at once", vec![], vec![("bass@lo", "g", "-24".into())]),
        (
            "bass g 0 → -24 dB in 12 steps (300 ms)",
            vec![],
            steps(12, 0.0, -24.0).map(|g| ("bass@lo", "g", format!("{g}"))).collect(),
        ),
        (
            "highpass 200 Hz, mix 0 → 1 at once",
            vec![("highpass@hp", "mix", "0".into()), ("highpass@hp", "f", "200".into())],
            vec![("highpass@hp", "mix", "1".into())],
        ),
        (
            "highpass 200 Hz, mix 0 → 1 in 8 steps (200 ms)",
            vec![("highpass@hp", "mix", "0".into()), ("highpass@hp", "f", "200".into())],
            steps(8, 0.0, 1.0).map(|m| ("highpass@hp", "mix", format!("{m}"))).collect(),
        ),
        (
            "highpass 10 Hz, mix 0 → 1 at once",
            vec![("highpass@hp", "mix", "0".into()), ("highpass@hp", "f", "10".into())],
            vec![("highpass@hp", "mix", "1".into())],
        ),
        (
            "highpass mix 1, f 10 → 200 Hz in 16 steps (400 ms)",
            vec![("highpass@hp", "f", "10".into()), ("highpass@hp", "mix", "1".into())],
            steps(16, 10f64.ln(), 200f64.ln())
                .map(|f| ("highpass@hp", "f", format!("{:.1}", f.exp())))
                .collect(),
        ),
    ];
    let mut events = Vec::new();
    for (label, setup, ramp) in &cases {
        for (target, cmd, arg) in setup {
            p.af_command("mix", cmd, arg, target).await.unwrap();
        }
        sleep(Duration::from_millis(800)).await;
        let t = Instant::now();
        for (i, (target, cmd, arg)) in ramp.iter().enumerate() {
            if i > 0 {
                sleep(Duration::from_millis(25)).await;
            }
            p.af_command("mix", cmd, arg, target).await.unwrap();
        }
        events.push((*label, t, ramp.len()));
        sleep(Duration::from_millis(800)).await;
        p.af_command("mix", "g", "0", "bass@lo").await.unwrap();
        p.af_command("mix", "f", "10", "highpass@hp").await.unwrap();
        p.af_command("mix", "mix", "1", "highpass@hp").await.unwrap();
    }
    sleep(Duration::from_millis(800)).await;
    let rec = ctx.finish(rec, "rec_afcmd");

    let win = RATE as usize / 1000;
    let env = signal::envelope(&rec.audio.r, win);
    let mut lat = Vec::new();
    let mut rtt = Vec::new();
    for (t, done) in &sends {
        rtt.push(done.duration_since(*t).as_secs_f64());
        let f = rec.frame_at(ctx.secs(*t)) as usize / win;
        let base = env[f.saturating_sub(100)..f.saturating_sub(5)].iter().copied().fold(0.0f32, f32::max);
        if let Some(i) = env[f.saturating_sub(5)..].iter().position(|&e| e < 0.5 * base) {
            lat.push(rec.time_of(((f.saturating_sub(5) + i) * win) as f64) - ctx.secs(*t));
        }
    }
    println!("- af-command reply time: {}", stats(&rtt));
    println!("- af-command (lowpass 20 kHz → 500 Hz) to audible change, 1 ms resolution: {}", stats(&lat));
    let l = &rec.audio.l;
    let d2 = |i: usize| (l[i] - 2.0 * l[i - 1] + l[i - 2]).abs() as f64;
    let clean = 0.5 * (80.0 * std::f64::consts::TAU / RATE as f64).powi(2);
    for (label, t, n) in events {
        let f = rec.frame_at(ctx.secs(t)) as usize;
        // The change is heard ~0.3 s after sending; the reset comes 0.8 s after the ramp.
        let end = f + ((0.8 + 0.025 * n as f64) * RATE as f64) as usize;
        let range = f + RATE as usize / 10..end.min(l.len());
        let worst = range.clone().filter(|&i| i >= 2).map(d2).fold(0.0, f64::max);
        let peak = range.map(|i| l[i].abs()).fold(0.0f32, f32::max);
        println!(
            "- {label}: largest 2nd difference {:.1}x a clean 80 Hz sine's ({:+.1} dB), peak {peak:.3} (tone 0.5)",
            worst / clean,
            20.0 * (worst / clean).log10()
        );
    }
}

// ---- 5: codecs ---------------------------------------------------------------------

fn decode_pcm(input: &Path, out: &Path, rate: u32) -> f64 {
    let t = Instant::now();
    let ok = std::process::Command::new("mpv")
        .args(["--no-config", "--really-quiet", "--no-video", "--ao=pcm", "--ao-pcm-waveheader=yes"])
        .args(["--audio-format=float", "--audio-channels=stereo", "--length=60"])
        .arg(format!("--audio-samplerate={rate}"))
        .arg(format!("--ao-pcm-file={}", out.display()))
        .arg(input)
        .status()
        .is_ok_and(|s| s.success());
    assert!(ok, "mpv --ao=pcm failed for {}", input.display());
    t.elapsed().as_secs_f64()
}

async fn exp_codecs(ctx: &Ctx) {
    println!("\n## 5. decode (mpv --ao=pcm) vs playback timing per codec");
    let codecs = ["wav", "flac", "mp3", "m4a", "opus"];
    let mut pcm_offset = Vec::new();
    for ext in codecs {
        let input = ctx.path(&format!("clicks_L.{ext}"));
        let out = ctx.path(&format!("pcm_{ext}.wav"));
        let took = decode_pcm(&input, &out, RATE);
        let w = signal::read_wav(&out).unwrap();
        let on: Vec<f64> = onsets(&w.l, 0).iter().map(|o| o / RATE as f64).collect();
        let off: Vec<f64> = on.iter().map(|&o| o - ctx.click_near(o)).collect();
        let s = stats(&off);
        // The analyser decodes at 22050 Hz: same timeline?
        let out22 = ctx.path(&format!("pcm22_{ext}.wav"));
        decode_pcm(&input, &out22, 22_050);
        let w22 = signal::read_wav(&out22).unwrap();
        let on22: Vec<f64> = signal::onsets_at(&w22.l, 0, signal::CLICK_THRESHOLD, 0.15)
            .iter()
            .map(|o| o / 22_050.0)
            .collect();
        let off22: Vec<f64> = on22.iter().map(|&o| o - ctx.click_near(o)).collect();
        println!(
            "- {ext}: decoded 60 s in {took:.2} s; decoded onset minus source onset {s}; at 22050 Hz: median {:.3} ms",
            stats(&off22).median * 1000.0
        );
        pcm_offset.push(s.median);
    }
    let rec = ctx.record();
    let (p, mut rx) = ctx.deck("Deck A", &production()).await;
    let mut runs = Vec::new();
    for ext in codecs {
        drain(&mut rx);
        p.load(&ctx.track(&format!("clicks_L.{ext}")), Some(Duration::from_secs(10)), true).await.unwrap();
        restarted(&mut rx).await;
        let cued = pts(&p).await.pts;
        let start = Instant::now();
        p.set_paused(false).await.unwrap();
        let mut samples = Vec::new();
        while start.elapsed() < Duration::from_millis(3500) {
            sleep(Duration::from_millis(200)).await;
            samples.push((pts(&p).await, 1.0));
        }
        let end = Instant::now();
        p.set_paused(true).await.unwrap();
        sleep(Duration::from_millis(200)).await;
        runs.push((ext, cued, start, end, samples));
    }
    let rec = ctx.finish(rec, "rec_codecs");
    for ((ext, cued, start, end, samples), pcm) in runs.into_iter().zip(pcm_offset) {
        let e = stats(&pts_errors(ctx, &rec, &rec.audio.l, &samples, ctx.secs(start) + 0.3, ctx.secs(end)));
        println!(
            "- {ext}: start=10 cued at audio-pts {cued:.6}; heard click minus audio-pts prediction from the source time {:.2} ms (sd {:.2}); from the decoded time {:.2} ms",
            e.median * 1000.0,
            e.sd * 1000.0,
            (e.median - pcm) * 1000.0
        );
    }
}

// ---- 6: two-deck alignment ----------------------------------------------------------

async fn exp_align(ctx: &Ctx, trials: usize) {
    println!("\n## 6. two-deck beat alignment ({trials} trials, correction by {:?})", ctx.correct);
    let rec = ctx.record();
    let chain = production();
    let (a, mut ra) = ctx.deck("Deck A", &chain).await;
    let (b, mut rb) = ctx.deck("Deck B", &chain).await;
    a.load(&ctx.track("clicks_L.wav"), Some(Duration::ZERO), false).await.unwrap();
    restarted(&mut ra).await;
    sleep(Duration::from_millis(500)).await;

    let mut lu_hist: Vec<f64> = Vec::new();
    let mut d_hist: Vec<f64> = Vec::new();
    let median = |v: &[f64], default: f64| if v.is_empty() { default } else { stats(v).median };
    let mut rows = Vec::new();
    for i in 0..trials {
        let bar_b = 40.0 + 4.0 * i as f64;
        drain(&mut rb);
        b.load(&ctx.track("clicks_R.wav"), Some(Duration::from_secs_f64(bar_b)), true).await.unwrap();
        restarted(&mut rb).await;
        let sa = pts(&a).await;
        let bar_a = ((sa.pts + 0.8) / BAR).ceil() * BAR;
        let target = sa.at + Duration::from_secs_f64(bar_a - sa.pts);
        // No prior: the first trial calibrates (the app would keep this per machine).
        let lu = median(&lu_hist, 0.0);
        sleep_until(target - Duration::from_secs_f64(lu)).await;
        let send = Instant::now();
        b.set_paused(false).await.unwrap();
        sleep(Duration::from_millis(1100)).await;
        let err = |sa: PtsSample, sb: PtsSample| (sb.pts - bar_b) - (sa.pts_at(sb.at, 1.0) - bar_a);
        let (pa, pb) = (pts(&a).await, pts(&b).await);
        let e0 = err(pa, pb);
        // When audio-pts says B's first sample was heard, relative to the call.
        lu_hist.push(pb.at.duration_since(send).as_secs_f64() - (pb.pts - bar_b));
        let mut e = e0;
        let mut seeks = Vec::new();
        for _ in 0..3 {
            if e.abs() < 0.001 {
                break;
            }
            let t = Instant::now();
            match ctx.correct {
                Correct::Seek => {
                    let d_est = median(&d_hist, 0.0);
                    drain(&mut rb);
                    b.seek_relative_exact(-e + d_est).await.unwrap();
                    restarted(&mut rb).await;
                    sleep(Duration::from_millis(300)).await;
                    let (pa, pb) = (pts(&a).await, pts(&b).await);
                    let e_new = err(pa, pb);
                    d_hist.push(d_est - e_new);
                    e = e_new;
                }
                Correct::Nudge => {
                    // B ahead (e > 0): run it 0.5 % slow for as long as it takes.
                    b.set_speed(1.0 - NUDGE * e.signum()).await.unwrap();
                    sleep(Duration::from_secs_f64(e.abs() / NUDGE)).await;
                    b.set_speed(1.0).await.unwrap();
                    // Let the audio buffered at the nudged speed play out.
                    sleep(Duration::from_millis(600)).await;
                    let (pa, pb) = (pts(&a).await, pts(&b).await);
                    e = err(pa, pb);
                }
            }
            seeks.push(t);
        }
        let settled = Instant::now();
        sleep(Duration::from_millis(1600)).await;
        let stop = Instant::now();
        b.set_paused(true).await.unwrap();
        sleep(Duration::from_millis(200)).await;
        rows.push((send, e0, seeks, e, settled, stop));
    }
    let rec = ctx.finish(rec, "rec_align");
    // Recorded L-minus-R onset difference in a window (median): positive = B ahead,
    // the same sign as the audio-pts error.
    let measure = |from: f64, to: f64| -> Option<f64> {
        let (f0, f1) = (rec.frame_at(from) as usize, rec.frame_at(to));
        let lo = onsets(&rec.audio.l, f0.saturating_sub(RATE as usize));
        let d: Vec<f64> = onsets(&rec.audio.r, f0)
            .into_iter()
            .filter(|&r| r < f1)
            .filter_map(|r| {
                let l = lo.iter().min_by(|x, y| (*x - r).abs().total_cmp(&(*y - r).abs()))?;
                Some((l - r) / RATE as f64)
            })
            .collect();
        (!d.is_empty()).then(|| stats(&d).median)
    };
    let (mut e0p, mut e0r, mut efp, mut efr, mut nseeks) = (vec![], vec![], vec![], vec![], vec![]);
    for (send, e0, seeks, ef, settled, stop) in &rows {
        let first_end = seeks.first().copied().unwrap_or(*settled);
        if let Some(m) = measure(ctx.secs(*send) - 0.01, ctx.secs(first_end)) {
            e0r.push(m);
        }
        if let Some(m) = measure(ctx.secs(*settled) + 0.1, ctx.secs(*stop)) {
            efr.push(m);
        }
        e0p.push(*e0);
        efp.push(*ef);
        nseeks.push(seeks.len() as f64);
    }
    println!(
        "- learned unpause latency (audio-pts): {:.2} ms; learned seek shift: {:.2} ms",
        median(&lu_hist, 0.0) * 1000.0,
        median(&d_hist, 0.0) * 1000.0
    );
    println!("- after the timed unpause, by audio-pts: {}", stats(&e0p));
    println!("- after the timed unpause, recorded:     {}", stats(&e0r));
    println!("- after correction, by audio-pts:  {}", stats(&efp));
    println!("- after correction, recorded:      {}", stats(&efr));
    println!("- corrections per trial: mean {:.2}", nseeks.iter().sum::<f64>() / nseeks.len().max(1) as f64);
}

// ---- main ------------------------------------------------------------------------

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let mut args = std::env::args().skip(1);
    let mut what = Vec::new();
    let mut correct = Correct::Nudge;
    let (mut dir, mut buffer, mut quantum, mut sink) =
        (PathBuf::from("target/s0b"), 0.2, 256u32, "s0b-sink".to_string());
    while let Some(a) = args.next() {
        match a.as_str() {
            "--dir" => dir = args.next().expect("--dir <path>").into(),
            "--buffer" => buffer = args.next().and_then(|v| v.parse().ok()).expect("--buffer <secs>"),
            "--quantum" => quantum = args.next().and_then(|v| v.parse().ok()).expect("--quantum <frames>"),
            "--sink" => sink = args.next().expect("--sink <name>"),
            "--correct" => {
                correct = match args.next().as_deref() {
                    Some("seek") => Correct::Seek,
                    Some("nudge") => Correct::Nudge,
                    _ => panic!("--correct seek|nudge"),
                }
            }
            other => what.push(other.to_string()),
        }
    }
    if what.is_empty() {
        what.push("all".into());
    }
    let all = what.iter().any(|w| w == "all");
    let wants = |w: &str| all || what.iter().any(|x| x == w);
    if wants("gen") || !dir.join("clicks_L.wav").exists() {
        generate(&dir);
    }
    let src = onsets(&signal::read_wav(&dir.join("clicks_L.wav")).unwrap().l, 0)
        .into_iter()
        .map(|o| o / RATE as f64)
        .collect();
    let ctx = Ctx { dir, sink, buffer, quantum, t0: Instant::now(), src, correct };
    println!("# S0b run: mpv --audio-buffer={buffer}, graph quantum {quantum} frames @ {RATE} Hz");
    if wants("pts") {
        exp_pts(&ctx, 60.0).await;
    }
    if wants("unpause") {
        exp_unpause(&ctx, 20).await;
    }
    if wants("speed") {
        exp_speed(&ctx).await;
    }
    if wants("cross") {
        exp_cross(&ctx).await;
    }
    if wants("afcmd") {
        exp_afcmd(&ctx, 10).await;
    }
    if wants("codecs") {
        exp_codecs(&ctx).await;
    }
    if wants("align") {
        exp_align(&ctx, 20).await;
    }
}
