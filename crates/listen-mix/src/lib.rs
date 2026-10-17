//! DJ logic for Listen, kept free of DSP, mpv and async so it is cheap to
//! test and can run anywhere:
//!
//! - [`score`]: how well one song flows into the next (tempo, key, energy,
//!   timbre, mixability, repetition) and a beam search that orders DJ picks.
//! - [`plan`](mod@plan): where and how to mix two analysed songs ([`TransitionPlan`]),
//!   and which queue pairs may be mixed at all ([`mix_eligible`]).
//! - [`render`](mod@render): an offline mono preview of a plan (dev tool for
//!   `listen --check-mix --render`).

pub mod genre;
pub mod plan;
pub mod render;
pub mod score;

pub use plan::{
    Envelope, EqEvent, EqTarget, Kind, PlanCtx, SpeedRamp, TransitionPlan, VoiceBreak, VoiceMode, Which,
    mix_eligible, mixable_source, plan,
};
pub use render::render;
pub use score::{FlowCtx, TimbreStats, Weights, octave_match, order, pair_cost, rank, tempo_cost};

#[cfg(test)]
pub(crate) mod testutil;
