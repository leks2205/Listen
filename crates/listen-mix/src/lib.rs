//! DJ logic for Listen, kept free of DSP, mpv and async so it is cheap to
//! test and can run anywhere:
//!
//! - [`score`]: how well one song flows into the next (tempo, key, energy,
//!   timbre, mixability, repetition) and a beam search that orders DJ picks.
//! - [`sequence`]: "Order for smooth mixing" of a whole playlist or queue ([`order_all`]).
//! - [`plan`](mod@plan): where and how to mix two analysed songs ([`TransitionPlan`]),
//!   and which queue pairs may be mixed at all ([`mix_eligible`]).
//! - [`learn`]: flow weights fitted to the listener's early skips after automatic handovers.
//! - [`pool`]: the scene gate for the DJs' candidate pools (AI DJ, Auto-DJ).
//! - [`pick`]: what automatic fills aim for: tempo, focus, forgotten-favourite slot.
//! - [`lyric`]: synced lyrics as a vocal map and cue flags for the planner.
//! - [`loudness`]: playback and blend gain from measured loudness, the one place a per-song gain is
//!   computed.
//! - [`echo`]: the echo of an echo-out as a sample-level effect (preview and Android mixer).
//! - [`render`](mod@render): an offline mono preview of a plan (dev tool for
//!   `listen --check-mix --render`).
//! - [`verify`]: plays a plan through on the analysis curves and falls back to the next option when
//!   it dips, bumps, clashes vocals or cuts a sung line.
//! - [`bridge`]: a short analysed song between two far-apart DJ picks (scene-gated, both legs
//!   beat-mixed).
//! - [`genre`]: the genre rule for blending over vocals at a song's edges.
//! - [`tempo`]: tempo targets (heart-rate fit) and speed-curve helpers.

pub mod bridge;
pub mod echo;
pub mod genre;
pub mod learn;
pub mod loudness;
pub mod lyric;
pub mod pick;
pub mod plan;
pub mod pool;
pub mod render;
pub mod score;
pub mod sequence;
pub mod tempo;
pub mod verify;

pub use loudness::{GainDb, GainSource, Levelling, RgTag, track_gain};
pub use plan::{
    Envelope, EqEvent, EqTarget, Kind, PlanCtx, SpeedRamp, TransitionPlan, VoiceBreak, VoiceMode, Which,
    mix_eligible, mixable_source, plan,
};
pub use render::render;
pub use score::{FlowCtx, TimbreStats, Weights, octave_match, order, pair_cost, rank, tempo_cost};
pub use sequence::{QueueOrder, order_all, order_queue, path_cost};

#[cfg(test)]
pub(crate) mod testutil;

#[cfg(test)]
mod tests {
    #[test]
    fn every_public_module_is_listed_in_the_crate_header() {
        let src = include_str!("lib.rs");
        let header: String = src.lines().filter(|l| l.starts_with("//!")).collect::<Vec<_>>().join("\n");
        for name in src.lines().filter_map(|l| l.strip_prefix("pub mod ")?.strip_suffix(';')) {
            let listed = header.contains(&format!("[`{name}`]"));
            assert!(listed, "pub mod {name} has no line in the crate header of lib.rs");
        }
    }
}
