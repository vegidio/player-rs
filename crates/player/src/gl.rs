//! OpenGL steps every platform's render thread takes alike. Each needs `gl`'s context to be current on this thread.

use crate::error::{Error, Result};
use glow::HasContext;

/// How long to wait for the GPU to finish a frame before giving up on it, in nanoseconds.
#[cfg(any(target_os = "macos", target_os = "linux"))]
const FENCE_TIMEOUT: i32 = 1_000_000_000;

/// Creates a framebuffer drawing into `texture`, bound to `target`, and checks that it is complete. Deletes the
/// framebuffer again on failure; `texture` stays the caller's. `what` names the texture in the error.
///
/// # Safety
///
/// `gl`'s context is current on this thread, and `texture` belongs to it.
pub(crate) unsafe fn framebuffer(
    gl: &glow::Context,
    target: u32,
    texture: glow::NativeTexture,
    what: &str,
) -> Result<glow::NativeFramebuffer> {
    // SAFETY: as the caller guarantees; the framebuffer is unbound before returning.
    unsafe {
        let fbo = gl.create_framebuffer().map_err(Error::Render)?;
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
        gl.framebuffer_texture_2d(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, target, Some(texture), 0);
        let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
        gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        if status != glow::FRAMEBUFFER_COMPLETE {
            gl.delete_framebuffer(fbo);
            return Err(Error::Render(format!("the {what} framebuffer is incomplete (status {status:#x})")));
        }
        Ok(fbo)
    }
}

/// Waits until the GPU has finished every command so far, with a fence.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn fence_wait(gl: &glow::Context) -> Result<()> {
    // SAFETY: the context is current on this thread; the fence is deleted right after the wait.
    unsafe {
        let fence = gl.fence_sync(glow::SYNC_GPU_COMMANDS_COMPLETE, 0).map_err(Error::Render)?;
        let status = gl.client_wait_sync(fence, glow::SYNC_FLUSH_COMMANDS_BIT, FENCE_TIMEOUT);
        gl.delete_sync(fence);
        if status != glow::ALREADY_SIGNALED && status != glow::CONDITION_SATISFIED {
            return Err(Error::Render(format!("the GPU did not finish the frame (status {status:#x})")));
        }
    }
    Ok(())
}
