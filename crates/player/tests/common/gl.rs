//! A headless OpenGL context for the render tests, current on the thread that creates it, plus an offscreen RGBA
//! framebuffer to draw into and read back.
//!
//! - macOS: raw CGL. Apple's software renderer by default, since CI VMs may have no usable GPU; set
//!   `PLAYER_TEST_GL=hardware` to use the GPU instead. glutin cannot select the software renderer.
//! - Linux: EGL on a device, with no window or surface. On CI, Mesa's llvmpipe (`LIBGL_ALWAYS_SOFTWARE=1`).
//! - Windows: WGL on a hidden window, which WGL needs to create a context. On CI, Mesa's `opengl32.dll`.

// Each test binary uses a different part of this helper.
#![allow(dead_code)]

use glow::HasContext;
use std::ffi::{CStr, c_void};

/// A function resolving GL function names for the current context, as mpv's render API wants it.
pub type Loader = Box<dyn FnMut(&CStr) -> *const c_void + Send + 'static>;

/// An OpenGL context made current on the creating thread. It must be used and dropped on that thread.
pub struct Gl {
    /// GL calls for the test's own setup and readback.
    pub gl: glow::Context,
    platform: platform::Context,
}

impl Gl {
    /// Creates a context and makes it current on this thread.
    pub fn new() -> Gl {
        let platform = platform::Context::new();
        let mut loader = platform.loader();
        // SAFETY: the context is current on this thread, and the loader resolves functions for it.
        let gl = unsafe { glow::Context::from_loader_function_cstr(|name| loader(name)) };
        Gl { gl, platform }
    }

    /// A new loader for this context, to hand to mpv.
    pub fn loader(&self) -> Loader {
        self.platform.loader()
    }

    /// The renderer's name, such as "Apple Software Renderer" or "llvmpipe (...)".
    pub fn renderer(&self) -> String {
        // SAFETY: the context is current on this thread.
        unsafe { self.gl.get_parameter_string(glow::RENDERER) }
    }

    /// Creates a `width` x `height` RGBA framebuffer backed by a texture and fills it with `fill`.
    pub fn framebuffer(&self, width: u32, height: u32, fill: [u8; 4]) -> Framebuffer {
        let gl = &self.gl;
        // SAFETY: the context is current on this thread; every object is created and bound here.
        unsafe {
            let texture = gl.create_texture().expect("a texture is created");
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                width as i32,
                height as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            );
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::NEAREST as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::NEAREST as i32);
            gl.bind_texture(glow::TEXTURE_2D, None);

            let fbo = gl.create_framebuffer().expect("a framebuffer is created");
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            gl.framebuffer_texture_2d(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::TEXTURE_2D, Some(texture), 0);
            let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
            assert_eq!(status, glow::FRAMEBUFFER_COMPLETE, "the framebuffer is complete");
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);

            let framebuffer = Framebuffer { fbo, texture, width, height };
            self.fill(&framebuffer, fill);
            framebuffer
        }
    }

    /// Fills the whole framebuffer with one colour.
    pub fn fill(&self, framebuffer: &Framebuffer, [r, g, b, a]: [u8; 4]) {
        let gl = &self.gl;
        // SAFETY: the context is current on this thread and the framebuffer belongs to it.
        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer.fbo));
            gl.viewport(0, 0, framebuffer.width as i32, framebuffer.height as i32);
            gl.clear_color(f32::from(r) / 255.0, f32::from(g) / 255.0, f32::from(b) / 255.0, f32::from(a) / 255.0);
            gl.clear(glow::COLOR_BUFFER_BIT);
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            gl.finish();
        }
    }

    /// Reads the framebuffer back as RGBA rows, bottom row first (GL's order).
    pub fn read(&self, framebuffer: &Framebuffer) -> Image {
        let gl = &self.gl;
        let mut pixels = vec![0u8; framebuffer.width as usize * framebuffer.height as usize * 4];
        // SAFETY: the context is current on this thread, the framebuffer belongs to it, and `pixels` holds exactly
        // `width * height` RGBA pixels with a pack alignment of 1.
        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer.fbo));
            gl.pixel_store_i32(glow::PACK_ALIGNMENT, 1);
            gl.read_pixels(
                0,
                0,
                framebuffer.width as i32,
                framebuffer.height as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut pixels)),
            );
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        }
        Image { width: framebuffer.width, height: framebuffer.height, pixels }
    }

    /// Deletes the framebuffer and its texture.
    pub fn delete(&self, framebuffer: Framebuffer) {
        // SAFETY: the context is current on this thread and the objects belong to it.
        unsafe {
            self.gl.delete_framebuffer(framebuffer.fbo);
            self.gl.delete_texture(framebuffer.texture);
        }
    }
}

/// An offscreen RGBA framebuffer created by [`Gl::framebuffer`].
pub struct Framebuffer {
    fbo: glow::NativeFramebuffer,
    texture: glow::NativeTexture,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

impl Framebuffer {
    /// The GL name of the framebuffer object.
    pub fn name(&self) -> u32 {
        self.fbo.0.get()
    }
}

/// Pixels read back from a framebuffer: RGBA, rows bottom first.
#[derive(Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Image {
    /// The RGBA pixel at column `x` and row `y`, counted from the bottom.
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let at = (y as usize * self.width as usize + x as usize) * 4;
        self.pixels[at..at + 4].try_into().unwrap()
    }

    /// The image mirrored top to bottom.
    pub fn flipped(&self) -> Image {
        let row = self.width as usize * 4;
        let pixels = self.pixels.chunks_exact(row).rev().flatten().copied().collect();
        Image { width: self.width, height: self.height, pixels }
    }
}

impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Image({}x{})", self.width, self.height)
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::Loader;
    use std::ffi::{CStr, c_char, c_int, c_void};
    use std::ptr;

    type CGLPixelFormatObj = *mut c_void;
    type CGLContextObj = *mut c_void;

    const K_CGL_PFA_RENDERER_ID: c_int = 70;
    const K_CGL_PFA_ACCELERATED: c_int = 73;
    const K_CGL_PFA_ALLOW_OFFLINE_RENDERERS: c_int = 96;
    const K_CGL_PFA_OPENGL_PROFILE: c_int = 99;
    const K_CGL_OGLP_VERSION_3_2_CORE: c_int = 0x3200;
    const K_CGL_RENDERER_GENERIC_FLOAT_ID: c_int = 0x0002_0400;
    /// `RTLD_DEFAULT` on macOS: search every image loaded in the process.
    const RTLD_DEFAULT: *mut c_void = -2isize as *mut c_void;

    #[link(name = "OpenGL", kind = "framework")]
    unsafe extern "C" {
        fn CGLChoosePixelFormat(attribs: *const c_int, pix: *mut CGLPixelFormatObj, npix: *mut c_int) -> c_int;
        fn CGLDestroyPixelFormat(pix: CGLPixelFormatObj) -> c_int;
        fn CGLCreateContext(pix: CGLPixelFormatObj, share: CGLContextObj, ctx: *mut CGLContextObj) -> c_int;
        fn CGLSetCurrentContext(ctx: CGLContextObj) -> c_int;
        fn CGLDestroyContext(ctx: CGLContextObj) -> c_int;
    }

    unsafe extern "C" {
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    }

    pub struct Context(CGLContextObj);

    impl Context {
        pub fn new() -> Context {
            let hardware = std::env::var("PLAYER_TEST_GL").is_ok_and(|value| value == "hardware");
            let renderer: &[c_int] = if hardware {
                &[K_CGL_PFA_ACCELERATED, K_CGL_PFA_ALLOW_OFFLINE_RENDERERS]
            } else {
                &[K_CGL_PFA_RENDERER_ID, K_CGL_RENDERER_GENERIC_FLOAT_ID]
            };
            let attribs: Vec<c_int> = [K_CGL_PFA_OPENGL_PROFILE, K_CGL_OGLP_VERSION_3_2_CORE]
                .iter()
                .chain(renderer)
                .chain(&[0])
                .copied()
                .collect();

            let mut pixel_format = ptr::null_mut();
            let mut count = 0;
            // SAFETY: `attribs` is a zero-terminated attribute list; both out-pointers are valid.
            let code = unsafe { CGLChoosePixelFormat(attribs.as_ptr(), &mut pixel_format, &mut count) };
            assert!(code == 0 && !pixel_format.is_null(), "no CGL pixel format (error {code}, hardware: {hardware})");

            let mut context = ptr::null_mut();
            // SAFETY: `pixel_format` is valid; no context is shared; the out-pointer is valid.
            let code = unsafe { CGLCreateContext(pixel_format, ptr::null_mut(), &mut context) };
            // SAFETY: the pixel format is no longer needed; the context keeps its own reference.
            unsafe { CGLDestroyPixelFormat(pixel_format) };
            assert!(code == 0 && !context.is_null(), "no CGL context (error {code})");

            // SAFETY: `context` is a valid context; making it current affects only this thread.
            let code = unsafe { CGLSetCurrentContext(context) };
            assert_eq!(code, 0, "the CGL context is made current");
            Context(context)
        }

        pub fn loader(&self) -> Loader {
            // The OpenGL framework exports every GL function, so a process-wide lookup resolves them.
            Box::new(|name: &CStr| {
                // SAFETY: `name` is NUL-terminated; `dlsym` only reads it.
                unsafe { dlsym(RTLD_DEFAULT, name.as_ptr()) }.cast_const()
            })
        }
    }

    impl Drop for Context {
        fn drop(&mut self) {
            // SAFETY: the context was created by `new` and is released only here, on the thread it is current on.
            unsafe {
                CGLSetCurrentContext(ptr::null_mut());
                CGLDestroyContext(self.0);
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::Loader;
    use glutin::api::egl::context::PossiblyCurrentContext;
    use glutin::api::egl::device::Device;
    use glutin::api::egl::display::Display;
    use glutin::config::{Api, ConfigSurfaceTypes, ConfigTemplateBuilder};
    use glutin::context::{ContextApi, ContextAttributesBuilder};
    use glutin::prelude::*;
    use std::ffi::CStr;

    pub struct Context {
        display: Display,
        _context: PossiblyCurrentContext,
    }

    impl Context {
        pub fn new() -> Context {
            // Prefer Mesa's software device when asked for software rendering, so a machine with a GPU still tests
            // the CI configuration.
            let software = std::env::var_os("LIBGL_ALWAYS_SOFTWARE").is_some();
            let mut devices: Vec<Device> = Device::query_devices().expect("EGL devices are listed").collect();
            devices.sort_by_key(|device| software != device.extensions().contains("EGL_MESA_device_software"));

            let mut failures = vec![];
            for device in &devices {
                match Self::on_device(device) {
                    Ok(context) => return context,
                    Err(error) => failures.push(format!("{:?}: {error}", device.name())),
                }
            }
            panic!("no EGL device gave a GL context: {failures:?}");
        }

        fn on_device(device: &Device) -> Result<Context, glutin::error::Error> {
            // SAFETY: no native display is passed.
            let display = unsafe { Display::with_device(device, None)? };
            let template = ConfigTemplateBuilder::new()
                .with_surface_type(ConfigSurfaceTypes::empty())
                .with_api(Api::OPENGL)
                .build();
            // SAFETY: the template names no native window.
            let config = unsafe { display.find_configs(template)? }
                .next()
                .ok_or_else(|| glutin::error::Error::from(glutin::error::ErrorKind::BadConfig))?;
            let attributes = ContextAttributesBuilder::new().with_context_api(ContextApi::OpenGl(None)).build(None);
            // SAFETY: the config comes from this display and no native window is involved.
            let context = unsafe { display.create_context(&config, &attributes)? };
            let context = context.make_current_surfaceless()?;
            Ok(Context { display, _context: context })
        }

        pub fn loader(&self) -> Loader {
            let display = self.display.clone();
            Box::new(move |name: &CStr| display.get_proc_address(name))
        }
    }
}

#[cfg(windows)]
mod platform {
    use super::Loader;
    use glutin::api::wgl::context::PossiblyCurrentContext;
    use glutin::api::wgl::display::Display;
    use glutin::api::wgl::surface::Surface;
    use glutin::config::ConfigTemplateBuilder;
    use glutin::context::{ContextApi, ContextAttributesBuilder};
    use glutin::prelude::*;
    use glutin::surface::{SurfaceAttributesBuilder, WindowSurface};
    use raw_window_handle::{RawDisplayHandle, RawWindowHandle, Win32WindowHandle, WindowsDisplayHandle};
    use std::ffi::CStr;
    use std::num::NonZeroIsize;
    use std::num::NonZeroU32;
    use std::ptr;
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WS_OVERLAPPEDWINDOW};

    pub struct Context {
        display: Display,
        // Dropped in declaration order: the context, then its surface, then the window.
        _context: PossiblyCurrentContext,
        _surface: Surface<WindowSurface>,
        _window: Window,
    }

    /// The hidden window, destroyed when dropped.
    struct Window(HWND);

    impl Drop for Window {
        fn drop(&mut self) {
            // SAFETY: the window was created by `Context::new` and is destroyed only here, on its thread.
            unsafe { DestroyWindow(self.0) };
        }
    }

    impl Context {
        pub fn new() -> Context {
            let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
            // SAFETY: a null module name returns this executable's handle.
            let instance = unsafe { GetModuleHandleW(ptr::null()) };
            // SAFETY: the system "STATIC" class needs no registration; the window is never shown.
            let window = unsafe {
                CreateWindowExW(
                    0,
                    class.as_ptr(),
                    ptr::null(),
                    WS_OVERLAPPEDWINDOW,
                    0,
                    0,
                    16,
                    16,
                    0,
                    0,
                    instance,
                    ptr::null(),
                )
            };
            assert_ne!(window, 0, "the hidden window is created");
            let window = Window(window);

            let mut handle = Win32WindowHandle::new(NonZeroIsize::new(window.0).unwrap());
            handle.hinstance = NonZeroIsize::new(instance);
            let raw_window = RawWindowHandle::Win32(handle);

            // SAFETY: `raw_window` is a valid window with its module handle, alive until this context is dropped.
            let display =
                unsafe { Display::new(RawDisplayHandle::Windows(WindowsDisplayHandle::new()), Some(raw_window)) }
                    .expect("a WGL display is created");
            let template = ConfigTemplateBuilder::new().compatible_with_native_window(raw_window).build();
            // SAFETY: the template's window is valid.
            let config = unsafe { display.find_configs(template) }
                .expect("WGL configs are listed")
                .next()
                .expect("a WGL config exists");
            let attributes = ContextAttributesBuilder::new()
                .with_context_api(ContextApi::OpenGl(None))
                .build(Some(raw_window));
            // SAFETY: the config comes from this display and the window is valid.
            let context = unsafe { display.create_context(&config, &attributes) }.expect("a WGL context is created");
            let size = NonZeroU32::new(16).unwrap();
            let surface_attributes = SurfaceAttributesBuilder::<WindowSurface>::new().build(raw_window, size, size);
            // SAFETY: the window is valid and outlives the surface.
            let surface = unsafe { display.create_window_surface(&config, &surface_attributes) }
                .expect("a WGL surface is created");
            let context = context.make_current(&surface).expect("the WGL context is made current");
            Context { display, _context: context, _surface: surface, _window: window }
        }

        pub fn loader(&self) -> Loader {
            let display = self.display.clone();
            Box::new(move |name: &CStr| display.get_proc_address(name))
        }
    }
}
