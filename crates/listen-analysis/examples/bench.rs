//! Analyser bench: random Navidrome songs through every candidate
//! analyser, compared with the BPM tags and with each other.
//!
//! ```sh
//! cargo run --release -p listen-analysis --example bench -- --n 60
//! ```
//! Server from `LISTEN_ND_URL` / `LISTEN_ND_USER` / `LISTEN_ND_PASS` or `.env.test`. Decoded audio is cached
//! in `target/analysis-bench/`; `--reuse` re-analyses the same songs without downloading.

#[cfg(unix)]
use listen_analysis::beats::{self, BeatEstimate};
#[cfg(unix)]
use listen_analysis::{BeatModel, BeatSource, Cancel, Opts, Pcm, spectral, structure, timbre};
#[cfg(unix)]
use listen_core::analysis::{BeatGrid, SectionKind};
#[cfg(unix)]
use listen_core::model::{ServerId, Song};
#[cfg(unix)]
use listen_source::MusicSource;
#[cfg(unix)]
use listen_subsonic::Subsonic;
#[cfg(unix)]
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::time::{Duration, Instant};

#[cfg(unix)]
mod synth;

/// What stratum-dsp found (for the bench and as a tempo/key cross-check).
#[cfg(unix)]
#[derive(Debug, Clone)]
struct StratumResult {
    bpm: f32,
    confidence: f32,
    estimate: BeatEstimate,
    key: Option<(listen_core::analysis::Camelot, f32)>,
}

/// Runs stratum-dsp on the trimmed mono signal (its own trimming is off so
/// times stay in file time).
#[cfg(unix)]
fn track_stratum(mono: &[f32], sample_rate: u32, audio_start: f32, audio_end: f32) -> Option<StratumResult> {
    let s = ((audio_start * sample_rate as f32) as usize).min(mono.len());
    let e = ((audio_end * sample_rate as f32) as usize).clamp(s, mono.len());
    let config = stratum_dsp::AnalysisConfig {
        enable_silence_trimming: false,
        enable_normalization: false,
        ..Default::default()
    };
    let r = stratum_dsp::analyze_audio(&mono[s..e], sample_rate, config).ok()?;
    let shift = |v: &[f32]| v.iter().map(|t| t + audio_start).collect::<Vec<_>>();
    let key = match r.key {
        stratum_dsp::Key::Major(pc) => listen_core::analysis::Camelot::from_pitch(pc as u8, false),
        stratum_dsp::Key::Minor(pc) => listen_core::analysis::Camelot::from_pitch(pc as u8, true),
    };
    Some(StratumResult {
        bpm: r.bpm,
        confidence: r.bpm_confidence.clamp(0.0, 1.0),
        estimate: BeatEstimate {
            beats: shift(&r.beat_grid.beats),
            downbeats: shift(&r.beat_grid.downbeats),
            confidence: Some(r.bpm_confidence.clamp(0.0, 1.0)),
        },
        key: (r.key_confidence > 0.0).then_some((key, r.key_confidence.clamp(0.0, 1.0))),
    })
}

#[cfg(unix)]
fn env_file_value(key: &str) -> Option<String> {
    let mut dir = Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf();
    loop {
        if let Ok(text) = std::fs::read_to_string(dir.join(".env.test"))
            && let Some(v) = text.lines().find_map(|l| l.strip_prefix(&format!("{key}=")))
        {
            let v = v.trim();
            // Shell-style quoting.
            let v = v.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')).unwrap_or(v);
            let v = v.strip_prefix('"').and_then(|x| x.strip_suffix('"')).unwrap_or(v);
            return Some(v.to_string());
        }
        if !dir.pop() {
            return None;
        }
    }
}

#[cfg(unix)]
fn setting(key: &str) -> String {
    std::env::var(key).ok().or_else(|| env_file_value(key)).unwrap_or_else(|| panic!("{key} not set"))
}

/// Process CPU time (all threads), for a per-core cost estimate.
#[cfg(unix)]
fn cpu_time() -> Duration {
    // SAFETY: getrusage fills the struct.
    let mut u: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut u) };
    let tv = |t: libc::timeval| Duration::new(t.tv_sec as u64, t.tv_usec as u32 * 1000);
    tv(u.ru_utime) + tv(u.ru_stime)
}

/// (wall, cpu) of `f`.
#[cfg(unix)]
fn timed<T>(f: impl FnOnce() -> T) -> (T, Duration, Duration) {
    let (w, c) = (Instant::now(), cpu_time());
    let out = f();
    (out, w.elapsed(), cpu_time().saturating_sub(c))
}

#[cfg(unix)]
fn save_pcm(path: &Path, pcm: &Pcm) {
    let mut bytes = Vec::with_capacity(pcm.left.len() * 8);
    for (l, r) in pcm.left.iter().zip(&pcm.right) {
        bytes.extend_from_slice(&l.to_le_bytes());
        bytes.extend_from_slice(&r.to_le_bytes());
    }
    std::fs::write(path, bytes).unwrap();
}

#[cfg(unix)]
fn load_pcm(path: &Path) -> Option<Pcm> {
    let bytes = std::fs::read(path).ok()?;
    let samples: Vec<f32> = bytes.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect();
    Some(Pcm::from_interleaved(listen_analysis::decode::SAMPLE_RATE, &samples))
}

#[cfg(unix)]
#[derive(Default)]
struct Stage {
    wall: Duration,
    cpu: Duration,
    n: u32,
}

#[cfg(unix)]
impl Stage {
    fn add(&mut self, wall: Duration, cpu: Duration) {
        self.wall += wall;
        self.cpu += cpu;
        self.n += 1;
    }
    fn report(&self, name: &str) {
        let n = self.n.max(1);
        println!("  {name:<22} wall {:>7.0} ms   cpu {:>7.0} ms", ms(self.wall / n), ms(self.cpu / n));
    }
}

#[cfg(unix)]
fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// Same tempo within 2 %; `octave`: also ×2 / ×½.
#[cfg(unix)]
fn agree(a: f32, b: f32, octave: bool) -> bool {
    let r = a / b;
    let near = |x: f32| (r / x - 1.0).abs() <= 0.02;
    near(1.0) || (octave && (near(2.0) || near(0.5)))
}

#[cfg(unix)]
struct Row {
    tag: Option<f32>,
    bt: Option<f32>,
    own: Option<f32>,
    stratum: Option<f32>,
    final_bpm: Option<f32>,
    bt_raw: Option<f32>,
    own_raw: Option<f32>,
    keys: (Option<String>, Option<String>),
}

#[cfg(unix)]
fn grid_of(est: Option<&BeatEstimate>, frames: &spectral::Frames, s: f32, e: f32) -> Option<BeatGrid> {
    est.and_then(|est| beats::build_grid(est, frames, s, e))
}

/// Median-interval BPM of raw beats, before the grid's confidence gate.
#[cfg(unix)]
fn raw_bpm(est: Option<&BeatEstimate>) -> Option<f32> {
    let est = est?;
    let mut d: Vec<f32> = est.beats.windows(2).map(|w| w[1] - w[0]).collect();
    if d.len() < 8 {
        return None;
    }
    d.sort_by(f32::total_cmp);
    Some(60.0 / d[d.len() / 2])
}

#[cfg(unix)]
fn sections_summary(f: &listen_core::analysis::Features) -> String {
    f.sections
        .iter()
        .map(|s| {
            let k = match s.kind {
                SectionKind::Intro => "I",
                SectionKind::Body => "B",
                SectionKind::Breakdown => "Bd",
                SectionKind::Build => "Bu",
                SectionKind::Drop => "D",
                SectionKind::Outro => "O",
            };
            format!("{k}{}", s.bars)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(unix)]
#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let n: usize = arg("--n").and_then(|v| v.parse().ok()).unwrap_or(60);
    let reuse = args.iter().any(|a| a == "--reuse");
    let skip_slow = args.iter().any(|a| a == "--fast");

    let cache = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/analysis-bench");
    std::fs::create_dir_all(&cache).unwrap();
    let models = BeatModel::ensure(&cache.join("models")).await.expect("Beat This! models");
    if let Some(count) = arg("--synthetic").and_then(|v| v.parse().ok()) {
        synthetic(count, &cache, &models).await;
        return;
    }

    let nd = Subsonic::new(
        ServerId("bench".into()),
        &setting("LISTEN_ND_URL"),
        &setting("LISTEN_ND_USER"),
        &setting("LISTEN_ND_PASS"),
    )
    .expect("server url");
    let list_path = cache.join("songs.json");
    let songs: Vec<Song> = if reuse && list_path.exists() {
        serde_json::from_slice(&std::fs::read(&list_path).unwrap()).unwrap()
    } else {
        // A random sample, but with at least half BPM-tagged songs when the
        // library has them (the tags are the only ground truth).
        let mut pool = Vec::new();
        for _ in 0..4 {
            pool.extend(nd.random_songs(500, None).await.expect("random songs"));
        }
        pool.sort_by(|a, b| a.id.cmp(&b.id));
        pool.dedup_by(|a, b| a.id == b.id);
        let tagged_in_pool = pool.iter().filter(|s| s.bpm.is_some_and(|b| b > 0)).count();
        println!("pool: {} distinct random songs, {tagged_in_pool} with a BPM tag", pool.len());
        let (tagged, untagged): (Vec<Song>, Vec<Song>) =
            pool.into_iter().partition(|s| s.bpm.is_some_and(|b| b > 0));
        let mut s: Vec<Song> = tagged.into_iter().take(n.div_ceil(2)).collect();
        let rest = n - s.len();
        s.extend(untagged.into_iter().take(rest));
        // Interleave so a partial run still has both kinds.
        s.sort_by_key(|x| x.id.bytes().fold(0u64, |h, b| h.wrapping_mul(31).wrapping_add(u64::from(b))));
        std::fs::write(&list_path, serde_json::to_vec(&s).unwrap()).unwrap();
        s
    };
    let songs: Vec<Song> = songs.into_iter().take(n).collect();
    println!(
        "{} songs, {} threads, {}; beat model {}",
        songs.len(),
        std::thread::available_parallelism().map_or(0, |n| n.get()),
        listen_analysis::mpv_version(),
        models.beat.file_name().unwrap().to_string_lossy()
    );

    let mut st: std::collections::BTreeMap<&str, Stage> = Default::default();
    let mut rows = Vec::new();
    let mut own_vs_bt_ms: Vec<f32> = Vec::new();
    let mut audio_secs = 0.0;
    for (i, song) in songs.iter().enumerate() {
        let pcm_path: PathBuf = cache.join(format!("{}.f32", song.id.replace('/', "_")));
        let pcm = match load_pcm(&pcm_path) {
            Some(p) => p,
            None => {
                let url = nd.analysis_url(song).expect("subsonic analysis url");
                let (r, w, c) = {
                    let (w, c) = (Instant::now(), cpu_time());
                    let r = listen_analysis::decode(&url, &Cancel::default()).await;
                    (r, w.elapsed(), cpu_time().saturating_sub(c))
                };
                st.entry("decode (mpv, network)").or_default().add(w, c);
                match r {
                    Ok(p) => {
                        save_pcm(&pcm_path, &p);
                        p
                    }
                    Err(e) => {
                        println!("{:>2} {} – decode failed: {e}", i + 1, song.title);
                        continue;
                    }
                }
            }
        };
        audio_secs += pcm.duration();
        let mono = pcm.mono();
        let ((s, e), w, c) = timed(|| structure::trim(&pcm));
        st.entry("trim").or_default().add(w, c);
        let (frames, w, c) = timed(|| spectral::analyze(&pcm));
        st.entry("stft features").or_default().add(w, c);

        let (bt, w, c) = timed(|| models.track(&mono, pcm.sample_rate).ok());
        st.entry("beat-this (small)").or_default().add(w, c);
        let (own, w, c) = timed(|| beats::track_own(&frames, s, e, None));
        st.entry("own DP tracker").or_default().add(w, c);
        let (strat, w, c) = if skip_slow {
            (None, Duration::ZERO, Duration::ZERO)
        } else {
            timed(|| track_stratum(&mono, pcm.sample_rate, s, e))
        };
        if !skip_slow {
            st.entry("stratum-dsp").or_default().add(w, c);
        }
        let (_bliss, w, c) = timed(|| timbre::bliss_vector(&mono));
        st.entry("bliss-audio timbre").or_default().add(w, c);

        let opts = Opts { beats: BeatSource::BeatThis(models.clone()), bpm_hint: None };
        let ((f, tm), w, c) = timed(|| listen_analysis::analyze_timed(&pcm, &opts));
        st.entry("full analyze()").or_default().add(w, c);
        for (name, d) in &tm.stages {
            let key: &'static str = match *name {
                "bars" => "  bars+vocal+novelty",
                "key" => "  key",
                "loudness" => "  loudness",
                _ => continue,
            };
            st.entry(key).or_default().add(*d, Duration::ZERO);
        }

        let g_bt = grid_of(bt.as_ref(), &frames, s, e);
        let cand = bt.as_ref().and_then(|est| beats::grid_candidate(est, &frames, s, e));
        let g_own = grid_of(own.as_ref(), &frames, s, e);
        let tag = song.bpm.filter(|b| *b > 0).map(|b| b as f32);
        let fmt = |b: Option<f32>| b.map_or("   -  ".into(), |b| format!("{b:6.1}"));
        let key_label = f.key.map(|(k, c)| format!("{}({c:.2})", k.label()));
        println!(
            "{:>2} {:<38.38} tag {}  bt {} (c {:.2} s {:.3})  own {}  str {} (c {:.2})  key {:<10} str-key {:<4} {:5.1} LUFS  in {:>2} out {:>2}  {}",
            i + 1,
            format!("{} – {}", song.artist_line(), song.title),
            fmt(tag),
            fmt(g_bt.as_ref().map(|g| g.bpm)),
            cand.as_ref().map_or(0.0, |g| g.confidence),
            cand.as_ref().map_or(0.0, |g| g.stability),
            fmt(g_own.as_ref().map(|g| g.bpm)),
            fmt(strat.as_ref().map(|r| r.bpm)),
            strat.as_ref().map_or(0.0, |r| r.confidence),
            key_label.clone().unwrap_or("-".into()),
            strat.as_ref().and_then(|r| r.key).map_or("-".into(), |(k, _)| k.label()),
            f.lufs,
            f.intro_instrumental_bars,
            f.outro_instrumental_bars,
            sections_summary(&f),
        );
        if let (Some(a), Some(b)) = (&g_bt, &g_own)
            && agree(a.bpm, b.bpm, false)
        {
            let mut d: Vec<f32> = b
                .beats
                .iter()
                .map(|t| {
                    let i = a.beat_at(*t).round();
                    (t - a.time_of(i)) * 1000.0
                })
                .collect();
            d.sort_by(f32::total_cmp);
            own_vs_bt_ms.push(d[d.len() / 2]);
        }
        rows.push(Row {
            tag,
            bt: g_bt.map(|g| g.bpm),
            own: g_own.map(|g| g.bpm),
            stratum: strat.as_ref().map(|r| r.bpm),
            final_bpm: f.grid.as_ref().map(|g| g.bpm),
            bt_raw: raw_bpm(bt.as_ref()),
            own_raw: raw_bpm(own.as_ref()),
            keys: (f.key.map(|(k, _)| k.label()), strat.as_ref().and_then(|r| r.key).map(|(k, _)| k.label())),
        });
    }

    println!("\nMean time per track ({:.0} s audio on average):", audio_secs / rows.len().max(1) as f32);
    for (name, s) in &st {
        s.report(name);
    }
    let tagged: Vec<&Row> = rows.iter().filter(|r| r.tag.is_some()).collect();
    println!("\nBPM vs tags ({} of {} songs tagged):", tagged.len(), rows.len());
    let score = |get: &dyn Fn(&Row) -> Option<f32>, name: &str| {
        let (mut exact, mut oct, mut none) = (0, 0, 0);
        for r in &tagged {
            match get(r) {
                Some(b) => {
                    exact += usize::from(agree(b, r.tag.unwrap(), false));
                    oct += usize::from(agree(b, r.tag.unwrap(), true));
                }
                None => none += 1,
            }
        }
        let pct = |x: usize| 100.0 * x as f32 / tagged.len().max(1) as f32;
        println!(
            "  {name:<26} exact ±2 %: {exact:>3} ({:4.1} %)  octave-tolerant: {oct:>3} ({:4.1} %)  no grid: {none}",
            pct(exact),
            pct(oct)
        );
    };
    score(&|r| r.bt_raw, "beat-this raw");
    score(&|r| r.bt, "beat-this grid");
    score(&|r| r.own_raw, "own raw");
    score(&|r| r.own, "own grid");
    score(&|r| r.stratum, "stratum-dsp");
    score(&|r| r.final_bpm, "chosen (analyze())");

    println!("\nAgreement between analysers (all songs, octave-tolerant / exact):");
    let pair = |a: &dyn Fn(&Row) -> Option<f32>, b: &dyn Fn(&Row) -> Option<f32>, name: &str| {
        let both: Vec<(f32, f32)> = rows.iter().filter_map(|r| Some((a(r)?, b(r)?))).collect();
        let oct = both.iter().filter(|(x, y)| agree(*x, *y, true)).count();
        let exact = both.iter().filter(|(x, y)| agree(*x, *y, false)).count();
        println!("  {name:<26} {oct:>3} / {exact:>3} of {}", both.len());
    };
    pair(&|r| r.bt_raw, &|r| r.stratum, "beat-this vs stratum");
    pair(&|r| r.bt_raw, &|r| r.own_raw, "beat-this vs own");
    pair(&|r| r.own_raw, &|r| r.stratum, "own vs stratum");
    own_vs_bt_ms.sort_by(f32::total_cmp);
    println!(
        "  beat phase own − beat-this (same tempo, {} songs): median {:+.1} ms, range {:+.1}..{:+.1}",
        own_vs_bt_ms.len(),
        own_vs_bt_ms.get(own_vs_bt_ms.len() / 2).copied().unwrap_or(f32::NAN),
        own_vs_bt_ms.first().copied().unwrap_or(f32::NAN),
        own_vs_bt_ms.last().copied().unwrap_or(f32::NAN),
    );
    let keys: Vec<_> = rows.iter().filter_map(|r| Some((r.keys.0.clone()?, r.keys.1.clone()?))).collect();
    println!("  key own vs stratum: same {} of {}", keys.iter().filter(|(a, b)| a == b).count(), keys.len());
}

#[cfg(unix)]
#[derive(Default)]
struct SynthScore {
    n: usize,
    gridded: usize,
    exact: usize,
    octave: usize,
    downbeat: usize,
    abs_err_ms: Vec<f32>,
    signed_err_ms: Vec<f32>,
}

#[cfg(unix)]
impl SynthScore {
    fn add(&mut self, grid: Option<&BeatGrid>, truth: &synth::Truth) {
        self.n += 1;
        let Some(g) = grid else { return };
        self.gridded += 1;
        let (e, o) = (agree(g.bpm, truth.bpm, false), agree(g.bpm, truth.bpm, true));
        self.exact += usize::from(e);
        self.octave += usize::from(o);
        if !e {
            return;
        }
        let period = 60.0 / truth.bpm;
        let total = truth.bars() * 4;
        let mut errs = Vec::new();
        for &b in &g.beats {
            let k = ((b - truth.start) / period).round();
            if k >= 0.0 && (k as usize) < total {
                errs.push((b - truth.beat(k as usize)) * 1000.0);
            }
        }
        if errs.is_empty() {
            return;
        }
        errs.sort_by(f32::total_cmp);
        self.signed_err_ms.push(errs[errs.len() / 2]);
        let mut abs: Vec<f32> = errs.iter().map(|e| e.abs()).collect();
        abs.sort_by(f32::total_cmp);
        self.abs_err_ms.push(abs[abs.len() / 2]);
        if let Some(d) = g.downbeats().next() {
            let k = ((d - truth.start) / period).round();
            self.downbeat += usize::from(k >= 0.0 && (k as usize).is_multiple_of(4));
        }
    }

    fn report(&self, name: &str) {
        let med = |v: &[f32]| {
            let mut v = v.to_vec();
            v.sort_by(f32::total_cmp);
            v.get(v.len() / 2).copied().unwrap_or(f32::NAN)
        };
        let p95 = |v: &[f32]| {
            let mut v: Vec<f32> = v.iter().map(|x| x.abs()).collect();
            v.sort_by(f32::total_cmp);
            v.get((v.len() as f32 * 0.95) as usize).or(v.last()).copied().unwrap_or(f32::NAN)
        };
        println!(
            "  {name:<20} grid {:>3}/{}  bpm exact {:>3} octave {:>3}  downbeat ok {:>3}/{:<3}  beat error median {:5.1} ms (p95 of songs {:5.1})  bias {:+5.1} ms",
            self.gridded,
            self.n,
            self.exact,
            self.octave,
            self.downbeat,
            self.exact,
            med(&self.abs_err_ms),
            p95(&self.abs_err_ms),
            med(&self.signed_err_ms),
        );
    }
}

/// Ground-truth run on generated songs (MP3 → mpv, like the real thing).
#[cfg(unix)]
async fn synthetic(count: u64, cache: &Path, models: &BeatModel) {
    let dir = cache.join("synth");
    std::fs::create_dir_all(&dir).unwrap();
    let (mut bt, mut own, mut strat, mut chosen) =
        (SynthScore::default(), SynthScore::default(), SynthScore::default(), SynthScore::default());
    let (mut key_ok, mut key_rel, mut skey_ok, mut vocal_acc, mut vocal_n) = (0, 0, 0, 0.0, 0);
    let mut edges = Vec::new();
    let mut decode_lags = Vec::new();
    for seed in 1..=count {
        let (truth, l, r) = synth::song(seed);
        let path = dir.join(format!("{seed:03}.mp3"));
        if !path.exists() {
            synth::write_mp3(&path, &l, &r).expect("ffmpeg");
        }
        let pcm = listen_analysis::decode(path.to_str().unwrap(), &Cancel::default()).await.expect("decode");
        let mono = pcm.mono();
        // Does mpv remove the MP3 encoder delay? Cross-correlate with the source.
        let lag = {
            let src: Vec<f32> = l.iter().step_by(2).copied().collect();
            let (a, len) = (22050 * 20, 22050 * 10);
            (-600i32..=600)
                .max_by(|&x, &y| {
                    let c = |lag: i32| -> f32 {
                        (0..len).map(|i| src[a + i] * pcm.left[(a as i32 + i as i32 + lag) as usize]).sum()
                    };
                    c(x).total_cmp(&c(y))
                })
                .unwrap()
        };
        decode_lags.push(lag as f32 / 22.05);
        let (s, e) = structure::trim(&pcm);
        let frames = spectral::analyze(&pcm);
        let g_bt = grid_of(models.track(&mono, pcm.sample_rate).ok().as_ref(), &frames, s, e);
        let g_own = grid_of(beats::track_own(&frames, s, e, None).as_ref(), &frames, s, e);
        let st = track_stratum(&mono, pcm.sample_rate, s, e);
        let g_st = grid_of(st.as_ref().map(|r| &r.estimate), &frames, s, e);
        let opts = Opts { beats: BeatSource::BeatThis(models.clone()), bpm_hint: None };
        let f = listen_analysis::analyze(&pcm, &opts);
        bt.add(g_bt.as_ref(), &truth);
        own.add(g_own.as_ref(), &truth);
        strat.add(g_st.as_ref(), &truth);
        chosen.add(f.grid.as_ref(), &truth);

        let true_key = listen_core::analysis::Camelot::from_pitch(truth.tonic, truth.minor);
        let k = f.key.map(|k| k.0);
        key_ok += usize::from(k == Some(true_key));
        key_rel += usize::from(k.is_some_and(|k| true_key.distance(k) <= 0.15));
        skey_ok += usize::from(st.as_ref().and_then(|r| r.key).map(|k| k.0) == Some(true_key));
        // Vocal bars 8..48 when the grid lines up with the truth.
        if let Some(g) = &f.grid
            && agree(g.bpm, truth.bpm, false)
            && g.downbeats().next().is_some_and(|d| ((d - truth.start) / (240.0 / truth.bpm)).abs() < 0.05)
        {
            let hits = f
                .bars
                .vocal
                .iter()
                .enumerate()
                .filter(|(b, v)| ((8..48).contains(b)) == (**v >= 128))
                .count();
            vocal_acc += hits as f32 / f.bars.vocal.len().max(1) as f32;
            vocal_n += 1;
            edges.push((f.intro_instrumental_bars, f.outro_instrumental_bars));
        }
        println!(
            "{seed:>3} truth {:5.1} bpm {:<3} {}  | bt {:>6} own {:>6} str {:>6} | key {} | in {:>2} out {:>2} | {}",
            truth.bpm,
            true_key.label(),
            if truth.four_on_floor { "4/floor" } else { "1&3    " },
            g_bt.as_ref().map_or("-".into(), |g| format!("{:.1}", g.bpm)),
            g_own.as_ref().map_or("-".into(), |g| format!("{:.1}", g.bpm)),
            g_st.as_ref().map_or("-".into(), |g| format!("{:.1}", g.bpm)),
            f.key.map_or("-".into(), |(k, c)| format!("{}({c:.2})", k.label())),
            f.intro_instrumental_bars,
            f.outro_instrumental_bars,
            sections_summary(&f),
        );
    }
    println!("\nSynthetic ground truth ({count} songs, MP3 via mpv):");
    bt.report("beat-this grid");
    own.report("own grid");
    strat.report("stratum grid");
    chosen.report("analyze()");
    println!("  mpv decode offset vs source (ms): {decode_lags:?}");
    println!(
        "  key: own exact {key_ok}/{count}, own within relative/±1 {key_rel}/{count}, stratum exact {skey_ok}/{count}"
    );
    if vocal_n > 0 {
        let ok_edges = edges.iter().filter(|(a, b)| a.abs_diff(8) <= 1 && b.abs_diff(8) <= 1).count();
        println!(
            "  vocal bars correct {:.0} % (aligned songs: {vocal_n}); instrumental intro/outro = 8±1 bars: {ok_edges}/{vocal_n}",
            100.0 * vocal_acc / vocal_n as f32
        );
    }
}

#[cfg(not(unix))]
fn main() {
    eprintln!("this developer tool needs Linux");
}
