//! AI DJ: picks the next set of 2–5 songs that fit a seed song's mood from a library candidate
//! pool and writes a radio-style line for the break, one [`gemini::Gemini::plan`] call per set;
//! [`gemini::Gemini::speak`] turns the line into a WAV clip (Gemini TTS).

pub mod gemini;
pub mod persona;
pub mod prompt;
pub mod quota;
pub mod wav;

pub use gemini::{DjError, Gemini, Usage};
pub use persona::{PERSONAS, Persona};
pub use prompt::{BreakMode, Plan, PlanRequest, Played, Show};

/// Default text model (free tier: ~500 requests a day).
pub const TEXT_MODEL: &str = "gemini-3.5-flash-lite";
/// Default speech model; `gemini-3.8-flash-tts` is the higher-quality alternative.
pub const TTS_MODEL: &str = "gemini-3.8-flash-lite-tts";
pub const TTS_MODELS: [&str; 2] = ["gemini-3.8-flash-lite-tts", "gemini-3.8-flash-tts"];
