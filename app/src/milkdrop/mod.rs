//! The Milkdrop visualizer: libprojectM 4 loaded at run time and drawn on a
//! headless OpenGL context by a render thread (Linux only).
//!
//! * [`ffi`]: the C API subset, `dlopen`ed (no link-time dependency).
//! * [`presets`]: finding `.milk` files and choosing the next one.
//! * `gl`, `engine` (Linux): the EGL context and the render thread.
//! * `subscription`: audio capture in, frames out, as an iced subscription.

pub mod ffi;
pub mod presets;

#[cfg(target_os = "linux")]
pub mod engine;
#[cfg(target_os = "linux")]
pub mod gl;
#[cfg(target_os = "linux")]
pub mod sub;

#[cfg(target_os = "linux")]
pub use engine::{Config, Out, request_next, request_pick, set_config};

use std::sync::OnceLock;

/// The library, loaded once for the whole run.
pub fn api() -> Result<&'static ffi::Api, &'static String> {
    static API: OnceLock<Result<ffi::Api, String>> = OnceLock::new();
    API.get_or_init(ffi::Api::load).as_ref()
}

/// `Ok` if Milkdrop can run here; else the one-line hint for the settings.
pub fn availability() -> Result<(), String> {
    if !cfg!(target_os = "linux") {
        return Err("Milkdrop is only available on Linux for now".into());
    }
    match api() {
        Ok(_) => Ok(()),
        Err(_) => Err("Milkdrop needs libprojectM 4".into()),
    }
}

/// Why the library didn't load (for the log).
pub fn load_error() -> Option<&'static String> {
    api().err()
}
