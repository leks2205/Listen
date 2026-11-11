//! Synced lyrics in the planner: a [`VocalMap`] (from `listen_lyrics::vocal_map`) is checked against
//! the audio vocal curve and merged into a song's bar curves, with cue flags ([`cue`]) for line ends,
//! vocal entries and the chorus. The planner then reads the merged `bars.vocal` and `bars.cue`.

use listen_core::analysis::{Features, VocalMap, cue};

/// Lyrics whose sung bars correlate less than this with the audio vocal curve are dropped.
pub const MIN_AGREEMENT: f32 = 0.0;
/// Lyrics that fit this much better shifted by up to [`MAX_SHIFT_BARS`] are dropped.
const SHIFT_MARGIN: f32 = 0.25;
const MAX_SHIFT_BARS: isize = 6;
/// A bar sung for at least this fraction counts as sung by the lyrics.
const SUNG_FRACTION: f32 = 0.25;
/// Weight of the audio vocal curve in bars the lyrics call instrumental. The audio curve over-reports
/// (synth leads, chants), and validated lyrics are near ground truth there; ad-libs are the price.
const AUDIO_IN_GAPS: f32 = 0.0;

/// Start and end (file seconds) of every bar of the curves: beat-grid bars, or 2-second pseudo-bars
/// from `audio_start` without a grid. Without curves, the bars up to the audio end.
pub fn bar_spans(f: &Features) -> Vec<(f32, f32)> {
    let mut n = f.bars.vocal.len().max(f.bars.energy.len());
    if n == 0 {
        n = match &f.grid {
            Some(g) => (0..g.bar_count()).take_while(|&b| g.bar_time(b) < f.audio_end).count(),
            None => ((f.audio_end - f.audio_start) / 2.0).max(0.0).ceil() as usize,
        };
    }
    (0..n)
        .map(|b| match &f.grid {
            Some(g) => (g.bar_time(b), g.bar_time(b + 1)),
            None => (f.audio_start + 2.0 * b as f32, f.audio_start + 2.0 * (b + 1) as f32),
        })
        .collect()
}

/// Fraction of each bar the lyrics say is sung.
fn sung_fractions(spans: &[(f32, f32)], map: &VocalMap) -> Vec<f32> {
    spans.iter().map(|&(a, b)| if b > a { map.sung_within(a, b) / (b - a) } else { 0.0 }).collect()
}

/// Pearson correlation of `aud[b]` with `lyr[b + shift]` over the bars both cover.
fn corr(aud: &[f32], lyr: &[f32], shift: isize) -> Option<f32> {
    let pairs: Vec<(f32, f32)> = (0..aud.len())
        .filter_map(|b| {
            let l = b.checked_add_signed(shift)?;
            lyr.get(l).map(|&y| (aud[b], y))
        })
        .collect();
    if pairs.len() < 8 {
        return None;
    }
    let n = pairs.len() as f32;
    let (ma, ml) = (pairs.iter().map(|p| p.0).sum::<f32>() / n, pairs.iter().map(|p| p.1).sum::<f32>() / n);
    let (mut cov, mut va, mut vl) = (0.0f32, 0.0f32, 0.0f32);
    for &(a, l) in &pairs {
        cov += (a - ma) * (l - ml);
        va += (a - ma) * (a - ma);
        vl += (l - ml) * (l - ml);
    }
    (va > 1e-6 && vl > 1e-6).then(|| cov / (va * vl).sqrt())
}

/// Pearson correlation between the lyrics' sung bars and the audio vocal curve, or `None` without an
/// audio curve or when either is flat (a song sung throughout).
pub fn agreement(f: &Features, map: &VocalMap) -> Option<f32> {
    let lyr = sung_fractions(&bar_spans(f), map);
    let aud: Vec<f32> = f.bars.vocal.iter().map(|&v| f32::from(v) / 255.0).collect();
    corr(&aud, &lyr, 0)
}

/// Whether the lyrics fit the audio: not anti-correlated with its vocal curve, and no shift of a few
/// bars fits clearly better (that means an offset or another version). The audio curve alone is too
/// rough for more: in densely sung songs both curves are nearly flat and correlate weakly.
pub fn lyrics_fit(f: &Features, map: &VocalMap) -> bool {
    let lyr = sung_fractions(&bar_spans(f), map);
    let aud: Vec<f32> = f.bars.vocal.iter().map(|&v| f32::from(v) / 255.0).collect();
    let Some(r0) = corr(&aud, &lyr, 0) else { return true };
    let shifted = (1..=MAX_SHIFT_BARS)
        .flat_map(|s| [s, -s])
        .filter_map(|s| corr(&aud, &lyr, s))
        .fold(f32::MIN, f32::max);
    r0 >= MIN_AGREEMENT && shifted < r0 + SHIFT_MARGIN
}

/// `f` with the lyrics merged in: `bars.vocal` follows the lyrics (where they are silent the audio
/// curve counts with `AUDIO_IN_GAPS`, currently 0: validated lyrics win) and `bars.cue` marks line
/// ends, vocal entries and the chorus.
/// `None` when the lyrics run past the audio or disagree with the audio vocal curve.
pub fn with_lyrics(f: &Features, map: &VocalMap) -> Option<Features> {
    let last = map.sung.last()?.1;
    if map.sung[0].0 < 0.0 || last > f.audio_end + 2.0 {
        return None;
    }
    if !lyrics_fit(f, map) {
        return None;
    }
    let spans = bar_spans(f);
    if spans.is_empty() {
        return None;
    }
    let lyr = sung_fractions(&spans, map);
    let mut out = f.clone();
    out.bars.vocal = (0..spans.len())
        .map(|b| {
            let audio = f.bars.vocal.get(b).map_or(0.0, |&v| f32::from(v) / 255.0);
            let l = lyr[b].min(1.0);
            let v = if l >= SUNG_FRACTION { l.max(audio) } else { l.max(AUDIO_IN_GAPS * audio) };
            (v * 255.0).round() as u8
        })
        .collect();
    let mut cues = vec![0u8; spans.len()];
    let bar_of = |t: f32| {
        let b = spans.partition_point(|s| s.0 <= t);
        (b > 0 && t < spans[b - 1].1 + 0.5).then(|| b - 1)
    };
    let mut mark = |t: f32, bit: u8| {
        if let Some(b) = bar_of(t) {
            cues[b] |= bit;
        }
    };
    for &e in &map.line_ends {
        mark(e, cue::LINE_END);
    }
    for &(s, _) in &map.sung {
        mark(s, cue::ENTRY);
    }
    for &(s, e) in &map.chorus {
        mark(s, cue::CHORUS);
        mark(e, cue::CHORUS_END);
    }
    out.bars.cue = cues;
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::FeatBuilder;

    /// 120 bpm: 2 s bars from 0.5 s on (FeatBuilder's grid).
    fn feat() -> Features {
        FeatBuilder::new(120.0, 64).intro(8).outro(8).build()
    }

    #[test]
    fn lyrics_that_match_the_audio_are_merged_with_cues() {
        let f = feat();
        let spans = bar_spans(&f);
        assert_eq!(spans.len(), f.bars.vocal.len());
        let t = |b: usize| spans[b].0;
        // Sung bars 8–24 and 30–56 (the audio says 8–56), a gap 24–30, chorus 40–48.
        let map = VocalMap {
            sung: vec![(t(8), t(24) - 0.3), (t(30), t(56))],
            line_ends: vec![t(12) - 0.2, t(24) - 0.3, t(44), t(56) - 0.1],
            chorus: vec![(t(40), t(48) - 0.2)],
        };
        assert!(agreement(&f, &map).unwrap() > 0.5);
        let m = with_lyrics(&f, &map).unwrap();
        let v = |b: usize| f32::from(m.bars.vocal[b]) / 255.0;
        assert!(v(10) > 0.9 && v(31) > 0.9);
        // The audio said "vocal" in the gap; the lyrics win.
        assert!(f.bars.vocal[26] > 200);
        assert!(v(26) < 0.4, "{}", v(26));
        assert!(v(2) < 0.05);
        let c = &m.bars.cue;
        assert_eq!(c.len(), spans.len());
        assert_ne!(c[11] & cue::LINE_END, 0);
        assert_ne!(c[23] & cue::LINE_END, 0);
        assert_ne!(c[8] & cue::ENTRY, 0);
        assert_ne!(c[30] & cue::ENTRY, 0);
        assert_ne!(c[40] & cue::CHORUS, 0);
        assert_ne!(c[47] & cue::CHORUS_END, 0);
        assert_eq!(c[20], 0);
        // Nothing else changes.
        assert_eq!(m.bars.energy, f.bars.energy);
        assert_eq!(m.grid, f.grid);
    }

    #[test]
    fn lyrics_that_disagree_or_overrun_are_dropped() {
        let f = feat();
        let t = |b: usize| bar_spans(&f)[b].0;
        // Sung only where the audio is instrumental: another version or an offset.
        let wrong = VocalMap { sung: vec![(t(0), t(8)), (t(57), t(63))], line_ends: vec![], chorus: vec![] };
        assert!(agreement(&f, &wrong).unwrap() < 0.0);
        assert!(with_lyrics(&f, &wrong).is_none());
        // The right shape four bars late: an offset.
        let late = VocalMap { sung: vec![(t(12), t(60))], line_ends: vec![], chorus: vec![] };
        assert!(agreement(&f, &late).unwrap() > 0.3);
        assert!(!lyrics_fit(&f, &late));
        let long = VocalMap { sung: vec![(t(8), f.audio_end + 10.0)], line_ends: vec![], chorus: vec![] };
        assert!(with_lyrics(&f, &long).is_none());
        assert!(with_lyrics(&f, &VocalMap::default()).is_none());
    }

    #[test]
    fn without_an_audio_curve_the_lyrics_are_trusted() {
        let mut f = feat();
        let t = |b: usize| bar_spans(&f)[b].0;
        let map = VocalMap { sung: vec![(t(4), t(20))], line_ends: vec![t(20)], chorus: vec![] };
        f.bars.vocal.clear();
        assert!(agreement(&f, &map).is_none());
        let none = Features { bars: Default::default(), ..f.clone() };
        assert_eq!(bar_spans(&none).len(), 64);
        let m = with_lyrics(&f, &map).unwrap();
        assert_eq!(m.bars.vocal.len(), f.bars.energy.len());
        assert_eq!(m.bars.vocal[10], 255);
        assert_eq!(m.bars.vocal[30], 0);
    }
}
