//! Music for the DJ end-to-end test (`tools/dj-e2e.sh`): twelve synthetic songs with known beat
//! grids, one gapless three-track album and nine singles for the Auto-DJ to mix.
//!
//! ```sh
//! cargo run --release -p listen-analysis --example djset -- <music dir>
//! ```
//!
//! Writes `<dir>/<artist>/<album>/NN - <title>.mp3|flac` and `<dir>/truth.json` (tempo and first
//! downbeat per title); existing files are kept.

#[allow(dead_code)] // the bench uses the rest
mod synth;

use synth::Part;

/// Intro with kick and hats, verse, breakdown, chorus, instrumental outro.
fn club(intro: usize, outro: usize) -> Vec<Part> {
    vec![
        Part::new(intro, true, false, false),
        Part::new(8, true, true, true),
        Part::new(4, false, false, true),
        Part::new(12, true, true, true),
        Part::new(outro, true, true, false),
    ]
}

/// Long instrumental intro and outro around a short vocal part.
fn long() -> Vec<Part> {
    vec![Part::new(16, true, false, false), Part::new(8, true, true, true), Part::new(16, true, true, false)]
}

/// Vocal from the first bar to the last.
fn vocal() -> Vec<Part> {
    vec![Part::new(12, true, true, true), Part::new(4, false, false, true), Part::new(16, true, true, true)]
}

/// Half-time: bars twice as long, so fewer of them.
fn half() -> Vec<Part> {
    vec![
        Part::new(4, true, false, false),
        Part::new(4, true, true, true),
        Part::new(8, true, true, true),
        Part::new(4, true, true, false),
    ]
}

struct Entry {
    artist: &'static str,
    album: &'static str,
    track: u32,
    title: &'static str,
    bpm: f32,
    four: bool,
    parts: fn() -> Vec<Part>,
    ext: &'static str,
}

#[allow(clippy::too_many_arguments)] // a table row
const fn e(
    artist: &'static str,
    album: &'static str,
    track: u32,
    title: &'static str,
    bpm: f32,
    four: bool,
    parts: fn() -> Vec<Part>,
    ext: &'static str,
) -> Entry {
    Entry { artist, album, track, title, bpm, four, parts, ext }
}

const SET: [Entry; 12] = [
    e("Test Band", "Run", 1, "Opening Run", 124.0, true, || club(8, 8), "mp3"),
    e("Test Band", "Run", 2, "Middle Run", 124.0, true, vocal, "flac"),
    e("Test Band", "Run", 3, "Closing Run", 124.0, true, || club(8, 8), "mp3"),
    e("Neon Steps", "Neon Steps EP", 1, "Neon Steps", 120.0, true, || club(8, 8), "flac"),
    e("Harbor", "Harbor Lights", 1, "Harbor Lights", 123.0, true, long, "mp3"),
    e("Paper Hearts", "Paper Hearts", 1, "Paper Hearts", 126.0, false, vocal, "flac"),
    e("Night Shift", "Midnight Drive", 1, "Midnight Drive", 128.0, true, || club(8, 8), "mp3"),
    e("Bloom", "Slow Bloom", 1, "Slow Bloom", 118.0, true, long, "flac"),
    e("Pulse Unit", "City Pulse", 1, "City Pulse", 125.0, true, || club(16, 8), "mp3"),
    e("Glass", "Glass Echo", 1, "Glass Echo", 122.0, false, vocal, "flac"),
    e("Lane Runner", "Fast Lane", 1, "Fast Lane", 136.0, true, || club(8, 8), "mp3"),
    e("Moon Choir", "Half Moon", 1, "Half Moon", 62.0, false, half, "flac"),
];

fn main() -> std::io::Result<()> {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("usage: djset <music dir>"));
    let mut truth = Vec::new();
    for (n, s) in SET.iter().enumerate() {
        let folder = dir.join(s.artist).join(s.album);
        std::fs::create_dir_all(&folder)?;
        let path = folder.join(format!("{:02} - {}.{}", s.track, s.title, s.ext));
        let (t, l, r) = synth::render(1000 + n as u64, s.bpm, s.four, (s.parts)());
        if !path.exists() {
            let tags = [
                ("title", s.title.to_string()),
                ("artist", s.artist.to_string()),
                ("album_artist", s.artist.to_string()),
                ("album", s.album.to_string()),
                ("track", s.track.to_string()),
                ("date", "2026".to_string()),
                ("genre", "Pop".to_string()),
            ];
            synth::encode(&path, &l, &r, &tags)?;
        }
        let secs = l.len() as f32 / synth::SR as f32;
        println!(
            "{:>5.1} bpm  {:>4.0} s  {:>2} bars  {}",
            s.bpm,
            secs,
            t.bars(),
            path.strip_prefix(&dir).unwrap_or(&path).display()
        );
        truth.push(serde_json::json!({
            "title": s.title,
            "bpm": s.bpm,
            "first_downbeat_s": t.start,
            "beats_per_bar": 4,
            "bars": t.bars(),
            "secs": secs,
        }));
    }
    std::fs::write(dir.join("truth.json"), serde_json::to_string_pretty(&truth)?)?;
    Ok(())
}
