//! The render thread: owns the CGL context, mpv's render context and every GL object, renders each frame into the
//! next surface of a ring and publishes it.

use super::cgl::{Context, Loader};
use super::convert::{Converter, Scratch};
use super::surface::Surface;
use super::{Frame, GlRenderer, SurfaceFormat};
use crate::error::{Error, Result};
use crate::player::Player;
use crate::render::{RenderContext, RenderTarget};
use glow::HasContext;
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;

/// Surfaces in the ring: a published frame is overwritten only after two newer ones are published.
const RING: usize = 3;

/// How long to wait for the GPU to finish a frame before giving up on it, in nanoseconds.
const FENCE_TIMEOUT: i32 = 1_000_000_000;

/// The host's notification, called on the render thread after each publication.
pub(super) type OnFrame = Box<dyn Fn() + Send + Sync + 'static>;

/// What the render thread and the [`SurfaceRenderer`](super::SurfaceRenderer) share.
#[derive(Default)]
pub(super) struct Shared {
    signals: Mutex<Signals>,
    wake: Condvar,
    latest: Mutex<Option<Frame>>,
}

#[derive(Default)]
struct Signals {
    /// mpv announced a frame or asked for a redraw.
    mpv_frame: bool,
    /// The size requested by the host, before rounding to the format.
    size: Option<(u32, u32)>,
    /// `size` changed since the last frame.
    resized: bool,
    shutdown: bool,
}

impl Shared {
    fn signals(&self) -> MutexGuard<'_, Signals> {
        self.signals.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// From mpv's threads: only sets a flag and wakes the render thread, as mpv requires.
    fn mpv_frame(&self) {
        self.signals().mpv_frame = true;
        self.wake.notify_one();
    }

    pub(super) fn set_size(&self, width: u32, height: u32) {
        let mut signals = self.signals();
        if signals.size != Some((width, height)) {
            signals.size = Some((width, height));
            signals.resized = true;
            self.wake.notify_one();
        }
    }

    pub(super) fn shutdown(&self) {
        self.signals().shutdown = true;
        self.wake.notify_one();
    }

    /// Waits for something to render, and returns the size to render at; `None` once shutting down.
    fn wait(&self) -> Option<Option<(u32, u32)>> {
        let mut signals = self.signals();
        while !(signals.mpv_frame || signals.resized || signals.shutdown) {
            signals = self.wake.wait(signals).unwrap_or_else(PoisonError::into_inner);
        }
        if signals.shutdown {
            return None;
        }
        signals.mpv_frame = false;
        signals.resized = false;
        Some(signals.size)
    }

    pub(super) fn latest(&self) -> Option<Frame> {
        self.latest.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    fn publish(&self, frame: Frame) {
        *self.latest.lock().unwrap_or_else(PoisonError::into_inner) = Some(frame);
    }
}

/// Starts the render thread for `player` and waits for it to be ready, so that its errors are returned here. Also
/// returns whether it draws with Apple's software renderer.
pub(super) fn spawn(
    player: &Player,
    format: SurfaceFormat,
    gl_renderer: GlRenderer,
    on_frame: OnFrame,
    shared: Arc<Shared>,
) -> Result<(JoinHandle<()>, bool)> {
    let (ready, initialised) = mpsc::sync_channel(1);
    // Only used to create the render context, then dropped: a renderer must not keep the player from stopping.
    let player = player.clone();
    let thread = std::thread::Builder::new()
        .name("player-surface".into())
        .spawn(move || {
            let renderer = Renderer::new(&player, format, gl_renderer, on_frame, shared);
            drop(player);
            match renderer {
                Ok(renderer) => {
                    let _ = ready.send(Ok(renderer.context.is_software()));
                    renderer.run();
                }
                Err(error) => {
                    let _ = ready.send(Err(error));
                }
            }
        })
        .map_err(|error| Error::Render(format!("could not start the render thread: {error}")))?;

    match initialised.recv() {
        Ok(Ok(software)) => Ok((thread, software)),
        Ok(Err(error)) => {
            let _ = thread.join();
            Err(error)
        }
        Err(_) => {
            let _ = thread.join();
            Err(Error::Render("the render thread stopped while starting".into()))
        }
    }
}

/// The render thread's state. Declared in drop order: everything GL-related is released in `Drop` while `context` is
/// still current, then the context itself is destroyed.
struct Renderer {
    render: Option<RenderContext>,
    ring: Vec<Surface>,
    scratch: Option<Scratch>,
    converter: Option<Converter>,
    gl: glow::Context,
    context: Context,
    format: SurfaceFormat,
    /// The size of the surfaces in `ring`.
    size: Option<(u32, u32)>,
    next: usize,
    sequence: u64,
    shared: Arc<Shared>,
    on_frame: OnFrame,
}

impl Renderer {
    fn new(
        player: &Player,
        format: SurfaceFormat,
        gl_renderer: GlRenderer,
        on_frame: OnFrame,
        shared: Arc<Shared>,
    ) -> Result<Renderer> {
        let context = Context::new(gl_renderer)?;
        let loader = Loader::new()?;
        // SAFETY: `context` is current on this thread, and the loader resolves its functions.
        let gl = unsafe { glow::Context::from_loader_function_cstr(|name| loader.get(name)) };

        let signal = Arc::clone(&shared);
        // SAFETY: `context` is current on this thread, where the render context is used and dropped (first, in
        // `Drop`), and it outlives the render context. It is a 3.2 core context.
        let render = unsafe { RenderContext::new(player, move |name| loader.get(name), move || signal.mpv_frame()) }?;
        let converter = match format {
            SurfaceFormat::Bgra => None,
            SurfaceFormat::Nv12 => Some(Converter::new(&gl)?),
        };

        Ok(Renderer {
            render: Some(render),
            ring: vec![],
            scratch: None,
            converter,
            gl,
            context,
            format,
            size: None,
            next: 0,
            sequence: 0,
            shared,
            on_frame,
        })
    }

    fn run(mut self) {
        while let Some(size) = self.shared.wait() {
            let render = self.render.as_mut().expect("the render context lives until the drop");
            // Clears mpv's update flag. A redraw request is rendered like a new frame.
            render.update();
            if let Some((width, height)) = size.filter(|&(width, height)| width > 0 && height > 0) {
                // A frame that cannot be drawn is skipped; the next signal tries again.
                let _ = self.draw(self.format.surface_size(width, height));
            }
        }
    }

    /// Renders the current picture into the next surface, waits for the GPU to finish it, publishes it and notifies.
    fn draw(&mut self, (width, height): (u32, u32)) -> Result<()> {
        if self.size != Some((width, height)) {
            self.rebuild(width, height)?;
        }
        let surface = &self.ring[self.next];
        self.next = (self.next + 1) % RING;

        let target = match &self.scratch {
            Some(scratch) => scratch.fbo,
            None => surface.planes[0].fbo,
        };
        let render = self.render.as_mut().expect("the render context lives until the drop");
        // Unflipped: the picture's top row goes to the surface's row 0, which Metal and CoreVideo read as the top.
        render.render(RenderTarget { fbo: target.0.get(), width, height }, false)?;
        if let (Some(converter), Some(scratch)) = (&self.converter, &self.scratch) {
            converter.convert(&self.gl, scratch, surface);
        }

        // Another API reads the surface next, and IOSurface does not synchronise GL with it.
        finish(&self.context, &self.gl)?;

        self.sequence += 1;
        self.shared.publish(Frame {
            buffer: surface.buffer.clone(),
            width,
            height,
            format: self.format,
            sequence: self.sequence,
        });
        render.report_swap();
        (self.on_frame)();
        Ok(())
    }

    /// Replaces the ring, and the NV12 scratch target, with ones of the new size. Frames the host holds keep their
    /// own surfaces alive.
    fn rebuild(&mut self, width: u32, height: u32) -> Result<()> {
        self.release();
        if self.format == SurfaceFormat::Nv12 {
            self.scratch = Some(Scratch::new(&self.gl, width, height)?);
        }
        for _ in 0..RING {
            self.ring.push(Surface::new(&self.context, &self.gl, self.format, width, height)?);
        }
        self.size = Some((width, height));
        Ok(())
    }

    /// Deletes the ring and the scratch target.
    fn release(&mut self) {
        self.size = None;
        self.next = 0;
        for surface in self.ring.drain(..) {
            surface.delete(&self.gl);
        }
        if let Some(scratch) = self.scratch.take() {
            scratch.delete(&self.gl);
        }
    }
}

/// Waits until the surface's contents are in its memory. On the GPU a fence suffices. Apple's software renderer
/// signals fences before its writes reach the IOSurface, so there only `glFinish` does.
fn finish(context: &Context, gl: &glow::Context) -> Result<()> {
    // SAFETY: the context is current on this thread; the fence is deleted right after the wait.
    unsafe {
        if context.is_software() {
            gl.finish();
            return Ok(());
        }
        let fence = gl.fence_sync(glow::SYNC_GPU_COMMANDS_COMPLETE, 0).map_err(Error::Render)?;
        let status = gl.client_wait_sync(fence, glow::SYNC_FLUSH_COMMANDS_BIT, FENCE_TIMEOUT);
        gl.delete_sync(fence);
        if status != glow::ALREADY_SIGNALED && status != glow::CONDITION_SATISFIED {
            return Err(Error::Render(format!("the GPU did not finish the frame (status {status:#x})")));
        }
    }
    Ok(())
}

impl Drop for Renderer {
    fn drop(&mut self) {
        // mpv's render context first, with the GL context current, as mpv requires; then this thread's GL objects.
        // The CGL context is destroyed after this, with the fields.
        drop(self.render.take());
        self.release();
        if let Some(converter) = self.converter.take() {
            converter.delete(&self.gl);
        }
    }
}
