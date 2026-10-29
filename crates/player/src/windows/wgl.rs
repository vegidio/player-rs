//! The render thread's offscreen WGL context on a hidden window, the GL function loader mpv and glow share, and
//! `WGL_NV_DX_interop2`, through which GL draws into Direct3D 11 textures.

use crate::error::{Error, Result};
use ::windows::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, GetLastError, HANDLE, HINSTANCE, HMODULE, HWND, LPARAM, LRESULT, WPARAM,
};
use ::windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11Texture2D};
use ::windows::Win32::Graphics::Gdi::{GetDC, HDC};
use ::windows::Win32::Graphics::OpenGL::{
    ChoosePixelFormat, HGLRC, PFD_DOUBLEBUFFER, PFD_DRAW_TO_WINDOW, PFD_MAIN_PLANE, PFD_SUPPORT_OPENGL, PFD_TYPE_RGBA,
    PIXELFORMATDESCRIPTOR, SetPixelFormat, wglCreateContext, wglDeleteContext, wglGetProcAddress, wglMakeCurrent,
};
use ::windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use ::windows::Win32::UI::WindowsAndMessaging::{
    CS_OWNDC, CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassExW, WINDOW_EX_STYLE, WNDCLASSEXW, WS_POPUP,
};
use ::windows::core::{BOOL, Interface, PCSTR, PCWSTR, w};
use std::ffi::{CStr, c_void};
use std::mem::transmute;

/// The hidden windows' class, registered once per process.
const CLASS: PCWSTR = w!("player-rs-surface");

const WGL_CONTEXT_MAJOR_VERSION_ARB: i32 = 0x2091;
const WGL_CONTEXT_MINOR_VERSION_ARB: i32 = 0x2092;
const WGL_CONTEXT_PROFILE_MASK_ARB: i32 = 0x9126;
const WGL_CONTEXT_CORE_PROFILE_BIT_ARB: i32 = 0x1;
const WGL_ACCESS_WRITE_DISCARD_NV: u32 = 0x2;

type CreateContextAttribs = unsafe extern "system" fn(HDC, HGLRC, *const i32) -> HGLRC;
type GetExtensionsString = unsafe extern "system" fn(HDC) -> *const i8;
type GetExtensionsStringExt = unsafe extern "system" fn() -> *const i8;
type DxOpenDevice = unsafe extern "system" fn(*mut c_void) -> HANDLE;
type DxCloseDevice = unsafe extern "system" fn(HANDLE) -> BOOL;
type DxRegisterObject = unsafe extern "system" fn(HANDLE, *mut c_void, u32, u32, u32) -> HANDLE;
type DxUnregisterObject = unsafe extern "system" fn(HANDLE, HANDLE) -> BOOL;
type DxLockObjects = unsafe extern "system" fn(HANDLE, i32, *mut HANDLE) -> BOOL;

/// A hidden window, which WGL needs for a device context with a pixel format. It is never shown.
struct Window(HWND);

impl Window {
    fn new() -> Result<Window> {
        // SAFETY: a null module name returns this executable's handle.
        let instance: HINSTANCE = unsafe { GetModuleHandleW(None) }
            .map_err(|error| Error::Render(format!("no module handle for the hidden window: {error}")))?
            .into();
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_OWNDC,
            lpfnWndProc: Some(window_procedure),
            hInstance: instance,
            lpszClassName: CLASS,
            ..Default::default()
        };
        // SAFETY: the class description is complete and its strings are static. Registering it again fails
        // harmlessly with ERROR_CLASS_ALREADY_EXISTS.
        if unsafe { RegisterClassExW(&class) } == 0 && unsafe { GetLastError() } != ERROR_CLASS_ALREADY_EXISTS {
            return Err(Error::Render(format!(
                "the hidden window's class cannot be registered: {}",
                ::windows::core::Error::from_thread()
            )));
        }
        // SAFETY: the class is registered for this module; the window is never shown, and destroyed on drop.
        let window = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                CLASS,
                PCWSTR::null(),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(instance),
                None,
            )
        }
        .map_err(|error| Error::Render(format!("the hidden window cannot be created: {error}")))?;
        Ok(Window(window))
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        // SAFETY: the window was created by `new` on this thread and is destroyed only here.
        let _ = unsafe { DestroyWindow(self.0) };
    }
}

unsafe extern "system" fn window_procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: the arguments are the ones Windows passed for this window.
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

/// A WGL context on a hidden window, current on the thread that created it: 3.2 core where the driver offers
/// `wglCreateContextAttribsARB`, otherwise a legacy context. It is not `Send`, so it is used and destroyed on that
/// thread only.
pub(super) struct Context {
    raw: HGLRC,
    dc: HDC,
    loader: Loader,
    extensions: String,
    // Destroyed after the context, by `Drop`'s end.
    _window: Window,
}

impl Context {
    /// Creates a context and makes it current on this thread.
    pub(super) fn new() -> Result<Context> {
        let fail =
            |what: &str, error: ::windows::core::Error| Error::Render(format!("no OpenGL context: {what}: {error}"));
        // Before the context, so no failure leaves one behind.
        let loader = Loader::new()?;
        let window = Window::new()?;
        // SAFETY: the window has its own device context (CS_OWNDC), valid until the window is destroyed.
        let dc = unsafe { GetDC(Some(window.0)) };
        let pixel_format = PIXELFORMATDESCRIPTOR {
            nSize: size_of::<PIXELFORMATDESCRIPTOR>() as u16,
            nVersion: 1,
            dwFlags: PFD_DRAW_TO_WINDOW | PFD_SUPPORT_OPENGL | PFD_DOUBLEBUFFER,
            iPixelType: PFD_TYPE_RGBA,
            cColorBits: 32,
            iLayerType: PFD_MAIN_PLANE.0 as u8,
            ..Default::default()
        };
        // SAFETY: `dc` is valid and the descriptor complete.
        let format = unsafe { ChoosePixelFormat(dc, &pixel_format) };
        if format == 0 {
            return Err(fail("no pixel format", ::windows::core::Error::from_thread()));
        }
        // SAFETY: as above; the window's pixel format is set once, here.
        unsafe { SetPixelFormat(dc, format, &pixel_format) }.map_err(|error| fail("SetPixelFormat", error))?;
        // SAFETY: `dc` has an OpenGL pixel format.
        let legacy = unsafe { wglCreateContext(dc) }.map_err(|error| fail("wglCreateContext", error))?;
        // SAFETY: `legacy` was created for `dc`; making it current affects only this thread.
        if let Err(error) = unsafe { wglMakeCurrent(dc, legacy) } {
            // SAFETY: the context is not current anywhere.
            let _ = unsafe { wglDeleteContext(legacy) };
            return Err(fail("wglMakeCurrent", error));
        }

        let mut context = Context { raw: legacy, dc, loader, extensions: String::new(), _window: window };
        context.extensions = context.read_extensions();
        context.upgrade_to_core();
        Ok(context)
    }

    /// Replaces the legacy context, which is current, with a 3.2 core one if the driver can create it.
    fn upgrade_to_core(&mut self) {
        let create = self.loader.get(c"wglCreateContextAttribsARB");
        if create.is_null() {
            return;
        }
        // SAFETY: the driver returned this address for `wglCreateContextAttribsARB`, which has this signature.
        let create: CreateContextAttribs = unsafe { transmute(create) };
        let attributes = [
            WGL_CONTEXT_MAJOR_VERSION_ARB,
            3,
            WGL_CONTEXT_MINOR_VERSION_ARB,
            2,
            WGL_CONTEXT_PROFILE_MASK_ARB,
            WGL_CONTEXT_CORE_PROFILE_BIT_ARB,
            0,
        ];
        // SAFETY: `dc` has an OpenGL pixel format and the attribute list is zero-terminated.
        let core = unsafe { create(self.dc, HGLRC::default(), attributes.as_ptr()) };
        if core.is_invalid() {
            return;
        }
        // SAFETY: `core` was created for `dc`; the legacy context is deleted once no longer current.
        unsafe {
            if wglMakeCurrent(self.dc, core).is_ok() {
                let _ = wglDeleteContext(self.raw);
                self.raw = core;
            } else {
                let _ = wglDeleteContext(core);
            }
        }
    }

    /// The WGL extension string, empty if the driver has neither the ARB nor the EXT query.
    fn read_extensions(&self) -> String {
        let arb = self.loader.get(c"wglGetExtensionsStringARB");
        let extensions = if arb.is_null() {
            let ext = self.loader.get(c"wglGetExtensionsStringEXT");
            if ext.is_null() {
                return String::new();
            }
            // SAFETY: the driver returned this address for `wglGetExtensionsStringEXT`, which has this signature.
            let ext: GetExtensionsStringExt = unsafe { transmute(ext) };
            // SAFETY: the function takes no arguments.
            unsafe { ext() }
        } else {
            // SAFETY: the driver returned this address for `wglGetExtensionsStringARB`, which has this signature.
            let arb: GetExtensionsString = unsafe { transmute(arb) };
            // SAFETY: `dc` is valid.
            unsafe { arb(self.dc) }
        };
        if extensions.is_null() {
            return String::new();
        }
        // SAFETY: the driver returns a NUL-terminated string that lives as long as the context.
        unsafe { CStr::from_ptr(extensions) }.to_string_lossy().into_owned()
    }

    /// Whether the driver lists the WGL extension `name`.
    pub(super) fn has_extension(&self, name: &str) -> bool {
        self.extensions.split_ascii_whitespace().any(|extension| extension == name)
    }

    /// A loader for this context's GL functions.
    pub(super) fn loader(&self) -> Loader {
        self.loader
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        // SAFETY: the context was created by `new` and is released only here, on the thread it is current on.
        unsafe {
            let _ = wglMakeCurrent(HDC::default(), HGLRC::default());
            let _ = wglDeleteContext(self.raw);
        }
    }
}

/// Resolves GL function names: extensions and GL 1.2+ through `wglGetProcAddress`, which needs a current context,
/// and the GL 1.1 functions, which it does not return, from `opengl32.dll`.
#[derive(Clone, Copy)]
pub(super) struct Loader {
    /// The `opengl32.dll` module, which stays loaded: this crate imports from it.
    opengl32: usize,
}

impl Loader {
    fn new() -> Result<Loader> {
        // SAFETY: the name is a static string; opengl32.dll is loaded, since this crate imports from it.
        let module = unsafe { GetModuleHandleW(w!("opengl32.dll")) }
            .map_err(|error| Error::Render(format!("opengl32.dll is not loaded: {error}")))?;
        Ok(Loader { opengl32: module.0 as usize })
    }

    /// The address of the GL function `name` for the context current on this thread, or null.
    pub(super) fn get(&self, name: &CStr) -> *const c_void {
        let name = PCSTR(name.as_ptr().cast());
        // SAFETY: `name` is NUL-terminated; the caller's context is current on this thread.
        if let Some(function) = unsafe { wglGetProcAddress(name) } {
            // Some drivers return small sentinels or -1 instead of null for unknown names.
            let address = function as usize;
            if !matches!(address, 1..=3 | usize::MAX) {
                return address as *const c_void;
            }
        }
        // SAFETY: the module stays loaded for the life of the process; `name` is NUL-terminated.
        unsafe { GetProcAddress(HMODULE(self.opengl32 as *mut c_void), name) }
            .map_or(std::ptr::null(), |function| function as *const c_void)
    }
}

/// `WGL_NV_DX_interop2`'s functions, for the context they were loaded for.
#[derive(Clone, Copy)]
struct Functions {
    open_device: DxOpenDevice,
    close_device: DxCloseDevice,
    register_object: DxRegisterObject,
    unregister_object: DxUnregisterObject,
    lock_objects: DxLockObjects,
    unlock_objects: DxLockObjects,
}

/// A Direct3D 11 device opened for GL through `WGL_NV_DX_interop2`. Closed on drop, after every object registered
/// with it has been unregistered.
pub(super) struct InteropDevice {
    functions: Functions,
    raw: HANDLE,
    _device: ID3D11Device,
}

impl InteropDevice {
    /// Opens `device` for `context`, which must be current on this thread. `renderer` and `adapter` name the GPUs in
    /// errors.
    pub(super) fn open(context: &Context, device: &ID3D11Device, renderer: &str, adapter: &str) -> Result<Self> {
        let functions = Self::functions(context).ok_or_else(|| {
            Error::Render(format!(
                "the OpenGL driver ({renderer}) cannot share textures with Direct3D 11: it lacks WGL_NV_DX_interop2"
            ))
        })?;
        // SAFETY: `device` is a valid D3D11 device, kept alive as long as the returned value, and the context the
        // functions were loaded for is current on this thread.
        let raw = unsafe { (functions.open_device)(device.as_raw()) };
        if raw.is_invalid() {
            return Err(Error::Render(format!(
                "the OpenGL driver ({renderer}) cannot share textures with Direct3D 11 on {adapter}: {}",
                ::windows::core::Error::from_thread()
            )));
        }
        Ok(InteropDevice { functions, raw, _device: device.clone() })
    }

    fn functions(context: &Context) -> Option<Functions> {
        if !context.has_extension("WGL_NV_DX_interop2") {
            return None;
        }
        let loader = context.loader();
        let get = |name: &CStr| Some(loader.get(name)).filter(|address| !address.is_null());
        // SAFETY: the driver returned each address for the function named, which has the signature declared above.
        unsafe {
            Some(Functions {
                open_device: transmute::<*const c_void, DxOpenDevice>(get(c"wglDXOpenDeviceNV")?),
                close_device: transmute::<*const c_void, DxCloseDevice>(get(c"wglDXCloseDeviceNV")?),
                register_object: transmute::<*const c_void, DxRegisterObject>(get(c"wglDXRegisterObjectNV")?),
                unregister_object: transmute::<*const c_void, DxUnregisterObject>(get(c"wglDXUnregisterObjectNV")?),
                lock_objects: transmute::<*const c_void, DxLockObjects>(get(c"wglDXLockObjectsNV")?),
                unlock_objects: transmute::<*const c_void, DxLockObjects>(get(c"wglDXUnlockObjectsNV")?),
            })
        }
    }

    /// Registers `texture`, of this device, as the storage of the GL texture `gl_texture`, which GL only writes.
    pub(super) fn register(&self, texture: &ID3D11Texture2D, gl_texture: u32) -> Result<InteropObject> {
        // SAFETY: `texture` belongs to this device and is kept alive by the returned object; `gl_texture` names a GL
        // texture of the current context.
        let raw = unsafe {
            (self.functions.register_object)(
                self.raw,
                texture.as_raw(),
                gl_texture,
                glow::TEXTURE_2D,
                WGL_ACCESS_WRITE_DISCARD_NV,
            )
        };
        if raw.is_invalid() {
            return Err(Error::Render(format!(
                "the Direct3D 11 texture cannot be registered with OpenGL: {}",
                ::windows::core::Error::from_thread()
            )));
        }
        Ok(InteropObject { raw, _texture: texture.clone() })
    }

    /// Unregisters `object`, after which its GL texture has no storage.
    pub(super) fn unregister(&self, object: InteropObject) {
        // SAFETY: `object` was registered with this device, and is unregistered once, here.
        let _ = unsafe { (self.functions.unregister_object)(self.raw, object.raw) };
    }

    /// Gives GL access to `object` until [`unlock`](Self::unlock).
    pub(super) fn lock(&self, object: &mut InteropObject) -> Result<()> {
        // SAFETY: `object` is registered with this device; the call reads one handle.
        let locked = unsafe { (self.functions.lock_objects)(self.raw, 1, &mut object.raw) };
        if !locked.as_bool() {
            return Err(Error::Render(format!(
                "the shared texture cannot be locked for OpenGL: {}",
                ::windows::core::Error::from_thread()
            )));
        }
        Ok(())
    }

    /// Hands `object` back to Direct3D 11. GL's work on it is ordered before Direct3D's later work on it.
    pub(super) fn unlock(&self, object: &mut InteropObject) -> Result<()> {
        // SAFETY: as in `lock`.
        let unlocked = unsafe { (self.functions.unlock_objects)(self.raw, 1, &mut object.raw) };
        if !unlocked.as_bool() {
            return Err(Error::Render(format!(
                "the shared texture cannot be unlocked: {}",
                ::windows::core::Error::from_thread()
            )));
        }
        Ok(())
    }
}

impl Drop for InteropDevice {
    fn drop(&mut self) {
        // SAFETY: the device was opened by `open` and is closed once, here.
        let _ = unsafe { (self.functions.close_device)(self.raw) };
    }
}

/// A Direct3D 11 texture registered with GL; see [`InteropDevice::register`].
pub(super) struct InteropObject {
    raw: HANDLE,
    _texture: ID3D11Texture2D,
}

#[cfg(test)]
pub(in crate::windows) mod tests {
    use super::*;
    use crate::test_gl;
    use crate::windows::thread::tests::on_render_thread;
    use glow::HasContext;

    /// A context current on this thread, with glow calling into it. Call it on a render thread
    /// ([`on_render_thread`]), which never exits.
    pub(in crate::windows) fn gl() -> (Context, glow::Context) {
        let context = Context::new().expect("a WGL context is created");
        let loader = context.loader();
        // SAFETY: `context` is current on this thread, and the loader resolves its functions.
        let gl = unsafe { glow::Context::from_loader_function_cstr(|name| loader.get(name)) };
        (context, gl)
    }

    #[test]
    fn context_is_created_and_destroyed() {
        if test_gl::skipped() {
            return;
        }
        on_render_thread(|| {
            for _ in 0..2 {
                let (context, gl) = gl();
                // SAFETY: the context is current on this thread.
                let (version, renderer) =
                    unsafe { (gl.get_parameter_string(glow::VERSION), gl.get_parameter_string(glow::RENDERER)) };
                assert!(!version.is_empty(), "the context reports a GL version");
                let interop = context.has_extension("WGL_NV_DX_interop2");
                eprintln!("WGL context: {renderer}, {version}, interop: {interop}");
                drop(gl);
                drop(context);
            }
        });
    }

    #[test]
    fn loader_resolves_core_and_extension_functions() {
        if test_gl::skipped() {
            return;
        }
        on_render_thread(|| {
            let (context, _gl) = gl();
            let loader = context.loader();
            assert!(!loader.get(c"glClear").is_null(), "a GL 1.1 function, from opengl32.dll");
            assert!(!loader.get(c"glGenFramebuffers").is_null(), "a GL 3.0 function, from the driver");
            assert!(loader.get(c"glNoSuchFunction").is_null());
        });
    }
}
