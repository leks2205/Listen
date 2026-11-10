//! Backend-independent core of Listen: library types, the play queue,
//! settings (with Feishin import), configuration and theme palettes.

pub mod analysis;
pub mod config;
pub mod describe;
pub mod exclude;
pub mod history;
pub mod memory;
pub mod model;
pub mod peq;
pub mod platform;
pub mod queue;
pub mod releases;
pub mod scene;
pub mod settings;
pub mod substitute;
pub mod text;
pub mod theme;
pub mod versions;
pub mod weekly;

/// The `User-Agent` every HTTP client of Listen sends (`Listen/<version>`; all crates share the
/// workspace version). Use this instead of spelling it out per client.
pub const USER_AGENT: &str = concat!("Listen/", env!("CARGO_PKG_VERSION"));
