//! The render thread: owns a headless OpenGL context and a projectM instance,
//! takes PCM from the audio capture, cycles the presets and hands finished
//! frames (RGBA) to the UI. Nothing here ever runs on the UI thread.

use super::gl::Headless;
use super::presets::{self, Preset};
use listen_core::settings::MilkdropSettings;
use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::Sender;

/// The size frames are rendered at; the widget scales them.
pub const WIDTH: i32 = 960;
pub const HEIGHT: i32 = 540;

/// What the render thread tells the app.
#[derive(Debug, Clone)]
pub enum Out {
    /// A finished picture, top-down RGBA8, `WIDTH` x `HEIGHT`.
    Frame(Vec<u8>),
    /// The preset on screen now.
    Preset(String),
    /// A preset that wouldn't load (name, projectM's message); skipped.
    Failed(String, String),
    /// The folder has no presets.
    NoPresets,
    /// Milkdrop can't run (no library, no OpenGL, ...).
    Error(String),
}

/// What the thread works from. Changed through [`set_config`].
#[derive(Debug, Clone)]
pub struct Config {
    /// The preset folder (textures are looked for in it and in `textures/`).
    pub root: std::path::PathBuf,
    pub presets: Arc<Vec<Preset>>,
    pub settings: MilkdropSettings,
}

impl PartialEq for Config {
    fn eq(&self, other: &Self) -> bool {
        self.root == other.root
            && Arc::ptr_eq(&self.presets, &other.presets)
            && self.settings == other.settings
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Action {
    Next,
    Pick(String),
}

#[derive(Default)]
struct Shared {
    generation: u64,
    config: Option<Config>,
    actions: VecDeque<Action>,
}

static SHARED: Mutex<Shared> = Mutex::new(Shared { generation: 0, config: None, actions: VecDeque::new() });

/// Sets what the running thread (and the next one) works from; unchanged
/// input costs nothing.
pub fn set_config(config: Config) {
    let Ok(mut s) = SHARED.lock() else { return };
    if s.config.as_ref() != Some(&config) {
        s.config = Some(config);
        s.generation += 1;
    }
}

/// Switch to the next preset (by the cycle's rules).
pub fn request_next() {
    if let Ok(mut s) = SHARED.lock() {
        s.actions.push_back(Action::Next);
    }
}

/// Forgets requests nobody has taken yet.
pub fn clear_requests() {
    if let Ok(mut s) = SHARED.lock() {
        s.actions.clear();
    }
}

/// Switch to the preset `name`.
pub fn request_pick(name: String) {
    if let Ok(mut s) = SHARED.lock() {
        s.actions.push_back(Action::Pick(name));
    }
}

/// A running render thread. Dropping it stops the thread.
pub struct Engine {
    stop: Arc<AtomicBool>,
    pcm: SyncSender<Vec<f32>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Engine {
    pub fn start(out: Sender<Out>) -> Engine {
        let stop = Arc::new(AtomicBool::new(false));
        let (pcm, rx) = sync_channel::<Vec<f32>>(256);
        let flag = stop.clone();
        let spawned = std::thread::Builder::new().name("milkdrop".into()).spawn({
            let out = out.clone();
            move || run(rx, out, flag)
        });
        let thread = match spawned {
            Ok(t) => Some(t),
            Err(e) => {
                let _ = out.try_send(Out::Error(format!("could not start the Milkdrop thread: {e}")));
                None
            }
        };
        Engine { stop, pcm, thread }
    }

    /// Stops the thread and waits until its OpenGL context is gone (dropping
    /// only asks it to stop).
    pub fn shutdown(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }

    /// Interleaved stereo samples; dropped when the thread can't keep up.
    pub fn feed(&self, samples: Vec<f32>) {
        let _ = self.pcm.try_send(samples);
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Everything the loop changes.
struct State<'a> {
    pm: super::ffi::ProjectM<'static>,
    out: &'a Sender<Out>,
    config: Option<Config>,
    current: Option<String>,
    shown_at: Instant,
    /// Presets that failed to load (not tried again until the folder changes).
    bad: HashSet<String>,
}

impl State<'_> {
    fn path_of(&self, name: &str) -> Option<std::path::PathBuf> {
        self.config.as_ref()?.presets.iter().find(|p| p.name == name).map(|p| p.path.clone())
    }

    /// Loads `name`; `false` (and the preset marked bad) if projectM refuses it.
    fn show(&mut self, name: &str, smooth: bool) -> bool {
        let Some(path) = self.path_of(name) else { return false };
        match self.pm.load_preset(&path, smooth) {
            Ok(()) => {
                self.current = Some(name.to_string());
                self.shown_at = Instant::now();
                let _ = self.out.try_send(Out::Preset(name.to_string()));
                true
            }
            Err(e) => {
                tracing::warn!("milkdrop: preset {name}: {e}");
                self.bad.insert(name.to_string());
                let _ = self.out.try_send(Out::Failed(name.to_string(), e));
                false
            }
        }
    }

    /// The next preset by the cycle's rules; skips the ones that fail.
    fn advance(&mut self, smooth: bool) {
        for _ in 0..8 {
            let Some(cfg) = &self.config else { return };
            let mut list = presets::candidates(&cfg.presets, &cfg.settings);
            list.retain(|n| !self.bad.contains(n));
            let randomize = cfg.settings.randomize;
            let Some(next) = presets::pick_next(&list, self.current.as_deref(), randomize, |n| {
                rand::random_range(0..n.max(1))
            })
            .cloned() else {
                return;
            };
            if self.show(&next, smooth) {
                return;
            }
        }
    }

    fn apply(&mut self, cfg: Config) {
        let folder_changed = self.config.as_ref().is_none_or(|c| !Arc::ptr_eq(&c.presets, &cfg.presets));
        self.pm.set_soft_cut_duration(f64::from(cfg.settings.blend_time.clamp(0.0, 10.0)));
        self.pm.set_fps(cfg.settings.fps() as i32);
        if self.config.as_ref().is_none_or(|c| c.root != cfg.root) {
            let textures = cfg.root.join("textures");
            self.pm.set_texture_paths(&[cfg.root.as_path(), textures.as_path()]);
        }
        let start = folder_changed || self.current.is_none();
        if folder_changed {
            self.bad.clear();
        }
        let wanted = cfg.settings.current_preset.clone();
        let have_wanted = wanted.as_ref().is_some_and(|w| cfg.presets.iter().any(|p| &p.name == w));
        self.config = Some(cfg);
        if self.config.as_ref().is_some_and(|c| c.presets.is_empty()) {
            self.current = None;
            let _ = self.out.try_send(Out::NoPresets);
        } else if start {
            self.current = None;
            match wanted {
                Some(w) if have_wanted && self.show(&w, false) => {}
                _ => self.advance(false),
            }
        }
    }
}

fn run(pcm: Receiver<Vec<f32>>, out: Sender<Out>, stop: Arc<AtomicBool>) {
    let fail = |msg: String| {
        tracing::warn!("milkdrop: {msg}");
        let _ = out.try_send(Out::Error(msg));
    };
    let api = match super::api() {
        Ok(a) => a,
        Err(e) => return fail(e.clone()),
    };
    let gl = match Headless::new(WIDTH, HEIGHT) {
        Ok(g) => g,
        Err(e) => return fail(e),
    };
    let Some(pm) = super::ffi::ProjectM::new(api, WIDTH as usize, HEIGHT as usize, 30) else {
        return fail("libprojectM could not start (needs OpenGL 3.3)".into());
    };
    let mut st =
        State { pm, out: &out, config: None, current: None, shown_at: Instant::now(), bad: HashSet::new() };
    let mut generation = 0;
    let mut last = Instant::now();
    loop {
        if stop.load(Ordering::Relaxed) || out.is_closed() {
            break;
        }
        // Settings and requests from the app.
        let (new_config, actions) = {
            let Ok(mut s) = SHARED.lock() else { break };
            let cfg = (s.generation != generation).then(|| s.config.clone()).flatten();
            generation = s.generation;
            (cfg, std::mem::take(&mut s.actions))
        };
        if let Some(c) = new_config {
            st.apply(c);
        }
        for a in actions {
            match a {
                Action::Next => st.advance(true),
                Action::Pick(name) => {
                    st.show(&name, true);
                }
            }
        }
        let Some(cfg) = st.config.clone() else {
            std::thread::sleep(Duration::from_millis(50));
            continue;
        };
        let period = Duration::from_secs_f32(1.0 / cfg.settings.fps() as f32);
        if cfg.presets.is_empty() || st.current.is_none() {
            // Nothing to draw (the app shows why).
            match pcm.recv_timeout(Duration::from_millis(100)) {
                Err(RecvTimeoutError::Disconnected) => break,
                _ => continue,
            }
        }
        if cfg.settings.cycle_presets
            && st.shown_at.elapsed() >= Duration::from_secs(u64::from(cfg.settings.cycle_time.clamp(1, 300)))
        {
            st.advance(true);
        }
        gl.bind();
        st.pm.render();
        let frame = gl.read_rgba();
        let _ = out.try_send(Out::Frame(frame));
        last += period;
        let now = Instant::now();
        if last < now {
            last = now; // fell behind: don't try to catch up
        }
        // Wait for the next frame, taking audio as it comes.
        let mut disconnected = false;
        while let Some(rest) = last.checked_duration_since(Instant::now()).filter(|d| !d.is_zero()) {
            match pcm.recv_timeout(rest) {
                Ok(chunk) => st.pm.add_pcm_stereo(&chunk),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }
        if disconnected {
            break;
        }
    }
    drop(st);
    drop(gl);
}
