//! The render thread: owns the GBM device, the EGL display and context, mpv's render context and every GL object,
//! renders each frame into the next dma-buf of a ring and publishes it.

use super::egl::{self, Context, Display};
use super::gbm::{self, Device, Node, Slot};
use super::{DrmDevice, Frame};
use crate::error::{Error, Result};
use crate::player::Player;
use crate::render::{RenderContext, RenderTarget};
use crate::surface::OnFrame;
use glow::HasContext;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc;
use std::thread::JoinHandle;

/// Buffers in the ring: a published frame is overwritten only after two newer ones are published.
const RING: usize = 3;

/// How long to wait for the GPU to finish a frame before giving up on it, in nanoseconds.
const FENCE_TIMEOUT: i32 = 1_000_000_000;

/// The size of the buffer allocated to check that a node can render with the modifiers asked for.
const PROBE_SIZE: (u32, u32) = (64, 64);

/// What the render thread and the [`SurfaceRenderer`](super::SurfaceRenderer) share.
pub(super) type Shared = crate::surface::Shared<Frame>;

/// What a started render thread renders on.
pub(super) struct Started {
    pub(super) thread: JoinHandle<()>,
    pub(super) device: DrmDevice,
    pub(super) modifier: u64,
}

/// Starts the render thread for `player` and waits for it to be ready, so that its errors are returned here.
pub(super) fn spawn(
    player: &Player,
    device: Option<DrmDevice>,
    modifiers: Vec<u64>,
    on_frame: OnFrame,
    shared: Arc<Shared>,
) -> Result<Started> {
    let (ready, initialised) = mpsc::sync_channel(1);
    // Only used to create the render context, then dropped: a renderer must not keep the player from stopping.
    let player = player.clone();
    let thread = std::thread::Builder::new()
        .name("player-surface".into())
        .spawn(move || {
            let renderer = Gpu::select(device, &modifiers)
                .and_then(|gpu| Renderer::new(&player, gpu, modifiers, on_frame, shared));
            drop(player);
            match renderer {
                Ok(renderer) => {
                    let _ = ready.send(Ok((renderer.gpu.node.device, renderer.gpu.modifier)));
                    renderer.run();
                }
                Err(error) => {
                    let _ = ready.send(Err(error));
                }
            }
        })
        .map_err(|error| Error::Render(format!("could not start the render thread: {error}")))?;

    match initialised.recv() {
        Ok(Ok((device, modifier))) => Ok(Started { thread, device, modifier }),
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

/// A DRM node with a GBM device, an EGL display and a current GL context on it. Declared in drop order: the context
/// is destroyed first, then the display, then the GBM device, then the node is closed (each holds the next).
struct Gpu {
    gl: glow::Context,
    context: Context,
    display: Rc<Display>,
    device: Rc<Device>,
    node: Node,
    /// The modifier the probe buffer was allocated with.
    modifier: u64,
}

impl Gpu {
    /// The node `device`, or the first node that can render with `modifiers`.
    fn select(device: Option<DrmDevice>, modifiers: &[u64]) -> Result<Gpu> {
        let nodes = gbm::nodes();
        if let Some(device) = device {
            let node = nodes
                .iter()
                .find(|node| node.device == device)
                .ok_or_else(|| Error::Render(format!("no DRM render node has the device number {device}")))?;
            return Gpu::new(node, modifiers)
                .map_err(|error| Error::Render(format!("{}: {}", node.path.display(), message(error))));
        }
        if nodes.is_empty() {
            return Err(Error::Render("no DRM render node is available".into()));
        }
        let mut failures = vec![];
        for node in &nodes {
            match Gpu::new(node, modifiers) {
                Ok(gpu) => return Ok(gpu),
                Err(error) => failures.push(format!("{}: {}", node.path.display(), message(error))),
            }
        }
        Err(Error::Render(format!("no usable DRM render node is available: {}", failures.join("; "))))
    }

    /// Sets up the node, and checks that a buffer with one of `modifiers` can be allocated and drawn into.
    fn new(node: &Node, modifiers: &[u64]) -> Result<Gpu> {
        let device = Device::open(node)?;
        let display = Display::new(Rc::clone(&device))?;
        let context = Context::new(&display)?;
        // SAFETY: `context` is current on this thread, and the loader resolves its functions.
        let gl = unsafe { glow::Context::from_loader_function_cstr(egl::get_proc_address) };
        if !gl.supported_extensions().contains("GL_OES_EGL_image") {
            return Err(Error::Render("the OpenGL driver has no GL_OES_EGL_image".into()));
        }
        let probe = Slot::new(&device, &display, &context, &gl, PROBE_SIZE, modifiers)?;
        let modifier = probe.buffer.modifier;
        probe.delete(&gl);
        Ok(Gpu { gl, context, display, device, node: node.clone(), modifier })
    }
}

/// The error's message, without the variant's prefix, to combine several.
fn message(error: Error) -> String {
    match error {
        Error::Render(message) => message,
        error => error.to_string(),
    }
}

/// The render thread's state. Everything GL-related is released in `Drop` while the context is still current, then
/// the context, display, GBM device and node, with `gpu`.
struct Renderer {
    render: Option<RenderContext>,
    ring: Vec<Slot>,
    gpu: Gpu,
    modifiers: Vec<u64>,
    /// The size of the buffers in `ring`.
    size: Option<(u32, u32)>,
    next: usize,
    sequence: u64,
    shared: Arc<Shared>,
    on_frame: OnFrame,
}

impl Renderer {
    fn new(player: &Player, gpu: Gpu, modifiers: Vec<u64>, on_frame: OnFrame, shared: Arc<Shared>) -> Result<Renderer> {
        let signal = Arc::clone(&shared);
        // SAFETY: the context is current on this thread, where the render context is used and dropped (first, in
        // `Drop`), and it outlives the render context. The node stays open until after that drop.
        let render = unsafe {
            RenderContext::with_drm_device(
                player,
                egl::get_proc_address,
                move || signal.mpv_frame(),
                Some(gpu.device.node_fd()),
            )
        }?;
        Ok(Renderer {
            render: Some(render),
            ring: vec![],
            gpu,
            modifiers,
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
            if let Some(size) = size.filter(|&(width, height)| width > 0 && height > 0) {
                // A frame that cannot be drawn is skipped; the next signal tries again.
                let _ = self.draw(size);
            }
        }
    }

    /// Renders the current picture into the next buffer, waits for the GPU to finish it, publishes it and notifies.
    fn draw(&mut self, (width, height): (u32, u32)) -> Result<()> {
        if self.size != Some((width, height)) {
            self.rebuild(width, height)?;
        }
        let slot = &self.ring[self.next];
        self.next = (self.next + 1) % RING;

        let render = self.render.as_mut().expect("the render context lives until the drop");
        // Unflipped: the picture's top row goes to the buffer's first row, which every importer reads as the top.
        render.render(RenderTarget { fbo: slot.fbo.0.get(), width, height }, false)?;
        // Another API reads the buffer next, and not every driver orders it after GL's writes.
        finish(&self.gpu.gl)?;

        self.sequence += 1;
        self.shared.publish(Frame {
            fd: Arc::clone(&slot.buffer.fd),
            device: self.gpu.node.device,
            modifier: slot.buffer.modifier,
            stride: slot.buffer.stride,
            offset: slot.buffer.offset,
            width,
            height,
            sequence: self.sequence,
        });
        render.report_swap();
        (self.on_frame)();
        Ok(())
    }

    /// Replaces the ring with buffers of the new size. Frames the host holds keep their own dma-bufs alive.
    fn rebuild(&mut self, width: u32, height: u32) -> Result<()> {
        self.release();
        let Gpu { gl, context, display, device, .. } = &self.gpu;
        for _ in 0..RING {
            self.ring.push(Slot::new(device, display, context, gl, (width, height), &self.modifiers)?);
        }
        self.size = Some((width, height));
        Ok(())
    }

    /// Deletes the ring.
    fn release(&mut self) {
        self.size = None;
        self.next = 0;
        for slot in self.ring.drain(..) {
            slot.delete(&self.gpu.gl);
        }
    }
}

/// Waits until the GPU has finished the frame.
fn finish(gl: &glow::Context) -> Result<()> {
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

impl Drop for Renderer {
    fn drop(&mut self) {
        // mpv's render context first, with the GL context current, as mpv requires, and while the node it was given
        // is open; then the ring's GL objects, images and bos. The context, display, GBM device and node follow,
        // with `gpu`.
        drop(self.render.take());
        self.release();
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// Whether a renderer can be built on this machine; prints why not.
    pub(in crate::linux) fn usable() -> bool {
        gpu().is_some()
    }

    /// A GPU on the first usable node with the default modifier, or `None` (with a message) where none exists.
    fn gpu() -> Option<Gpu> {
        match Gpu::select(None, &[gbm::LINEAR]) {
            Ok(gpu) => Some(gpu),
            Err(error) => {
                eprintln!("skipped: {error}");
                None
            }
        }
    }

    #[test]
    fn creates_and_destroys_a_context() {
        if crate::test_gl::skipped() {
            return;
        }
        let Some(_gpu) = gpu() else { return };
        // Twice, so the first teardown left nothing behind that the second setup trips over.
        drop(_gpu);
        assert!(gpu().is_some());
    }

    /// Reads a dma-buf back through `gpu`'s context, as a consumer would import it: rows in memory order.
    fn read(gpu: &Gpu, plane: &egl::Plane<'_>) -> Vec<u8> {
        let image = gpu.display.import(plane).expect("the dma-buf imports");
        let gl = &gpu.gl;
        let mut pixels = vec![0; plane.width as usize * plane.height as usize * 4];
        // SAFETY: the context is current on this thread; the objects are deleted before returning.
        unsafe {
            let texture = gl.create_texture().unwrap();
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            image.bind_texture(&gpu.context);
            let fbo = gl.create_framebuffer().unwrap();
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            gl.framebuffer_texture_2d(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::TEXTURE_2D, Some(texture), 0);
            assert_eq!(gl.check_framebuffer_status(glow::FRAMEBUFFER), glow::FRAMEBUFFER_COMPLETE);
            gl.read_pixels(
                0,
                0,
                plane.width as i32,
                plane.height as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut pixels)),
            );
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            gl.delete_framebuffer(fbo);
            gl.delete_texture(texture);
        }
        pixels
    }

    /// Allocates a slot, fills its top half red and its bottom half blue through GL, destroys the bo and the whole
    /// GBM device, and reads the dma-buf back through a second device and context.
    #[test]
    fn slot_outlives_its_device_and_imports_elsewhere() {
        if crate::test_gl::skipped() {
            return;
        }
        let Some(first) = gpu() else { return };
        let (width, height) = (32, 16);
        let slot = Slot::new(&first.device, &first.display, &first.context, &first.gl, (width, height), &[gbm::LINEAR])
            .expect("a slot is allocated");
        // SAFETY: the context is current on this thread; the slot's framebuffer belongs to it.
        unsafe {
            let gl = &first.gl;
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(slot.fbo));
            gl.enable(glow::SCISSOR_TEST);
            // GL's row 0 is the buffer's first row.
            for (rows, [r, g, b]) in [(0, [1.0, 0.0, 0.0]), (height / 2, [0.0, 0.0, 1.0])] {
                gl.scissor(0, rows as i32, width as i32, (height / 2) as i32);
                gl.clear_color(r, g, b, 1.0);
                gl.clear(glow::COLOR_BUFFER_BIT);
            }
            gl.disable(glow::SCISSOR_TEST);
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        }
        finish(&first.gl).unwrap();
        assert_eq!(slot.buffer.modifier, gbm::LINEAR);
        let (fd, stride, offset) = (Arc::clone(&slot.buffer.fd), slot.buffer.stride, slot.buffer.offset);
        assert!(stride >= width * 4, "stride {stride}");
        slot.delete(&first.gl);
        drop(first);

        let second = gpu().expect("the node is still usable");
        let plane = egl::Plane {
            fd: std::os::fd::AsFd::as_fd(&*fd),
            width,
            height,
            fourcc: gbm::ABGR8888,
            stride,
            offset,
            modifier: gbm::LINEAR,
        };
        let pixels = read(&second, &plane);
        let pixel = |x: u32, y: u32| {
            let at = ((y * width + x) * 4) as usize;
            [pixels[at], pixels[at + 1], pixels[at + 2], pixels[at + 3]]
        };
        assert_eq!(pixel(5, 2), [255, 0, 0, 255], "the first rows are red, in R, G, B, A order");
        assert_eq!(pixel(20, height - 2), [0, 0, 255, 255], "the last rows are blue");
    }

    #[test]
    fn unknown_device_is_named() {
        let device = DrmDevice { major: 4095, minor: 4095 };
        let Err(error) = Gpu::select(Some(device), &[gbm::LINEAR]) else {
            panic!("a GPU was found on {device}")
        };
        let message = message(error);
        assert!(message.contains("4095:4095"), "{message}");
    }
}
