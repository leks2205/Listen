//! Item 18 acceptance: pink noise rendered by a real mpv through the headphone-correction stage
//! (`listen_core::peq::mpv_filter`) matches the profile's computed curve within ±0.5 dB, and the
//! preamp keeps the output from clipping. Skipped when no mpv binary is available (`LISTEN_MPV`
//! overrides the path).

use listen_core::peq::{self, Profile};
use std::f64::consts::PI;
use std::path::{Path, PathBuf};

const SR: u32 = 48_000;

fn mpv_binary() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("LISTEN_MPV") {
        return Some(p.into());
    }
    std::process::Command::new("mpv").arg("--version").output().ok()?;
    Some("mpv".into())
}

/// Pink noise (Paul Kellet's filter over a fixed-seed white noise), stereo, peak at `peak`.
fn pink(secs: u32, peak: f32) -> Vec<f32> {
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut white = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
    };
    let mut b = [0.0f64; 7];
    let mono: Vec<f64> = (0..secs * SR)
        .map(|_| {
            let w = white();
            b[0] = 0.99886 * b[0] + w * 0.0555179;
            b[1] = 0.99332 * b[1] + w * 0.0750759;
            b[2] = 0.96900 * b[2] + w * 0.1538520;
            b[3] = 0.86650 * b[3] + w * 0.3104856;
            b[4] = 0.55000 * b[4] + w * 0.5329522;
            b[5] = -0.7616 * b[5] - w * 0.0168980;
            let out = b.iter().sum::<f64>() + w * 0.5362;
            b[6] = w * 0.115926;
            out
        })
        .collect();
    let max = mono.iter().fold(0.0f64, |m, &x| m.max(x.abs()));
    mono.iter().flat_map(|&x| [(x / max) as f32 * peak; 2]).collect()
}

fn write_wav(path: &Path, samples: &[f32]) {
    let bytes = (samples.len() * 4) as u32;
    let mut d = Vec::with_capacity(44 + bytes as usize);
    d.extend_from_slice(b"RIFF");
    d.extend_from_slice(&(36 + bytes).to_le_bytes());
    d.extend_from_slice(b"WAVEfmt ");
    d.extend_from_slice(&16u32.to_le_bytes());
    d.extend_from_slice(&3u16.to_le_bytes()); // IEEE float
    d.extend_from_slice(&2u16.to_le_bytes());
    d.extend_from_slice(&SR.to_le_bytes());
    d.extend_from_slice(&(SR * 8).to_le_bytes());
    d.extend_from_slice(&8u16.to_le_bytes());
    d.extend_from_slice(&32u16.to_le_bytes());
    d.extend_from_slice(b"data");
    d.extend_from_slice(&bytes.to_le_bytes());
    samples.iter().for_each(|s| d.extend_from_slice(&s.to_le_bytes()));
    std::fs::write(path, d).unwrap();
}

/// The float samples of mpv's PCM output (data chunk found by name).
fn read_wav(path: &Path) -> Vec<f32> {
    let d = std::fs::read(path).unwrap();
    let at = d.windows(4).position(|w| w == b"data").expect("data chunk") + 8;
    d[at..].as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect()
}

/// Plays `input` through mpv with `af` and returns the output samples (float, 48 kHz stereo).
fn render(mpv: &Path, input: &Path, out: &Path, af: &str) -> Vec<f32> {
    let status = std::process::Command::new(mpv)
        .args([
            "--no-config",
            "--really-quiet",
            "--no-video",
            "--ao=pcm",
            "--audio-format=float",
            "--audio-samplerate=48000",
            "--audio-channels=stereo",
            "--replaygain=no",
            "--volume=100",
            "--gapless-audio=no",
        ])
        .arg(format!("--ao-pcm-file={}", out.display()))
        .arg(format!("--af={af}"))
        .arg(input)
        .status()
        .expect("mpv runs");
    assert!(status.success(), "mpv failed with af {af}");
    read_wav(out)
}

/// Transfer magnitude (dB) from `x` to `y` (left channel) at `freq`: Hann-windowed blocks,
/// |Σ Y·X*| / Σ |X|² over all blocks (single-bin DFT per block).
fn transfer_db(x: &[f32], y: &[f32], freq: f64) -> f64 {
    let block = 16_384usize;
    let n = x.len().min(y.len()) / 2;
    let (mut cross_re, mut cross_im, mut power) = (0.0, 0.0, 0.0);
    let mut start = SR as usize; // skip the first second (filter settling)
    while start + block <= n {
        let (mut xr, mut xi, mut yr, mut yi) = (0.0, 0.0, 0.0, 0.0);
        for i in 0..block {
            let w = 0.5 - 0.5 * (2.0 * PI * i as f64 / block as f64).cos();
            let ph = 2.0 * PI * freq * i as f64 / f64::from(SR);
            let (s, c) = ph.sin_cos();
            let xv = f64::from(x[2 * (start + i)]) * w;
            let yv = f64::from(y[2 * (start + i)]) * w;
            xr += xv * c;
            xi -= xv * s;
            yr += yv * c;
            yi -= yv * s;
        }
        cross_re += yr * xr + yi * xi;
        cross_im += yi * xr - yr * xi;
        power += xr * xr + xi * xi;
        start += block / 2;
    }
    10.0 * ((cross_re * cross_re + cross_im * cross_im).sqrt() / power).powi(2).log10()
}

#[test]
fn pink_noise_through_mpv_matches_the_profile() {
    let Some(mpv) = mpv_binary() else {
        eprintln!("no mpv; skipped");
        return;
    };
    let dir = std::env::temp_dir().join(format!("listen-hpeq-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let input = dir.join("pink.wav");
    write_wav(&input, &pink(12, 0.891)); // −1 dBFS peak

    // Strong real profiles: the Verite (big cut and boost in the presence region), a Harman
    // in-ear with a large bass shelf, and the profile with the largest boost in the index.
    let mut profiles: Vec<Profile> = ["ZMF Verite", "Sony WH-1000XM5", "Samsung Galaxy Buds2 Pro"]
        .iter()
        .map(|n| peq::search(n, 1)[0].profile.clone())
        .collect();
    let loudest = peq::index()
        .iter()
        .max_by(|a, b| a.profile.max_boost_db(48_000.0).total_cmp(&b.profile.max_boost_db(48_000.0)))
        .unwrap();
    profiles.push(loudest.profile.clone());

    let reference = render(&mpv, &input, &dir.join("ref.wav"), "");
    let freqs: Vec<f64> =
        (0..=27).map(|i| 31.25 * 2f64.powf(f64::from(i) / 3.0)).filter(|&f| f < 18_000.0).collect();
    for p in &profiles {
        let af = peq::mpv_filter(p);
        let out = render(&mpv, &input, &dir.join("out.wav"), &af);
        let want = p.with_safe_preamp(f64::from(SR));
        let mut worst = (0.0f64, 0.0);
        for &f in &freqs {
            let got = transfer_db(&reference, &out, f);
            let err = got - want.response_db(f, f64::from(SR));
            if err.abs() > worst.0.abs() {
                worst = (err, f);
            }
        }
        let peak = out.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
        eprintln!("{}: worst {:+.3} dB at {:.0} Hz, peak {:.3}", p.name, worst.0, worst.1, peak);
        assert!(worst.0.abs() <= 0.5, "{}: {:+.3} dB off at {:.0} Hz", p.name, worst.0, worst.1);
        assert!(peak < 1.0, "{} clips: peak {peak}", p.name);
    }
    let _ = std::fs::remove_dir_all(&dir);
}
