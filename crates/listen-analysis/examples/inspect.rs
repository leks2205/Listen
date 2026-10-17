//! Per-bar view of one file or URL (for tuning the structure heuristics).
//!
//! ```sh
//! cargo run --release -p listen-analysis --example inspect -- song.mp3 [models-dir]
//! ```

use listen_analysis::{BeatModel, BeatSource, Cancel, Opts, spectral, structure};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let input = args.get(1).expect("usage: inspect <file-or-url> [models-dir]");
    let beats = match args.get(2).and_then(|d| BeatModel::in_dir(std::path::Path::new(d))) {
        Some(m) => BeatSource::BeatThis(m),
        None => BeatSource::Own,
    };
    let pcm = listen_analysis::decode(input, &Cancel::default()).await.expect("decode");
    let opts = Opts { beats, bpm_hint: None };
    let (f, tm) = listen_analysis::analyze_timed(&pcm, &opts);
    let frames = spectral::analyze(&pcm);
    let bounds = structure::bar_bounds(f.grid.as_ref(), f.audio_start, f.audio_end);
    let stats = structure::bar_stats(&frames, &bounds);
    println!(
        "{:.2}–{:.2} s, grid {:?}, key {:?}, {:.1} LUFS, phrase {}+{}, in {} out {}",
        f.audio_start,
        f.audio_end,
        f.grid.as_ref().map(|g| (g.bpm, g.confidence, g.stability, g.downbeat_phase)),
        f.key.map(|(k, c)| (k.label(), c)),
        f.lufs,
        f.phrase_bars,
        f.phrase_offset,
        f.intro_instrumental_bars,
        f.outro_instrumental_bars
    );
    println!("bar   time  energy  low  vocal(raw)       cen  novelty");
    for (b, start) in bounds.iter().enumerate().take(stats.energy.len()) {
        println!(
            "{b:>3} {:6.1}  {:>5}  {:>4}  {:>4} ({:.4})  {:>4}  {:>4} {}",
            start,
            f.bars.energy[b],
            f.bars.low[b],
            f.bars.vocal[b],
            stats.vocal_raw[b],
            f.bars.centroid[b],
            f.bars.novelty[b],
            "#".repeat(usize::from(f.bars.novelty[b]) / 16)
        );
    }
    println!("sections: {:?}", f.sections.iter().map(|s| (s.start_bar, s.bars, s.kind)).collect::<Vec<_>>());
    println!("timbre: {:?}\nmood: {:?}", f.timbre, f.mood);
    for (name, d) in tm.stages {
        println!("  {name:<10} {:>6.1} ms", d.as_secs_f64() * 1000.0);
    }
}
