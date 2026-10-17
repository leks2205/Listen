//! Key-detection bench on synthetic songs with known keys (`synth::song`, and pop arrangements with
//! tuning offsets from `synth::key_song`), each round-tripped through MP3 and mpv.
//!
//! ```sh
//! cargo run --release -p listen-analysis --example keys -- [--basic 40] [--pop 120] [-v]
//! ```

use listen_analysis::{Cancel, Opts, key, spectral, structure};
use listen_core::analysis::Camelot;
use std::path::Path;

mod synth;

#[derive(Default)]
struct Score {
    n: usize,
    exact: usize,
    relative: usize,
    fifth: usize,
    parallel: usize,
    other: usize,
    none: usize,
    /// Detected minor / truth minor.
    said_minor: usize,
    minor: usize,
    conf_right: Vec<f32>,
    conf_wrong: Vec<f32>,
}

impl Score {
    fn add(&mut self, truth: Camelot, got: Option<(Camelot, f32)>) -> &'static str {
        self.n += 1;
        self.minor += usize::from(truth.minor);
        let Some((k, c)) = got else {
            self.none += 1;
            return "none";
        };
        self.said_minor += usize::from(k.minor);
        let class = if k == truth {
            self.exact += 1;
            "exact"
        } else if k.number == truth.number {
            self.relative += 1;
            "relative"
        } else if truth.distance(k) <= 0.1 {
            self.fifth += 1;
            "fifth"
        } else if k.minor != truth.minor
            && (i16::from(k.number) - i16::from(truth.number)).rem_euclid(12)
                == if truth.minor { 3 } else { 9 }
        {
            self.parallel += 1;
            "parallel"
        } else {
            self.other += 1;
            "OTHER"
        };
        if k == truth { &mut self.conf_right } else { &mut self.conf_wrong }.push(c);
        class
    }

    fn report(&self, name: &str) {
        let mean = |v: &[f32]| if v.is_empty() { 0.0 } else { v.iter().sum::<f32>() / v.len() as f32 };
        let low = |v: &[f32]| v.iter().filter(|&&c| c < 0.3).count();
        println!(
            "{name}: exact {}/{} ({:.0} %), relative {}, fifth {}, parallel {}, other {}, none {} | said minor {} (truth {}) | conf right {:.2} ({} < 0.3), wrong {:.2} ({} < 0.3)",
            self.exact,
            self.n,
            100.0 * self.exact as f32 / self.n.max(1) as f32,
            self.relative,
            self.fifth,
            self.parallel,
            self.other,
            self.none,
            self.said_minor,
            self.minor,
            mean(&self.conf_right),
            low(&self.conf_right),
            mean(&self.conf_wrong),
            low(&self.conf_wrong),
        );
    }
}

type Song = (synth::Truth, Vec<f32>, Vec<f32>);

async fn run(
    name: &str,
    count: u64,
    make: fn(u64) -> Song,
    dir: &Path,
    verbose: bool,
    dump: &mut Option<std::fs::File>,
) {
    let mut score = Score::default();
    for seed in 1..=count {
        let path = dir.join(format!("{name}-{seed:03}.mp3"));
        let (truth, l, r) = make(seed);
        if !path.exists() {
            synth::write_mp3(&path, &l, &r).expect("ffmpeg");
        }
        let pcm = listen_analysis::decode(path.to_str().unwrap(), &Cancel::default()).await.expect("decode");
        // Full pipeline with the built-in beat tracker (the key uses the grid's phrase starts).
        let f = listen_analysis::analyze(&pcm, &Opts::default());
        let got = f.key;
        let want = Camelot::from_pitch(truth.tonic, truth.minor);
        let class = score.add(want, got);
        if let Some(out) = dump.as_mut() {
            use std::io::Write;
            let frames = spectral::analyze(&pcm);
            let bounds = structure::bar_bounds(f.grid.as_ref(), f.audio_start, f.audio_end);
            let starts = if f.grid.is_some() {
                key::phrase_starts(&bounds, f.phrase_bars, f.phrase_offset)
            } else {
                vec![]
            };
            let p = key::pitch_profile(&frames, f.audio_start, f.audio_end, &starts);
            writeln!(
                out,
                "{{\"set\":\"{name}\",\"seed\":{seed},\"tonic\":{},\"minor\":{},\"prog\":{},\"cents\":{},\"est_cents\":{},\"treble\":{:?},\"bass\":{:?},\"phrase_bass\":{:?}}}",
                truth.tonic, truth.minor, truth.prog, truth.cents, p.cents, p.treble, p.bass, p.phrase_bass
            )
            .unwrap();
        }
        if verbose || class != "exact" {
            println!(
                "{name} {seed:>3} prog {:>2} {:+5.1} ct  truth {:<3} got {:<11} {class}",
                truth.prog,
                truth.cents,
                want.label(),
                got.map_or("-".into(), |(k, c)| format!("{}({c:.2})", k.label())),
            );
        }
    }
    score.report(name);
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str, d: u64| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .and_then(|v| v.parse().ok())
            .unwrap_or(d)
    };
    let verbose = args.iter().any(|a| a == "-v");
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/analysis-bench/synth-keys");
    std::fs::create_dir_all(&dir).unwrap();
    let mut dump = args
        .iter()
        .position(|a| a == "--dump")
        .and_then(|i| args.get(i + 1))
        .map(|p| std::fs::File::create(p).expect("dump file"));
    run("basic", arg("--basic", 40), synth::song, &dir, verbose, &mut dump).await;
    run("pop", arg("--pop", 120), synth::key_song, &dir, verbose, &mut dump).await;
}
