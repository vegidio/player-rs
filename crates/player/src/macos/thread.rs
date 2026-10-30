//! The render thread: owns the CGL context, mpv's render context and every GL object, renders each frame into the
//! next surface of a ring and publishes it.

use super::cgl::{Context, Loader};
use super::convert::{Converter, Scratch};
use super::surface::Surface;
use super::{Frame, GlRenderer, SurfaceFormat};
use crate::error::Result;
use crate::player::Player;
use crate::render::{RenderContext, RenderTarget};
use crate::surface::{self, Attachment, Backend, OnFrame, RING};
use std::sync::Arc;
use std::thread::JoinHandle;

/// Starts the render thread for `player` and waits for it to be ready, so that its errors are returned here. Also
/// returns whether it draws with Apple's software renderer.
pub(super) fn spawn(
    player: &Player,
    format: SurfaceFormat,
    gl_renderer: GlRenderer,
    on_frame: OnFrame,
) -> Result<(Attachment<Frame, JoinHandle<()>>, bool)> {
    surface::start(player, surface::spawn_thread, on_frame, move |player, shared| {
        let context = Context::new(gl_renderer)?;
        let loader = Loader::new()?;
        // SAFETY: `context` is current on this thread, and the loader resolves its functions.
        let gl = unsafe { glow::Context::from_loader_function_cstr(|name| loader.get(name)) };

        let signal = Arc::clone(shared);
        // SAFETY: `context` is current on this thread, where the render context is used and dropped (before the
        // renderer), and it outlives the render context. It is a 3.2 core context.
        let render = unsafe { RenderContext::new(player, move |name| loader.get(name), move || signal.mpv_frame()) }?;
        let nv12 = match format {
            SurfaceFormat::Bgra => None,
            SurfaceFormat::Nv12 => Some(Nv12 { converter: Converter::new(&gl)?, scratch: None }),
        };
        let software = context.is_software();
        Ok((Renderer { ring: vec![], nv12, gl, context, format }, render, software))
    })
}

/// The NV12 conversion: mpv renders RGB into `scratch`, which `converter` turns into the surface's two planes.
struct Nv12 {
    converter: Converter,
    /// Exists while the ring does.
    scratch: Option<Scratch>,
}

/// The render thread's GL state. Declared in drop order: the GL objects are released in `Drop` while `context` is
/// still current, then the context itself is destroyed.
struct Renderer {
    ring: Vec<Surface>,
    nv12: Option<Nv12>,
    gl: glow::Context,
    context: Context,
    format: SurfaceFormat,
}

impl Backend for Renderer {
    type Frame = Frame;

    fn frame_size(&self, width: u32, height: u32) -> (u32, u32) {
        self.format.surface_size(width, height)
    }

    fn rebuild(&mut self, width: u32, height: u32) -> Result<()> {
        if let Some(nv12) = &mut self.nv12 {
            nv12.scratch = Some(Scratch::new(&self.gl, width, height)?);
        }
        for _ in 0..RING {
            self.ring.push(Surface::new(&self.context, &self.gl, self.format, width, height)?);
        }
        Ok(())
    }

    fn draw(
        &mut self,
        render: &mut RenderContext,
        slot: usize,
        (width, height): (u32, u32),
        sequence: u64,
    ) -> Result<Frame> {
        let surface = &self.ring[slot];
        let scratch = self.nv12.as_ref().and_then(|nv12| Some((&nv12.converter, nv12.scratch.as_ref()?)));
        let target = match scratch {
            Some((_, scratch)) => scratch.fbo,
            None => surface.planes[0].fbo,
        };
        // Unflipped: the picture's top row goes to the surface's row 0, which Metal and CoreVideo read as the top.
        render.render(RenderTarget { fbo: target.0.get(), width, height }, false)?;
        if let Some((converter, scratch)) = scratch {
            converter.convert(&self.gl, scratch, surface);
        }

        // Another API reads the surface next, and IOSurface does not synchronise GL with it. On the GPU a fence
        // suffices. Apple's software renderer signals fences before its writes reach the IOSurface, so there only
        // `glFinish` does.
        if self.context.is_software() {
            // SAFETY: the context is current on this thread.
            unsafe { glow::HasContext::finish(&self.gl) };
        } else {
            crate::gl::fence_wait(&self.gl)?;
        }

        Ok(Frame { buffer: surface.buffer.clone(), width, height, format: self.format, sequence })
    }

    fn release(&mut self) {
        for surface in self.ring.drain(..) {
            surface.delete(&self.gl);
        }
        if let Some(scratch) = self.nv12.as_mut().and_then(|nv12| nv12.scratch.take()) {
            scratch.delete(&self.gl);
        }
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        // After mpv's render context and the ring; the CGL context is destroyed after this, with the fields.
        if let Some(nv12) = self.nv12.take() {
            nv12.converter.delete(&self.gl);
        }
    }
}
