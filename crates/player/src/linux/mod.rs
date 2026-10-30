//! Linux: a player's video as dma-bufs, for toolkits that draw with Vulkan or EGL.
//!
//! A [`SurfaceRenderer`] has its own render thread with its own OpenGL context, on the EGL display of a DRM device's
//! GBM platform, with no window or surface. mpv draws each frame it announces through OpenGL straight into the next
//! buffer of a small ring of GBM buffers, and each is published once the GPU has finished it. Every buffer is a
//! single-plane RGBA dma-buf, which another graphics API on the same machine imports without a copy. Frames never pass
//! through the CPU, also with hardware decoding, which is the default for [`VideoOutput::Render`] players on Linux. No
//! OpenGL, no EGL and no `unsafe` code are needed to use it.
//!
//! ```no_run
//! use player::prelude::*;
//! use player::linux::SurfaceRenderer;
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
//!     // Import it on the toolkit's own device, such as wgpu-hal's `texture_from_dmabuf_fd` with a duplicate of the
//!     // fd, or an EGLImage, and sample it.
//!     let _fd = frame.fd().try_clone_to_owned();
//!     let (_fourcc, _modifier, _stride, _offset) = (frame.fourcc(), frame.modifier(), frame.stride(), frame.offset());
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! - **Threads.** mpv's threads only wake the render thread. The render thread renders, publishes and then calls
//!   `on_frame`. [`SurfaceRenderer::set_size`] and [`SurfaceRenderer::latest`] can be called from any thread and never
//!   wait on mpv.
//! - **Sizing.** Nothing is published until a size is set. Set the size of the area the video is shown in, in device
//!   pixels, so it is never upscaled afterwards. The video is letterboxed into it. A new size is rendered right away,
//!   also while paused. Setting the same size again does nothing.
//! - **Format.** Frames are `DRM_FORMAT_ABGR8888` buffers ([`Frame::fourcc`]): R, G, B, A bytes in memory order, which
//!   is Vulkan's `R8G8B8A8_UNORM` and wgpu's `Rgba8Unorm`. They hold mpv's RGB picture with its first row at the top,
//!   in one memory plane. They are 8-bit SDR, so mpv tone-maps HDR video.
//! - **Frames** are published only once the GPU has finished them, so they need no synchronisation to be read. A
//!   frame stays unchanged until two newer frames have been published, so take the newest from
//!   [`SurfaceRenderer::latest`] on every paint rather than sampling an old one. Its dma-buf stays valid for as long
//!   as the frame or a duplicate of its fd is held, also after the renderer is dropped.
//! - **Devices.** The renderer uses the first DRM render node, in order of device number, on which it can be built,
//!   unless one is chosen with [`SurfaceRendererBuilder::device`]. Pass the consumer's own render node, as Vulkan
//!   reports it through `VK_EXT_physical_device_drm`, so the buffers are on its GPU. Where no render node can be used,
//!   primary (`card`) nodes are tried after them, as software drivers need.
//! - **Layouts.** Buffers are linear unless [`SurfaceRendererBuilder::modifiers`] offers other DRM format modifiers the
//!   consumer can import. Linear buffers import on any device, also across GPUs; tiled ones are faster on the GPU that
//!   made them. Only layouts with one memory plane are used.
//! - **Drivers.** The EGL driver needs the GBM platform, `EGL_EXT_image_dma_buf_import(_modifiers)` and
//!   `EGL_KHR_surfaceless_context`, as Mesa's and NVIDIA's have. Without a DRM device, as on WSL2 by default,
//!   creation fails with "no DRM render node is available". On Mesa's software renderers, mpv scales bilinearly
//!   unless the player sets `scale` (see [`RenderContext`](crate::RenderContext#software-opengl)).
//! - **Hardware decoding.** [`VideoOutput::Render`] players default to `hwdec=vaapi,nvdec` on Linux: VA-API (Intel
//!   and AMD) or NVDEC (NVIDIA), both handing their frames to OpenGL without a copy, and software decoding where
//!   neither can decode. The renderer gives mpv its DRM node, which VA-API needs. Override it with
//!   [`PlayerBuilder::mpv_option`](crate::PlayerBuilder::mpv_option)`("hwdec", ...)`. This has been verified with
//!   software drivers only; Mesa on Intel and AMD GPUs, NVIDIA's driver, tiled layouts and zero-copy hardware decoding
//!   are not verified yet.
//! - **Media loaded before the renderer** shows its video once the renderer exists (see
//!   [`SurfaceRendererBuilder::build`]).
//! - **Platforms.** This module exists on Linux only; macOS and Windows have their own in `player::macos` and
//!   `player::windows`.

// Internals: every function here taking a `glow::Context` or an `egl::Context` needs that context to be current. That
// holds on the thread that created it, which is not `Send`, since nothing here makes another current.
mod egl;
mod gbm;
mod thread;

use crate::builder::VideoOutput;
use crate::error::{Error, Result};
use crate::event;
use crate::player::Player;
use crate::surface::OnFrame;
use std::fmt;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::sync::Arc;
use std::thread::JoinHandle;
use thread::Shared;

/// `DRM_FORMAT_MOD_LINEAR`: rows one after the other, the layout every importer of dma-bufs understands.
pub const MODIFIER_LINEAR: u64 = gbm::LINEAR;

/// Renders a player's video into dma-bufs on its own render thread, and publishes the newest one.
///
/// Created with [`SurfaceRenderer::builder`]. Dropping it stops the render thread and releases its OpenGL, EGL and
/// GBM state; the player keeps playing without video.
pub struct SurfaceRenderer {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
    device: DrmDevice,
    modifier: u64,
    /// The player's state shared with its event thread, told when this renderer goes away. Holding it does not keep
    /// the player alive.
    player: Arc<event::Shared>,
}

impl SurfaceRenderer {
    /// Starts building a surface renderer for `player`, which must be built with [`VideoOutput::Render`].
    pub fn builder(player: &Player) -> SurfaceRendererBuilder {
        SurfaceRendererBuilder {
            player: player.clone(),
            device: None,
            modifiers: vec![MODIFIER_LINEAR],
            on_frame: Box::new(|| {}),
        }
    }

    /// Sets the size of the frames to render, in pixels. Until it is set, and while either dimension is zero, no
    /// frame is published. A change is rendered right away, also while paused.
    pub fn set_size(&self, width: u32, height: u32) {
        self.shared.set_size(width, height);
    }

    /// The newest published frame, if any. Never waits on mpv or on the render thread.
    pub fn latest(&self) -> Option<Frame> {
        self.shared.latest()
    }

    /// The DRM device the frames' buffers are on.
    pub fn device(&self) -> DrmDevice {
        self.device
    }

    /// The DRM format modifier of the frames' buffers, one of those offered to
    /// [`SurfaceRendererBuilder::modifiers`].
    pub fn modifier(&self) -> u64 {
        self.modifier
    }
}

impl Drop for SurfaceRenderer {
    fn drop(&mut self) {
        crate::surface::detached(&self.player);
        self.shared.shutdown();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl fmt::Debug for SurfaceRenderer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SurfaceRenderer")
            .field("device", &self.device)
            .field("modifier", &format_args!("{:#x}", self.modifier))
            .field("latest", &self.latest())
            .finish_non_exhaustive()
    }
}

/// Builds a [`SurfaceRenderer`]; created by [`SurfaceRenderer::builder`].
pub struct SurfaceRendererBuilder {
    player: Player,
    device: Option<DrmDevice>,
    modifiers: Vec<u64>,
    on_frame: OnFrame,
}

impl SurfaceRendererBuilder {
    /// The DRM device to render on, by its device number, such as the consumer's own render node. Defaults to the
    /// first render node, in order of device number, on which the renderer can be built.
    pub fn device(mut self, device: DrmDevice) -> Self {
        self.device = Some(device);
        self
    }

    /// The DRM format modifiers the consumer can import RGBA (`DRM_FORMAT_ABGR8888`) buffers with, in order of
    /// preference. The renderer uses one of them that its driver can allocate with a single memory plane. Defaults to
    /// [`MODIFIER_LINEAR`] alone.
    pub fn modifiers(mut self, modifiers: impl IntoIterator<Item = u64>) -> Self {
        self.modifiers = modifiers.into_iter().collect();
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
    /// context or surface renderer, if no DRM render node is available, if none has the device number asked for, if
    /// no OpenGL context can be created on it, or if its driver cannot allocate and import a single-plane buffer with
    /// any of the modifiers offered. The player keeps working.
    pub fn build(self) -> Result<SurfaceRenderer> {
        if self.player.video_output() != VideoOutput::Render {
            return Err(Error::Render("the player was not built with VideoOutput::Render".into()));
        }
        if self.modifiers.is_empty() {
            return Err(Error::Render("no DRM format modifier was offered".into()));
        }
        let shared = Arc::new(Shared::default());
        let started = thread::spawn(&self.player, self.device, self.modifiers, self.on_frame, Arc::clone(&shared))?;
        crate::surface::attached(&self.player);
        Ok(SurfaceRenderer {
            shared,
            thread: Some(started.thread),
            device: started.device,
            modifier: started.modifier,
            player: Arc::clone(self.player.shared()),
        })
    }
}

impl fmt::Debug for SurfaceRendererBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SurfaceRendererBuilder")
            .field("device", &self.device)
            .field("modifiers", &gbm::list(&self.modifiers))
            .finish_non_exhaustive()
    }
}

/// A published frame: a single-plane RGBA dma-buf holding the whole picture.
///
/// Its contents stay unchanged until at least two newer frames have been published, and its dma-buf stays valid for
/// as long as the frame, or a duplicate of its fd, is held, also after the renderer has been dropped.
#[derive(Clone)]
pub struct Frame {
    fd: Arc<OwnedFd>,
    device: DrmDevice,
    modifier: u64,
    stride: u32,
    offset: u32,
    width: u32,
    height: u32,
    sequence: u64,
}

impl Frame {
    /// The dma-buf's file descriptor. Duplicate it ([`BorrowedFd::try_clone_to_owned`]) for an importer that takes
    /// ownership, such as Vulkan.
    pub fn fd(&self) -> BorrowedFd<'_> {
        self.fd.as_fd()
    }

    /// The DRM fourcc of the pixels: `DRM_FORMAT_ABGR8888`, R, G, B, A bytes in memory order.
    pub fn fourcc(&self) -> u32 {
        gbm::ABGR8888
    }

    /// The DRM format modifier of the buffer's layout.
    pub fn modifier(&self) -> u64 {
        self.modifier
    }

    /// The distance between the starts of two rows, in bytes.
    pub fn stride(&self) -> u32 {
        self.stride
    }

    /// Where the plane starts in the dma-buf, in bytes.
    pub fn offset(&self) -> u32 {
        self.offset
    }

    /// The DRM device the buffer is on.
    pub fn device(&self) -> DrmDevice {
        self.device
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
            .field("device", &self.device)
            .field("modifier", &format_args!("{:#x}", self.modifier))
            .field("stride", &self.stride)
            .field("offset", &self.offset)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("sequence", &self.sequence)
            .finish_non_exhaustive()
    }
}

/// A DRM device node's device number (`st_rdev`), as `VK_EXT_physical_device_drm` reports a Vulkan device's render
/// node (`renderMajor`, `renderMinor`). Render nodes are usually 226:128 and up.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct DrmDevice {
    /// The major number: 226 for DRM.
    pub major: u32,
    /// The minor number.
    pub minor: u32,
}

impl fmt::Display for DrmDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.major, self.minor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::any::Any;

    /// Drops the player and its surface renderer in either order, and checks that the mpv instance is released with
    /// the last of them, and not before.
    #[test]
    fn instance_is_released_with_its_last_owner() {
        if crate::test_gl::skipped() || !thread::tests::usable() {
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
    fn device_is_shown_as_major_and_minor() {
        assert_eq!(DrmDevice { major: 226, minor: 128 }.to_string(), "226:128");
    }

    #[test]
    fn no_modifiers_is_an_error() {
        let player = Player::builder().video_output(VideoOutput::Render).mpv_option("ao", "null").build().unwrap();
        let Err(Error::Render(message)) = SurfaceRenderer::builder(&player).modifiers([]).build() else {
            panic!("a renderer was built without modifiers")
        };
        assert!(message.contains("modifier"), "{message}");
    }
}
