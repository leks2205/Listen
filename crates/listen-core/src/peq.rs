//! Parametric EQ and AutoEq headphone correction.
//!
//! A [`Profile`] is a preamp plus peaking and shelf biquads (RBJ cookbook, the same formulas
//! AutoEq and ffmpeg's `equalizer`/`lowshelf`/`highshelf` use). The desktop renders it as one mpv
//! `lavfi` stage ([`mpv_filter`]); Android runs [`Cascade`] in its mixer. Profiles come from the
//! bundled AutoEq index ([`index`], [`search`], [`suggest`]) or an imported `ParametricEQ.txt`
//! ([`parse_parametric_eq`]), and are bound to an output device by its id. Every consumer limits a
//! band to [`FREQ_RANGE`], [`Q_RANGE`] and [`GAIN_RANGE`] through [`Filter::clamped`], so both
//! platforms play, and the drawn curve and the preamp describe, the same numbers.

use serde::{Deserialize, Serialize};
use std::f64::consts::PI;
use std::io::Read;
use std::sync::OnceLock;

use crate::text::fold;

/// Kind of one parametric band.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterKind {
    Peak,
    LowShelf,
    HighShelf,
}

/// One parametric band: centre (peak) or corner (shelf) frequency, gain, Q.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Filter {
    pub kind: FilterKind,
    pub freq: f32,
    pub gain: f32,
    pub q: f32,
}

/// Centre/corner frequencies a band may have (Hz).
pub const FREQ_RANGE: std::ops::RangeInclusive<f32> = 10.0..=22_000.0;
/// Q values a band may have (mpv's `equalizer` accepts up to 40).
pub const Q_RANGE: std::ops::RangeInclusive<f32> = 0.01..=40.0;
/// Gains a band may have (dB).
pub const GAIN_RANGE: std::ops::RangeInclusive<f32> = -30.0..=30.0;

impl Filter {
    /// The band inside [`FREQ_RANGE`], [`Q_RANGE`] and [`GAIN_RANGE`]: what every consumer
    /// ([`coefficients`], so the response, the preamp and [`Cascade`]; [`mpv_filter`]; the
    /// importer) uses. Clamp through here, never with literals at one consumer.
    pub fn clamped(&self) -> Filter {
        let clamp = |v: f32, r: &std::ops::RangeInclusive<f32>| v.clamp(*r.start(), *r.end());
        Filter {
            kind: self.kind,
            freq: clamp(self.freq, &FREQ_RANGE),
            gain: clamp(self.gain, &GAIN_RANGE),
            q: clamp(self.q, &Q_RANGE),
        }
    }
}

/// A headphone correction: the profile's name, where it comes from and its bands.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub name: String,
    /// Measurement source ("oratory1990", "crinacle", ... or "imported").
    pub source: String,
    /// Preamp in dB (negative: room for the largest boost).
    pub preamp: f32,
    pub filters: Vec<Filter>,
}

/// Normalised biquad coefficients `[b0, b1, b2, a1, a2]` (a0 = 1) at sample rate `sr`, of the band
/// as [`Filter::clamped`] limits it (and below Nyquist).
pub fn coefficients(f: &Filter, sr: f64) -> [f64; 5] {
    let f = &f.clamped();
    let a = 10f64.powf(f64::from(f.gain) / 40.0);
    let w0 = 2.0 * PI * (f64::from(f.freq) / sr).clamp(1e-6, 0.499);
    let (sin, cos) = w0.sin_cos();
    let alpha = sin / (2.0 * f64::from(f.q));
    let (b0, b1, b2, a0, a1, a2) = match f.kind {
        FilterKind::Peak => {
            (1.0 + alpha * a, -2.0 * cos, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * cos, 1.0 - alpha / a)
        }
        FilterKind::LowShelf => {
            let s = 2.0 * a.sqrt() * alpha;
            (
                a * ((a + 1.0) - (a - 1.0) * cos + s),
                2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
                a * ((a + 1.0) - (a - 1.0) * cos - s),
                (a + 1.0) + (a - 1.0) * cos + s,
                -2.0 * ((a - 1.0) + (a + 1.0) * cos),
                (a + 1.0) + (a - 1.0) * cos - s,
            )
        }
        FilterKind::HighShelf => {
            let s = 2.0 * a.sqrt() * alpha;
            (
                a * ((a + 1.0) + (a - 1.0) * cos + s),
                -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                a * ((a + 1.0) + (a - 1.0) * cos - s),
                (a + 1.0) - (a - 1.0) * cos + s,
                2.0 * ((a - 1.0) - (a + 1.0) * cos),
                (a + 1.0) - (a - 1.0) * cos - s,
            )
        }
    };
    [b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0]
}

/// Magnitude of one biquad at `freq` in dB.
fn biquad_db(c: &[f64; 5], freq: f64, sr: f64) -> f64 {
    let w = 2.0 * PI * freq / sr;
    let (z1r, z1i) = (w.cos(), -w.sin());
    let (z2r, z2i) = ((2.0 * w).cos(), -(2.0 * w).sin());
    let nr = c[0] + c[1] * z1r + c[2] * z2r;
    let ni = c[1] * z1i + c[2] * z2i;
    let dr = 1.0 + c[3] * z1r + c[4] * z2r;
    let di = c[3] * z1i + c[4] * z2i;
    10.0 * ((nr * nr + ni * ni) / (dr * dr + di * di)).log10()
}

/// Largest value of `f` between `lo` and `hi` (golden-section search on a log-frequency axis;
/// `f` is taken as having one peak in the bracket, which holds between neighbours of a sample grid).
fn peak_between(f: &impl Fn(f64) -> f64, lo: f64, hi: f64) -> f64 {
    const INV_PHI: f64 = 0.618_033_988_749_894_9;
    let (mut a, mut b) = (lo.ln(), hi.ln());
    let mut c = b - INV_PHI * (b - a);
    let mut d = a + INV_PHI * (b - a);
    let (mut fc, mut fd) = (f(c.exp()), f(d.exp()));
    let mut best = f(lo).max(f(hi)).max(fc).max(fd);
    for _ in 0..60 {
        if fc > fd {
            b = d;
            (d, fd) = (c, fc);
            c = b - INV_PHI * (b - a);
            fc = f(c.exp());
            best = best.max(fc);
        } else {
            a = c;
            (c, fc) = (d, fd);
            d = a + INV_PHI * (b - a);
            fd = f(d.exp());
            best = best.max(fd);
        }
    }
    best
}

impl Profile {
    /// The profile's response at `freq` in dB, preamp included.
    pub fn response_db(&self, freq: f64, sr: f64) -> f64 {
        let bands: f64 = self.filters.iter().map(|f| biquad_db(&coefficients(f, sr), freq, sr)).sum();
        f64::from(self.preamp) + bands
    }

    /// Largest boost of the bands alone (dB) from 20 Hz to 20 kHz (at most 95 % of Nyquist): the
    /// true peak, not a grid sample. A 1/24-octave grid plus every band's own frequency is sampled,
    /// then each local maximum is refined between its neighbours, so a narrow (high-Q) band that
    /// falls between grid points is still measured at its full height.
    pub fn max_boost_db(&self, sr: f64) -> f64 {
        let top = (sr / 2.0 * 0.95).clamp(20.0, 20_000.0);
        let boost = |f: f64| self.response_db(f, sr) - f64::from(self.preamp);
        let centres = self
            .filters
            .iter()
            .map(|f| f64::from(f.freq))
            .filter(|f| f.is_finite())
            .map(|f| f.clamp(20.0, top));
        let mut fs: Vec<f64> = (0..=240)
            .map(|i| 20.0 * 2f64.powf(f64::from(i) / 24.0))
            .take_while(|&f| f <= top)
            .chain(centres)
            .chain([top])
            .collect();
        fs.sort_by(f64::total_cmp);
        fs.dedup();
        let db: Vec<f64> = fs.iter().map(|&f| boost(f)).collect();
        let mut best = db.iter().copied().fold(0.0, f64::max);
        for i in 0..fs.len() {
            let left = if i > 0 { db[i - 1] } else { f64::NEG_INFINITY };
            let right = db.get(i + 1).copied().unwrap_or(f64::NEG_INFINITY);
            if db[i] > 0.0 && db[i] >= left && db[i] >= right {
                let (lo, hi) = (fs[i.saturating_sub(1)], fs[(i + 1).min(fs.len() - 1)]);
                best = best.max(peak_between(&boost, lo, hi));
            }
        }
        best
    }

    /// The preamp actually used: the profile's (limited to -30..0 dB), lowered further when the
    /// bands boost more than it leaves room for. No frequency from 20 Hz to 20 kHz comes out louder
    /// than it went in, however narrow or large the boost, so the correction itself never clips.
    pub fn safe_preamp(&self, sr: f64) -> f32 {
        let need = -(self.max_boost_db(sr) as f32);
        self.preamp.clamp(-30.0, 0.0).min(need)
    }

    /// The same profile with [`Self::safe_preamp`] as its preamp.
    pub fn with_safe_preamp(&self, sr: f64) -> Profile {
        Profile { preamp: self.safe_preamp(sr), ..self.clone() }
    }
}

/// The key a profile is bound under: the device id without its audio-output prefix, so mpv's
/// `pipewire/bluez_output.X` and `pulse/bluez_output.X` (or a sink name from the system) are one
/// device. `auto` stays `auto` (the system default when it can't be resolved).
pub fn device_key(id: &str) -> &str {
    let id = id.trim();
    match id.split_once('/') {
        Some((ao, rest)) if !ao.is_empty() && !rest.is_empty() && !ao.contains(['.', ' ']) => rest,
        _ => id,
    }
}

/// mpv `af` label of the headphone-correction stage.
pub const MPV_LABEL: &str = "hpeq";

/// The profile as one mpv stage: preamp `volume`, then one biquad per band (64-bit precision).
/// The preamp is [`Profile::safe_preamp`] at 48 kHz. Empty for a profile without bands.
pub fn mpv_filter(p: &Profile) -> String {
    if p.filters.is_empty() {
        return String::new();
    }
    let mut parts = vec![format!("volume={:.2}dB", p.safe_preamp(48_000.0))];
    for f in p.filters.iter().map(Filter::clamped) {
        let name = match f.kind {
            FilterKind::Peak => "equalizer",
            FilterKind::LowShelf => "lowshelf",
            FilterKind::HighShelf => "highshelf",
        };
        parts.push(format!("{name}=f={:.2}:t=q:w={:.4}:g={:.2}:r=f64", f.freq, f.q, f.gain));
    }
    format!("@{MPV_LABEL}:lavfi=[{}]", parts.join(","))
}

/// What [`parse_parametric_eq_report`] read from an AutoEq or Equalizer APO text.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ParametricEqImport {
    /// The sum of every `Preamp:` line in dB (Equalizer APO adds them up).
    pub preamp: f32,
    pub filters: Vec<Filter>,
    /// The lines that ask for something this profile can't express (other filter types, a peak
    /// without Q or bandwidth, channel or device sections, graphic EQ ...), trimmed, in order.
    /// Callers show their number so an import never changes the sound without a word.
    pub skipped: Vec<String>,
}

/// Reads AutoEq's `ParametricEQ.txt` and the peak/shelf subset of Equalizer APO's format:
/// `Preamp: -6.3 dB` and `Filter N: ON PK Fc 105 Hz Gain 5.6 dB Q 0.70` lines (the number may be
/// left out: `Filter: ON ...`). Peaks are `PK`, `PEQ` or `Modal` and need `Q q` or `BW Oct n`;
/// shelves are `LSC`/`LS` and `HSC`/`HS` with `Q q`, a slope `LSC 12 dB`, or neither (Q 0.7, as
/// AutoEq writes them). Disabled (`OFF`) filters, comments and lines that are not
/// `Command: parameters` are ignored, as APO ignores them; anything else is listed in `skipped`.
pub fn parse_parametric_eq_report(text: &str) -> ParametricEqImport {
    let mut out = ParametricEqImport::default();
    for line in text.lines().map(str::trim) {
        let words: Vec<&str> = line.split_whitespace().collect();
        let Some(&command) = words.first() else { continue };
        if command.starts_with('#') || !line.contains(':') {
            continue;
        }
        let command = command.trim_end_matches(':');
        if command.eq_ignore_ascii_case("preamp") {
            match words.get(1).and_then(|v| v.parse::<f32>().ok()).filter(|v| v.is_finite()) {
                Some(v) => out.preamp += v,
                None => out.skipped.push(line.to_string()),
            }
            continue;
        }
        if !command.eq_ignore_ascii_case("filter") {
            // `Channel: all` and `Device: all` change nothing; every other command does.
            let all = words.len() == 2 && words[1].eq_ignore_ascii_case("all");
            if !(all && (command.eq_ignore_ascii_case("channel") || command.eq_ignore_ascii_case("device"))) {
                out.skipped.push(line.to_string());
            }
            continue;
        }
        if words.iter().any(|w| w.eq_ignore_ascii_case("off")) {
            continue;
        }
        match apo_filter(&words) {
            Some(f) => out.filters.push(f),
            None => out.skipped.push(line.to_string()),
        }
    }
    out
}

/// One enabled APO `Filter` line as a band; `None` when this profile can't express it.
fn apo_filter(words: &[&str]) -> Option<Filter> {
    let on = words.iter().position(|w| w.eq_ignore_ascii_case("on"))?;
    let words = &words[on + 1..];
    let number = |i: usize| words.get(i)?.parse::<f32>().ok().filter(|v| v.is_finite());
    let after = |key: &str| number(words.iter().position(|w| w.eq_ignore_ascii_case(key))? + 1);
    let kind = match words.first()?.to_ascii_uppercase().as_str() {
        "PK" | "PEQ" | "MODAL" => FilterKind::Peak,
        "LSC" | "LS" => FilterKind::LowShelf,
        "HSC" | "HS" => FilterKind::HighShelf,
        // LP, HP, BP, NO, AP, IIR, ...: no peak or shelf.
        _ => return None,
    };
    let (freq, gain) = (after("fc")?, after("gain")?);
    // `BW Oct n`: the bandwidth in octaves.
    let bw_q = words
        .windows(3)
        .find(|w| w[0].eq_ignore_ascii_case("bw") && w[1].eq_ignore_ascii_case("oct"))
        .and_then(|w| w[2].parse::<f64>().ok())
        .filter(|n| n.is_finite() && *n > 0.0)
        .map(|n| (2f64.powf(n).sqrt() / (2f64.powf(n) - 1.0)) as f32);
    let q = match kind {
        FilterKind::Peak => after("q").or(bw_q)?,
        FilterKind::LowShelf | FilterKind::HighShelf => {
            // `LS 6dB` / `HS 12dB` are first/second-order shelves at a corner frequency: another filter.
            if words.get(1).is_some_and(|w| w.len() > 2 && w.to_ascii_lowercase().ends_with("db")) {
                return None;
            }
            // `LSC x dB`: a slope of x dB per octave (RBJ's S = x / 12).
            let slope = number(1).filter(|_| words.get(2).is_some_and(|w| w.eq_ignore_ascii_case("db")));
            match (after("q"), slope) {
                (Some(q), _) => q,
                (None, Some(x)) if x > 0.0 => shelf_q(gain, f64::from(x) / 12.0)?,
                (None, Some(_)) => return None,
                // AutoEq's value, about RBJ's steepest slope without overshoot (S = 1, Q 0.707).
                (None, None) => 0.7,
            }
        }
    };
    (freq > 0.0 && q > 0.0 && q.is_finite()).then_some(Filter { kind, freq, gain, q }.clamped())
}

/// The Q of a shelf with RBJ slope `s` (1 = 12 dB per octave) at `gain` dB; `None` when the slope
/// is steeper than a biquad shelf allows at that gain.
fn shelf_q(gain: f32, s: f64) -> Option<f32> {
    let a = 10f64.powf(f64::from(gain) / 40.0);
    let inv_q2 = (a + 1.0 / a) * (1.0 / s - 1.0) + 2.0;
    (inv_q2 > 0.0).then(|| (1.0 / inv_q2.sqrt()) as f32)
}

/// [`parse_parametric_eq_report`] as `(preamp, filters)`; `None` when there is no enabled filter.
/// It does not say which lines were left out: prefer the report where the user can be told.
pub fn parse_parametric_eq(text: &str) -> Option<(f32, Vec<Filter>)> {
    let r = parse_parametric_eq_report(text);
    (!r.filters.is_empty()).then_some((r.preamp, r.filters))
}

/// One profile of the bundled AutoEq index.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub name: String,
    pub source: String,
    /// "over-ear", "in-ear" or "earbud".
    pub kind: String,
    pub profile: Profile,
}

fn parse_entry(line: &str) -> Option<Entry> {
    let mut cols = line.split('\t');
    let (name, source, kind, preamp, bands) =
        (cols.next()?, cols.next()?, cols.next()?, cols.next()?, cols.next()?);
    let filters = bands
        .split(';')
        .map(|b| {
            let v: Vec<&str> = b.split(' ').collect();
            let kind = match *v.first()? {
                "P" => FilterKind::Peak,
                "L" => FilterKind::LowShelf,
                "H" => FilterKind::HighShelf,
                _ => return None,
            };
            Some(Filter {
                kind,
                freq: v.get(1)?.parse().ok()?,
                gain: v.get(2)?.parse().ok()?,
                q: v.get(3)?.parse().ok()?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(Entry {
        name: name.to_string(),
        source: source.to_string(),
        kind: kind.to_string(),
        profile: Profile {
            name: name.to_string(),
            source: source.to_string(),
            preamp: preamp.parse().ok()?,
            filters,
        },
    })
}

/// The bundled AutoEq index (about 6000 headphones, one profile per model; oratory1990 first,
/// then crinacle, then the other sources). Decompressed on first use. The data is MIT-licensed
/// (`data/AUTOEQ-LICENSE`); every bundled data file must keep its notice in the repository's `NOTICE.md`.
pub fn index() -> &'static [Entry] {
    static INDEX: OnceLock<Vec<Entry>> = OnceLock::new();
    INDEX.get_or_init(|| {
        let gz: &[u8] = include_bytes!("../data/autoeq.tsv.gz");
        let mut text = String::new();
        if flate2::read::GzDecoder::new(gz).read_to_string(&mut text).is_err() {
            return Vec::new();
        }
        text.lines().filter(|l| !l.starts_with('#')).filter_map(parse_entry).collect()
    })
}

fn words(s: &str) -> Vec<String> {
    s.split(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == ',' || c == '/')
        .map(fold)
        .filter(|w| !w.is_empty())
        .collect()
}

/// Index entries whose name contains every word of `query` (hyphens and case ignored), best
/// first: whole-name matches, then names starting with the query, then shorter names.
pub fn search(query: &str, limit: usize) -> Vec<&'static Entry> {
    let q = words(query);
    if q.is_empty() {
        return Vec::new();
    }
    let whole = fold(query);
    let mut hits: Vec<(u8, usize, &Entry)> = index()
        .iter()
        .filter_map(|e| {
            let name = fold(&e.name);
            if !q.iter().all(|w| name.contains(w.as_str())) {
                return None;
            }
            let rank = if name == whole {
                0
            } else if name.starts_with(&whole) || words(&e.name).iter().skip(1).any(|w| w.starts_with(&q[0]))
            {
                1
            } else {
                2
            };
            Some((rank, e.name.len(), e))
        })
        .collect();
    hits.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)).then_with(|| a.2.name.cmp(&b.2.name)));
    hits.into_iter().take(limit).map(|h| h.2).collect()
}

/// The profile a device name most likely means ("WH-1000XM5", "Galaxy Buds2 Pro", "Sony
/// WH-1000XM5 Stereo"): the entry whose model words (the name without the brand, variants in
/// parentheses ignored) all appear in the device name, the longest such model. `None` when no
/// model of at least 4 characters matches.
pub fn suggest(device_name: &str) -> Option<&'static Entry> {
    let label = fold(device_name);
    if label.len() < 3 {
        return None;
    }
    index()
        .iter()
        .filter(|e| !e.name.contains('('))
        .filter_map(|e| {
            let model: Vec<String> = words(&e.name).into_iter().skip(1).collect();
            let len: usize = model.iter().map(String::len).sum();
            let distinctive = model.iter().any(|w| w.len() >= 4 && w.chars().any(|c| c.is_ascii_digit()))
                || model.iter().map(String::len).max().unwrap_or(0) >= 6;
            (len >= 4 && distinctive && model.iter().all(|w| label.contains(w.as_str()))).then_some((len, e))
        })
        .max_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.name.len().cmp(&a.1.name.len())))
        .map(|(_, e)| e)
}

/// A running biquad cascade over interleaved stereo `f32` (Android's mixer stage). Transposed
/// direct form II in `f64`; the preamp is applied as a linear gain first.
#[derive(Debug, Clone)]
pub struct Cascade {
    gain: f64,
    coeffs: Vec<[f64; 5]>,
    /// Per band and channel: the two state values.
    state: Vec<[[f64; 2]; 2]>,
}

impl Cascade {
    /// The profile at `sr` with [`Profile::safe_preamp`].
    pub fn new(p: &Profile, sr: f64) -> Self {
        let coeffs: Vec<[f64; 5]> = p.filters.iter().map(|f| coefficients(f, sr)).collect();
        let state = vec![[[0.0; 2]; 2]; coeffs.len()];
        Cascade { gain: 10f64.powf(f64::from(p.safe_preamp(sr)) / 20.0), coeffs, state }
    }

    pub fn is_empty(&self) -> bool {
        self.coeffs.is_empty()
    }

    /// Filters interleaved stereo samples in place.
    pub fn process(&mut self, buf: &mut [f32]) {
        if self.coeffs.is_empty() {
            return;
        }
        for frame in buf.as_chunks_mut::<2>().0 {
            for (ch, s) in frame.iter_mut().enumerate() {
                let mut x = f64::from(*s) * self.gain;
                for (c, st) in self.coeffs.iter().zip(self.state.iter_mut()) {
                    let z = &mut st[ch];
                    let y = c[0] * x + z[0];
                    z[0] = c[1] * x - c[3] * y + z[1];
                    z[1] = c[2] * x - c[4] * y;
                    x = y;
                }
                *s = x as f32;
            }
        }
    }

    /// Clears the filter state (after a seek or a new stream).
    pub fn reset(&mut self) {
        self.state.iter_mut().for_each(|s| *s = [[0.0; 2]; 2]);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn bundled_data_is_listed_in_notice() {
        let dir = env!("CARGO_MANIFEST_DIR");
        let notice =
            std::fs::read_to_string(format!("{dir}/../../NOTICE.md")).expect("NOTICE.md at the repo root");
        for entry in std::fs::read_dir(format!("{dir}/data")).unwrap() {
            let name = entry.unwrap().file_name().into_string().unwrap();
            let path = format!("crates/listen-core/data/{name}");
            assert!(notice.contains(&path), "NOTICE.md must name the bundled {path} (with its licence)");
        }
    }

    use super::*;

    const VERITE: &str = "Preamp: -6.3 dB
Filter 1: ON LSC Fc 105 Hz Gain 5.6 dB Q 0.70
Filter 2: ON PK Fc 170 Hz Gain -3.9 dB Q 0.18
Filter 3: ON PK Fc 6906 Hz Gain 8.9 dB Q 0.44
Filter 4: ON PK Fc 3388 Hz Gain 4.0 dB Q 2.96
Filter 5: ON PK Fc 5593 Hz Gain -12.7 dB Q 1.81
Filter 6: ON HSC Fc 10000 Hz Gain -4.3 dB Q 0.70
Filter 7: OFF PK Fc 1422 Hz Gain 1.0 dB Q 2.38
";

    fn verite() -> Profile {
        let (preamp, filters) = parse_parametric_eq(VERITE).unwrap();
        Profile { name: "ZMF Verite".into(), source: "test".into(), preamp, filters }
    }

    #[test]
    fn parses_autoeq_text() {
        let p = verite();
        assert_eq!(p.preamp, -6.3);
        assert_eq!(p.filters.len(), 6, "the OFF filter is skipped");
        assert_eq!(p.filters[0], Filter { kind: FilterKind::LowShelf, freq: 105.0, gain: 5.6, q: 0.7 });
        assert_eq!(p.filters[5].kind, FilterKind::HighShelf);
        assert!(parse_parametric_eq("Preamp: -3 dB\n").is_none());
    }

    #[test]
    fn apo_unnumbered_filter_and_bandwidth_lines_parse() {
        let (_, f) = parse_parametric_eq("Filter: ON PK Fc 1000 Hz Gain 3 dB BW Oct 1").unwrap();
        assert_eq!(f.len(), 1);
        assert!((f[0].q - 2f32.sqrt()).abs() < 1e-4, "{}", f[0].q);
        // A peak without Q or bandwidth is not given a shelf's Q: it is left out and reported.
        assert_eq!(parse_parametric_eq("Filter 1: ON PK Fc 1000 Hz Gain 3 dB"), None);
        let r = parse_parametric_eq_report(
            "Filter 1: ON PK Fc 1000 Hz Gain 3 dB\nFilter 2: ON PEQ Fc 100 Hz Gain 1.0 dB BW Oct 0.167",
        );
        assert_eq!(r.skipped, ["Filter 1: ON PK Fc 1000 Hz Gain 3 dB"]);
        assert!((r.filters[0].q - 8.64).abs() < 0.01, "{}", r.filters[0].q);
        // Several preamps add up, as in APO.
        let r =
            parse_parametric_eq_report("Preamp: -3 dB\nPreamp: -2.5 dB\nFilter: ON LS Fc 300 Hz Gain 5.0 dB");
        assert_eq!(r.preamp, -5.5);
        assert_eq!(r.filters, [Filter { kind: FilterKind::LowShelf, freq: 300.0, gain: 5.0, q: 0.7 }]);
        // A slope of 12 dB per octave is RBJ's S = 1: Q 1/sqrt(2) at any gain.
        let r = parse_parametric_eq_report("Filter: ON HSC 12 dB Fc 100 Hz Gain -6.0 dB");
        assert!((r.filters[0].q - 0.5f32.sqrt()).abs() < 1e-4, "{:?}", r.filters);
    }

    #[test]
    fn unsupported_apo_lines_are_reported_not_misread() {
        let text = "# comment
Device: all
Channel: all
Preamp: -6 dB
Filter 1: ON PK Fc 50 Hz Gain -3.0 dB Q 10.00
Filter 2: OFF PK Fc 80 Hz Gain 2.0 dB Q 1.00
Filter 3: ON HP Fc 30 Hz
Filter 4: ON LS 6dB Fc 50.0 Hz Gain 7.2 dB
Channel: L
GraphicEQ: 25 6; 40 4.5
plain words are ignored
";
        let r = parse_parametric_eq_report(text);
        assert_eq!(r.preamp, -6.0);
        assert_eq!(r.filters, [Filter { kind: FilterKind::Peak, freq: 50.0, gain: -3.0, q: 10.0 }]);
        assert_eq!(
            r.skipped,
            [
                "Filter 3: ON HP Fc 30 Hz",
                "Filter 4: ON LS 6dB Fc 50.0 Hz Gain 7.2 dB",
                "Channel: L",
                "GraphicEQ: 25 6; 40 4.5"
            ]
        );
        assert!(parse_parametric_eq_report(VERITE).skipped.is_empty());
        assert!(parse_parametric_eq_report("hd 600").skipped.is_empty(), "a search is not an import");
    }

    #[test]
    fn single_bands_have_their_gain_where_they_should() {
        let sr = 48_000.0;
        let pk = Filter { kind: FilterKind::Peak, freq: 1000.0, gain: 6.0, q: 1.0 };
        assert!((biquad_db(&coefficients(&pk, sr), 1000.0, sr) - 6.0).abs() < 1e-6);
        assert!(biquad_db(&coefficients(&pk, sr), 50.0, sr).abs() < 0.1);
        let ls = Filter { kind: FilterKind::LowShelf, freq: 200.0, gain: -4.0, q: 0.7 };
        assert!((biquad_db(&coefficients(&ls, sr), 20.0, sr) + 4.0).abs() < 0.05);
        assert!(biquad_db(&coefficients(&ls, sr), 10_000.0, sr).abs() < 0.05);
        assert!((biquad_db(&coefficients(&ls, sr), 200.0, sr) + 2.0).abs() < 0.05, "half the gain at fc");
        let hs = Filter { kind: FilterKind::HighShelf, freq: 8000.0, gain: 5.0, q: 0.7 };
        assert!((biquad_db(&coefficients(&hs, sr), 20_000.0, sr) - 5.0).abs() < 0.3);
        assert!(biquad_db(&coefficients(&hs, sr), 100.0, sr).abs() < 0.05);
    }

    /// The response peak on a grid 20 times finer than the one `max_boost_db` samples, plus
    /// every band centre: an independent check, not the function's own grid.
    fn dense_peak(p: &Profile, sr: f64) -> f64 {
        (0..=4800)
            .map(|i| 20.0 * 2f64.powf(f64::from(i) / 480.0))
            .take_while(|&f| f <= 20_000.0)
            .chain(p.filters.iter().map(|f| f64::from(f.freq)))
            .map(|f| p.response_db(f, sr))
            .fold(f64::MIN, f64::max)
    }

    #[test]
    fn preamp_leaves_room_for_every_boost() {
        let p = verite();
        let boost = p.max_boost_db(48_000.0);
        assert!(boost > 0.0);
        assert!(f64::from(p.safe_preamp(48_000.0)) <= -boost + 1e-6);
        assert!(p.safe_preamp(48_000.0) <= p.preamp);
        let peak = dense_peak(&p.with_safe_preamp(48_000.0), 48_000.0);
        assert!(peak <= 1e-4, "total response peaks at {peak} dB");
    }

    #[test]
    fn max_boost_sees_a_narrow_band_between_grid_points() {
        // Half-way between two 1/24-octave grid points, Q 40 (mpv's ceiling).
        let freq = (20.0 * 2f64.powf(100.5 / 24.0)) as f32;
        let p = Profile {
            filters: vec![Filter { kind: FilterKind::Peak, freq, gain: 12.0, q: 40.0 }],
            ..Profile::default()
        };
        for sr in [44_100.0, 48_000.0, 96_000.0] {
            assert!(p.max_boost_db(sr) > 11.9, "{sr}: {}", p.max_boost_db(sr));
            let peak = dense_peak(&p.with_safe_preamp(sr), sr);
            assert!(peak <= 1e-4, "{sr}: total response peaks at {peak} dB");
        }
        // Two narrow bands close together: their sum peaks off both centres.
        let p = Profile {
            filters: vec![
                Filter { kind: FilterKind::Peak, freq: 1000.0, gain: 8.0, q: 12.0 },
                Filter { kind: FilterKind::Peak, freq: 1030.0, gain: 8.0, q: 12.0 },
            ],
            ..Profile::default()
        };
        assert!(dense_peak(&p.with_safe_preamp(48_000.0), 48_000.0) <= 1e-4);
    }

    #[test]
    fn the_correction_never_clips_beyond_a_30_db_boost() {
        let band = |freq| Filter { kind: FilterKind::Peak, freq, gain: 20.0, q: 1.0 };
        let p = Profile { filters: vec![band(1000.0), band(1100.0)], ..Profile::default() };
        assert!(p.max_boost_db(48_000.0) > 30.0);
        assert!(dense_peak(&p.with_safe_preamp(48_000.0), 48_000.0) <= 1e-4);
        // The profile's own preamp stays limited to -30..0 dB when the bands need less.
        let quiet = Profile {
            preamp: -50.0,
            filters: vec![Filter { gain: 3.0, ..band(1000.0) }],
            ..Profile::default()
        };
        assert_eq!(quiet.safe_preamp(48_000.0), -30.0);
    }

    #[test]
    fn cascade_matches_the_computed_response() {
        // A sine through the cascade comes out at the computed response.
        let p = verite();
        let sr = 48_000.0;
        for freq in [60.0, 170.0, 1000.0, 3388.0, 5593.0, 12_000.0] {
            let mut c = Cascade::new(&p, sr);
            let n: u32 = 48_000;
            let mut buf: Vec<f32> = (0..n)
                .flat_map(|i| {
                    let v = (0.25 * (2.0 * PI * freq * f64::from(i) / sr).sin()) as f32;
                    [v, v]
                })
                .collect();
            c.process(&mut buf);
            let tail = &buf[n as usize..];
            let rms = (tail.iter().map(|&x| f64::from(x).powi(2)).sum::<f64>() / tail.len() as f64).sqrt();
            let got = 20.0 * (rms / (0.25 / 2f64.sqrt())).log10();
            let want = p.with_safe_preamp(sr).response_db(freq, sr);
            assert!((got - want).abs() < 0.05, "{freq} Hz: {got:.3} vs {want:.3}");
        }
    }

    #[test]
    fn mpv_stage_and_cascade_use_the_same_limits() {
        let wild = Filter { kind: FilterKind::Peak, freq: 30_000.0, gain: 35.0, q: 60.0 };
        let tame = wild.clamped();
        assert_eq!(tame, Filter { kind: FilterKind::Peak, freq: 22_000.0, gain: 30.0, q: 40.0 });
        assert_eq!(coefficients(&wild, 96_000.0), coefficients(&tame, 96_000.0));
        let low = Filter { kind: FilterKind::LowShelf, freq: 1.0, gain: -50.0, q: 0.0001 };
        assert_eq!(coefficients(&low, 48_000.0), coefficients(&low.clamped(), 48_000.0));
        let p = |f| Profile { filters: vec![f], ..Profile::default() };
        assert_eq!(p(wild).safe_preamp(96_000.0), p(tame).safe_preamp(96_000.0));
        let stage = mpv_filter(&p(wild));
        assert!(stage.contains("equalizer=f=22000.00:t=q:w=40.0000:g=30.00:r=f64"), "{stage}");
        // The importer stores what plays.
        let (_, f) = parse_parametric_eq("Filter: ON PK Fc 1000 Hz Gain 35 dB Q 60").unwrap();
        assert_eq!(f[0], Filter { kind: FilterKind::Peak, freq: 1000.0, gain: 30.0, q: 40.0 });
    }

    #[test]
    fn mpv_stage_names_every_band() {
        let f = mpv_filter(&verite());
        assert!(f.starts_with("@hpeq:lavfi=[volume=-"), "{f}");
        assert!(f.contains("lowshelf=f=105.00:t=q:w=0.7000:g=5.60:r=f64"), "{f}");
        assert!(f.contains("highshelf=f=10000.00"));
        assert_eq!(f.matches("equalizer=").count(), 4);
        assert_eq!(mpv_filter(&Profile::default()), "");
    }

    #[test]
    fn index_loads_and_searches() {
        let idx = index();
        assert!(idx.len() > 5000, "{}", idx.len());
        assert!(idx.iter().all(|e| !e.profile.filters.is_empty()));
        let hits = search("wh-1000xm5", 5);
        assert_eq!(hits.first().map(|e| e.name.as_str()), Some("Sony WH-1000XM5"));
        let hits = search("hd 600", 5);
        assert!(hits.iter().any(|e| e.name == "Sennheiser HD 600"), "{hits:?}");
        assert!(search("", 5).is_empty());
        assert!(search("zzzz qqqq", 5).is_empty());
    }

    #[test]
    fn device_keys_drop_the_output_prefix() {
        assert_eq!(device_key("pipewire/bluez_output.AA_BB.1"), "bluez_output.AA_BB.1");
        assert_eq!(device_key("pulse/bluez_output.AA_BB.1"), "bluez_output.AA_BB.1");
        assert_eq!(device_key("bluez_output.AA_BB.1"), "bluez_output.AA_BB.1");
        assert_eq!(device_key("auto"), "auto");
        assert_eq!(device_key("wasapi/{0.0.0.00000000}.{abc}"), "{0.0.0.00000000}.{abc}");
    }

    #[test]
    fn device_names_suggest_their_headphones() {
        let name = |s: &str| suggest(s).map(|e| e.name.clone());
        assert_eq!(name("WH-1000XM5").as_deref(), Some("Sony WH-1000XM5"));
        assert_eq!(name("Galaxy Buds2 Pro").as_deref(), Some("Samsung Galaxy Buds2 Pro"));
        assert_eq!(name("Built-in Audio Analog Stereo"), None);
        assert_eq!(name("HDMI / DisplayPort 1 Output"), None);
        assert_eq!(name("Speaker"), None);
    }
}
