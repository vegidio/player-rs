//! macOS: a player's video as IOSurface-backed `CVPixelBuffer`s, for toolkits that draw with Metal.
//!
//! A [`SurfaceRenderer`] owns a render thread with its own offscreen OpenGL context. It draws each frame mpv announces
//! into the next surface of a small ring and publishes it once the GPU has finished it. Frames never pass through the
//! CPU, also with VideoToolbox hardware decoding, which is the default for [`VideoOutput::Render`] players on macOS. No
//! OpenGL and no `unsafe` code are needed to use it.
//!
//! ```no_run
//! use player::macos::{SurfaceFormat, SurfaceRenderer};
//! use player::prelude::*;
//!
//! let player = Player::builder().video_output(VideoOutput::Render).build()?;
//! let renderer = SurfaceRenderer::builder(&player)
//!     .format(SurfaceFormat::Nv12)
//!     .on_frame(|| { /* wake the UI thread, which then repaints */ })
//!     .build()?;
//! renderer.set_size(1920, 1080); // the video area, in device pixels
//! player.load(Source::file("movie.mp4")?)?;
//!
//! // On the UI thread, when painting:
//! if let Some(frame) = renderer.latest() {
//!     let _surface = frame.pixel_buffer(); // hand it to Metal, or to GPUI's `paint_surface`
//! }
//! # Ok::<(), player::Error>(())
//! ```
//!
//! - **Threads.** mpv's threads only wake the render thread. The render thread renders, publishes and then calls
//!   `on_frame`. [`SurfaceRenderer::set_size`] and [`SurfaceRenderer::latest`] can be called from any thread and never
//!   wait on mpv.
//! - **Sizing.** Nothing is published until a size is set. Set the size of the area the video is shown in, in device
//!   pixels, so it is never upscaled afterwards. The video is letterboxed into it. A new size is rendered right away,
//!   also while paused. Setting the same size again does nothing.
//! - **Formats.** [`SurfaceFormat::Bgra`] holds mpv's RGB picture. [`SurfaceFormat::Nv12`] encodes it on the GPU as
//!   full-range BT.601 YCbCr with chroma at half resolution, since that is the only surface format GPUI's `surface()`
//!   accepts. NV12 slightly softens fine coloured detail. Both are 8-bit SDR, so mpv tone-maps HDR video.
//! - **Frames** stay unchanged until two newer frames have been published, and readable for as long as they are held,
//!   also after the renderer is dropped.
//! - **Media loaded before the renderer** shows its video once the renderer exists (see
//!   [`SurfaceRendererBuilder::build`]).
//! - **Platforms.** This module exists on macOS only; Windows and Linux have their own in `player::windows` and
//!   `player::linux`.

// Internals: every function here taking a `glow::Context` needs the `cgl::Context` it was loaded for to be current. That
// holds on the thread that created the `cgl::Context`, which is not `Send`, since nothing here makes another current.
mod cgl;
mod convert;
mod surface;
mod thread;

#[cfg(any(doc, test))]
use crate::builder::VideoOutput;
#[cfg(doc)]
use crate::error::Error;
use crate::error::Result;
use crate::player::Player;
use crate::surface::{Attachment, OnFrame};
use objc2_core_foundation::CFRetained;
use objc2_core_video::CVPixelBuffer;
use std::fmt;
use std::thread::JoinHandle;

/// Renders a player's video into IOSurface-backed `CVPixelBuffer`s on a thread it owns, and publishes the newest one.
///
/// Created with [`SurfaceRenderer::builder`]. Dropping it stops the render thread; the player keeps playing without
/// video.
pub struct SurfaceRenderer {
    attachment: Attachment<Frame, JoinHandle<()>>,
}

impl SurfaceRenderer {
    /// Starts building a surface renderer for `player`, which must be built with [`VideoOutput::Render`].
    pub fn builder(player: &Player) -> SurfaceRendererBuilder {
        SurfaceRendererBuilder {
            player: player.clone(),
            format: SurfaceFormat::default(),
            gl_renderer: GlRenderer::default(),
            on_frame: Box::new(|| {}),
        }
    }

    /// Sets the size of the frames to render, in pixels. Until it is set, and while either dimension is zero, no
    /// frame is published. A change is rendered right away, also while paused. NV12 rounds odd dimensions up by one.
    pub fn set_size(&self, width: u32, height: u32) {
        self.attachment.set_size(width, height);
    }

    /// The newest published frame, if any. Never waits on mpv or on the render thread.
    pub fn latest(&self) -> Option<Frame> {
        self.attachment.latest()
    }
}

impl fmt::Debug for SurfaceRenderer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SurfaceRenderer").field("latest", &self.latest()).finish_non_exhaustive()
    }
}

/// Builds a [`SurfaceRenderer`]; created by [`SurfaceRenderer::builder`].
pub struct SurfaceRendererBuilder {
    player: Player,
    format: SurfaceFormat,
    gl_renderer: GlRenderer,
    on_frame: OnFrame,
}

impl SurfaceRendererBuilder {
    /// The surfaces' pixel format. Defaults to [`SurfaceFormat::Bgra`].
    pub fn format(mut self, format: SurfaceFormat) -> Self {
        self.format = format;
        self
    }

    /// The OpenGL renderer to draw with. Defaults to [`GlRenderer::Auto`]. When the renderer ends up drawing with
    /// Apple's software renderer, it sets the player's `scale`, `dscale` and `cscale` options to `bilinear` (see
    /// [`GlRenderer::Software`]).
    pub fn gl_renderer(mut self, gl_renderer: GlRenderer) -> Self {
        self.gl_renderer = gl_renderer;
        self
    }

    /// Called on the render thread after each frame is published, so [`SurfaceRenderer::latest`] returns it. Never
    /// called once the renderer has been dropped.
    pub fn on_frame(mut self, on_frame: impl Fn() + Send + Sync + 'static) -> Self {
        self.on_frame = Box::new(on_frame);
        self
    }

    /// Starts the render thread and creates its OpenGL context. Media loaded before shows its video: its video track
    /// is selected again, or, for media without audio, which ends at once when its video cannot start, it is loaded
    /// again (announced by a new [`Event::Loaded`](crate::Event::Loaded) after the
    /// [`Event::Ended`](crate::Event::Ended) of the failed load).
    ///
    /// # Errors
    ///
    /// Returns [`Error::Render`] if the player was not built with [`VideoOutput::Render`], if it already has a render
    /// context or surface renderer, or if no OpenGL context can be created or used. The player keeps working.
    pub fn build(self) -> Result<SurfaceRenderer> {
        crate::render::require_render_output(&self.player)?;
        let (attachment, software) = thread::spawn(&self.player, self.format, self.gl_renderer, self.on_frame)?;
        if software {
            use_bilinear_scaling(&self.player);
        }
        crate::surface::attached(&self.player);
        Ok(SurfaceRenderer { attachment })
    }
}

impl fmt::Debug for SurfaceRendererBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SurfaceRendererBuilder")
            .field("format", &self.format)
            .field("gl_renderer", &self.gl_renderer)
            .finish_non_exhaustive()
    }
}

/// mpv's default scalers sample lookup-table textures, which Apple's software renderer often draws as black. Bilinear
/// scaling samples the video directly, and is the only scaling a CPU renderer can afford anyway.
fn use_bilinear_scaling(player: &Player) {
    for option in ["scale", "dscale", "cscale"] {
        let _ = player.set_property(option, "bilinear".to_string());
    }
}

/// A published frame: an IOSurface-backed pixel buffer holding the whole picture.
///
/// Its contents stay unchanged until at least two newer frames have been published, and it stays valid to read for as
/// long as it is held, also after the renderer has been dropped.
#[derive(Clone)]
pub struct Frame {
    buffer: CFRetained<CVPixelBuffer>,
    width: u32,
    height: u32,
    format: SurfaceFormat,
    sequence: u64,
}

// SAFETY: CoreFoundation reference counting is thread-safe, and `Frame` only hands out shared references to a buffer
// the render thread no longer writes while it can be read (see the type's docs).
unsafe impl Send for Frame {}
// SAFETY: as above.
unsafe impl Sync for Frame {}

impl Frame {
    /// The IOSurface-backed pixel buffer, as GPUI's `Window::paint_surface` and Metal texture caches take it.
    pub fn pixel_buffer(&self) -> &CFRetained<CVPixelBuffer> {
        &self.buffer
    }

    /// The width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// The height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The pixel format.
    pub fn format(&self) -> SurfaceFormat {
        self.format
    }

    /// The frame's number: 1 for a renderer's first frame, then one more for each.
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
}

impl fmt::Debug for Frame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Frame")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("format", &self.format)
            .field("sequence", &self.sequence)
            .finish_non_exhaustive()
    }
}

/// The pixel format of a surface renderer's surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum SurfaceFormat {
    /// `kCVPixelFormatType_32BGRA`: the RGB picture mpv draws, 8 bits per channel.
    #[default]
    Bgra,
    /// `kCVPixelFormatType_420YpCbCr8BiPlanarFullRange`: the picture encoded as full-range BT.601 YCbCr, with chroma
    /// at half the width and height. Width and height are even.
    Nv12,
}

/// Which of the system's OpenGL renderers a surface renderer draws with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum GlRenderer {
    /// The GPU if there is one, otherwise Apple's software renderer.
    #[default]
    Auto,
    /// The GPU. Fails where there is none, as on virtual machines.
    Hardware,
    /// Apple's software renderer, which works everywhere, slowly. mpv's default scalers draw black on it, so a surface
    /// renderer using it switches the player to bilinear scaling (`scale`, `dscale` and `cscale`).
    Software,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_gl;
    use std::any::Any;
    use std::sync::Arc;

    /// Drops the player and its surface renderer in either order, and checks that the mpv instance is released with
    /// the last of them, and not before.
    #[test]
    fn instance_is_released_with_its_last_owner() {
        if test_gl::skipped() {
            return;
        }
        for player_first in [true, false] {
            let player = Player::builder().video_output(VideoOutput::Render).mpv_option("ao", "null").build().unwrap();
            let instance = Arc::downgrade(player.handle());
            let renderer = SurfaceRenderer::builder(&player).gl_renderer(surface::tests::renderer()).build().unwrap();

            let (first, second): (Box<dyn Any>, Box<dyn Any>) = if player_first {
                (Box::new(player), Box::new(renderer))
            } else {
                (Box::new(renderer), Box::new(player))
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
                "the second drop releases the instance (player first: {player_first})"
            );
        }
    }

    #[test]
    fn renderer_and_frame_are_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SurfaceRenderer>();
        assert_send_sync::<Frame>();
    }
}
