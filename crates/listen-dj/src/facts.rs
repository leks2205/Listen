//! Conclusions instead of raw numbers: how a candidate fits the song playing now ("keys compatible",
//! "big tempo jump"). Language models follow a stated conclusion far more reliably than one they have to
//! derive from "124 bpm, 8A" pairs themselves.

use listen_core::analysis::FlowFeatures;

const KEY_CONF: f32 = 0.3;
const BPM_CONF: f32 = 0.35;

/// Tempo change from `a` to `b` in percent, counting half and double time as the same tempo.
fn tempo_gap(a: f32, b: f32) -> f32 {
    [b, b * 2.0, b / 2.0].iter().map(|t| (t / a - 1.0).abs() * 100.0).fold(f32::INFINITY, f32::min)
}

/// "keys compatible, tempo close, energy up" — what is known of how `to` follows `from`; empty when
/// neither has enough analysis.
pub fn fit(from: &FlowFeatures, to: &FlowFeatures) -> String {
    let mut out: Vec<&str> = Vec::new();
    if let (Some((a, ca)), Some((b, cb))) = (from.key, to.key)
        && ca >= KEY_CONF
        && cb >= KEY_CONF
    {
        out.push(match a.distance(b) {
            d if d <= 0.15 => "keys compatible",
            d if d <= 0.35 => "keys close",
            _ => "keys clash",
        });
    }
    if let (Some(a), Some(b)) = (from.bpm, to.bpm)
        && from.bpm_confidence >= BPM_CONF
        && to.bpm_confidence >= BPM_CONF
        && a > 0.0
        && b > 0.0
    {
        out.push(match tempo_gap(a, b) {
            g if g <= 6.0 => "tempo close",
            g if g <= 12.0 => "tempo step",
            _ => "big tempo jump",
        });
    }
    let step = ((to.mood.energy - from.mood.energy) * 10.0).round();
    out.push(match step {
        s if s >= 2.0 => "energy up",
        s if s <= -2.0 => "energy down",
        _ => "same energy",
    });
    out.join(", ")
}

/// A candidate's note for the prompt: its measured tempo/key/energy, then how it fits the song playing
/// now ("124 bpm, 8A, energy 7/10; from now: keys compatible, tempo close, same energy").
pub fn candidate_note(song: &FlowFeatures, now: Option<&FlowFeatures>) -> String {
    match now.map(|n| fit(n, song)).filter(|f| !f.is_empty()) {
        Some(f) => format!("{}; from now: {f}", song.describe()),
        None => song.describe(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use listen_core::analysis::{Camelot, Mood};

    fn f(bpm: f32, key: (u8, bool), energy: f32) -> FlowFeatures {
        FlowFeatures {
            bpm: Some(bpm),
            bpm_confidence: 0.9,
            key: Some((Camelot { number: key.0, minor: key.1 }, 0.8)),
            lufs: -9.0,
            mood: Mood { energy, brightness: 0.5, ..Default::default() },
            timbre: vec![],
            mixable_in: true,
            mixable_out: true,
            vocal: None,
        }
    }

    #[test]
    fn states_conclusions() {
        let now = f(124.0, (8, true), 0.6);
        assert_eq!(fit(&now, &f(126.0, (9, true), 0.65)), "keys compatible, tempo close, same energy");
        assert_eq!(fit(&now, &f(62.5, (8, false), 0.6)), "keys compatible, tempo close, same energy");
        assert_eq!(fit(&now, &f(135.0, (10, true), 0.9)), "keys close, tempo step, energy up");
        assert_eq!(fit(&now, &f(95.0, (2, false), 0.2)), "keys clash, big tempo jump, energy down");
        let mut unsure = f(90.0, (3, true), 0.6);
        unsure.bpm_confidence = 0.1;
        unsure.key = None;
        assert_eq!(fit(&now, &unsure), "same energy");
        assert_eq!(
            candidate_note(&f(126.0, (9, true), 0.65), Some(&now)),
            "126 bpm, 9A, energy 7/10; from now: keys compatible, tempo close, same energy"
        );
        assert_eq!(candidate_note(&now, None), "124 bpm, 8A, energy 6/10");
    }
}
