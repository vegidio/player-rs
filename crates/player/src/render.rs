//! Drawing a player's video into an OpenGL framebuffer the application owns: [`RenderContext`].

use crate::builder::VideoOutput;
use crate::error::{Error, Result, mpv_message};
use crate::mpv::Handle;
use crate::mpv::render::RawContext;
use crate::player::Player;
use std::ffi::{CStr, c_int, c_void};
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// The OpenGL framebuffer a [`RenderContext`] draws into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderTarget {
    /// The framebuffer object's name; `0` is the default framebuffer of the current GL context.
    pub fbo: u32,
    /// The width to draw at, in pixels.
    pub width: u32,
    /// The height to draw at, in pixels.
    pub height: u32,
}

/// Draws a player's video into OpenGL framebuffers the application owns, with mpv doing all decoding, scaling, colour
/// conversion and subtitle rendering on the GPU.
///
/// The player must be built with [`VideoOutput::Render`], and the render context created before the first
/// [`Player::load`]: media loaded while no render context exists plays without video until it is loaded again.
///
/// # Threads
///
/// | Thread | What runs there |
/// |---|---|
/// | mpv's own threads | `on_frame`, which must only signal the render thread (wake an event loop, notify a condvar, send on a channel) |
/// | The render thread, with the GL context current | [`new`](Self::new), [`update`](Self::update), [`render`](Self::render), [`report_swap`](Self::report_swap) and the drop |
/// | Any thread | [`Player`] methods |
///
/// A render context is [`Send`], so it can be created on one thread and moved to the render thread, but not
/// [`Sync`]: mpv allows one call on it at a time. On the render thread, prefer [`Player::state`], which never waits
/// on mpv, to the `Player` methods that do, as those can delay frames.
///
/// # Lifetime
///
/// A render context keeps the player's mpv instance alive, so it may be dropped before or after the last [`Player`]
/// clone. Dropping the last `Player` stops playback; the instance is released once the render context is dropped too.
/// Dropping the render context while media plays turns off video, and playback continues without it.
pub struct RenderContext {
    // Dropped in declaration order: mpv's render context first, as mpv requires it to be freed before the instance;
    // then the flag, so a new render context can be created; then this reference to the instance.
    raw: RawContext,
    _active: ActiveFlag,
    _mpv: Arc<Handle>,
}

impl RenderContext {
    /// Creates the render context for `player`.
    ///
    /// - `get_proc_address` returns the address of an OpenGL function of the current GL context, or null, as
    ///   `eglGetProcAddress`, `wglGetProcAddress` or glutin's `Display::get_proc_address` do. mpv may call it at any
    ///   time while the render context exists.
    /// - `on_frame` is called by mpv, on its own threads, whenever a new frame is ready or the picture must be
    ///   redrawn. It must only signal the render thread, which then calls [`update`](Self::update) and
    ///   [`render`](Self::render). It must not call any method of this crate, since mpv may be waiting on it. It is
    ///   not called again once the render context has been dropped.
    ///
    /// # Safety
    ///
    /// The OpenGL context that `get_proc_address` resolves functions for must be current on the calling thread for
    /// this call, for every later method call and for the drop, and it must outlive the render context. It must
    /// support OpenGL 2.1 or later, or OpenGL ES 2.0 or later.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Render`] if the player was not built with [`VideoOutput::Render`], if it already has a
    /// render context, or if mpv cannot render with the GL context. The player keeps working in every case.
    pub unsafe fn new(
        player: &Player,
        get_proc_address: impl FnMut(&CStr) -> *const c_void + Send + 'static,
        on_frame: impl Fn() + Send + Sync + 'static,
    ) -> Result<RenderContext> {
        // SAFETY: the caller's guarantees are `new`'s; no DRM device is given.
        unsafe { Self::with_drm_device(player, get_proc_address, on_frame, None) }
    }

    /// [`new`](Self::new), also giving mpv the DRM device the GL context renders on, for hardware decoding.
    ///
    /// # Safety
    ///
    /// As for [`new`](Self::new); `drm_render_fd`, if given, must be an open DRM device that stays open until the
    /// render context is dropped.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) unsafe fn with_drm_device(
        player: &Player,
        get_proc_address: impl FnMut(&CStr) -> *const c_void + Send + 'static,
        on_frame: impl Fn() + Send + Sync + 'static,
        drm_render_fd: Option<c_int>,
    ) -> Result<RenderContext> {
        if player.video_output() != VideoOutput::Render {
            return Err(Error::Render("the player was not built with VideoOutput::Render".into()));
        }
        let active = ActiveFlag::claim(player.render_active())
            .ok_or_else(|| Error::Render("the player already has a render context".into()))?;

        // SAFETY: the caller guarantees the GL context is current now and for every later call and the drop, and that
        // the DRM device stays open.
        let raw = unsafe {
            RawContext::create(player.handle(), Box::new(get_proc_address), Box::new(on_frame), drm_render_fd)
        }
        .map_err(|code| Error::Render(format!("mpv could not create the render context: {}", mpv_message(code))))?;

        Ok(RenderContext { raw, _active: active, _mpv: Arc::clone(player.handle()) })
    }

    /// Whether a new video frame is ready to be drawn. Call it after each `on_frame` notification; when it returns
    /// `false`, the notification asked for a redraw of the current picture, which [`render`](Self::render) also does.
    pub fn update(&mut self) -> bool {
        self.raw.update()
    }

    /// Draws the current video frame into `target`, scaled to fit with its aspect ratio kept and the rest filled
    /// black. With no media loaded the target is filled black.
    ///
    /// Without `flip_y`, the video's top row is drawn into the framebuffer's row 0, which suits a texture later read
    /// top row first (as Metal, Direct3D and Vulkan do). Set `flip_y` when the result is shown through OpenGL, which
    /// puts row 0 at the bottom, as when drawing into the default framebuffer (`fbo` 0).
    ///
    /// # Errors
    ///
    /// Returns [`Error::Render`] if the target's width or height is zero or above `i32::MAX`, without drawing, or if
    /// mpv fails to draw.
    pub fn render(&mut self, target: RenderTarget, flip_y: bool) -> Result<()> {
        let (fbo, width, height) = check_target(target)?;
        self.raw.render(fbo, width, height, flip_y).map_err(|code| Error::Render(mpv_message(code)))
    }

    /// Tells mpv that the frame last drawn is now on screen, which mpv uses to time the frames that follow. Optional:
    /// playback continues without it.
    pub fn report_swap(&mut self) {
        self.raw.report_swap();
    }
}

impl fmt::Debug for RenderContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RenderContext").finish_non_exhaustive()
    }
}

/// Validates a target and converts it to mpv's `int`s.
fn check_target(target: RenderTarget) -> Result<(c_int, c_int, c_int)> {
    let RenderTarget { fbo, width, height } = target;
    let invalid = || Error::Render(format!("invalid target: framebuffer {fbo}, {width}x{height}"));
    if width == 0 || height == 0 {
        return Err(invalid());
    }
    let int = |value: u32| c_int::try_from(value).map_err(|_| invalid());
    Ok((int(fbo)?, int(width)?, int(height)?))
}

/// Marks a player as having a render context, for as long as this value lives.
struct ActiveFlag(Arc<AtomicBool>);

impl ActiveFlag {
    /// Sets the flag, or returns `None` if it is already set.
    fn claim(flag: &Arc<AtomicBool>) -> Option<Self> {
        flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).ok()?;
        Some(Self(Arc::clone(flag)))
    }
}

impl Drop for ActiveFlag {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_gl::{self as gl, Gl};
    use std::any::Any;

    fn player(video_output: VideoOutput) -> Player {
        Player::builder().video_output(video_output).mpv_option("ao", "null").build().unwrap()
    }

    #[test]
    fn rejects_a_player_without_render_output() {
        let player = player(VideoOutput::Null);
        // SAFETY: creation fails before mpv or the loader is used, so no GL context is needed.
        let result = unsafe { RenderContext::new(&player, |_| std::ptr::null(), || {}) };
        let Err(Error::Render(message)) = result else {
            panic!("a render context was created: {result:?}")
        };
        assert!(message.contains("VideoOutput::Render"), "{message}");

        // The player keeps working.
        player.set_volume(0.5).unwrap();
        assert_eq!(player.get_property::<f64>("volume").unwrap(), 50.0);
    }

    #[test]
    fn rejects_a_second_render_context() {
        let player = player(VideoOutput::Render);
        let _first = ActiveFlag::claim(player.render_active()).unwrap();
        // SAFETY: as above.
        let result = unsafe { RenderContext::new(&player, |_| std::ptr::null(), || {}) };
        let Err(Error::Render(message)) = result else {
            panic!("a second render context was created: {result:?}")
        };
        assert!(message.contains("already has a render context"), "{message}");
    }

    #[test]
    fn active_flag_is_released_on_drop() {
        let flag = Arc::new(AtomicBool::new(false));
        let first = ActiveFlag::claim(&flag).unwrap();
        assert!(ActiveFlag::claim(&flag).is_none());
        drop(first);
        assert!(ActiveFlag::claim(&flag).is_some());
    }

    #[test]
    fn rejects_empty_and_oversized_targets() {
        for (width, height) in [(0, 120), (160, 0), (0, 0), (u32::MAX, 120)] {
            let result = check_target(RenderTarget { fbo: 0, width, height });
            assert!(matches!(result, Err(Error::Render(_))), "{width}x{height} was accepted");
        }
        assert_eq!(check_target(RenderTarget { fbo: 3, width: 160, height: 120 }).unwrap(), (3, 160, 120));
    }

    /// Drops the player and its render context in either order, and checks that the mpv instance is released with
    /// the last of them, and not before.
    #[test]
    fn instance_is_released_with_its_last_owner() {
        if gl::skipped() {
            return;
        }
        let gl = Gl::new();
        for player_first in [true, false] {
            let player = player(VideoOutput::Render);
            let instance = Arc::downgrade(player.handle());
            // SAFETY: `gl` is current on this thread, where the render context is dropped, and outlives it.
            let render = unsafe { RenderContext::new(&player, gl.loader(), || {}) }.unwrap();

            let (first, second): (Box<dyn Any>, Box<dyn Any>) = if player_first {
                (Box::new(player), Box::new(render))
            } else {
                (Box::new(render), Box::new(player))
            };
            drop(first);
            assert!(
                instance.strong_count() > 0,
                "the instance outlives the first drop (player first: {player_first})"
            );
            drop(second);
            assert_eq!(
                instance.strong_count(),
                0,
                "the instance is released by the second drop (player first: {player_first})"
            );
        }
    }

    #[test]
    fn render_context_is_send_but_not_sync() {
        fn assert_send<T: Send>() {}
        assert_send::<RenderContext>();

        // Resolves only if `RenderContext` is not `Sync`: otherwise both impls apply and the call is ambiguous.
        trait AmbiguousIfSync<A> {
            fn check() {}
        }
        impl<T: ?Sized> AmbiguousIfSync<()> for T {}
        impl<T: ?Sized + Sync> AmbiguousIfSync<u8> for T {}
        <RenderContext as AmbiguousIfSync<_>>::check();
    }
}
