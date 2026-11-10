//! Parametric EQ and AutoEq headphone correction (item 18).
//!
//! A [`Profile`] is a preamp plus peaking and shelf biquads (RBJ cookbook, the same formulas
//! AutoEq and ffmpeg's `equalizer`/`lowshelf`/`highshelf` use). The desktop renders it as one mpv
//! `lavfi` stage ([`mpv_filter`]); Android runs [`Cascade`] in its mixer. Profiles come from the
//! bundled AutoEq index ([`index`], [`search`], [`suggest`]) or an imported `ParametricEQ.txt`
//! ([`parse_parametric_eq`]), and are bound to an output device by its id.

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

/// Normalised biquad coefficients `[b0, b1, b2, a1, a2]` (a0 = 1) at sample rate `sr`.
pub fn coefficients(f: &Filter, sr: f64) -> [f64; 5] {
    let a = 10f64.powf(f64::from(f.gain) / 40.0);
    let w0 = 2.0 * PI * (f64::from(f.freq) / sr).clamp(1e-6, 0.499);
    let (sin, cos) = w0.sin_cos();
    let alpha = sin / (2.0 * f64::from(f.q).max(0.01));
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

impl Profile {
    /// The profile's response at `freq` in dB, preamp included.
    pub fn response_db(&self, freq: f64, sr: f64) -> f64 {
        let bands: f64 = self.filters.iter().map(|f| biquad_db(&coefficients(f, sr), freq, sr)).sum();
        f64::from(self.preamp) + bands
    }

    /// Largest boost of the bands alone (dB) on a 1/24-octave grid from 20 Hz to 20 kHz.
    pub fn max_boost_db(&self, sr: f64) -> f64 {
        let top = (sr / 2.0 * 0.95).min(20_000.0);
        (0..=240)
            .map(|i| 20.0 * 2f64.powf(f64::from(i) / 24.0))
            .take_while(|&f| f <= top)
            .map(|f| self.response_db(f, sr) - f64::from(self.preamp))
            .fold(0.0, f64::max)
    }

    /// The preamp actually used: the profile's, lowered further when a band boosts more than it
    /// leaves room for, so the correction itself never clips.
    pub fn safe_preamp(&self, sr: f64) -> f32 {
        let need = -(self.max_boost_db(sr) as f32);
        self.preamp.min(need).clamp(-30.0, 0.0)
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
    for f in &p.filters {
        let name = match f.kind {
            FilterKind::Peak => "equalizer",
            FilterKind::LowShelf => "lowshelf",
            FilterKind::HighShelf => "highshelf",
        };
        parts.push(format!(
            "{name}=f={:.2}:t=q:w={:.4}:g={:.2}:r=f64",
            f.freq.clamp(10.0, 22_000.0),
            f.q.clamp(0.01, 40.0),
            f.gain.clamp(-30.0, 30.0)
        ));
    }
    format!("@{MPV_LABEL}:lavfi=[{}]", parts.join(","))
}

/// Reads AutoEq's `ParametricEQ.txt` (also Equalizer APO's format): `Preamp: -6.3 dB` and
/// `Filter N: ON PK Fc 105 Hz Gain 5.6 dB Q 0.70` lines (`LSC`/`LS`, `HSC`/`HS` for shelves).
/// `None` when there is no enabled filter.
pub fn parse_parametric_eq(text: &str) -> Option<(f32, Vec<Filter>)> {
    let mut preamp = 0.0;
    let mut filters = Vec::new();
    for line in text.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        if words.first().is_some_and(|w| w.eq_ignore_ascii_case("preamp:")) {
            if let Some(v) = words.get(1).and_then(|v| v.parse().ok()) {
                preamp = v;
            }
            continue;
        }
        if !words.first().is_some_and(|w| w.eq_ignore_ascii_case("filter"))
            || !words.iter().any(|w| w.eq_ignore_ascii_case("on"))
        {
            continue;
        }
        let after = |key: &str| {
            let i = words.iter().position(|w| w.eq_ignore_ascii_case(key))?;
            words.get(i + 1)?.parse::<f32>().ok()
        };
        let kind = words.iter().find_map(|w| match w.to_ascii_uppercase().as_str() {
            "PK" | "PEQ" => Some(FilterKind::Peak),
            "LSC" | "LS" => Some(FilterKind::LowShelf),
            "HSC" | "HS" => Some(FilterKind::HighShelf),
            _ => None,
        });
        let (Some(kind), Some(freq), Some(gain)) = (kind, after("fc"), after("gain")) else { continue };
        // Shelves without a Q are AutoEq's 0.7.
        let q = after("q").unwrap_or(0.7);
        if freq.is_finite() && gain.is_finite() && q.is_finite() && freq > 0.0 && q > 0.0 {
            filters.push(Filter { kind, freq, gain, q });
        }
    }
    (!filters.is_empty()).then_some((preamp, filters))
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
/// then crinacle, then the other sources). Decompressed on first use.
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

    #[test]
    fn preamp_leaves_room_for_every_boost() {
        let p = verite();
        let boost = p.max_boost_db(48_000.0);
        assert!(boost > 0.0);
        assert!(f64::from(p.safe_preamp(48_000.0)) <= -boost + 1e-6);
        assert!(p.safe_preamp(48_000.0) <= p.preamp);
        let s = p.with_safe_preamp(48_000.0);
        let peak = (0..=240)
            .map(|i| s.response_db(20.0 * 2f64.powf(f64::from(i) / 24.0), 48_000.0))
            .fold(f64::MIN, f64::max);
        assert!(peak <= 1e-6, "total response peaks at {peak} dB");
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
