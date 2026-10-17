//! Spectrum analyzer: FFT → bands in dB (or linear amplitude) with smoothing, weighting and
//! falling/fading peaks, optionally per channel. Options mirror audioMotion-analyzer's.

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};
use std::sync::Arc;

pub const SAMPLE_RATE: u32 = 48_000;

/// How frequencies map to the horizontal axis in the discrete and graph modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FreqScale {
    #[default]
    Log,
    Linear,
    Bark,
    Mel,
}

impl FreqScale {
    /// The frequency in the scale's own unit (monotonic).
    pub fn warp(self, f: f32) -> f32 {
        match self {
            FreqScale::Log => f.max(1.0).ln(),
            FreqScale::Linear => f,
            FreqScale::Bark => 26.81 * f / (1960.0 + f) - 0.53,
            FreqScale::Mel => 2595.0 * (1.0 + f / 700.0).log10(),
        }
    }

    pub fn unwarp(self, u: f32) -> f32 {
        match self {
            FreqScale::Log => u.exp(),
            FreqScale::Linear => u,
            FreqScale::Bark => 1960.0 * (u + 0.53) / (26.28 - u),
            FreqScale::Mel => 700.0 * (10f32.powf(u / 2595.0) - 1.0),
        }
    }

    /// Horizontal position (0..=1) of `f` between `min` and `max`.
    pub fn position(self, f: f32, min: f32, max: f32) -> f32 {
        let (lo, hi) = (self.warp(min), self.warp(max));
        if (hi - lo).abs() < 1e-9 { 0.0 } else { ((self.warp(f) - lo) / (hi - lo)).clamp(0.0, 1.0) }
    }
}

/// Frequency weighting curve added to the levels (dB).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Weighting {
    #[default]
    None,
    A,
    B,
    C,
    D,
    /// ITU-R 468.
    Z,
}

impl Weighting {
    /// Gain in dB at `f` Hz.
    pub fn db(self, f: f32) -> f32 {
        let f = f64::from(f.max(1.0));
        let f2 = f * f;
        let db = |r: f64| 20.0 * r.max(1e-12).log10();
        let v = match self {
            Weighting::None => 0.0,
            Weighting::A => {
                let c = 12194f64 * 12194.0;
                db(c * f2 * f2
                    / ((f2 + 20.6 * 20.6) * ((f2 + 107.7 * 107.7) * (f2 + 737.9 * 737.9)).sqrt() * (f2 + c)))
                    + 2.0
            }
            Weighting::B => {
                let c = 12194f64 * 12194.0;
                db(c * f2 * f / ((f2 + 20.6 * 20.6) * (f2 + 158.5 * 158.5).sqrt() * (f2 + c))) + 0.17
            }
            Weighting::C => {
                let c = 12194f64 * 12194.0;
                db(c * f2 / ((f2 + 20.6 * 20.6) * (f2 + c))) + 0.06
            }
            Weighting::D => {
                let h = ((1_037_918.48 - f2).powi(2) + 1_080_768.16 * f2)
                    / ((9_837_328.0 - f2).powi(2) + 11_723_776.0 * f2);
                db(f / 6.896_688_849_647_6e-5 * (h / ((f2 + 79_919.29) * (f2 + 1_345_600.0))).sqrt())
            }
            Weighting::Z => {
                let h1 = -4.737_338_981_378_384e-24 * f.powi(6) + 2.043_828_333_606_125e-15 * f.powi(4)
                    - 1.363_894_795_463_638e-7 * f2
                    + 1.0;
                let h2 = 1.306_612_257_412_824e-19 * f.powi(5) - 2.118_150_887_518_656e-11 * f.powi(3)
                    + 5.559_488_023_498_642e-4 * f;
                18.2 + db(1.246_332_637_532_143e-4 * f / (h1 * h1 + h2 * h2).sqrt())
            }
        };
        v as f32
    }
}

/// Fractional-octave bands (audioMotion modes 1–8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Octave {
    /// Bands per octave: 24, 12, 8, 6, 4, 3, 2 or 1.
    pub per_octave: u32,
    /// ANSI/IEC preferred (base-10) band centres instead of equal-tempered.
    pub ansi: bool,
}

impl Octave {
    /// Bands per octave of audioMotion's `mode` 1–8 (`None` for 0, 10, …).
    pub fn of_mode(mode: u8, ansi: bool) -> Option<Octave> {
        let per_octave = match mode {
            1 => 24,
            2 => 12,
            3 => 8,
            4 => 6,
            5 => 4,
            6 => 3,
            7 => 2,
            8 => 1,
            _ => return None,
        };
        Some(Octave { per_octave, ansi })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub fft_size: usize,
    /// Bars of the discrete / graph modes (ignored with [`Config::octave`]).
    pub bands: usize,
    pub min_freq: f32,
    pub max_freq: f32,
    pub min_db: f32,
    pub max_db: f32,
    /// 0 = no smoothing, → 1 = very smooth.
    pub smoothing: f32,
    pub scale: FreqScale,
    pub weighting: Weighting,
    pub octave: Option<Octave>,
    /// Levels are the amplitude (0..1) instead of dB.
    pub linear_amplitude: bool,
    /// Exponent boost of weak linear amplitudes (audioMotion's `linearBoost`).
    pub linear_boost: f32,
    /// Input gain.
    pub volume: f32,
    /// Both channels apart (`Frame::right`).
    pub dual: bool,
    pub peak_hold_ms: u32,
    pub peak_fade_ms: u32,
    /// Peaks fade out in place instead of falling.
    pub fade_peaks: bool,
    /// Fall speed of peaks; 11 is audioMotion's default.
    pub gravity: f32,
    /// Time between two frames, for the peak timings.
    pub frame_ms: f32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            fft_size: 8192,
            bands: 160,
            min_freq: 20.0,
            max_freq: 22_050.0,
            min_db: -85.0,
            max_db: -25.0,
            smoothing: 0.6,
            scale: FreqScale::Log,
            weighting: Weighting::None,
            octave: None,
            linear_amplitude: false,
            linear_boost: 4.0,
            volume: 1.0,
            dual: false,
            peak_hold_ms: 500,
            peak_fade_ms: 900,
            fade_peaks: false,
            gravity: 11.0,
            frame_ms: 33.0,
        }
    }
}

/// One channel of a frame: band levels, peak levels and peak opacity, all 0..=1
/// (`peak_alpha` empty when no peak is fading).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Channel {
    pub levels: Vec<f32>,
    pub peaks: Vec<f32>,
    pub peak_alpha: Vec<f32>,
}

/// One frame for drawing: the mix of both channels, or with [`Config::dual`] the left one plus `right`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Frame {
    pub levels: Vec<f32>,
    pub peaks: Vec<f32>,
    pub peak_alpha: Vec<f32>,
    pub right: Option<Channel>,
}

impl Frame {
    /// A frame of `bands` bars at one level (silence, tests).
    pub fn flat(bands: usize, level: f32) -> Frame {
        Frame { levels: vec![level; bands], peaks: vec![level; bands], ..Frame::default() }
    }
}

#[derive(Default)]
struct ChannelState {
    smoothed: Vec<f32>,
    peaks: Vec<f32>,
    peak_hold: Vec<u32>,
    peak_alpha: Vec<f32>,
}

impl ChannelState {
    fn new(bands: usize) -> Self {
        Self {
            smoothed: vec![0.0; bands],
            peaks: vec![0.0; bands],
            peak_hold: vec![0; bands],
            peak_alpha: vec![1.0; bands],
        }
    }
}

pub struct Analyzer {
    cfg: Config,
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    /// Per source (deck): ring of its latest `fft_size` stereo samples; the FFT runs on their sum.
    rings: Vec<Ring>,
    /// Per band: FFT bin range.
    ranges: Vec<(usize, usize)>,
    /// Per band: weighting gain in dB at its centre.
    weights: Vec<f32>,
    state: [ChannelState; 2],
}

impl Analyzer {
    pub fn new(mut cfg: Config) -> Self {
        cfg.fft_size = cfg.fft_size.clamp(1024, 32_768).next_power_of_two();
        let n = cfg.fft_size;
        let fft = FftPlanner::new().plan_fft_forward(n);
        // Blackman-Harris window, like audioMotion's default.
        let window = (0..n)
            .map(|i| {
                let x = std::f32::consts::TAU * i as f32 / (n - 1) as f32;
                0.35875 - 0.48829 * x.cos() + 0.14128 * (2.0 * x).cos() - 0.01168 * (3.0 * x).cos()
            })
            .collect();
        let bands = bands(&cfg);
        let ranges = bands.iter().map(|b| (b.lo, b.hi)).collect::<Vec<_>>();
        let weights = bands.iter().map(|b| cfg.weighting.db(b.center)).collect();
        let count = ranges.len();
        Self {
            cfg,
            fft,
            window,
            rings: vec![Ring::new(n)],
            ranges,
            weights,
            state: [ChannelState::new(count), ChannelState::new(count)],
        }
    }

    pub fn config(&self) -> &Config {
        &self.cfg
    }

    /// Applies new options, keeping the recent samples when the FFT size is unchanged.
    pub fn set_config(&mut self, cfg: Config) {
        let mut next = Analyzer::new(cfg);
        if next.window.len() == self.window.len() {
            next.rings = std::mem::take(&mut self.rings);
        }
        *self = next;
    }

    /// Feeds interleaved stereo f32 samples of the first (or only) source.
    pub fn push_stereo(&mut self, samples: &[f32]) {
        self.push_source(0, samples);
    }

    /// Feeds interleaved stereo f32 samples of `source` (e.g. a deck); frames show the sum of all sources.
    pub fn push_source(&mut self, source: usize, samples: &[f32]) {
        let n = self.window.len();
        if source >= self.rings.len() {
            self.rings.resize_with(source + 1, || Ring::new(n));
        }
        self.rings[source].push_stereo(samples);
    }

    /// Silences a source (e.g. a stopped deck) so its last samples leave the sum.
    pub fn clear_source(&mut self, source: usize) {
        if let Some(ring) = self.rings.get_mut(source) {
            ring.clear();
        }
    }

    /// Computes the current frame (call at display rate, e.g. 30–60 Hz).
    pub fn frame(&mut self) -> Frame {
        if self.cfg.dual {
            let left = self.channel(0, Mix::Left);
            let right = self.channel(1, Mix::Right);
            Frame { levels: left.levels, peaks: left.peaks, peak_alpha: left.peak_alpha, right: Some(right) }
        } else {
            let c = self.channel(0, Mix::Both);
            Frame { levels: c.levels, peaks: c.peaks, peak_alpha: c.peak_alpha, right: None }
        }
    }

    fn channel(&mut self, slot: usize, mix: Mix) -> Channel {
        let n = self.window.len();
        let gain = self.cfg.volume.max(0.0);
        let mut buf: Vec<Complex<f32>> = (0..n)
            .map(|i| {
                let v: f32 = self.rings.iter().map(|r| r.at(i, mix)).sum();
                Complex::new(v * gain * self.window[i], 0.0)
            })
            .collect();
        self.fft.process(&mut buf);
        let norm = 2.0 / (n as f32 * 0.35875); // window coherent gain
        let range = (self.cfg.max_db - self.cfg.min_db).max(1.0);
        let s = self.cfg.smoothing.clamp(0.0, 0.99);
        let hold_ticks = (self.cfg.peak_hold_ms as f32 / self.cfg.frame_ms.max(1.0)).ceil() as u32;
        let fade_step = self.cfg.frame_ms / (self.cfg.peak_fade_ms.max(1) as f32);
        let fall = 0.02 * (self.cfg.gravity / 11.0).max(0.05) * (self.cfg.frame_ms / 33.0);
        let st = &mut self.state[slot];
        let mut levels = Vec::with_capacity(self.ranges.len());
        for (b, &(lo, hi)) in self.ranges.iter().enumerate() {
            let mag = buf[lo..=hi].iter().map(|c| c.norm() * norm).fold(0.0f32, f32::max);
            let level = if self.cfg.linear_amplitude {
                let m = mag * 10f32.powf(self.weights[b] / 20.0);
                m.clamp(0.0, 1.0).powf(1.0 / self.cfg.linear_boost.max(1.0))
            } else {
                let db = 20.0 * mag.max(1e-12).log10() + self.weights[b];
                ((db - self.cfg.min_db) / range).clamp(0.0, 1.0)
            };
            // audioMotion smooths like the Web Audio analyser (time-constant average).
            let v = st.smoothed[b] * s + level * (1.0 - s);
            st.smoothed[b] = v;
            if v >= st.peaks[b] {
                st.peaks[b] = v;
                st.peak_hold[b] = hold_ticks;
                st.peak_alpha[b] = 1.0;
            } else if st.peak_hold[b] > 0 {
                st.peak_hold[b] -= 1;
            } else if self.cfg.fade_peaks {
                st.peak_alpha[b] -= fade_step;
                if st.peak_alpha[b] <= 0.0 {
                    st.peaks[b] = v;
                    st.peak_alpha[b] = 1.0;
                }
            } else {
                st.peaks[b] = (st.peaks[b] - fall).max(v);
            }
            levels.push(v);
        }
        let peak_alpha = if self.cfg.fade_peaks { st.peak_alpha.clone() } else { Vec::new() };
        Channel { levels, peaks: st.peaks.clone(), peak_alpha }
    }
}

#[derive(Clone, Copy)]
enum Mix {
    Both,
    Left,
    Right,
}

/// Stereo ring buffer holding one source's latest samples.
struct Ring {
    l: Vec<f32>,
    r: Vec<f32>,
    pos: usize,
}

impl Ring {
    fn new(n: usize) -> Self {
        Self { l: vec![0.0; n], r: vec![0.0; n], pos: 0 }
    }

    fn push_stereo(&mut self, samples: &[f32]) {
        for &[l, r] in samples.as_chunks::<2>().0 {
            let (l, r) = if l.is_finite() && r.is_finite() { (l, r) } else { (0.0, 0.0) };
            self.l[self.pos] = l;
            self.r[self.pos] = r;
            self.pos = (self.pos + 1) % self.l.len();
        }
    }

    /// The `i`-th oldest sample.
    fn at(&self, i: usize, mix: Mix) -> f32 {
        let j = (self.pos + i) % self.l.len();
        match mix {
            Mix::Both => (self.l[j] + self.r[j]) * 0.5,
            Mix::Left => self.l[j],
            Mix::Right => self.r[j],
        }
    }

    fn clear(&mut self) {
        self.l.fill(0.0);
        self.r.fill(0.0);
    }
}

/// A band: FFT bins `lo..=hi` and its centre frequency.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Band {
    pub lo: usize,
    pub hi: usize,
    pub center: f32,
}

/// The bands of `cfg`: evenly spaced on the frequency scale, or fractional-octave bands.
pub fn bands(cfg: &Config) -> Vec<Band> {
    let n = cfg.fft_size.clamp(1024, 32_768).next_power_of_two();
    let bin_hz = SAMPLE_RATE as f32 / n as f32;
    let max_bin = n / 2 - 1;
    let nyquist = SAMPLE_RATE as f32 / 2.0;
    let (fmin, fmax) = (cfg.min_freq.max(bin_hz), cfg.max_freq.min(nyquist));
    // (lower edge, upper edge, centre) in Hz.
    let mut edges: Vec<(f32, f32, f32)> = Vec::new();
    if let Some(o) = cfg.octave {
        let n = o.per_octave.max(1) as f32;
        let (ratio, reference) =
            if o.ansi { (10f32.powf(3.0 / (10.0 * n)), 1000.0) } else { (2f32.powf(1.0 / n), 440.0) };
        let half = ratio.sqrt();
        for k in -200..=200 {
            let c = reference * ratio.powi(k);
            if c < fmin || c > fmax {
                continue;
            }
            edges.push((c / half, c * half, c));
        }
    } else {
        let count = cfg.bands.max(2);
        let (lo, hi) = (cfg.scale.warp(fmin), cfg.scale.warp(fmax));
        for i in 0..count {
            let u = |x: f32| cfg.scale.unwarp(lo + (hi - lo) * x / count as f32);
            let (f0, f1) = (u(i as f32), u(i as f32 + 1.0));
            edges.push((f0, f1, u(i as f32 + 0.5)));
        }
    }
    let mut out: Vec<Band> = Vec::new();
    for (f0, f1, center) in edges {
        let b0 = ((f0 / bin_hz).floor() as usize).clamp(1, max_bin);
        let b1 = ((f1 / bin_hz).ceil() as usize).clamp(b0, max_bin);
        // Merge bands that would map to the same bins (low frequencies).
        if out.last().is_some_and(|p| p.lo == b0) {
            continue;
        }
        out.push(Band { lo: b0, hi: b1, center });
    }
    out
}

/// Bin ranges of [`bands`].
pub fn band_ranges(cfg: &Config) -> Vec<(usize, usize)> {
    bands(cfg).into_iter().map(|b| (b.lo, b.hi)).collect()
}

/// Centre frequency of every band of `cfg` (for axis labels).
pub fn band_centers(cfg: &Config) -> Vec<f32> {
    bands(cfg).into_iter().map(|b| b.center).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, amp: f32, secs: f32) -> Vec<f32> {
        let n = (SAMPLE_RATE as f32 * secs) as usize;
        (0..n)
            .flat_map(|i| {
                let v = amp * (std::f32::consts::TAU * freq * i as f32 / SAMPLE_RATE as f32).sin();
                [v, v]
            })
            .collect()
    }

    #[test]
    fn a_tone_lights_up_its_band() {
        let mut a = Analyzer::new(Config { smoothing: 0.0, ..Config::default() });
        // −40 dBFS stays inside the −85…−25 dB window, so bands don't saturate.
        a.push_stereo(&sine(1000.0, 0.01, 0.5));
        let f = a.frame();
        let (peak_band, peak) = f
            .levels
            .iter()
            .enumerate()
            .fold((0, 0.0f32), |acc, (i, &v)| if v > acc.1 { (i, v) } else { acc });
        assert!(peak > 0.5 && peak < 1.0, "tone level in range, got {peak}");
        let (lo, hi) = a.ranges[peak_band];
        let bin_hz = SAMPLE_RATE as f32 / a.cfg.fft_size as f32;
        assert!((lo as f32 * bin_hz..=(hi + 1) as f32 * bin_hz).contains(&1000.0));
        // Far-away bands stay quiet.
        assert!(f.levels[0] < 0.2 && *f.levels.last().unwrap() < 0.2);
    }

    #[test]
    fn silence_is_zero_and_nan_is_ignored() {
        let mut a = Analyzer::new(Config::default());
        a.push_stereo(&[f32::NAN, 0.0, 0.0, 0.0]);
        let f = a.frame();
        assert!(f.levels.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn sources_are_summed_and_can_be_cleared() {
        let cfg = Config { smoothing: 0.0, ..Config::default() };
        let (mut one, mut two) = (Analyzer::new(cfg.clone()), Analyzer::new(cfg));
        one.push_source(0, &sine(1000.0, 0.01, 0.5));
        two.push_source(0, &sine(1000.0, 0.01, 0.5));
        two.push_source(1, &sine(5000.0, 0.01, 0.5));
        let band = |a: &Analyzer, hz: f32| {
            let bin = (hz * a.cfg.fft_size as f32 / SAMPLE_RATE as f32) as usize;
            a.ranges.iter().position(|&(lo, hi)| (lo..=hi).contains(&bin)).unwrap()
        };
        let (b1k, b5k) = (band(&one, 1000.0), band(&one, 5000.0));
        let (f1, f2) = (one.frame(), two.frame());
        assert!(f1.levels[b5k] < 0.2, "one source: no 5 kHz");
        assert!(f2.levels[b1k] > 0.5 && f2.levels[b5k] > 0.5, "both decks visible");
        // A stopped deck no longer shows up.
        two.clear_source(1);
        let f3 = two.frame();
        assert!(f3.levels[b5k] < 0.2 && f3.levels[b1k] > 0.5);
    }

    #[test]
    fn bands_are_ordered_and_distinct() {
        let r = band_ranges(&Config::default());
        assert!(r.len() > 60);
        assert!(r.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn scales_round_trip_and_stay_monotonic() {
        for scale in [FreqScale::Log, FreqScale::Linear, FreqScale::Bark, FreqScale::Mel] {
            let mut last = -1.0;
            for f in [20.0, 100.0, 1000.0, 8000.0, 20000.0] {
                let u = scale.warp(f);
                assert!(u > last, "{scale:?} monotonic");
                last = u;
                assert!((scale.unwarp(u) - f).abs() / f < 1e-2, "{scale:?} round trip at {f}");
            }
            assert!((scale.position(20.0, 20.0, 20000.0)).abs() < 1e-5);
            assert!((scale.position(20000.0, 20.0, 20000.0) - 1.0).abs() < 1e-5);
        }
        // Log puts 1 kHz about 60% of the way along 20 Hz–20 kHz; linear next to nothing.
        assert!((FreqScale::Log.position(1000.0, 20.0, 20000.0) - 0.565).abs() < 0.01);
        assert!(FreqScale::Linear.position(1000.0, 20.0, 20000.0) < 0.06);
    }

    #[test]
    fn weighting_curves_are_the_standard_ones() {
        assert!(Weighting::A.db(1000.0).abs() < 0.1, "A is 0 dB at 1 kHz");
        assert!((Weighting::A.db(100.0) + 19.1).abs() < 0.3);
        assert!((Weighting::C.db(1000.0)).abs() < 0.1);
        assert!(Weighting::B.db(100.0) > Weighting::A.db(100.0));
        assert!(Weighting::Z.db(1000.0).abs() < 0.5, "468 weighting is 0 dB at 1 kHz");
        assert!((Weighting::Z.db(6300.0) - 12.2).abs() < 0.5, "and +12.2 dB at 6.3 kHz");
        assert_eq!(Weighting::None.db(50.0), 0.0);
    }

    #[test]
    fn octave_bands_have_the_requested_density() {
        let cfg = |per_octave| Config {
            octave: Some(Octave { per_octave, ansi: false }),
            fft_size: 16384,
            ..Config::default()
        };
        let full = bands(&cfg(1)).len();
        let third = bands(&cfg(3)).len();
        assert!((9..=11).contains(&full), "10 octaves 20 Hz–22 kHz, got {full}");
        assert!((28..=32).contains(&third), "3 bands per octave, got {third}");
        assert_eq!(Octave::of_mode(8, false), Some(Octave { per_octave: 1, ansi: false }));
        assert_eq!(Octave::of_mode(10, false), None);
        let ansi = bands(&Config { octave: Some(Octave { per_octave: 3, ansi: true }), ..Config::default() });
        assert!(ansi.iter().any(|b| (b.center - 1000.0).abs() < 1.0), "ANSI bands include 1 kHz");
    }

    #[test]
    fn dual_channels_are_analysed_apart() {
        let mut a = Analyzer::new(Config { smoothing: 0.0, dual: true, ..Config::default() });
        // 1 kHz on the left only, 5 kHz on the right only.
        let l = sine(1000.0, 0.01, 0.5);
        let r = sine(5000.0, 0.01, 0.5);
        let mixed: Vec<f32> = l.chunks(2).zip(r.chunks(2)).flat_map(|(l, r)| [l[0], r[0]]).collect();
        a.push_stereo(&mixed);
        let f = a.frame();
        let right = f.right.expect("right channel");
        let band = |hz: f32| {
            let bin = (hz * a.cfg.fft_size as f32 / SAMPLE_RATE as f32) as usize;
            a.ranges.iter().position(|&(lo, hi)| (lo..=hi).contains(&bin)).unwrap()
        };
        assert!(f.levels[band(1000.0)] > 0.5 && f.levels[band(5000.0)] < 0.2);
        assert!(right.levels[band(5000.0)] > 0.5 && right.levels[band(1000.0)] < 0.2);
    }

    #[test]
    fn peaks_hold_then_fall_or_fade() {
        let loud = sine(1000.0, 0.05, 0.5);
        let run = |fade: bool| {
            let mut a = Analyzer::new(Config { smoothing: 0.0, fade_peaks: fade, ..Config::default() });
            a.push_stereo(&loud);
            let f0 = a.frame();
            let top = f0.peaks.iter().cloned().fold(0.0, f32::max);
            a.clear_source(0);
            // Within the hold time the peak stays; later it falls (or fades in place).
            let held = (0..10).map(|_| a.frame()).last().unwrap();
            let later = (0..30).map(|_| a.frame()).last().unwrap();
            (top, held, later)
        };
        let (top, held, later) = run(false);
        assert!(held.peaks.iter().cloned().fold(0.0, f32::max) >= top - 1e-4, "held");
        assert!(later.peaks.iter().cloned().fold(0.0, f32::max) < top - 0.1, "fell");
        assert!(later.peak_alpha.is_empty());
        let (_, _, later) = run(true);
        assert!(!later.peak_alpha.is_empty() && later.peak_alpha.iter().any(|&a| a < 1.0), "fading");
    }

    #[test]
    fn linear_amplitude_and_volume() {
        let tone = sine(1000.0, 0.01, 0.5);
        let level = |cfg: Config| {
            let mut a = Analyzer::new(Config { smoothing: 0.0, ..cfg });
            a.push_stereo(&tone);
            a.frame().levels.iter().cloned().fold(0.0, f32::max)
        };
        let quiet = level(Config { linear_amplitude: true, linear_boost: 1.0, ..Config::default() });
        let boosted = level(Config { linear_amplitude: true, linear_boost: 4.0, ..Config::default() });
        assert!((quiet - 0.01).abs() < 0.003, "amplitude 0.01, got {quiet}");
        assert!(boosted > quiet * 2.0);
        let loud = level(Config { volume: 4.0, ..Config::default() });
        assert!(loud > level(Config::default()));
    }

    #[test]
    fn reconfiguring_keeps_the_samples() {
        let mut a = Analyzer::new(Config { smoothing: 0.0, ..Config::default() });
        a.push_stereo(&sine(1000.0, 0.01, 0.5));
        a.set_config(Config { smoothing: 0.0, scale: FreqScale::Linear, ..Config::default() });
        assert!(a.frame().levels.iter().cloned().fold(0.0, f32::max) > 0.5);
    }
}
