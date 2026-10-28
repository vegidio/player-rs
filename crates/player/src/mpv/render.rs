//! The only place that calls libmpv's render API (`render.h`, `render_gl.h`), with the OpenGL backend.
//!
//! Every function here requires the GL context the render context was created with to be current on the calling
//! thread. [`RenderContext`](crate::RenderContext) makes that its callers' contract (its `unsafe` constructor), and
//! takes `&mut self` so only one call runs at a time, as mpv requires.

use super::{Handle, check};
use crate::sys;
use std::ffi::{CStr, c_char, c_int, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::{self, NonNull};
use std::sync::{Mutex, PoisonError};

/// Resolves a GL function name to its address for the render context's GL context.
pub(crate) type Loader = Box<dyn FnMut(&CStr) -> *const c_void + Send + 'static>;

/// Called by mpv, on its own threads, when a new frame is ready or the picture must be redrawn.
pub(crate) type Callback = Box<dyn Fn() + Send + Sync + 'static>;

/// Owns one `mpv_render_context` and the loader and callback mpv holds pointers to. Freed when dropped, after which
/// mpv no longer calls either.
pub(crate) struct RawContext {
    ctx: NonNull<sys::mpv_render_context>,
    // Boxed so the pointers given to mpv stay valid however this value moves. The loader is behind a mutex because
    // mpv does not document from where it calls it.
    _loader: Box<Mutex<Loader>>,
    _callback: Box<Callback>,
}

// SAFETY: mpv allows a render context to be used from any thread, one call at a time; `RawContext`'s methods take
// `&mut self` and it is not `Sync`, so calls cannot overlap. The loader and callback it owns are `Send`.
unsafe impl Send for RawContext {}

impl RawContext {
    /// Creates an OpenGL render context for `mpv` and installs `callback` as its update callback.
    ///
    /// # Safety
    ///
    /// The GL context that `loader` resolves functions for must be current on the calling thread, and must stay
    /// current for every later call on the returned value and for its drop.
    pub(crate) unsafe fn create(mpv: &Handle, loader: Loader, callback: Callback) -> Result<Self, c_int> {
        let loader = Box::new(Mutex::new(loader));
        let callback = Box::new(callback);

        let mut init = sys::mpv_opengl_init_params {
            get_proc_address: Some(get_proc_address),
            get_proc_address_ctx: ptr::from_ref::<Mutex<Loader>>(&loader).cast_mut().cast(),
        };
        let mut params = [
            param(sys::MPV_RENDER_PARAM_API_TYPE, sys::MPV_RENDER_API_TYPE_OPENGL.as_ptr().cast_mut().cast()),
            param(sys::MPV_RENDER_PARAM_OPENGL_INIT_PARAMS, (&raw mut init).cast()),
            end(),
        ];

        let mut ctx = ptr::null_mut();
        // SAFETY: the handle is valid and initialised; `params` is terminated by MPV_RENDER_PARAM_INVALID, its API
        // type is a NUL-terminated string, and `init` outlives the call. `get_proc_address_ctx` points to the boxed
        // loader, which lives as long as the context. The caller guarantees the GL context is current.
        check(unsafe { sys::mpv_render_context_create(&mut ctx, mpv.as_ptr(), params.as_mut_ptr()) })?;
        let ctx = NonNull::new(ctx).ok_or(sys::MPV_ERROR_GENERIC)?;

        // SAFETY: `ctx` is valid; `callback_ctx` points to the boxed callback, which lives until after
        // `mpv_render_context_free`, the point from which mpv no longer calls it.
        unsafe {
            sys::mpv_render_context_set_update_callback(
                ctx.as_ptr(),
                Some(on_update),
                ptr::from_ref::<Callback>(&callback).cast_mut().cast(),
            );
        }

        Ok(Self { ctx, _loader: loader, _callback: callback })
    }

    /// Whether a new frame is ready to be drawn (`MPV_RENDER_UPDATE_FRAME`).
    pub(crate) fn update(&mut self) -> bool {
        // SAFETY: the context is valid, the GL context is current (see `create`), and `&mut self` excludes
        // concurrent calls.
        let flags = unsafe { sys::mpv_render_context_update(self.ctx.as_ptr()) };
        flags & sys::MPV_RENDER_UPDATE_FRAME as u64 != 0
    }

    /// Draws the current frame into framebuffer `fbo`, `width` x `height` pixels, optionally flipped vertically.
    pub(crate) fn render(&mut self, fbo: c_int, width: c_int, height: c_int, flip_y: bool) -> Result<(), c_int> {
        // `internal_format` 0: unknown, which mpv accepts.
        let mut target = sys::mpv_opengl_fbo { fbo, w: width, h: height, internal_format: 0 };
        let mut flip = c_int::from(flip_y);
        let mut params = [
            param(sys::MPV_RENDER_PARAM_OPENGL_FBO, (&raw mut target).cast()),
            param(sys::MPV_RENDER_PARAM_FLIP_Y, (&raw mut flip).cast()),
            end(),
        ];
        // SAFETY: as in `update`; `params` is terminated by MPV_RENDER_PARAM_INVALID and its data outlives the call.
        check(unsafe { sys::mpv_render_context_render(self.ctx.as_ptr(), params.as_mut_ptr()) })
    }

    /// Tells mpv that the last drawn frame was presented.
    pub(crate) fn report_swap(&mut self) {
        // SAFETY: as in `update`.
        unsafe { sys::mpv_render_context_report_swap(self.ctx.as_ptr()) }
    }
}

impl Drop for RawContext {
    fn drop(&mut self) {
        // SAFETY: the context is valid and freed only here, with the GL context current (see `create`). Once this
        // returns mpv no longer calls the loader or the callback, so they are dropped right after, with the fields.
        unsafe { sys::mpv_render_context_free(self.ctx.as_ptr()) }
    }
}

fn param(type_: sys::mpv_render_param_type, data: *mut c_void) -> sys::mpv_render_param {
    sys::mpv_render_param { type_, data }
}

/// The terminator every parameter array ends with.
fn end() -> sys::mpv_render_param {
    param(sys::MPV_RENDER_PARAM_INVALID, ptr::null_mut())
}

/// mpv's `get_proc_address`: forwards to the boxed [`Loader`].
unsafe extern "C" fn get_proc_address(ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    // SAFETY: `ctx` is the `Mutex<Loader>` given in `create`, alive as long as the render context; `name` is a
    // NUL-terminated string mpv keeps alive for the call.
    let (loader, name) = unsafe { (&*ctx.cast::<Mutex<Loader>>(), CStr::from_ptr(name)) };
    // A panic must not unwind into C; a function that cannot be resolved is reported as null.
    catch_unwind(AssertUnwindSafe(|| loader.lock().unwrap_or_else(PoisonError::into_inner)(name)))
        .map_or(ptr::null_mut(), <*const c_void>::cast_mut)
}

/// mpv's update callback: calls the boxed [`Callback`].
unsafe extern "C" fn on_update(ctx: *mut c_void) {
    // SAFETY: `ctx` is the `Callback` given in `create`, alive until after `mpv_render_context_free`.
    let callback = unsafe { &*ctx.cast::<Callback>() };
    // A panic must not unwind into C, and mpv has no way to receive it.
    let _ = catch_unwind(AssertUnwindSafe(callback));
}
