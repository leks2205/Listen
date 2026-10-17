//! Headless OpenGL 3.3 core context for libprojectM (EGL pbuffer, read back as RGBA). projectM 4.1
//! draws into the default framebuffer whatever is bound, hence a full-size pbuffer rather than an
//! FBO. Linux only; EGL and libGL are loaded at run time.

use glow::HasContext;
use khronos_egl as egl;
use std::ffi::c_void;

type Instance = egl::DynamicInstance<egl::EGL1_4>;
type Shared = std::rc::Rc<Instance>;

const EGL_PLATFORM_DEVICE_EXT: u32 = 0x313F;
const EGL_PLATFORM_SURFACELESS_MESA: u32 = 0x31DD;

type GetPlatformDisplay = unsafe extern "C" fn(u32, *mut c_void, *const egl::Int) -> *mut c_void;
type QueryDevices = unsafe extern "C" fn(egl::Int, *mut *mut c_void, *mut egl::Int) -> u32;

pub struct Headless {
    egl: Shared,
    display: egl::Display,
    context: egl::Context,
    surface: egl::Surface,
    pub gl: glow::Context,
    pub width: i32,
    pub height: i32,
    /// `GL_RENDERER` of the context, for the log.
    pub renderer: String,
}

/// Displays to try, best first: a GPU device (no window system needed), Mesa surfaceless, default.
fn displays(egl: &Instance) -> Vec<(String, egl::Display)> {
    let mut out = Vec::new();
    let forced = std::env::var("LISTEN_MILKDROP_EGL").unwrap_or_default();
    let want = |name: &str| forced.is_empty() || forced == name;
    // SAFETY: the transmutes give the extension functions their documented C signatures.
    let platform: Option<GetPlatformDisplay> =
        egl.get_proc_address("eglGetPlatformDisplayEXT").map(|p| unsafe { std::mem::transmute(p) });
    let query: Option<QueryDevices> =
        egl.get_proc_address("eglQueryDevicesEXT").map(|p| unsafe { std::mem::transmute(p) });
    if let (Some(platform), Some(query)) = (platform, query)
        && want("device")
    {
        let mut devices = [std::ptr::null_mut::<c_void>(); 8];
        let mut n: egl::Int = 0;
        // SAFETY: out arrays of the stated size.
        if unsafe { query(8, devices.as_mut_ptr(), &mut n) } != 0 {
            for (i, d) in devices.iter().take(n.max(0) as usize).enumerate() {
                // SAFETY: a device handle from eglQueryDevicesEXT.
                let ptr = unsafe { platform(EGL_PLATFORM_DEVICE_EXT, *d, std::ptr::null()) };
                if !ptr.is_null() {
                    // SAFETY: a display handle from EGL.
                    out.push((format!("device {i}"), unsafe { egl::Display::from_ptr(ptr) }));
                }
            }
        }
    }
    if let Some(platform) = platform
        && want("surfaceless")
    {
        // SAFETY: documented use of the extension.
        let ptr = unsafe { platform(EGL_PLATFORM_SURFACELESS_MESA, std::ptr::null_mut(), std::ptr::null()) };
        if !ptr.is_null() {
            // SAFETY: a display handle from EGL.
            out.push(("surfaceless".into(), unsafe { egl::Display::from_ptr(ptr) }));
        }
    }
    if want("default")
        && let Some(d) = unsafe { egl.get_display(egl::DEFAULT_DISPLAY) }
    {
        out.push(("default".into(), d));
    }
    out
}

impl Headless {
    /// Makes a context with a `width` x `height` RGBA framebuffer current on this thread until drop.
    pub fn new(width: i32, height: i32) -> Result<Self, String> {
        // SAFETY: loading the system's EGL.
        let lib = ["libEGL.so.1", "libEGL.so"]
            .iter()
            .find_map(|n| unsafe { libloading::Library::new(n) }.ok())
            .ok_or("libEGL not found")?;
        // SAFETY: the symbols are looked up by their EGL 1.4 names.
        let egl: Shared = std::rc::Rc::new(
            unsafe { Instance::load_required_from(lib) }.map_err(|e| format!("libEGL: {e}"))?,
        );
        let mut last = String::from("no EGL display");
        for (name, display) in displays(&egl) {
            match Self::try_display(egl.clone(), display, width, height) {
                Ok(h) => {
                    tracing::info!("milkdrop: EGL {name}, GL renderer {}", h.renderer);
                    return Ok(h);
                }
                Err(e) => {
                    tracing::debug!("milkdrop: EGL {name}: {e}");
                    last = format!("{name}: {e}");
                }
            }
        }
        Err(format!("no OpenGL context for Milkdrop ({last})"))
    }

    fn try_display(egl: Shared, display: egl::Display, width: i32, height: i32) -> Result<Self, String> {
        egl.initialize(display).map_err(|e| format!("eglInitialize: {e}"))?;
        let r = Self::with_display(&egl, display, width, height);
        if r.is_err() {
            let _ = egl.terminate(display);
        }
        r.map(|(context, surface, gl, renderer)| Self {
            egl,
            display,
            context,
            surface,
            gl,
            width,
            height,
            renderer,
        })
    }

    fn with_display(
        egl: &Instance,
        display: egl::Display,
        width: i32,
        height: i32,
    ) -> Result<(egl::Context, egl::Surface, glow::Context, String), String> {
        egl.bind_api(egl::OPENGL_API).map_err(|e| format!("no desktop OpenGL: {e}"))?;
        let config_attribs = [
            egl::SURFACE_TYPE,
            egl::PBUFFER_BIT,
            egl::RENDERABLE_TYPE,
            egl::OPENGL_BIT,
            egl::RED_SIZE,
            8,
            egl::GREEN_SIZE,
            8,
            egl::BLUE_SIZE,
            8,
            egl::ALPHA_SIZE,
            8,
            egl::DEPTH_SIZE,
            24,
            egl::STENCIL_SIZE,
            8,
            egl::NONE,
        ];
        let config = egl
            .choose_first_config(display, &config_attribs)
            .map_err(|e| format!("eglChooseConfig: {e}"))?
            .ok_or("no EGL config")?;
        let ctx_attribs = [
            egl::CONTEXT_MAJOR_VERSION,
            3,
            egl::CONTEXT_MINOR_VERSION,
            3,
            egl::CONTEXT_OPENGL_PROFILE_MASK,
            egl::CONTEXT_OPENGL_CORE_PROFILE_BIT,
            egl::NONE,
        ];
        let context = egl
            .create_context(display, config, None, &ctx_attribs)
            .map_err(|e| format!("eglCreateContext: {e}"))?;
        let surface = match egl.create_pbuffer_surface(
            display,
            config,
            &[egl::WIDTH, width, egl::HEIGHT, height, egl::NONE],
        ) {
            Ok(s) => s,
            Err(e) => {
                let _ = egl.destroy_context(display, context);
                return Err(format!("eglCreatePbufferSurface: {e}"));
            }
        };
        if let Err(e) = egl.make_current(display, Some(surface), Some(surface), Some(context)) {
            let _ = egl.destroy_surface(display, surface);
            let _ = egl.destroy_context(display, context);
            return Err(format!("eglMakeCurrent: {e}"));
        }
        // SAFETY: the context is current; names resolve through EGL.
        let gl = unsafe {
            glow::Context::from_loader_function(|s| {
                egl.get_proc_address(s).map_or(std::ptr::null(), |p| p as *const c_void)
            })
        };
        // SAFETY: a GL call on the current context.
        let renderer = unsafe { gl.get_parameter_string(glow::RENDERER) };
        Ok((context, surface, gl, renderer))
    }

    /// Prepares the default framebuffer (the pbuffer) for a frame.
    pub fn bind(&self) {
        // SAFETY: the context is current on this thread.
        unsafe {
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            self.gl.viewport(0, 0, self.width, self.height);
        }
    }

    /// The framebuffer as top-down RGBA8.
    pub fn read_rgba(&self) -> Vec<u8> {
        let (w, h) = (self.width as usize, self.height as usize);
        let mut bottom_up = vec![0u8; w * h * 4];
        // SAFETY: the buffer is w*h*4 bytes.
        unsafe {
            self.gl.bind_framebuffer(glow::READ_FRAMEBUFFER, None);
            self.gl.read_pixels(
                0,
                0,
                self.width,
                self.height,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut bottom_up)),
            );
        }
        let row = w * 4;
        let mut out = Vec::with_capacity(bottom_up.len());
        for y in (0..h).rev() {
            out.extend_from_slice(&bottom_up[y * row..(y + 1) * row]);
        }
        // projectM leaves alpha at whatever the preset wrote; the picture is opaque.
        for px in out.as_chunks_mut::<4>().0 {
            px[3] = 255;
        }
        out
    }
}

impl Drop for Headless {
    fn drop(&mut self) {
        let _ = self.egl.make_current(self.display, None, None, None);
        let _ = self.egl.destroy_surface(self.display, self.surface);
        let _ = self.egl.destroy_context(self.display, self.context);
        let _ = self.egl.terminate(self.display);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clears_and_reads_back_top_down() {
        let Ok(h) = Headless::new(64, 32) else {
            eprintln!("skipped: no headless OpenGL here");
            return;
        };
        // SAFETY: the context is current on this thread.
        unsafe {
            h.bind();
            h.gl.enable(glow::SCISSOR_TEST);
            h.gl.clear_color(0.0, 0.0, 0.0, 1.0);
            h.gl.clear(glow::COLOR_BUFFER_BIT);
            // The bottom-left 8x8 corner in GL's bottom-up coordinates.
            h.gl.scissor(0, 0, 8, 8);
            h.gl.clear_color(1.0, 0.0, 0.0, 1.0);
            h.gl.clear(glow::COLOR_BUFFER_BIT);
            h.gl.disable(glow::SCISSOR_TEST);
        }
        let px = h.read_rgba();
        assert_eq!(px.len(), 64 * 32 * 4);
        let at = |x: usize, y: usize| &px[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4];
        assert_eq!(at(2, 29), [255, 0, 0, 255], "bottom-left is red in a top-down image");
        assert_eq!(at(2, 2), [0, 0, 0, 255]);
        assert_eq!(at(40, 29), [0, 0, 0, 255]);
    }
}
