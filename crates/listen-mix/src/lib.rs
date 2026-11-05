//! DJ logic for Listen, kept free of DSP, mpv and async so it is cheap to
//! test and can run anywhere:
//!
//! - [`score`]: how well one song flows into the next (tempo, key, energy,
//!   timbre, mixability, repetition) and a beam search that orders DJ picks.
//! - [`sequence`]: "Order for smooth mixing" of a whole playlist or queue ([`order_all`]).
//! - [`plan`](mod@plan): where and how to mix two analysed songs ([`TransitionPlan`]),
//!   and which queue pairs may be mixed at all ([`mix_eligible`]).
//! - [`pick`]: what automatic fills aim for: tempo, focus, forgotten-favourite slot (contract C4).
//! - [`lyric`]: synced lyrics as a vocal map and cue flags for the planner.
//! - [`loudness`]: playback and blend gain from measured loudness (contract C5).
//! - [`echo`]: the echo of an echo-out as a sample-level effect (preview and Android mixer).
//! - [`render`](mod@render): an offline mono preview of a plan (dev tool for
//!   `listen --check-mix --render`).
//! - [`verify`]: plays a plan through on the analysis curves and falls back to the next option when
//!   it dips, bumps, clashes vocals or cuts a sung line.

pub mod bridge;
pub mod echo;
pub mod genre;
pub mod loudness;
pub mod lyric;
pub mod pick;
pub mod plan;
pub mod render;
pub mod score;
pub mod sequence;
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
