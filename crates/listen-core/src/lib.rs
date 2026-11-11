//! Backend-independent core of Listen, shared by the desktop app and the phone (no UI, no network code).
//! Look here before writing a helper another app already needs:
//!
//! - `model`, `queue`, `settings` (with Feishin import), `config`, `platform`, `theme`: library types,
//!   the play queue, settings, configuration files, helper programs per OS and theme palettes;
//! - `history`, `memory` (ACT-R activation: forgotten favourites, worn-out songs), `exclude`
//!   (snooze, not interested), `weekly`: the listening history and what it says;
//! - `versions`, `substitute`, `text`: song identity (one key for every version, the same recording
//!   on another source, name folding);
//! - `scene`: the scene and era gate every song a DJ adds must pass;
//! - `analysis`, `describe`: the audio-analysis and describe-it record types;
//! - `peq`: parametric EQ and headphone correction;
//! - `releases`: the release radar.

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

#[cfg(test)]
mod tests {
    /// The crate header is the map contributors and the porting inventory read: every module is on it.
    #[test]
    fn every_module_is_named_in_the_crate_header() {
        let src = include_str!("lib.rs");
        let header: String = src.lines().filter(|l| l.starts_with("//!")).collect::<Vec<_>>().join("\n");
        for m in src.lines().filter_map(|l| l.strip_prefix("pub mod ")).map(|l| l.trim_end_matches(';')) {
            assert!(header.contains(&format!("`{m}`")), "lib.rs header does not name `{m}`");
        }
    }
}
