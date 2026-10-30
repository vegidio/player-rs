//! The render thread's EGL display on a GBM device, its surfaceless OpenGL context, and the EGLImages that put
//! dma-bufs behind GL textures.
//!
//! The EGL 1.4 core functions are linked directly from libEGL; the platform display, image functions and
//! `glEGLImageTargetTexture2DOES` are extensions, loaded through `eglGetProcAddress`.

use super::gbm;
use crate::error::{Error, Result};
use std::ffi::{CStr, c_char, c_void};
use std::os::fd::{AsRawFd, BorrowedFd};
use std::ptr;
use std::rc::Rc;

type EGLDisplay = *mut c_void;
type EGLContext = *mut c_void;
type EGLConfig = *mut c_void;
type EGLImage = *mut c_void;
type EGLBoolean = u32;
type EGLenum = u32;
type EGLint = i32;

const EGL_TRUE: EGLBoolean = 1;
const EGL_NONE: EGLint = 0x3038;
const EGL_EXTENSIONS: EGLint = 0x3055;
const EGL_RENDERABLE_TYPE: EGLint = 0x3040;
const EGL_SURFACE_TYPE: EGLint = 0x3033;
const EGL_OPENGL_BIT: EGLint = 0x0008;
const EGL_OPENGL_ES3_BIT: EGLint = 0x0040;
const EGL_OPENGL_API: EGLenum = 0x30A2;
const EGL_OPENGL_ES_API: EGLenum = 0x30A0;
const EGL_CONTEXT_MAJOR_VERSION: EGLint = 0x3098;
const EGL_CONTEXT_MINOR_VERSION: EGLint = 0x30FB;
const EGL_CONTEXT_OPENGL_PROFILE_MASK: EGLint = 0x30FD;
const EGL_CONTEXT_OPENGL_CORE_PROFILE_BIT: EGLint = 0x0001;
const EGL_PLATFORM_GBM_KHR: EGLenum = 0x31D7;
const EGL_LINUX_DMA_BUF_EXT: EGLenum = 0x3270;
const EGL_WIDTH: EGLint = 0x3057;
const EGL_HEIGHT: EGLint = 0x3056;
const EGL_LINUX_DRM_FOURCC_EXT: EGLint = 0x3271;
const EGL_DMA_BUF_PLANE0_FD_EXT: EGLint = 0x3272;
const EGL_DMA_BUF_PLANE0_OFFSET_EXT: EGLint = 0x3273;
const EGL_DMA_BUF_PLANE0_PITCH_EXT: EGLint = 0x3274;
const EGL_DMA_BUF_PLANE0_MODIFIER_LO_EXT: EGLint = 0x3443;
const EGL_DMA_BUF_PLANE0_MODIFIER_HI_EXT: EGLint = 0x3444;

unsafe extern "C" {
    fn eglInitialize(display: EGLDisplay, major: *mut EGLint, minor: *mut EGLint) -> EGLBoolean;
    fn eglTerminate(display: EGLDisplay) -> EGLBoolean;
    fn eglQueryString(display: EGLDisplay, name: EGLint) -> *const c_char;
    fn eglBindAPI(api: EGLenum) -> EGLBoolean;
    fn eglChooseConfig(
        display: EGLDisplay,
        attributes: *const EGLint,
        configs: *mut EGLConfig,
        size: EGLint,
        count: *mut EGLint,
    ) -> EGLBoolean;
    fn eglCreateContext(
        display: EGLDisplay,
        config: EGLConfig,
        share: EGLContext,
        attributes: *const EGLint,
    ) -> EGLContext;
    fn eglDestroyContext(display: EGLDisplay, context: EGLContext) -> EGLBoolean;
    fn eglMakeCurrent(display: EGLDisplay, draw: *mut c_void, read: *mut c_void, context: EGLContext) -> EGLBoolean;
    fn eglGetProcAddress(name: *const c_char) -> *const c_void;
    fn eglGetError() -> EGLint;
}

type GetPlatformDisplayExt =
    unsafe extern "C" fn(platform: EGLenum, native: *mut c_void, attributes: *const EGLint) -> EGLDisplay;
type CreateImageKhr = unsafe extern "C" fn(
    display: EGLDisplay,
    context: EGLContext,
    target: EGLenum,
    buffer: *mut c_void,
    attributes: *const EGLint,
) -> EGLImage;
type DestroyImageKhr = unsafe extern "C" fn(display: EGLDisplay, image: EGLImage) -> EGLBoolean;
type ImageTargetTexture2dOes = unsafe extern "C" fn(target: u32, image: EGLImage);

/// Resolves an EGL or GL function of the current context, or returns null. Thread-safe.
pub(super) fn get_proc_address(name: &CStr) -> *const c_void {
    // SAFETY: `name` is NUL-terminated and outlives the call; the function has no other preconditions.
    unsafe { eglGetProcAddress(name.as_ptr()) }
}

/// Loads an extension function, or fails naming it.
///
/// # Safety
///
/// `F` must be the function pointer type of `name`.
unsafe fn load<F>(name: &CStr) -> Result<F> {
    let address = get_proc_address(name);
    if address.is_null() {
        return Err(Error::Render(format!("EGL has no {}", name.to_string_lossy())));
    }
    // SAFETY: the caller guarantees the type; function and data pointers have the same size here.
    Ok(unsafe { std::mem::transmute_copy::<*const c_void, F>(&address) })
}

/// The last EGL error, for messages.
fn error() -> String {
    // SAFETY: no preconditions; it reads this thread's error state.
    format!("EGL error {:#x}", unsafe { eglGetError() })
}

/// Whether the space-separated `extensions` list has `name`.
fn has(extensions: &str, name: &str) -> bool {
    extensions.split_ascii_whitespace().any(|extension| extension == name)
}

/// An initialised EGL display on a GBM device's platform, terminated when dropped. Contexts and images hold it, and
/// it holds the GBM device, so each is released after everything using it.
pub(super) struct Display {
    raw: EGLDisplay,
    no_config: bool,
    create_image: CreateImageKhr,
    destroy_image: DestroyImageKhr,
    _gbm: Rc<gbm::Device>,
}

impl Display {
    /// Opens the display of a GBM device, and checks the extensions the renderer needs.
    pub(super) fn new(gbm: Rc<gbm::Device>) -> Result<Rc<Display>> {
        // SAFETY: `EGL_NO_DISPLAY` asks for the client extensions, which needs no display.
        let client = unsafe { string(ptr::null_mut(), EGL_EXTENSIONS) };
        if !has(&client, "EGL_KHR_platform_gbm") && !has(&client, "EGL_MESA_platform_gbm") {
            return Err(Error::Render("EGL has no GBM platform (EGL_KHR_platform_gbm)".into()));
        }
        // SAFETY: the types match the extensions' prototypes.
        let get_platform_display = unsafe { load::<GetPlatformDisplayExt>(c"eglGetPlatformDisplayEXT")? };
        // SAFETY: as above.
        let (create_image, destroy_image) =
            unsafe { (load::<CreateImageKhr>(c"eglCreateImageKHR")?, load::<DestroyImageKhr>(c"eglDestroyImageKHR")?) };

        // SAFETY: the GBM device is valid and outlives the display, which holds it; the attribute list is empty.
        let raw = unsafe { get_platform_display(EGL_PLATFORM_GBM_KHR, gbm.as_ptr(), [EGL_NONE].as_ptr()) };
        if raw.is_null() {
            return Err(Error::Render(format!("no EGL display on the GBM device ({})", error())));
        }
        // SAFETY: `raw` is a display; the version out-pointers may be null.
        if unsafe { eglInitialize(raw, ptr::null_mut(), ptr::null_mut()) } != EGL_TRUE {
            return Err(Error::Render(format!("the EGL display cannot be initialised ({})", error())));
        }
        // SAFETY: `raw` is initialised.
        let extensions = unsafe { string(raw, EGL_EXTENSIONS) };
        let mut display = Display { raw, no_config: false, create_image, destroy_image, _gbm: gbm };
        display.no_config = has(&extensions, "EGL_KHR_no_config_context");
        for needed in [
            "EGL_EXT_image_dma_buf_import",
            "EGL_EXT_image_dma_buf_import_modifiers",
            "EGL_KHR_surfaceless_context",
        ] {
            if !has(&extensions, needed) {
                return Err(Error::Render(format!("the EGL driver has no {needed}")));
            }
        }
        Ok(Rc::new(display))
    }

    /// Imports a single-plane dma-buf as an image.
    pub(super) fn import(self: &Rc<Self>, plane: &Plane<'_>) -> Result<Image> {
        let int = |value: u32| EGLint::try_from(value).map_err(|_| Error::Render(format!("dma-buf value {value}")));
        let attributes = [
            EGL_WIDTH,
            int(plane.width)?,
            EGL_HEIGHT,
            int(plane.height)?,
            EGL_LINUX_DRM_FOURCC_EXT,
            // Fourccs are bit patterns; EGL takes them as `EGLint`.
            plane.fourcc as EGLint,
            EGL_DMA_BUF_PLANE0_FD_EXT,
            plane.fd.as_raw_fd(),
            EGL_DMA_BUF_PLANE0_OFFSET_EXT,
            int(plane.offset)?,
            EGL_DMA_BUF_PLANE0_PITCH_EXT,
            int(plane.stride)?,
            EGL_DMA_BUF_PLANE0_MODIFIER_LO_EXT,
            plane.modifier as u32 as EGLint,
            EGL_DMA_BUF_PLANE0_MODIFIER_HI_EXT,
            (plane.modifier >> 32) as u32 as EGLint,
            EGL_NONE,
        ];
        // SAFETY: the display is initialised; dma-buf images take no context or client buffer; the attribute list is
        // terminated and the fd open for the call (EGL duplicates what it keeps).
        let raw = unsafe {
            (self.create_image)(self.raw, ptr::null_mut(), EGL_LINUX_DMA_BUF_EXT, ptr::null_mut(), attributes.as_ptr())
        };
        if raw.is_null() {
            return Err(Error::Render(format!("the dma-buf cannot be imported into EGL ({})", error())));
        }
        Ok(Image { raw, display: Rc::clone(self) })
    }
}

impl Drop for Display {
    fn drop(&mut self) {
        // SAFETY: the display is initialised, and every context and image on it is gone, since they hold it.
        unsafe { eglTerminate(self.raw) };
    }
}

/// Reads an EGL string, empty when it is missing.
///
/// # Safety
///
/// `display` must be initialised, or null for client strings.
unsafe fn string(display: EGLDisplay, name: EGLint) -> String {
    // SAFETY: as the caller guarantees; EGL returns a static NUL-terminated string or null.
    let raw = unsafe { eglQueryString(display, name) };
    if raw.is_null() {
        return String::new();
    }
    // SAFETY: non-null strings from EGL are NUL-terminated and live as long as the display.
    unsafe { CStr::from_ptr(raw) }.to_string_lossy().into_owned()
}

/// What [`Display::import`] needs to know about a dma-buf's single plane.
pub(super) struct Plane<'a> {
    pub(super) fd: BorrowedFd<'a>,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) fourcc: u32,
    pub(super) stride: u32,
    pub(super) offset: u32,
    pub(super) modifier: u64,
}

/// An EGLImage of a dma-buf, destroyed when dropped.
pub(super) struct Image {
    raw: EGLImage,
    display: Rc<Display>,
}

impl Image {
    /// Makes this image the storage of the texture bound to `GL_TEXTURE_2D` of the current context.
    pub(super) fn bind_texture(&self, context: &Context) {
        // SAFETY: `context` is current on this thread (it is not `Send`), and the image belongs to its display.
        unsafe { (context.image_target_texture)(glow::TEXTURE_2D, self.raw) };
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        // SAFETY: the image is valid and destroyed only here; textures using it keep their storage until deleted.
        unsafe { (self.display.destroy_image)(self.display.raw, self.raw) };
    }
}

/// An OpenGL context with no surface, current on the thread that created it. It is not `Send`, so it is used and
/// destroyed on that thread only.
pub(super) struct Context {
    raw: EGLContext,
    display: Rc<Display>,
    image_target_texture: ImageTargetTexture2dOes,
}

impl Context {
    /// Creates a 3.2 core context, or a legacy one, or an OpenGL ES 3 one, and makes it current with no surface.
    pub(super) fn new(display: &Rc<Display>) -> Result<Context> {
        let core = [
            EGL_CONTEXT_MAJOR_VERSION,
            3,
            EGL_CONTEXT_MINOR_VERSION,
            2,
            EGL_CONTEXT_OPENGL_PROFILE_MASK,
            EGL_CONTEXT_OPENGL_CORE_PROFILE_BIT,
            EGL_NONE,
        ];
        let legacy = [EGL_NONE];
        let gles = [EGL_CONTEXT_MAJOR_VERSION, 3, EGL_NONE];
        // Resolving needs no current context. The function exists with GL_OES_EGL_image, checked once current.
        // SAFETY: the type matches the extension's prototype.
        let image_target_texture = unsafe { load::<ImageTargetTexture2dOes>(c"glEGLImageTargetTexture2DOES")? };
        let raw = Self::create(display, EGL_OPENGL_API, EGL_OPENGL_BIT, &core)
            .or_else(|_| Self::create(display, EGL_OPENGL_API, EGL_OPENGL_BIT, &legacy))
            .or_else(|_| Self::create(display, EGL_OPENGL_ES_API, EGL_OPENGL_ES3_BIT, &gles))?;

        // SAFETY: as in `create`; the context has no surface, which EGL_KHR_surfaceless_context allows.
        if unsafe { eglMakeCurrent(display.raw, ptr::null_mut(), ptr::null_mut(), raw) } != EGL_TRUE {
            let message = format!("the EGL context cannot be made current ({})", error());
            // SAFETY: the context was created on this display and is not current.
            unsafe { eglDestroyContext(display.raw, raw) };
            return Err(Error::Render(message));
        }
        Ok(Context { raw, display: Rc::clone(display), image_target_texture })
    }

    fn create(display: &Display, api: EGLenum, renderable: EGLint, attributes: &[EGLint]) -> Result<EGLContext> {
        // SAFETY: `api` is a valid client API.
        if unsafe { eglBindAPI(api) } != EGL_TRUE {
            return Err(Error::Render(format!("EGL cannot bind the API {api:#x} ({})", error())));
        }
        let mut config = ptr::null_mut();
        if !display.no_config {
            let wanted = [EGL_RENDERABLE_TYPE, renderable, EGL_SURFACE_TYPE, 0, EGL_NONE];
            let mut count = 0;
            // SAFETY: the display is initialised, the attribute list terminated, and the out-pointers valid for one.
            let chosen = unsafe { eglChooseConfig(display.raw, wanted.as_ptr(), &mut config, 1, &mut count) };
            if chosen != EGL_TRUE || count < 1 {
                return Err(Error::Render(format!("no EGL config for the API {api:#x} ({})", error())));
            }
        }
        // SAFETY: the display is initialised, the config is null only with EGL_KHR_no_config_context, nothing is
        // shared, and the attribute list is terminated.
        let raw = unsafe { eglCreateContext(display.raw, config, ptr::null_mut(), attributes.as_ptr()) };
        if raw.is_null() {
            return Err(Error::Render(format!("no EGL context ({})", error())));
        }
        Ok(raw)
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        // SAFETY: the context is current on this thread; it is released from the thread before being destroyed.
        unsafe {
            eglMakeCurrent(self.display.raw, ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
            eglDestroyContext(self.display.raw, self.raw);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_lists_match_whole_names() {
        let list = "EGL_EXT_image_dma_buf_import_modifiers EGL_KHR_surfaceless_context";
        assert!(has(list, "EGL_KHR_surfaceless_context"));
        assert!(has(list, "EGL_EXT_image_dma_buf_import_modifiers"));
        assert!(!has(list, "EGL_EXT_image_dma_buf_import"), "a prefix of another name does not count");
    }
}
