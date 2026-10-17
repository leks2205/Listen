//! The small part of libprojectM 4's C API Listen uses, loaded at run time
//! (`dlopen`): there is no link-time dependency, so Listen builds and runs
//! where the library is missing and Milkdrop is simply unavailable.
//! Signatures are from `projectM-4/{core,parameters,audio,callbacks,render_opengl}.h`.

use libloading::Library;
use std::ffi::{CString, c_char, c_void};
use std::path::Path;

pub type Handle = *mut c_void;

/// `projectm_channels`: the samples are interleaved LRLR.
pub const STEREO: u32 = 2;

type FailedCb = extern "C" fn(*const c_char, *const c_char, *mut c_void);

/// Where the library is looked for: the dynamic loader's own search first,
/// then where upstream's `cmake --install` puts it.
pub fn candidates() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    if cfg!(windows) {
        if let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent()
        {
            v.push(dir.join("projectM-4.dll").to_string_lossy().into_owned());
        }
        v.push("projectM-4.dll".into());
    } else {
        v.extend(
            [
                "libprojectM-4.so.4",
                "/usr/local/lib64/libprojectM-4.so.4",
                "/usr/local/lib/libprojectM-4.so.4",
                "/usr/lib64/libprojectM-4.so.4",
            ]
            .map(String::from),
        );
    }
    v
}

/// The loaded library and its entry points. Dropping it unloads the library,
/// so keep it alive as long as any [`ProjectM`] made from it.
pub struct Api {
    create: unsafe extern "C" fn() -> Handle,
    destroy: unsafe extern "C" fn(Handle),
    set_window_size: unsafe extern "C" fn(Handle, usize, usize),
    render: unsafe extern "C" fn(Handle),
    pcm_add_float: unsafe extern "C" fn(Handle, *const f32, u32, u32),
    pcm_max_samples: unsafe extern "C" fn() -> u32,
    load_preset_file: unsafe extern "C" fn(Handle, *const c_char, bool),
    set_preset_duration: unsafe extern "C" fn(Handle, f64),
    set_soft_cut_duration: unsafe extern "C" fn(Handle, f64),
    set_hard_cut_enabled: unsafe extern "C" fn(Handle, bool),
    set_mesh_size: unsafe extern "C" fn(Handle, usize, usize),
    set_fps: unsafe extern "C" fn(Handle, i32),
    set_preset_locked: unsafe extern "C" fn(Handle, bool),
    set_texture_search_paths: unsafe extern "C" fn(Handle, *const *const c_char, usize),
    set_failed_cb: unsafe extern "C" fn(Handle, FailedCb, *mut c_void),
    version: unsafe extern "C" fn(*mut i32, *mut i32, *mut i32),
    _lib: Library,
}

// The function pointers are plain `extern "C"` pointers into the library.
// A `ProjectM` instance is only ever used by one thread.
unsafe impl Send for Api {}
unsafe impl Sync for Api {}

fn sym_name(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes.strip_suffix(&[0]).unwrap_or(bytes)).into_owned()
}

impl Api {
    /// Loads the first library of [`candidates`] that opens.
    pub fn load() -> Result<Api, String> {
        Self::load_from(&candidates())
    }

    pub fn load_from<S: AsRef<str>>(paths: &[S]) -> Result<Api, String> {
        let mut last = String::from("no library path to try");
        for p in paths {
            // SAFETY: loading a shared library runs its initialisers; projectM's are benign.
            match unsafe { Library::new(p.as_ref()) } {
                Ok(lib) => return Self::bind(lib),
                Err(e) => last = format!("{}: {e}", p.as_ref()),
            }
        }
        Err(format!("libprojectM 4 not found ({last})"))
    }

    fn bind(lib: Library) -> Result<Api, String> {
        macro_rules! get {
            ($name:literal) => {
                // SAFETY: see the struct; the field type is the C signature.
                *unsafe { lib.get($name) }
                    .map_err(|e| format!("libprojectM 4 has no {}: {e}", sym_name($name)))?
            };
        }
        Ok(Api {
            create: get!(b"projectm_create\0"),
            destroy: get!(b"projectm_destroy\0"),
            set_window_size: get!(b"projectm_set_window_size\0"),
            render: get!(b"projectm_opengl_render_frame\0"),
            pcm_add_float: get!(b"projectm_pcm_add_float\0"),
            pcm_max_samples: get!(b"projectm_pcm_get_max_samples\0"),
            load_preset_file: get!(b"projectm_load_preset_file\0"),
            set_preset_duration: get!(b"projectm_set_preset_duration\0"),
            set_soft_cut_duration: get!(b"projectm_set_soft_cut_duration\0"),
            set_hard_cut_enabled: get!(b"projectm_set_hard_cut_enabled\0"),
            set_mesh_size: get!(b"projectm_set_mesh_size\0"),
            set_fps: get!(b"projectm_set_fps\0"),
            set_preset_locked: get!(b"projectm_set_preset_locked\0"),
            set_texture_search_paths: get!(b"projectm_set_texture_search_paths\0"),
            set_failed_cb: get!(b"projectm_set_preset_switch_failed_event_callback\0"),
            version: get!(b"projectm_get_version_components\0"),
            _lib: lib,
        })
    }

    /// `major.minor.patch` of the loaded library.
    pub fn version(&self) -> (i32, i32, i32) {
        let (mut a, mut b, mut c) = (0, 0, 0);
        // SAFETY: three valid out pointers.
        unsafe { (self.version)(&mut a, &mut b, &mut c) };
        (a, b, c)
    }
}

/// Presets that failed to load, filled by projectM's callback.
type Failures = Box<std::sync::Mutex<Vec<(String, String)>>>;

extern "C" fn on_failed(file: *const c_char, message: *const c_char, user: *mut c_void) {
    let text = |p: *const c_char| {
        if p.is_null() {
            String::new()
        } else {
            // SAFETY: projectM passes NUL-terminated strings valid during the call.
            unsafe { std::ffi::CStr::from_ptr(p) }.to_string_lossy().into_owned()
        }
    };
    // SAFETY: `user` is the `Failures` box of the instance that registered us,
    // which outlives the instance (it is dropped after `projectm_destroy`).
    let failures = unsafe { &*(user as *const std::sync::Mutex<Vec<(String, String)>>) };
    if let Ok(mut f) = failures.lock() {
        f.push((text(file), text(message)));
    }
}

/// One projectM instance. Create and use it only on the thread whose OpenGL
/// context is current (and keep that context current).
pub struct ProjectM<'a> {
    api: &'a Api,
    handle: Handle,
    failures: Failures,
    /// Frames per channel projectM keeps; longer chunks are split.
    max_samples: usize,
}

impl<'a> ProjectM<'a> {
    /// Needs a current OpenGL (3.3 core) context. `None` if projectM could not start.
    pub fn new(api: &'a Api, width: usize, height: usize, fps: i32) -> Option<Self> {
        // SAFETY: plain calls; the context is current by the contract of `new`.
        let handle = unsafe { (api.create)() };
        if handle.is_null() {
            return None;
        }
        let failures: Failures = Box::default();
        let pm = ProjectM {
            api,
            handle,
            failures,
            max_samples: (unsafe { (api.pcm_max_samples)() } as usize).max(64),
        };
        // SAFETY: the handle is valid; the box's address is stable until drop.
        unsafe {
            (api.set_failed_cb)(handle, on_failed, &*pm.failures as *const _ as *mut c_void);
            (api.set_window_size)(handle, width, height);
            (api.set_fps)(handle, fps);
            // Listen switches presets itself (Feishin's cycling); projectM's own
            // timers and beat-triggered hard cuts only fire requests nobody handles.
            (api.set_preset_duration)(handle, 3600.0);
            (api.set_hard_cut_enabled)(handle, false);
            (api.set_mesh_size)(handle, 96, 54);
        }
        Some(pm)
    }

    pub fn set_window_size(&mut self, w: usize, h: usize) {
        // SAFETY: valid handle.
        unsafe { (self.api.set_window_size)(self.handle, w, h) }
    }

    pub fn set_fps(&mut self, fps: i32) {
        // SAFETY: valid handle.
        unsafe { (self.api.set_fps)(self.handle, fps) }
    }

    pub fn set_soft_cut_duration(&mut self, seconds: f64) {
        // SAFETY: valid handle.
        unsafe { (self.api.set_soft_cut_duration)(self.handle, seconds) }
    }

    pub fn set_preset_locked(&mut self, locked: bool) {
        // SAFETY: valid handle.
        unsafe { (self.api.set_preset_locked)(self.handle, locked) }
    }

    /// Folders projectM looks for the textures presets name.
    pub fn set_texture_paths(&mut self, paths: &[&Path]) {
        let c: Vec<CString> =
            paths.iter().filter_map(|p| CString::new(p.to_string_lossy().as_bytes()).ok()).collect();
        let ptrs: Vec<*const c_char> = c.iter().map(|s| s.as_ptr()).collect();
        // SAFETY: valid handle; the strings live over the call (projectM copies them).
        unsafe { (self.api.set_texture_search_paths)(self.handle, ptrs.as_ptr(), ptrs.len()) }
    }

    /// Loads a preset file, blending from the one on screen when `smooth`.
    /// `Err` with projectM's message when the preset can't be loaded.
    pub fn load_preset(&mut self, file: &Path, smooth: bool) -> Result<(), String> {
        let c = CString::new(file.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
        if let Ok(mut f) = self.failures.lock() {
            f.clear();
        }
        // SAFETY: valid handle and NUL-terminated path.
        unsafe { (self.api.load_preset_file)(self.handle, c.as_ptr(), smooth) };
        match self.failures.lock().ok().and_then(|mut f| f.pop()) {
            Some((_, message)) => {
                Err(if message.is_empty() { "the preset failed to load".into() } else { message })
            }
            None => Ok(()),
        }
    }

    /// Interleaved stereo samples (-1..1).
    pub fn add_pcm_stereo(&mut self, samples: &[f32]) {
        for chunk in samples.chunks(self.max_samples * 2) {
            let frames = chunk.len() / 2;
            if frames == 0 {
                continue;
            }
            // SAFETY: valid handle; `frames` stereo frames are in `chunk`.
            unsafe { (self.api.pcm_add_float)(self.handle, chunk.as_ptr(), frames as u32, STEREO) }
        }
    }

    /// Draws one frame into the framebuffer bound now.
    pub fn render(&mut self) {
        // SAFETY: valid handle, context current.
        unsafe { (self.api.render)(self.handle) }
    }
}

impl Drop for ProjectM<'_> {
    fn drop(&mut self) {
        // SAFETY: valid handle, destroyed once; the context is still current.
        unsafe { (self.api.destroy)(self.handle) }
    }
}
