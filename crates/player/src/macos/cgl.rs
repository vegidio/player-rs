//! The render thread's offscreen CGL context, and the GL function loader mpv and glow share.

use super::GlRenderer;
use crate::error::{Error, Result};
use objc2_io_surface::IOSurfaceRef;
use std::ffi::{CStr, c_char, c_int, c_uint, c_void};
use std::ptr;
use std::sync::Arc;

type CGLPixelFormatObj = *mut c_void;
type CGLContextObj = *mut c_void;

const K_CGL_PFA_RENDERER_ID: c_int = 70;
const K_CGL_PFA_ACCELERATED: c_int = 73;
const K_CGL_PFA_ALLOW_OFFLINE_RENDERERS: c_int = 96;
const K_CGL_PFA_OPENGL_PROFILE: c_int = 99;
const K_CGL_PFA_SUPPORTS_AUTOMATIC_GRAPHICS_SWITCHING: c_int = 101;
const K_CGL_OGLP_VERSION_3_2_CORE: c_int = 0x3200;
/// Apple's software renderer.
const K_CGL_RENDERER_GENERIC_FLOAT_ID: c_int = 0x0002_0400;
const RTLD_LAZY: c_int = 0x1;
const OPENGL_FRAMEWORK: &CStr = c"/System/Library/Frameworks/OpenGL.framework/OpenGL";

#[link(name = "OpenGL", kind = "framework")]
unsafe extern "C" {
    fn CGLChoosePixelFormat(attribs: *const c_int, pix: *mut CGLPixelFormatObj, npix: *mut c_int) -> c_int;
    fn CGLDestroyPixelFormat(pix: CGLPixelFormatObj) -> c_int;
    fn CGLCreateContext(pix: CGLPixelFormatObj, share: CGLContextObj, ctx: *mut CGLContextObj) -> c_int;
    fn CGLSetCurrentContext(ctx: CGLContextObj) -> c_int;
    fn CGLDestroyContext(ctx: CGLContextObj) -> c_int;
    fn CGLTexImageIOSurface2D(
        ctx: CGLContextObj,
        target: c_uint,
        internal_format: c_uint,
        width: c_int,
        height: c_int,
        format: c_uint,
        type_: c_uint,
        surface: *const IOSurfaceRef,
        plane: c_uint,
    ) -> c_int;
}

unsafe extern "C" {
    fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> c_int;
}

/// A 3.2 core CGL context with no drawable, current on the thread that created it. It is not `Send`, so it is used
/// and destroyed on that thread only.
pub(super) struct Context {
    raw: CGLContextObj,
    software: bool,
}

impl Context {
    /// Creates a context on `renderer` and makes it current on this thread.
    pub(super) fn new(renderer: GlRenderer) -> Result<Context> {
        match renderer {
            GlRenderer::Hardware => Self::with_attributes(
                false,
                &[
                    K_CGL_PFA_ACCELERATED,
                    K_CGL_PFA_ALLOW_OFFLINE_RENDERERS,
                    K_CGL_PFA_SUPPORTS_AUTOMATIC_GRAPHICS_SWITCHING,
                ],
            )
            .map_err(|message| Error::Render(format!("hardware OpenGL renderer: {message}"))),
            GlRenderer::Software => {
                Self::with_attributes(true, &[K_CGL_PFA_RENDERER_ID, K_CGL_RENDERER_GENERIC_FLOAT_ID])
                    .map_err(|message| Error::Render(format!("software OpenGL renderer: {message}")))
            }
            GlRenderer::Auto => Self::new(GlRenderer::Hardware).or_else(|_| Self::new(GlRenderer::Software)),
        }
    }

    fn with_attributes(software: bool, renderer: &[c_int]) -> std::result::Result<Context, String> {
        let attributes: Vec<c_int> = [K_CGL_PFA_OPENGL_PROFILE, K_CGL_OGLP_VERSION_3_2_CORE]
            .iter()
            .chain(renderer)
            .chain(&[0])
            .copied()
            .collect();

        let mut pixel_format = ptr::null_mut();
        let mut count = 0;
        // SAFETY: `attributes` is a zero-terminated attribute list; both out-pointers are valid.
        let code = unsafe { CGLChoosePixelFormat(attributes.as_ptr(), &mut pixel_format, &mut count) };
        if code != 0 || pixel_format.is_null() {
            return Err(format!("no CGL pixel format (error {code})"));
        }

        let mut raw = ptr::null_mut();
        // SAFETY: `pixel_format` is valid; no context is shared; the out-pointer is valid.
        let code = unsafe { CGLCreateContext(pixel_format, ptr::null_mut(), &mut raw) };
        // SAFETY: the pixel format is no longer needed; the context keeps its own reference.
        unsafe { CGLDestroyPixelFormat(pixel_format) };
        if code != 0 || raw.is_null() {
            return Err(format!("no CGL context (error {code})"));
        }

        let context = Context { raw, software };
        // SAFETY: `raw` is a valid context; making it current affects only this thread.
        let code = unsafe { CGLSetCurrentContext(raw) };
        if code != 0 {
            return Err(format!("the CGL context cannot be made current (error {code})"));
        }
        Ok(context)
    }

    /// Whether this is Apple's software renderer.
    pub(super) fn is_software(&self) -> bool {
        self.software
    }

    /// Binds `plane` of `surface` as the storage of the rectangle texture bound to `GL_TEXTURE_RECTANGLE`, so GL draws
    /// into the surface's memory.
    ///
    /// # Safety
    ///
    /// A texture of this context must be bound to `GL_TEXTURE_RECTANGLE`, and the size and formats must describe the
    /// plane.
    pub(super) unsafe fn bind_io_surface(
        &self,
        surface: &IOSurfaceRef,
        plane: u32,
        (width, height): (u32, u32),
        (internal_format, format, type_): (u32, u32, u32),
    ) -> Result<()> {
        let size = |value: u32| c_int::try_from(value).map_err(|_| Error::Render(format!("surface size {value}")));
        // SAFETY: this context is current on this thread (it is not `Send`), the caller guarantees the binding and
        // the plane's description, and `surface` is a valid IOSurface for the call.
        let code = unsafe {
            CGLTexImageIOSurface2D(
                self.raw,
                glow::TEXTURE_RECTANGLE,
                internal_format,
                size(width)?,
                size(height)?,
                format,
                type_,
                surface,
                plane,
            )
        };
        if code != 0 {
            return Err(Error::Render(format!("CGLTexImageIOSurface2D failed for plane {plane} (error {code})")));
        }
        Ok(())
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        // SAFETY: the context was created by `new` and is released only here, on the thread it is current on.
        unsafe {
            CGLSetCurrentContext(ptr::null_mut());
            CGLDestroyContext(self.raw);
        }
    }
}

/// Resolves GL function names in the OpenGL framework, which exports every function of every CGL context.
#[derive(Clone)]
pub(super) struct Loader(Arc<Library>);

struct Library(*mut c_void);

// SAFETY: a `dlopen` handle may be used with `dlsym` and closed from any thread.
unsafe impl Send for Library {}
// SAFETY: as above; `dlsym` is thread-safe.
unsafe impl Sync for Library {}

impl Drop for Library {
    fn drop(&mut self) {
        // SAFETY: the handle came from `dlopen` and is closed once, here.
        unsafe { dlclose(self.0) };
    }
}

impl Loader {
    pub(super) fn new() -> Result<Loader> {
        // SAFETY: the path is NUL-terminated; the framework is already loaded, since this crate links it.
        let handle = unsafe { dlopen(OPENGL_FRAMEWORK.as_ptr(), RTLD_LAZY) };
        if handle.is_null() {
            return Err(Error::Render("the OpenGL framework cannot be opened".into()));
        }
        Ok(Loader(Arc::new(Library(handle))))
    }

    /// The address of the GL function `name`, or null.
    pub(super) fn get(&self, name: &CStr) -> *const c_void {
        // SAFETY: the handle is open while `self` lives; `name` is NUL-terminated and only read.
        unsafe { dlsym(self.0.0, name.as_ptr()) }.cast_const()
    }
}
