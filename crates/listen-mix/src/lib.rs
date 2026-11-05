//! DJ logic for Listen, kept free of DSP, mpv and async so it is cheap to
//! test and can run anywhere:
//!
//! - [`score`]: how well one song flows into the next (tempo, key, energy,
//!   timbre, mixability, repetition) and a beam search that orders DJ picks.
//! - [`sequence`]: "Order for smooth mixing" of a whole playlist or queue ([`order_all`]).
//! - [`plan`](mod@plan): where and how to mix two analysed songs ([`TransitionPlan`]),
//!   and which queue pairs may be mixed at all ([`mix_eligible`]).
//! - [`pick`]: what automatic fills aim for: tempo, focus, forgotten-favourite slot (contract C4).
//! - [`loudness`]: playback and blend gain from measured loudness (contract C5).
//! - [`render`](mod@render): an offline mono preview of a plan (dev tool for
//!   `listen --check-mix --render`).

pub mod bridge;
pub mod genre;
pub mod loudness;
pub mod pick;
pub mod plan;
pub mod render;
pub mod score;
pub mod sequence;

pub use loudness::{GainDb, GainSource, Levelling, RgTag, track_gain};
pub use plan::{
    Envelope, EqEvent, EqTarget, Kind, PlanCtx, SpeedRamp, TransitionPlan, VoiceBreak, VoiceMode, Which,
    mix_eligible, mixable_source, plan,
};
pub use render::render;
pub use score::{FlowCtx, TimbreStats, Weights, octave_match, order, pair_cost, rank, tempo_cost};
pub use sequence::{order_all, path_cost};

#[cfg(test)]
pub(crate) mod testutil;
