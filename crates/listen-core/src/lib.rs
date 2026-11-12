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
//! - `files`: saving a whole file atomically (`files::write_atomic`: every save that must not be left
//!   half-written);
//! - `markup`: plain text for notification servers that read markup (every notification sender);
//! - `releases`: the release radar.

pub mod analysis;
pub mod config;
pub mod describe;
pub mod exclude;
pub mod files;
pub mod history;
pub mod markup;
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

/// The `User-Agent` of Listen's own HTTP clients (`Listen/<version>`; all crates share the workspace
/// version). Set it on every reqwest client builder instead of spelling it out
/// (`every_http_client_sends_the_listen_user_agent` in the app checks). Exceptions: MusicBrainz takes
/// [`MUSICBRAINZ_USER_AGENT`]; YouTube requests that must look like a browser or the iOS app send their
/// own (listen-ytmusic `token`, `loudness`); the DLNA proxy forwards the player's headers.
pub const USER_AGENT: &str = concat!("Listen/", env!("CARGO_PKG_VERSION"));

/// The `User-Agent` of MusicBrainz requests: [`USER_AGENT`] plus the contact MusicBrainz asks every
/// client for (the project page). Disc lookups (listen-rip) and credit cards (listen-lyrics) use it.
pub const MUSICBRAINZ_USER_AGENT: &str =
    concat!("Listen/", env!("CARGO_PKG_VERSION"), " ( ", env!("CARGO_PKG_REPOSITORY"), " )");

#[cfg(test)]
mod user_agent_tests {
    use super::{MUSICBRAINZ_USER_AGENT, USER_AGENT};

    #[test]
    fn musicbrainz_requests_name_the_app_and_a_contact() {
        let contact = MUSICBRAINZ_USER_AGENT.strip_prefix(USER_AGENT).unwrap();
        assert!(contact.starts_with(" ( https://") && contact.ends_with(" )"), "{MUSICBRAINZ_USER_AGENT}");
    }
}

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
