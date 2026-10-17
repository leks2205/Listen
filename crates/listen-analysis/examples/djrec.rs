//! Checks for the DJ end-to-end test (`tools/dj-e2e.sh`).
//!
//! ```sh
//! djrec locks <listen.log>        # phase errors once the lock has settled
//! djrec check <rec.wav> <recording start, unix s> <listen.log> [actions.txt]
//! ```
//!
//! `check` scans the sink recording (16-bit WAV from `pw-record`) around every blend
//! in the log (`mix blend: starts in … s, lasts … s`), against the 10 s before it:
//! - **gaps**: 50 ms windows more than 30 dB quieter;
//! - **bumps**: the blend more than 3 dB louder (RMS over a beat);
//! - **flams**: hi-hat onsets doubled 8–100 ms apart (misaligned beats).
//!
//! `actions.txt` (`<unix s> <MPRIS method>` per line) adds gap/bump checks after each
//! press. Log times map to the recording via its start time (~50 ms accuracy).

use std::path::Path;

const RATE: f64 = 48000.0;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("locks") if args.len() == 2 => locks(&args[1]),
        Some("check") if (4..=5).contains(&args.len()) => {
            check(Path::new(&args[1]), args[2].parse().expect("start"), &args[3], args.get(4).map(Path::new))
        }
        _ => {
            eprintln!(
                "usage: djrec locks <listen.log> | djrec check <rec.wav> <start unix s> <listen.log> [actions]"
            );
            std::process::exit(2);
        }
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

/// `mix phase error … ms (locked)` lines of the log: all mixes together.
fn locks(log: &str) {
    let text = std::fs::read_to_string(log).expect("read log");
    let mut v: Vec<f64> = text
        .lines()
        .filter(|l| l.contains("(locked)"))
        .filter_map(|l| number_after(l, "mix phase error "))
        .map(f64::abs)
        .collect();
    v.sort_by(f64::total_cmp);
    println!(
        "phase |error| after lock-in, {} readings: median {:.2} ms, p95 {:.2} ms, max {:.2} ms",
        v.len(),
        percentile(&v, 0.5),
        percentile(&v, 0.95),
        v.last().copied().unwrap_or(f64::NAN)
    );
}

/// Mono samples of a 16-bit stereo WAV (the data chunk is read to the end:
/// a killed `pw-record` leaves the sizes unset).
fn read_wav(path: &Path) -> Vec<f32> {
    let bytes = std::fs::read(path).expect("read wav");
    let mut i = 12;
    while i + 8 <= bytes.len() {
        let id = &bytes[i..i + 4];
        let len = u32::from_le_bytes(bytes[i + 4..i + 8].try_into().unwrap()) as usize;
        if id == b"data" {
            i += 8;
            break;
        }
        i += 8 + len;
    }
    bytes[i.min(bytes.len())..]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| {
            let l = i16::from_le_bytes([c[0], c[1]]) as f32;
            let r = i16::from_le_bytes([c[2], c[3]]) as f32;
            (l + r) / 65536.0
        })
        .collect()
}

/// Seconds since the epoch of a log line's `2024-01-02T03:04:05.123456Z`.
fn log_time(line: &str) -> Option<f64> {
    let ts = line.split_whitespace().next()?;
    let (date, time) = ts.trim_end_matches('Z').split_once('T')?;
    let mut d = date.split('-').map(|x| x.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let mut t = time.split(':');
    let (h, min, s) =
        (t.next()?.parse::<f64>().ok()?, t.next()?.parse::<f64>().ok()?, t.next()?.parse::<f64>().ok()?);
    // Days from the civil date (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days as f64 * 86400.0 + h * 3600.0 + min * 60.0 + s)
}

fn number_after(line: &str, key: &str) -> Option<f64> {
    let rest = &line[line.find(key)? + key.len()..];
    let end =
        rest.find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+'))).unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn rms_db(x: &[f32]) -> f64 {
    let e: f64 = x.iter().map(|&v| f64::from(v) * f64::from(v)).sum::<f64>() / x.len().max(1) as f64;
    10.0 * (e + 1e-12).log10()
}

/// Hi-hat onsets (s): energy of the twice-differentiated signal in 2 ms
/// frames, rising 6 dB over the previous 30 ms and above a floor.
fn hat_onsets(x: &[f32], floor_db: f64) -> Vec<f64> {
    let hp: Vec<f32> = x.windows(3).map(|w| w[2] - 2.0 * w[1] + w[0]).collect();
    let frame = 96;
    let e: Vec<f64> = hp.chunks(frame).map(rms_db).collect();
    let mut out: Vec<f64> = Vec::new();
    for i in 15..e.len() {
        let before = e[i - 15..i - 1].iter().copied().fold(f64::MIN, f64::max);
        if e[i] > floor_db && e[i] > before + 6.0 && e[i] >= e[i - 1] {
            let t = (i * frame) as f64 / RATE;
            if out.last().is_none_or(|&p| t - p > 0.006) {
                out.push(t);
            }
        }
    }
    out
}

fn flams(onsets: &[f64]) -> usize {
    onsets.windows(2).filter(|w| (0.008..0.100).contains(&(w[1] - w[0]))).count()
}

struct Window {
    what: String,
    from: f64,
    secs: f64,
}

fn check(wav: &Path, start: f64, log: &str, actions: Option<&Path>) {
    let x = read_wav(wav);
    let total = x.len() as f64 / RATE;
    let text = std::fs::read_to_string(log).expect("read log");
    let mut windows = Vec::new();
    let mut what = String::new();
    for line in text.lines() {
        if let Some(i) = line.find("mix “") {
            let rest = &line[i + 4..];
            // "“A” → “B”: Kind …" (only planned mixes, not the debug notes).
            if let Some((pair, kind)) = rest.split_once(": ")
                && kind.starts_with(|c: char| c.is_ascii_uppercase())
            {
                let kind = kind.split([' ', '{']).next().unwrap_or("");
                what = format!("{pair} {kind}");
            }
        }
        if line.contains("mix blend: starts in")
            && let (Some(t), Some(dt), Some(secs)) =
                (log_time(line), number_after(line, "starts in "), number_after(line, "lasts "))
        {
            windows.push(Window { what: what.clone(), from: t - start + dt, secs });
        }
    }
    println!("recording {total:.0} s, {} blends in the log", windows.len());
    let at = |t: f64| ((t.max(0.0) * RATE) as usize).min(x.len());
    let (mut bad_gap, mut bad_bump, mut bad_flam) = (0, 0, 0);
    for w in &windows {
        let (a, b) = (w.from, w.from + w.secs);
        if b + 0.5 > total || a < 10.0 {
            println!("  {}: outside the recording", w.what);
            continue;
        }
        let before = &x[at(a - 10.0)..at(a)];
        let level = rms_db(before);
        let blend = &x[at(a)..at(b)];
        let gap = blend.chunks((0.05 * RATE) as usize).map(rms_db).fold(f64::MAX, f64::min);
        let beat = (0.5 * RATE) as usize;
        let bump = blend.chunks(beat).map(rms_db).fold(f64::MIN, f64::max) - level;
        let floor = level - 30.0;
        let fl_before = flams(&hat_onsets(before, floor)) as f64 / 10.0;
        let fl_blend = flams(&hat_onsets(blend, floor)) as f64 / w.secs.max(0.1);
        let gap_bad = gap < level - 30.0;
        let bump_bad = bump > 3.0;
        // A plain fade isn't beat-matched: its flams are expected.
        let flam_bad = !w.what.contains("Fade") && fl_blend > fl_before + 0.5;
        bad_gap += usize::from(gap_bad);
        bad_bump += usize::from(bump_bad);
        bad_flam += usize::from(flam_bad);
        println!(
            "  {:<44} at {a:>6.1} s for {:>4.1} s: level {level:>5.1} dB, quietest 50 ms {:>+5.1} dB{}, \
             loudest beat {bump:>+4.1} dB{}, flams/s {fl_before:.2} → {fl_blend:.2}{}",
            w.what,
            w.secs,
            gap - level,
            if gap_bad { " GAP" } else { "" },
            if bump_bad { " BUMP" } else { "" },
            if flam_bad { " FLAMS" } else { "" },
        );
    }
    println!("blends with a gap: {bad_gap}, with a loudness bump: {bad_bump}, with flams: {bad_flam}");

    // After each button press: the music goes on, from one deck.
    let Some(text) = actions.and_then(|p| std::fs::read_to_string(p).ok()) else { return };
    let mut paused_at = None;
    for line in text.lines() {
        let mut it = line.split_whitespace();
        let (Some(Ok(t)), Some(what)) = (it.next().map(str::parse::<f64>), it.next()) else { continue };
        let t = t - start;
        if what == "Pause" {
            paused_at = Some(t);
            continue;
        }
        let reference = paused_at.take().unwrap_or(t);
        if reference < 10.0 || t + 8.0 > total {
            continue;
        }
        let level = rms_db(&x[at(reference - 10.0)..at(reference)]);
        let after = &x[at(t + 1.0)..at(t + 8.0)];
        let gap = after.chunks((0.05 * RATE) as usize).map(rms_db).fold(f64::MAX, f64::min) - level;
        let bump = after.chunks((0.5 * RATE) as usize).map(rms_db).fold(f64::MIN, f64::max) - level;
        println!(
            "  after {what:<6} at {t:>6.1} s: quietest 50 ms {gap:>+5.1} dB{}, loudest beat {bump:>+4.1} dB{}",
            if gap < -30.0 { " GAP" } else { "" },
            if bump > 3.0 { " BUMP" } else { "" }
        );
    }
}
