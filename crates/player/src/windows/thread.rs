//! The render thread: owns the WGL context, the Direct3D 11 device, mpv's render context and every GL and Direct3D
//! object. It renders each frame through the interop into a target texture, copies it on the GPU into the next
//! texture of a ring and publishes that one.

use super::d3d::{self, Adapter, Device, Slot};
use super::wgl::{Context, InteropDevice, InteropObject};
use super::{AdapterLuid, Frame};
use crate::error::{Error, Result};
use crate::player::Player;
use crate::render::{RenderContext, RenderTarget};
use crate::surface::OnFrame;
use ::windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use glow::HasContext;
use std::panic::{self, AssertUnwindSafe};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError};

/// Textures in the ring: a published frame is overwritten only after two newer ones are published.
const RING: usize = 3;

/// What the render thread and the [`SurfaceRenderer`](super::SurfaceRenderer) share.
pub(super) type Shared = crate::surface::Shared<Frame>;

/// Starts the render thread for `player` and waits for it to be ready, so that its errors are returned here. Also
/// returns the adapter it renders on.
pub(super) fn spawn(
    player: &Player,
    adapter: Option<AdapterLuid>,
    on_frame: OnFrame,
    shared: Arc<Shared>,
) -> Result<(Joined, AdapterLuid)> {
    let (ready, initialised) = mpsc::sync_channel(1);
    // Only used to create the render context, then dropped: a renderer must not keep the player from stopping.
    let player = player.clone();
    let thread = pooled(Box::new(move || {
        let renderer = Renderer::new(&player, adapter, on_frame, shared);
        drop(player);
        match renderer {
            Ok(renderer) => {
                let _ = ready.send(Ok(renderer.adapter));
                renderer.run();
            }
            Err(error) => {
                let _ = ready.send(Err(error));
            }
        }
    }))?;

    match initialised.recv() {
        Ok(Ok(adapter)) => Ok((thread, adapter)),
        Ok(Err(error)) => {
            thread.join();
            Err(error)
        }
        Err(_) => {
            thread.join();
            Err(Error::Render("the render thread stopped while starting".into()))
        }
    }
}

/// What a render thread runs for one renderer.
type Job = Box<dyn FnOnce() + Send + 'static>;

/// A job, and the sender [`Joined`] waits on, dropped once the thread is back in [`IDLE`].
type Work = (Job, mpsc::Sender<()>);

/// Render threads that finished their renderer and wait for the next one.
///
/// A render thread never exits. NVIDIA's OpenGL driver, when a thread that used a context exits, waits under the
/// loader lock for one of its own threads to exit, which needs the loader lock too. Exiting render threads therefore
/// deadlock the process when the driver is busy, as with several players decoding with NVDEC.
static IDLE: Mutex<Vec<mpsc::Sender<Work>>> = Mutex::new(Vec::new());

/// Waits for the job of a render thread to finish; the thread itself goes back to [`IDLE`]. The mutex only makes it
/// `Sync`, as the [`SurfaceRenderer`](super::SurfaceRenderer) holding it is.
pub(super) struct Joined(Mutex<mpsc::Receiver<()>>);

impl Joined {
    pub(super) fn join(self) {
        // Disconnected, with nothing ever sent, when the job is done and its thread is idle again, also when the job
        // panicked.
        let _ = self.0.into_inner().unwrap_or_else(PoisonError::into_inner).recv();
    }
}

/// Runs `job` on an idle render thread, or on a new one.
fn pooled(job: Job) -> Result<Joined> {
    let (done, joined) = mpsc::channel::<()>();
    let mut work = (job, done);
    let idle = IDLE.lock().unwrap_or_else(PoisonError::into_inner).pop();
    if let Some(thread) = idle {
        match thread.send(work) {
            Ok(()) => return Ok(Joined(Mutex::new(joined))),
            Err(mpsc::SendError(unsent)) => work = unsent,
        }
    }

    let (thread, jobs) = mpsc::channel::<Work>();
    thread.send(work).expect("the receiver is alive");
    std::thread::Builder::new()
        .name("player-surface".into())
        .spawn(move || {
            for (job, done) in &jobs {
                // A panic must not end the thread either; the job's own state is released while unwinding.
                let _ = panic::catch_unwind(AssertUnwindSafe(job));
                // Idle before the join returns, so a renderer built right after a drop reuses this thread.
                IDLE.lock().unwrap_or_else(PoisonError::into_inner).push(thread.clone());
                drop(done);
            }
        })
        .map_err(|error| Error::Render(format!("could not start the render thread: {error}")))?;
    Ok(Joined(Mutex::new(joined)))
}

/// The texture mpv draws into: NVIDIA's interop does not register textures shared through NT handles, so this one is
/// not shared, and each frame is copied from it into a [`Slot`].
struct Target {
    texture: ID3D11Texture2D,
    object: InteropObject,
    gl_texture: glow::NativeTexture,
    fbo: glow::NativeFramebuffer,
}

impl Target {
    fn new(device: &Device, interop: &InteropDevice, gl: &glow::Context, width: u32, height: u32) -> Result<Target> {
        let texture = device.texture(width, height, false)?;
        // SAFETY: the GL context is current on this thread, as everything here that calls `gl`; the objects created
        // here are deleted again on failure.
        unsafe {
            let gl_texture = gl.create_texture().map_err(Error::Render)?;
            let mut object = match interop.register(&texture, gl_texture.0.get()) {
                Ok(object) => object,
                Err(error) => {
                    gl.delete_texture(gl_texture);
                    return Err(error);
                }
            };
            let fbo = match gl.create_framebuffer() {
                Ok(fbo) => fbo,
                Err(error) => {
                    interop.unregister(object);
                    gl.delete_texture(gl_texture);
                    return Err(Error::Render(error));
                }
            };
            // The texture has storage, and can be attached, only while locked.
            let attached = interop.lock(&mut object).map(|()| {
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
                gl.framebuffer_texture_2d(
                    glow::FRAMEBUFFER,
                    glow::COLOR_ATTACHMENT0,
                    glow::TEXTURE_2D,
                    Some(gl_texture),
                    0,
                );
                let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
                gl.bind_framebuffer(glow::FRAMEBUFFER, None);
                status
            });
            let mut target = Target { texture, object, gl_texture, fbo };
            let status = match attached.and_then(|status| interop.unlock(&mut target.object).map(|()| status)) {
                Ok(status) => status,
                Err(error) => {
                    target.delete(interop, gl);
                    return Err(error);
                }
            };
            if status != glow::FRAMEBUFFER_COMPLETE {
                target.delete(interop, gl);
                return Err(Error::Render(format!(
                    "the shared texture's framebuffer is incomplete (status {status:#x})"
                )));
            }
            Ok(target)
        }
    }

    /// Unregisters the texture from GL and deletes the GL objects; the Direct3D texture is released with `self`.
    fn delete(self, interop: &InteropDevice, gl: &glow::Context) {
        interop.unregister(self.object);
        // SAFETY: the objects were created on `gl`'s context, which is current on this thread.
        unsafe {
            gl.delete_framebuffer(self.fbo);
            gl.delete_texture(self.gl_texture);
        }
    }
}

/// The render thread's state. Declared in drop order: everything GL-related is released in `Drop` while `context` is
/// still current, then the Direct3D device, then the context itself.
struct Renderer {
    render: Option<RenderContext>,
    target: Option<Target>,
    ring: Vec<Slot>,
    interop: Option<InteropDevice>,
    gl: glow::Context,
    device: Device,
    /// Current on this thread for as long as the renderer lives.
    _context: Context,
    adapter: AdapterLuid,
    /// The size of `target` and the textures in `ring`.
    size: Option<(u32, u32)>,
    next: usize,
    sequence: u64,
    shared: Arc<Shared>,
    on_frame: OnFrame,
}

impl Renderer {
    fn new(player: &Player, adapter: Option<AdapterLuid>, on_frame: OnFrame, shared: Arc<Shared>) -> Result<Renderer> {
        let context = Context::new()?;
        let loader = context.loader();
        // SAFETY: `context` is current on this thread, and the loader resolves its functions.
        let gl = unsafe { glow::Context::from_loader_function_cstr(|name| loader.get(name)) };
        // SAFETY: as above.
        let (vendor, renderer) =
            unsafe { (gl.get_parameter_string(glow::VENDOR), gl.get_parameter_string(glow::RENDERER)) };

        let (adapter, device) = d3d::device(adapter)?;
        let interop = InteropDevice::open(&context, &device.raw, &renderer, &adapter.name)?;
        check_same_gpu(&vendor, &renderer, &adapter)?;

        let signal = Arc::clone(&shared);
        // SAFETY: `context` is current on this thread, where the render context is used and dropped (first, in
        // `Drop`), and it outlives the render context.
        let render = unsafe { RenderContext::new(player, move |name| loader.get(name), move || signal.mpv_frame()) }?;

        Ok(Renderer {
            render: Some(render),
            target: None,
            ring: vec![],
            interop: Some(interop),
            gl,
            device,
            _context: context,
            adapter: adapter.luid,
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

    /// Renders the current picture into the target, copies it into the next slot, waits for the GPU to finish,
    /// publishes the slot and notifies.
    fn draw(&mut self, (width, height): (u32, u32)) -> Result<()> {
        if self.size != Some((width, height)) {
            self.rebuild(width, height)?;
        }
        let interop = self.interop.as_ref().expect("the interop device lives until the drop");
        let target = self.target.as_mut().expect("a target exists while a size is set");
        let slot = &self.ring[self.next];
        self.next = (self.next + 1) % RING;

        let render = self.render.as_mut().expect("the render context lives until the drop");
        interop.lock(&mut target.object)?;
        // Unflipped: the picture's top row goes to the texture's row 0, which Direct3D reads as the top.
        let rendered = render.render(RenderTarget { fbo: target.fbo.0.get(), width, height }, false);
        interop.unlock(&mut target.object)?;
        rendered?;
        // The unlock orders GL's drawing before this copy; the wait makes the copy visible to other devices.
        self.device.copy(&slot.texture, &target.texture);
        self.device.finish()?;

        self.sequence += 1;
        self.shared.publish(Frame {
            handle: Arc::clone(&slot.handle),
            adapter: self.adapter,
            width,
            height,
            sequence: self.sequence,
        });
        render.report_swap();
        (self.on_frame)();
        Ok(())
    }

    /// Replaces the target and the ring with ones of the new size. Frames the host holds keep their own textures
    /// alive through their handles.
    fn rebuild(&mut self, width: u32, height: u32) -> Result<()> {
        self.release();
        let interop = self.interop.as_ref().expect("the interop device lives until the drop");
        self.target = Some(Target::new(&self.device, interop, &self.gl, width, height)?);
        for _ in 0..RING {
            self.ring.push(Slot::new(&self.device, width, height)?);
        }
        self.size = Some((width, height));
        Ok(())
    }

    /// Deletes the target and releases the ring.
    fn release(&mut self) {
        self.size = None;
        self.next = 0;
        self.ring.clear();
        if let (Some(target), Some(interop)) = (self.target.take(), &self.interop) {
            target.delete(interop, &self.gl);
        }
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        // mpv's render context first, with the GL context current, as mpv requires; then the interop objects, while
        // both GL and Direct3D are alive, then the interop device. The Direct3D device and the WGL context are
        // released after this, with the fields.
        drop(self.render.take());
        self.release();
        drop(self.interop.take());
    }
}

/// Fails when the GL driver runs on another GPU than the adapter, as on hybrid laptops. The interop usually refuses to
/// open there already; this catches drivers that open it anyway, which would copy every frame between GPUs.
fn check_same_gpu(vendor: &str, renderer: &str, adapter: &Adapter) -> Result<()> {
    match gl_vendor_id(vendor) {
        Some(id) if id != adapter.vendor => Err(Error::Render(format!(
            "the OpenGL driver renders on {renderer}, but the Direct3D 11 adapter is {}; both must use one GPU",
            adapter.name
        ))),
        _ => Ok(()),
    }
}

/// The PCI vendor ID of a `GL_VENDOR` string, for the vendors whose drivers have the interop.
fn gl_vendor_id(vendor: &str) -> Option<u32> {
    let vendor = vendor.to_ascii_lowercase();
    if vendor.contains("nvidia") {
        Some(0x10de)
    } else if vendor.contains("ati technologies") || vendor.contains("advanced micro devices") || vendor == "amd" {
        Some(0x1002)
    } else if vendor.contains("intel") {
        Some(0x8086)
    } else {
        None
    }
}

#[cfg(test)]
pub(in crate::windows) mod tests {
    use super::*;

    /// Runs `test` on a pooled render thread and returns its result, so that no test thread that used OpenGL exits
    /// (see [`IDLE`]). Panics if `test` panicked.
    pub(in crate::windows) fn on_render_thread<T: Send + 'static>(test: impl FnOnce() -> T + Send + 'static) -> T {
        let (result, received) = mpsc::sync_channel(1);
        pooled(Box::new(move || {
            let _ = result.send(test());
        }))
        .expect("a render thread starts")
        .join();
        received.recv().expect("the test panicked on the render thread")
    }

    #[test]
    fn gl_vendors_map_to_pci_ids() {
        assert_eq!(gl_vendor_id("NVIDIA Corporation"), Some(0x10de));
        assert_eq!(gl_vendor_id("ATI Technologies Inc."), Some(0x1002));
        assert_eq!(gl_vendor_id("Intel"), Some(0x8086));
        assert_eq!(gl_vendor_id("Mesa"), None);
    }

    #[test]
    fn mismatched_gpus_are_an_error_naming_both() {
        let adapter =
            Adapter { luid: AdapterLuid { low: 1, high: 0 }, vendor: 0x1002, name: "AMD Radeon(TM) Graphics".into() };
        let Err(Error::Render(message)) = check_same_gpu("NVIDIA Corporation", "NVIDIA GeForce RTX 5090", &adapter)
        else {
            panic!("the mismatch was accepted")
        };
        assert!(message.contains("NVIDIA GeForce RTX 5090") && message.contains("AMD Radeon"), "{message}");
        assert!(check_same_gpu("ATI Technologies Inc.", "AMD Radeon", &adapter).is_ok());
        assert!(check_same_gpu("Mesa", "llvmpipe", &adapter).is_ok(), "unknown vendors are not compared");
    }
}
