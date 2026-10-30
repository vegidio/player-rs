//! Windows: a player's video as Direct3D 11 textures shared through NT handles, for toolkits that draw with Direct3D.
//!
//! A [`SurfaceRenderer`] has its own render thread with its own offscreen OpenGL context and Direct3D 11 device. mpv draws
//! each frame it announces through OpenGL into a Direct3D 11 texture (`WGL_NV_DX_interop2`), which is copied on the
//! GPU into the next texture of a small ring and published once the GPU has finished it. Frames never pass through
//! the CPU, also with hardware decoding, which is the default for [`VideoOutput::Render`] players on Windows. No
//! OpenGL, no Direct3D device of the renderer and no `unsafe` code are needed to use it.
//!
//! ```no_run
//! use player::prelude::*;
//! use player::windows::SurfaceRenderer;
//!
//! let player = Player::builder().video_output(VideoOutput::Render).build()?;
//! let renderer = SurfaceRenderer::builder(&player)
//!     .on_frame(|| { /* wake the UI thread, which then repaints */ })
//!     .build()?;
//! renderer.set_size(1920, 1080); // the video area, in device pixels
//! player.load(Source::file("movie.mp4")?)?;
//!
//! // On the UI thread, on every paint:
//! if let Some(frame) = renderer.latest() {
//!     // Open it on the toolkit's own device on `frame.adapter()`, with `ID3D11Device1::OpenSharedResource1`
//!     // (or `ID3D12Device::OpenSharedHandle`), and sample it.
//!     let _handle = frame.shared_handle();
//! }
//! # Ok::<(), player::Error>(())
//! ```
//!
//! - **Threads.** mpv's threads only wake the render thread. The render thread renders, publishes and then calls
//!   `on_frame`. [`SurfaceRenderer::set_size`] and [`SurfaceRenderer::latest`] can be called from any thread and never
//!   wait on mpv. Render threads never exit: a dropped renderer's thread is reused by the next one built, because
//!   NVIDIA's OpenGL driver can deadlock the process when a thread that used it exits.
//! - **Sizing.** Nothing is published until a size is set. Set the size of the area the video is shown in, in device
//!   pixels, so it is never upscaled afterwards. The video is letterboxed into it. A new size is rendered right away,
//!   also while paused. Setting the same size again does nothing.
//! - **Format.** Frames are `DXGI_FORMAT_B8G8R8A8_UNORM` textures holding mpv's RGB picture, row 0 at the top. They are
//!   8-bit SDR, so mpv tone-maps HDR video.
//! - **Frames** are published only once the GPU has finished them, so they need no synchronisation to be read. A
//!   frame stays unchanged until two newer frames have been published, so take the newest from
//!   [`SurfaceRenderer::latest`] on every paint rather than sampling an old one. Its texture stays valid for as long
//!   as the frame or a handle opened from it is held, also after the renderer is dropped.
//! - **Adapters.** The renderer uses the first DXGI adapter that supports Direct3D 11, the one GPUI picks, unless one
//!   is chosen by its LUID with [`SurfaceRendererBuilder::adapter`]. The consumer's device must be on the same adapter
//!   ([`Frame::adapter`]).
//! - **Hybrid GPUs.** OpenGL runs on whichever GPU its driver picks, which on laptops with two GPUs can differ from the
//!   Direct3D adapter; [`SurfaceRendererBuilder::build`] then fails. Steer the application to one GPU: export
//!   `NvOptimusEnablement` and `AmdPowerXpressRequestHighPerformance` from the executable, or choose the GPU for it in
//!   Windows' graphics settings.
//! - **Drivers.** The OpenGL driver must have `WGL_NV_DX_interop2`, as NVIDIA's, AMD's and Intel's do. Mesa's software
//!   OpenGL does not, so there creation fails.
//! - **Hardware decoding.** [`VideoOutput::Render`] players default to mpv's `hwdec=auto-safe` on Windows, which uses
//!   NVDEC on NVIDIA GPUs without copying frames, and software decoding where no decoder hands its frames to OpenGL.
//!   Override it with [`PlayerBuilder::mpv_option`](crate::PlayerBuilder::mpv_option)`("hwdec", ...)`.
//! - **Media loaded before the renderer** shows its video once the renderer exists (see
//!   [`SurfaceRendererBuilder::build`]).
//! - **Platforms.** This module exists on Windows only; macOS and Linux have their own in `player::macos` and
//!   `player::linux`.

// Internals: every function here taking a `glow::Context` or an `InteropDevice` needs the `wgl::Context` it was loaded
// for to be current. That holds on the thread that created the `wgl::Context`, which is not `Send`, since nothing here
// makes another current.
mod d3d;
mod thread;
mod wgl;

#[cfg(any(doc, test))]
use crate::builder::VideoOutput;
#[cfg(doc)]
use crate::error::Error;
use crate::error::Result;
use crate::player::Player;
use crate::surface::{Attachment, OnFrame};
use std::fmt;
use std::os::windows::io::{AsHandle, BorrowedHandle, OwnedHandle};
use std::sync::Arc;

/// Renders a player's video into Direct3D 11 textures on its own render thread, and publishes the newest one.
///
/// Created with [`SurfaceRenderer::builder`]. Dropping it releases the render thread's OpenGL and Direct3D state and
/// returns the thread to the pool; the player keeps playing without video.
pub struct SurfaceRenderer {
    attachment: Attachment<Frame, thread::Joined>,
    adapter: AdapterLuid,
}

impl SurfaceRenderer {
    /// Starts building a surface renderer for `player`, which must be built with [`VideoOutput::Render`].
    pub fn builder(player: &Player) -> SurfaceRendererBuilder {
        SurfaceRendererBuilder { player: player.clone(), adapter: None, on_frame: Box::new(|| {}) }
    }

    /// Sets the size of the frames to render, in pixels. Until it is set, and while either dimension is zero, no
    /// frame is published. A change is rendered right away, also while paused.
    pub fn set_size(&self, width: u32, height: u32) {
        self.attachment.set_size(width, height);
    }

    /// The newest published frame, if any. Never waits on mpv or on the render thread.
    pub fn latest(&self) -> Option<Frame> {
        self.attachment.latest()
    }

    /// The adapter the frames' textures are on.
    pub fn adapter(&self) -> AdapterLuid {
        self.adapter
    }
}

impl fmt::Debug for SurfaceRenderer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SurfaceRenderer")
            .field("adapter", &self.adapter)
            .field("latest", &self.latest())
            .finish_non_exhaustive()
    }
}

/// Builds a [`SurfaceRenderer`]; created by [`SurfaceRenderer::builder`].
pub struct SurfaceRendererBuilder {
    player: Player,
    adapter: Option<AdapterLuid>,
    on_frame: OnFrame,
}

impl SurfaceRendererBuilder {
    /// The adapter to render on, by its LUID, such as the one the consumer's own device is on. Defaults to the first
    /// DXGI adapter that supports Direct3D 11, which is the one GPUI picks.
    pub fn adapter(mut self, adapter: AdapterLuid) -> Self {
        self.adapter = Some(adapter);
        self
    }

    /// Called on the render thread after each frame is published, so [`SurfaceRenderer::latest`] returns it. Never
    /// called once the renderer has been dropped.
    pub fn on_frame(mut self, on_frame: impl Fn() + Send + Sync + 'static) -> Self {
        self.on_frame = Box::new(on_frame);
        self
    }

    /// Starts the render thread and creates its OpenGL context and Direct3D 11 device. Media loaded before shows its
    /// video: its video track is selected again, or, for media without audio, which ends at once when its video cannot
    /// start, it is loaded again (announced by a new [`Event::Loaded`](crate::Event::Loaded) after the
    /// [`Event::Ended`](crate::Event::Ended) of the failed load).
    ///
    /// # Errors
    ///
    /// Returns [`Error::Render`] if the player was not built with [`VideoOutput::Render`], if it already has a render
    /// context or surface renderer, if no OpenGL context or Direct3D 11 device can be created, if no adapter has the
    /// LUID asked for, or if the OpenGL driver cannot share textures with Direct3D 11 on the adapter. The player keeps
    /// working.
    pub fn build(self) -> Result<SurfaceRenderer> {
        crate::render::require_render_output(&self.player)?;
        let (attachment, adapter) = thread::spawn(&self.player, self.adapter, self.on_frame)?;
        crate::surface::attached(&self.player);
        Ok(SurfaceRenderer { attachment, adapter })
    }
}

impl fmt::Debug for SurfaceRendererBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SurfaceRendererBuilder").field("adapter", &self.adapter).finish_non_exhaustive()
    }
}

/// A published frame: a BGRA Direct3D 11 texture holding the whole picture, shared through an NT handle.
///
/// Its contents stay unchanged until at least two newer frames have been published, and its texture stays valid for
/// as long as the frame is held, also after the renderer has been dropped.
#[derive(Clone)]
pub struct Frame {
    handle: Arc<OwnedHandle>,
    adapter: AdapterLuid,
    width: u32,
    height: u32,
    sequence: u64,
}

impl Frame {
    /// The NT handle of the `DXGI_FORMAT_B8G8R8A8_UNORM` texture, for `ID3D11Device1::OpenSharedResource1` or
    /// `ID3D12Device::OpenSharedHandle` on a device on [`Frame::adapter`]. A texture opened from it stays valid for as
    /// long as it is held.
    pub fn shared_handle(&self) -> BorrowedHandle<'_> {
        self.handle.as_handle()
    }

    /// The adapter the texture is on.
    pub fn adapter(&self) -> AdapterLuid {
        self.adapter
    }

    /// The width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// The height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The frame's number: 1 for a renderer's first frame, then one more for each.
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
}

impl fmt::Debug for Frame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Frame")
            .field("adapter", &self.adapter)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("sequence", &self.sequence)
            .finish_non_exhaustive()
    }
}

/// A graphics adapter's locally unique identifier: DXGI's `AdapterLuid`, as `DXGI_ADAPTER_DESC1` and
/// `ID3D12Device::GetAdapterLuid` report it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct AdapterLuid {
    /// `LUID::LowPart`.
    pub low: u32,
    /// `LUID::HighPart`.
    pub high: i32,
}

impl fmt::Display for AdapterLuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:08x}:{:08x}", self.high, self.low)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::any::Any;

    /// Whether this machine cannot run the renderer: GL tests are skipped, or its OpenGL driver lacks the interop,
    /// as Mesa's on CI does.
    pub(super) fn skipped() -> bool {
        if crate::test_gl::skipped() {
            return true;
        }
        let interop = thread::tests::on_render_thread(|| {
            wgl::Context::new().is_ok_and(|context| context.has_extension("WGL_NV_DX_interop2"))
        });
        if !interop {
            eprintln!("skipped: the OpenGL driver lacks WGL_NV_DX_interop2");
        }
        !interop
    }

    /// Drops the player and its surface renderer in either order, and checks that the mpv instance is released with
    /// the last of them, and not before.
    #[test]
    fn instance_is_released_with_its_last_owner() {
        if skipped() {
            return;
        }
        for player_first in [true, false] {
            let player = Player::builder().video_output(VideoOutput::Render).mpv_option("ao", "null").build().unwrap();
            let instance = Arc::downgrade(player.handle());
            let renderer = SurfaceRenderer::builder(&player).build().unwrap();

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

    #[test]
    fn luid_is_shown_in_hex() {
        assert_eq!(AdapterLuid { low: 0x130b2, high: 0 }.to_string(), "00000000:000130b2");
        assert_eq!(AdapterLuid { low: 1, high: -1 }.to_string(), "ffffffff:00000001");
    }
}
