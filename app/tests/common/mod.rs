//! Helpers shared by the test files in `app/tests/`; include them with `mod common;`.
//!
//! Every Simulator test takes `common::gpu()` and holds the guard while its `Simulator` lives, builds the
//! `Simulator` with `common::iced_settings()`, and writes pictures with `common::save_png()`. Click points
//! that depend on the app's layout come from `common::layout`. A message and the messages its tasks produce
//! are processed with `common::settle()`. `tests/shared_test_helpers.rs` fails when a test file copies one of
//! these instead.
//!
//! Each `tests/*.rs` file is compiled as its own crate, so every test binary gets its own copy of this module
//! (and of the lock). Files use only some of the helpers, hence the `dead_code` allowance.
#![allow(dead_code)]

pub mod layout;

use iced::Task;
use iced_test::simulator::Snapshot;
use listen::app::{Listen, Message};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};

/// One lock per test binary; see [`gpu`].
static GPU: Mutex<()> = Mutex::new(());

/// Hold the returned guard while a `Simulator` lives. The headless renderer creates a Vulkan instance per
/// `Simulator`, and creating and dropping several at once races in the Vulkan loader and now and then crashes
/// the test process. The lock serialises the `Simulator`s of the tests inside one test binary (they run on
/// parallel threads); separate test binaries are separate processes and need no shared lock. A test that
/// panicked while holding the lock does not block the others.
pub fn gpu() -> MutexGuard<'static, ()> {
    GPU.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The settings the app runs with: its bundled fonts, Inter as the default font. Text widths, and with them
/// the layout and the click points of a test, depend on the fonts.
pub fn iced_settings() -> iced::Settings {
    iced::Settings {
        fonts: listen::FONTS.iter().map(|f| std::borrow::Cow::Borrowed(*f)).collect(),
        default_font: listen::app::INTER,
        ..Default::default()
    }
}

/// Sends `m` to `app`, then feeds the produced messages back `rounds` times (the follow-up rounds); messages
/// still pending after that are dropped. `run` drives one task to the messages it produces (each file runs
/// tasks on the runtime its fakes need).
pub fn settle(app: &mut Listen, m: Message, rounds: usize, run: fn(Task<Message>) -> Vec<Message>) {
    let mut pending = run(app.update(m));
    for _ in 0..rounds {
        pending = pending.into_iter().flat_map(|m| run(app.update(m))).collect();
    }
}

/// Writes `snapshot` to `target/snapshots/<name>-<renderer>.png` for people to look at and returns the path
/// without the suffix. This is a render check, not a comparison: `matches_image` writes the file only when none
/// exists, so the old `<name>-*` files are removed first and nothing is compared. A test that promises how a
/// page looks needs its own assertion on widget bounds or text.
pub fn save_png(snapshot: &Snapshot, name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/snapshots");
    std::fs::create_dir_all(&dir).unwrap();
    for entry in std::fs::read_dir(&dir).unwrap().flatten() {
        if entry.file_name().to_string_lossy().starts_with(&format!("{name}-")) {
            std::fs::remove_file(entry.path()).unwrap();
        }
    }
    let path = dir.join(name);
    snapshot.matches_image(&path).expect("png written"); // writes a fresh PNG, compares nothing
    path
}
