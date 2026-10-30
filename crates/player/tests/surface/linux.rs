//! Linux: [`SurfaceRenderer`] into single-plane RGBA dma-bufs. Each frame is read back as an application would use
//! it: its fd is imported into a second EGL display and context, on a GBM device of its own on the frame's DRM node,
//! and read with `glReadPixels`.
//!
//! The scenarios need a usable DRM node. Without one, as on WSL2 or CI machines without `vgem`, they pass without
//! running, and only the creation errors are tested. With software OpenGL, load `vgem` first (see the README). Set
//! `PLAYER_TEST_GL=hardware` to also test hardware decoding.

use super::*;
use crate::common::gl::{self, Gl};
use glow::HasContext;
use player::linux::{DrmDevice, Frame, MODIFIER_LINEAR, SurfaceRenderer};
use std::cell::RefCell;
use std::collections::HashSet;
use std::ffi::{CStr, c_char, c_int, c_void};
use std::fs::{File, OpenOptions};
use std::os::fd::{AsRawFd, BorrowedFd};
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::ptr;
use std::sync::OnceLock;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};

/// The error creation fails with where no DRM node can be used.
const NO_NODE: &str = "DRM render node is available";

/// `I915_FORMAT_MOD_X_TILED`: a layout software drivers cannot allocate.
const X_TILED: u64 = 0x0100_0000_0000_0001;

/// Whether the scenarios needing a DRM node are skipped: GL tests are skipped, or no node can be used.
fn skipped() -> bool {
    static NO_NODE_REASON: OnceLock<Option<String>> = OnceLock::new();
    if gl::skipped() {
        return true;
    }
    let reason = NO_NODE_REASON.get_or_init(|| match SurfaceRenderer::builder(&render_player(Autoplay::Off)).build() {
        Ok(_) => None,
        Err(Error::Render(message)) if message.contains(NO_NODE) => Some(message),
        Err(error) => panic!("the surface renderer cannot be created: {error}"),
    });
    if let Some(reason) = reason {
        eprintln!("skipped: {reason}");
    }
    reason.is_some()
}

/// Whether OpenGL is Mesa's software renderer, as the environment asks for.
fn software() -> bool {
    let env = |name: &str| std::env::var(name).unwrap_or_default();
    env("LIBGL_ALWAYS_SOFTWARE") == "1" || matches!(env("GALLIUM_DRIVER").as_str(), "llvmpipe" | "softpipe")
}

/// Creates a surface renderer for `player`, notifying on the returned channel.
fn renderer(player: &Player) -> (SurfaceRenderer, Receiver<()>) {
    let (sender, frames) = mpsc::channel();
    let renderer = SurfaceRenderer::builder(player)
        .on_frame(move || {
            let _ = sender.send(());
        })
        .build()
        .expect("the surface renderer is created");
    (renderer, frames)
}

/// A render player and its surface renderer, declared so the renderer is dropped first.
struct Rig {
    renderer: SurfaceRenderer,
    frames: Receiver<()>,
    player: Player,
}

fn rig(autoplay: Autoplay) -> Rig {
    let player = render_player(autoplay);
    let (renderer, frames) = renderer(&player);
    Rig { renderer, frames, player }
}

impl Rig {
    /// Waits for a published frame `accept` returns true for. Fails after [`TIMEOUT`].
    fn wait_frame(&self, what: &str, accept: impl Fn(&Frame) -> bool) -> Frame {
        wait_frame(&self.renderer, &self.frames, &self.player, what, accept)
    }
}

fn wait_frame(
    renderer: &SurfaceRenderer,
    frames: &Receiver<()>,
    player: &Player,
    what: &str,
    accept: impl Fn(&Frame) -> bool,
) -> Frame {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if let Some(frame) = renderer.latest().filter(&accept) {
            return frame;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}; state: {:?}", player.state());
        let _ = frames.recv_timeout(Duration::from_millis(50));
    }
}

/// The DRM nodes in the order the renderer tries them: render nodes, then card nodes, each by device number.
fn nodes() -> Vec<(String, DrmDevice)> {
    let mut nodes: Vec<(bool, String, DrmDevice)> = std::fs::read_dir("/dev/dri")
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| {
                    let name = entry.file_name().into_string().ok()?;
                    let render = name.starts_with("renderD");
                    let metadata = entry.metadata().ok()?;
                    let wanted = (render || name.starts_with("card")) && metadata.file_type().is_char_device();
                    let rdev = metadata.rdev();
                    // glibc's `gnu_dev_major` and `gnu_dev_minor`.
                    let device = DrmDevice {
                        major: (((rdev >> 32) & 0xffff_f000) | ((rdev >> 8) & 0xfff)) as u32,
                        minor: (((rdev >> 12) & 0xffff_ff00) | (rdev & 0xff)) as u32,
                    };
                    wanted.then(|| (render, entry.path().display().to_string(), device))
                })
                .collect()
        })
        .unwrap_or_default();
    nodes.sort_by_key(|(render, _, device)| (!render, device.minor));
    nodes.into_iter().map(|(_, path, device)| (path, device)).collect()
}

// ------------------------------------------------------------------------------------------------------------------
// Readback: a second EGL display and context on the frame's node, importing the frame's dma-buf.

type EGLDisplay = *mut c_void;
type EGLContext = *mut c_void;
type EGLImage = *mut c_void;
type EGLint = i32;

const EGL_NONE: EGLint = 0x3038;
const EGL_OPENGL_API: u32 = 0x30A2;
const EGL_PLATFORM_GBM_KHR: u32 = 0x31D7;
const EGL_LINUX_DMA_BUF_EXT: u32 = 0x3270;

unsafe extern "C" {
    fn eglInitialize(display: EGLDisplay, major: *mut EGLint, minor: *mut EGLint) -> u32;
    fn eglTerminate(display: EGLDisplay) -> u32;
    fn eglBindAPI(api: u32) -> u32;
    fn eglCreateContext(
        display: EGLDisplay,
        config: *mut c_void,
        share: EGLContext,
        attributes: *const EGLint,
    ) -> EGLContext;
    fn eglDestroyContext(display: EGLDisplay, context: EGLContext) -> u32;
    fn eglMakeCurrent(display: EGLDisplay, draw: *mut c_void, read: *mut c_void, context: EGLContext) -> u32;
    fn eglGetProcAddress(name: *const c_char) -> *const c_void;
    fn eglGetError() -> EGLint;
    fn gbm_create_device(fd: c_int) -> *mut c_void;
    fn gbm_device_destroy(gbm: *mut c_void);
}

fn proc_address(name: &CStr) -> *const c_void {
    // SAFETY: the name is NUL-terminated.
    unsafe { eglGetProcAddress(name.as_ptr()) }
}

/// Loads an extension function.
///
/// # Safety
///
/// `F` must be the function pointer type of `name`.
unsafe fn load<F>(name: &CStr) -> F {
    let address = proc_address(name);
    assert!(!address.is_null(), "EGL has no {name:?}");
    // SAFETY: as the caller guarantees.
    unsafe { std::mem::transmute_copy(&address) }
}

/// A consumer of frames on one DRM node: its own GBM device, EGL display and current context, on this thread.
struct Importer {
    device: DrmDevice,
    gl: glow::Context,
    display: EGLDisplay,
    context: EGLContext,
    gbm: *mut c_void,
    _node: File,
}

impl Importer {
    fn new(device: DrmDevice) -> Importer {
        let (path, _) = nodes().into_iter().find(|(_, found)| *found == device).expect("the frame's node exists");
        let node = OpenOptions::new().read(true).write(true).open(&path).expect("the node opens");
        type GetPlatformDisplay = unsafe extern "C" fn(u32, *mut c_void, *const EGLint) -> EGLDisplay;
        // SAFETY: valid fd and display; the context is created without a config (EGL_KHR_no_config_context, which
        // the drivers this runs on have) and made current with no surface.
        unsafe {
            let gbm = gbm_create_device(node.as_raw_fd());
            assert!(!gbm.is_null(), "a GBM device on {path}");
            let display =
                load::<GetPlatformDisplay>(c"eglGetPlatformDisplayEXT")(EGL_PLATFORM_GBM_KHR, gbm, [EGL_NONE].as_ptr());
            assert_eq!(eglInitialize(display, ptr::null_mut(), ptr::null_mut()), 1, "EGL {:#x}", eglGetError());
            eglBindAPI(EGL_OPENGL_API);
            let context = eglCreateContext(display, ptr::null_mut(), ptr::null_mut(), [EGL_NONE].as_ptr());
            assert!(!context.is_null(), "an EGL context ({:#x})", eglGetError());
            assert_eq!(eglMakeCurrent(display, ptr::null_mut(), ptr::null_mut(), context), 1);
            let gl = glow::Context::from_loader_function_cstr(proc_address);
            Importer { device, gl, display, context, gbm, _node: node }
        }
    }

    /// Imports the frame's dma-buf with its own description, and reads it, rows in memory order.
    fn read(&self, frame: &Frame) -> Vec<u8> {
        type CreateImage = unsafe extern "C" fn(EGLDisplay, EGLContext, u32, *mut c_void, *const EGLint) -> EGLImage;
        type DestroyImage = unsafe extern "C" fn(EGLDisplay, EGLImage) -> u32;
        type TargetTexture = unsafe extern "C" fn(u32, EGLImage);
        let attributes = [
            0x3057, // EGL_WIDTH
            frame.width() as EGLint,
            0x3056, // EGL_HEIGHT
            frame.height() as EGLint,
            0x3271, // EGL_LINUX_DRM_FOURCC_EXT
            frame.fourcc() as EGLint,
            0x3272, // EGL_DMA_BUF_PLANE0_FD_EXT
            frame.fd().as_raw_fd(),
            0x3273, // EGL_DMA_BUF_PLANE0_OFFSET_EXT
            frame.offset() as EGLint,
            0x3274, // EGL_DMA_BUF_PLANE0_PITCH_EXT
            frame.stride() as EGLint,
            0x3443, // EGL_DMA_BUF_PLANE0_MODIFIER_LO_EXT
            frame.modifier() as u32 as EGLint,
            0x3444, // EGL_DMA_BUF_PLANE0_MODIFIER_HI_EXT
            (frame.modifier() >> 32) as u32 as EGLint,
            EGL_NONE,
        ];
        let gl = &self.gl;
        let mut pixels = vec![0; frame.width() as usize * frame.height() as usize * 4];
        // SAFETY: this importer's context is current on this thread; every object is deleted before returning.
        unsafe {
            let image = load::<CreateImage>(c"eglCreateImageKHR")(
                self.display,
                ptr::null_mut(),
                EGL_LINUX_DMA_BUF_EXT,
                ptr::null_mut(),
                attributes.as_ptr(),
            );
            assert!(!image.is_null(), "the frame's dma-buf imports ({:#x})", eglGetError());
            let texture = gl.create_texture().unwrap();
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            load::<TargetTexture>(c"glEGLImageTargetTexture2DOES")(glow::TEXTURE_2D, image);
            let fbo = gl.create_framebuffer().unwrap();
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            gl.framebuffer_texture_2d(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::TEXTURE_2D, Some(texture), 0);
            assert_eq!(gl.check_framebuffer_status(glow::FRAMEBUFFER), glow::FRAMEBUFFER_COMPLETE);
            gl.read_pixels(
                0,
                0,
                frame.width() as i32,
                frame.height() as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut pixels)),
            );
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            gl.delete_framebuffer(fbo);
            gl.delete_texture(texture);
            load::<DestroyImage>(c"eglDestroyImageKHR")(self.display, image);
        }
        pixels
    }
}

impl Drop for Importer {
    fn drop(&mut self) {
        // SAFETY: the context is current on this thread and released before it is destroyed; the GBM device outlives
        // the display, and the node the device.
        unsafe {
            eglMakeCurrent(self.display, ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
            eglDestroyContext(self.display, self.context);
            eglTerminate(self.display);
            gbm_device_destroy(self.gbm);
        }
    }
}

thread_local! {
    /// One importer per thread, kept between reads: frames are read many times while waiting for one.
    static IMPORTER: RefCell<Option<Importer>> = const { RefCell::new(None) };
}

/// A frame read back as RGB, top row first.
struct Picture {
    width: usize,
    height: usize,
    rgb: Vec<[u8; 3]>,
}

impl Picture {
    /// Reads a frame through a second EGL context on its node.
    fn of(frame: &Frame) -> Picture {
        let rgba = IMPORTER.with_borrow_mut(|importer| {
            if importer.as_ref().is_none_or(|importer| importer.device != frame.device()) {
                *importer = None;
                *importer = Some(Importer::new(frame.device()));
            }
            importer.as_ref().unwrap().read(frame)
        });
        let rgb = rgba.as_chunks::<4>().0.iter().map(|&[r, g, b, _]| [r, g, b]).collect();
        Picture { width: frame.width() as usize, height: frame.height() as usize, rgb }
    }

    fn pixel(&self, x: usize, y: usize) -> [u8; 3] {
        self.rgb[y * self.width + x]
    }

    /// The number of distinct colours, ignoring the lowest bits so rounding does not count.
    fn colours(&self) -> usize {
        self.rgb.iter().map(|[r, g, b]| (r >> 4, g >> 4, b >> 4)).collect::<HashSet<_>>().len()
    }

    /// The mean number of distinct colours per row over `rows`.
    fn row_colours(&self, rows: std::ops::Range<usize>) -> f64 {
        let count = rows.len();
        let total: usize = rows
            .map(|y| {
                let row = &self.rgb[y * self.width..(y + 1) * self.width];
                row.iter().map(|[r, g, b]| (r >> 4, g >> 4, b >> 4)).collect::<HashSet<_>>().len()
            })
            .sum();
        total as f64 / count as f64
    }

    /// Whether `testsrc` is upright: its scrolling gradient, many colours along each row, is in the lower part of the
    /// picture (rows 88-108 of 120), and the mirrored rows hold only a few colour bars.
    fn is_upright_test_pattern(&self) -> bool {
        let scale = |row: usize| row * self.height / 120;
        let gradient = self.row_colours(scale(92)..scale(104));
        let mirrored = self.row_colours(scale(16)..scale(28));
        gradient > 2.0 * mirrored
    }
}

/// Whether a frame shows video rather than a blank picture.
fn shows_video(frame: &Frame) -> bool {
    Picture::of(frame).colours() >= 8
}

/// The size of the dma-buf behind `fd`, in bytes.
fn dma_buf_size(fd: BorrowedFd<'_>) -> u64 {
    let file = File::from(fd.try_clone_to_owned().unwrap());
    // A dma-buf reports its size through `lseek(SEEK_END)`.
    std::io::Seek::seek(&mut &file, std::io::SeekFrom::End(0)).expect("the dma-buf reports its size")
}

// ------------------------------------------------------------------------------------------------------------------
// Creation

#[test]
fn rejects_a_player_without_render_output() {
    let player = Player::builder().mpv_option("ao", "null").build().unwrap();
    let result = SurfaceRenderer::builder(&player).build();
    let Err(Error::Render(message)) = result else {
        panic!("a surface renderer was created: {result:?}")
    };
    assert!(message.contains("VideoOutput::Render"), "{message}");

    player.set_volume(0.5).unwrap();
    assert_eq!(player.get_property::<f64>("volume").unwrap(), 50.0);
}

#[test]
fn without_a_render_node() {
    if gl::skipped() {
        return;
    }
    let player = render_player(Autoplay::Off);
    let result = SurfaceRenderer::builder(&player).build();
    let Err(Error::Render(message)) = result else {
        eprintln!("skipped: this machine has a usable DRM node");
        return;
    };
    assert!(message.contains(NO_NODE), "{message}");

    player.load(url(TEST_SOURCE)).unwrap();
    player.set_volume(0.5).unwrap();
    assert_eq!(player.get_property::<f64>("volume").unwrap(), 50.0, "the player keeps working");
}

#[test]
fn unknown_device() {
    if gl::skipped() {
        return;
    }
    let player = render_player(Autoplay::Off);
    let device = DrmDevice { major: 226, minor: 4095 };
    let result = SurfaceRenderer::builder(&player).device(device).build();
    let Err(Error::Render(message)) = result else {
        panic!("a surface renderer was created on a missing device: {result:?}")
    };
    assert!(message.contains("226:4095"), "the error names the device number: {message}");

    player.set_volume(0.5).unwrap();
    assert_eq!(player.get_property::<f64>("volume").unwrap(), 50.0, "the player keeps working");
}

#[test]
fn layouts_the_driver_cannot_use() {
    if skipped() {
        return;
    }
    let player = render_player(Autoplay::Off);
    let result = SurfaceRenderer::builder(&player).modifiers([X_TILED]).build();
    let Err(Error::Render(message)) = result else {
        eprintln!("skipped: this driver allocates X-tiled buffers");
        return;
    };
    assert!(message.contains(&format!("{X_TILED:#x}")), "the error names the layouts: {message}");

    player.set_volume(0.5).unwrap();
    assert_eq!(player.get_property::<f64>("volume").unwrap(), 50.0, "the player keeps working");
}

#[test]
fn default_device_is_the_first_that_builds() {
    if skipped() {
        return;
    }
    let player = render_player(Autoplay::Off);
    let expected = nodes()
        .into_iter()
        .map(|(_, device)| device)
        .find(|&device| SurfaceRenderer::builder(&player).device(device).build().is_ok())
        .expect("a node builds");
    let rig = rig(Autoplay::Off);
    assert_eq!(rig.renderer.device(), expected);
    rig.renderer.set_size(16, 16);
    assert_eq!(rig.wait_frame("a frame", |_| true).device(), expected);
}

#[test]
fn chosen_device() {
    if skipped() {
        return;
    }
    let device = rig(Autoplay::Off).renderer.device();
    let player = render_player(Autoplay::Off);
    let renderer = SurfaceRenderer::builder(&player).device(device).build().expect("the renderer is created");
    assert_eq!(renderer.device(), device);
}

#[test]
fn default_layout_is_linear() {
    if skipped() {
        return;
    }
    let rig = rig(Autoplay::Off);
    assert_eq!(rig.renderer.modifier(), MODIFIER_LINEAR);
    rig.renderer.set_size(16, 16);
    assert_eq!(rig.wait_frame("a frame", |_| true).modifier(), MODIFIER_LINEAR);
}

#[test]
fn layouts_are_taken_in_order_of_preference() {
    if skipped() {
        return;
    }
    // The unusable layout is skipped for the linear one.
    let player = render_player(Autoplay::Off);
    let renderer = SurfaceRenderer::builder(&player).modifiers([X_TILED, MODIFIER_LINEAR]).build().unwrap();
    assert!([X_TILED, MODIFIER_LINEAR].contains(&renderer.modifier()));
}

#[test]
fn second_renderer_fails_and_the_first_keeps_working() {
    if skipped() {
        return;
    }
    let rig = rig(Autoplay::Off);
    let second = SurfaceRenderer::builder(&rig.player).build();
    let Err(Error::Render(message)) = second else {
        panic!("a second surface renderer was created: {second:?}")
    };
    assert!(message.contains("already has a render context"), "{message}");

    rig.renderer.set_size(160, 120);
    rig.player.load(url(TEST_SOURCE)).unwrap();
    rig.wait_frame("the first renderer to show video", shows_video);
}

#[test]
fn fails_while_a_render_context_exists() {
    if skipped() {
        return;
    }
    let gl = Gl::new();
    let player = render_player(Autoplay::Off);
    // SAFETY: `gl` is current on this thread, where the render context is used and dropped, and outlives it.
    let mut render = unsafe { RenderContext::new(&player, gl.loader(), || {}) }.expect("the render context is created");

    let result = SurfaceRenderer::builder(&player).build();
    let Err(Error::Render(message)) = result else {
        panic!("a surface renderer was created next to a render context: {result:?}")
    };
    assert!(message.contains("already has a render context"), "{message}");

    // The render context keeps working, and releases the player for a surface renderer once dropped.
    let framebuffer = gl.framebuffer(160, 120, [0, 0, 0, 255]);
    render.update();
    render
        .render(
            RenderTarget { fbo: framebuffer.name(), width: framebuffer.width, height: framebuffer.height },
            false,
        )
        .expect("the render context still renders");
    gl.delete(framebuffer);
    drop(render);
    drop(gl);
    let (renderer, frames) = renderer(&player);
    renderer.set_size(160, 120);
    player.load(url(TEST_SOURCE)).unwrap();
    wait_frame(&renderer, &frames, &player, "the surface renderer to show video", shows_video);
}

#[test]
fn recreated_after_drop_publishes_frames() {
    if skipped() {
        return;
    }
    let player = render_player(Autoplay::Off);
    let (first, _frames) = renderer(&player);
    drop(first);
    let (second, frames) = renderer(&player);
    second.set_size(160, 120);
    player.load(url(TEST_SOURCE)).unwrap();
    wait_frame(&second, &frames, &player, "the new renderer to show video", shows_video);
}

// ------------------------------------------------------------------------------------------------------------------
// Sizes and publication

#[test]
fn no_frame_without_a_size() {
    if skipped() {
        return;
    }
    let rig = rig(Autoplay::On);
    let events = rig.player.subscribe();
    rig.player.load(url(TEST_SOURCE)).unwrap();
    wait_position_past(&events, Duration::from_millis(500));
    assert!(rig.renderer.latest().is_none(), "a frame was published");
    assert!(rig.frames.try_recv().is_err(), "a frame was announced");
}

#[test]
fn frames_while_playing_have_the_set_size() {
    if skipped() {
        return;
    }
    let rig = rig(Autoplay::On);
    rig.renderer.set_size(320, 180);
    rig.player.load(url(TEST_SOURCE)).unwrap();

    let first = rig.wait_frame("a video frame", shows_video);
    let mut sequence = first.sequence();
    for _ in 0..10 {
        rig.frames.recv_timeout(TIMEOUT).expect("frames keep being announced");
        let frame = rig.renderer.latest().expect("a frame is published before it is announced");
        assert_eq!((frame.width(), frame.height()), (320, 180));
        assert!(frame.sequence() >= sequence, "frames are published in order");
        sequence = frame.sequence();
    }
    assert!(sequence > first.sequence(), "the latest frame changes while playing");
    assert_eq!(Picture::of(&rig.renderer.latest().unwrap()).width, 320);
}

#[test]
fn latest_frame_from_another_thread() {
    if skipped() {
        return;
    }
    let rig = rig(Autoplay::On);
    rig.renderer.set_size(160, 120);
    rig.player.load(url(TEST_SOURCE)).unwrap();
    rig.wait_frame("a video frame", shows_video);

    std::thread::scope(|scope| {
        scope.spawn(|| {
            for _ in 0..20 {
                let frame = rig.renderer.latest().expect("a frame is published");
                assert_eq!((frame.width(), frame.height()), (160, 120));
                assert!(shows_video(&frame), "the frame is complete");
            }
            // This thread's importer is dropped with it.
            IMPORTER.with_borrow_mut(Option::take);
        });
    });
}

#[test]
fn resize_while_paused() {
    if skipped() {
        return;
    }
    let rig = rig(Autoplay::Off);
    rig.renderer.set_size(160, 120);
    rig.player.load(url(TEST_SOURCE)).unwrap();
    rig.wait_frame("the first frame", shows_video);
    assert!(rig.player.state().paused);

    rig.renderer.set_size(240, 180);
    let frame = rig.wait_frame("a frame at the new size", |frame| (frame.width(), frame.height()) == (240, 180));
    assert!(shows_video(&frame), "the paused picture is shown at the new size");
}

#[test]
fn zero_size_stops_publication() {
    if skipped() {
        return;
    }
    let rig = rig(Autoplay::On);
    let events = rig.player.subscribe();
    rig.renderer.set_size(160, 120);
    rig.player.load(url(TEST_SOURCE)).unwrap();
    rig.wait_frame("a video frame", shows_video);

    rig.renderer.set_size(0, 120);
    // Let a frame drawn before the change land, then nothing more may be published. Notifications of earlier frames
    // are still queued, so wait until none has come for a while.
    while rig.frames.recv_timeout(Duration::from_millis(200)).is_ok() {}
    let sequence = rig.renderer.latest().unwrap().sequence();
    let now = rig.player.state().position.unwrap_or_default();
    wait_position_past(&events, now + Duration::from_millis(400));
    assert_eq!(rig.renderer.latest().unwrap().sequence(), sequence, "a frame was published at zero size");

    rig.renderer.set_size(160, 120);
    rig.wait_frame("frames again", |frame| frame.sequence() > sequence);
}

#[test]
fn black_with_nothing_loaded() {
    if skipped() {
        return;
    }
    let rig = rig(Autoplay::Off);
    rig.renderer.set_size(160, 120);
    let picture = Picture::of(&rig.wait_frame("a frame", |_| true));
    let lit = picture.rgb.iter().filter(|pixel| !is_black(**pixel)).count();
    assert_eq!(lit, 0, "the frame is black, first pixel: {:?}", picture.rgb[0]);
}

// ------------------------------------------------------------------------------------------------------------------
// Content

#[test]
fn test_pattern_is_upright() {
    if skipped() {
        return;
    }
    let rig = rig(Autoplay::Off);
    rig.renderer.set_size(160, 120);
    rig.player.load(url(TEST_SOURCE)).unwrap();
    let picture = Picture::of(&rig.wait_frame("the first frame", shows_video));
    assert!(picture.colours() >= 8, "the frame shows the test pattern, not a blank image");
    assert!(
        picture.is_upright_test_pattern(),
        "the gradient is at the bottom: {:.1} colours per row there, {:.1} in the mirrored rows",
        picture.row_colours(92..104),
        picture.row_colours(16..28)
    );
}

#[test]
fn solid_colours() {
    if skipped() {
        return;
    }
    let rig = rig(Autoplay::Off);
    rig.renderer.set_size(160, 120);
    rig.player.load(url(FIELDS_SOURCE)).unwrap();
    let picture = Picture::of(&rig.wait_frame("the first frame", |frame| Picture::of(frame).colours() >= 4));
    // Read as R, G, B, A bytes in memory order.
    let fields: [[u8; 3]; 5] = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 255], [128, 128, 128]];
    for (field, expected) in fields.into_iter().enumerate() {
        let actual = picture.pixel(16 + 32 * field, 60);
        let close = expected.iter().zip(actual).all(|(e, a)| f64::from(e.abs_diff(a)) <= 0.03 * 255.0);
        assert!(close, "field {field}: {actual:?}, expected {expected:?}");
    }
}

#[test]
fn letterboxing() {
    if skipped() {
        return;
    }
    let rig = rig(Autoplay::Off);
    rig.renderer.set_size(320, 120);
    rig.player.load(url(TEST_SOURCE)).unwrap();
    let picture = Picture::of(&rig.wait_frame("the first frame", shows_video));
    // The 160x120 video fills columns 80..240; a few columns at each edge are skipped for filtering.
    for y in 0..120 {
        for x in (0..76).chain(244..320) {
            assert!(is_black(picture.pixel(x, y)), "pixel ({x}, {y}) is in a bar but is {:?}", picture.pixel(x, y));
        }
    }
    let lit = (84..236)
        .flat_map(|x| (0..120).map(move |y| (x, y)))
        .filter(|&(x, y)| !is_black(picture.pixel(x, y)));
    assert!(lit.count() > 152 * 120 / 2, "the video fills the middle");
}

/// YUV video scaled down by an uneven factor keeps its colours over the whole picture on every frame. On Mesa's
/// software renderers, mpv's default rendering draws it black or in wrong colours, and bilinear scaling sometimes
/// black or crossed by black lines, for the whole run, unless the core puts mpv in its dumb mode.
#[test]
fn yuv_colours_scaled_down() {
    if skipped() {
        return;
    }
    let rig = rig(Autoplay::On);
    rig.player.set_property("loop-file", "inf".to_string()).unwrap();
    // The 160x120 fields at about 0.73: 117x88, with less than a pixel of bar at each side.
    let (width, height) = (118, 88);
    rig.renderer.set_size(width, height);
    let source = FIELDS_SOURCE.replace("[out0]", ",format=yuv420p[out0]");
    rig.player.load(url(&source)).unwrap();
    // The first frames may come before the video's.
    let first = rig.wait_frame("the video at the set size", |frame| {
        (frame.width(), frame.height()) == (width, height) && Picture::of(frame).colours() >= 4
    });
    let fields: [[u8; 3]; 5] = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 255], [128, 128, 128]];
    for later in 0..5 {
        let frame = rig.wait_frame("a newer frame", |frame| frame.sequence() > first.sequence() + 5 * later);
        let picture = Picture::of(&frame);
        for (field, expected) in fields.into_iter().enumerate() {
            let actual = picture.pixel(117 * (2 * field + 1) / 10, 44);
            let close = expected.iter().zip(actual).all(|(e, a)| f64::from(e.abs_diff(a)) <= 0.05 * 255.0);
            assert!(close, "frame {}, field {field}: {actual:?}, expected {expected:?}", frame.sequence());
        }
        // No field is black, nor is any blend of two.
        for y in 2..height as usize - 2 {
            for x in 2..width as usize - 2 {
                let pixel = picture.pixel(x, y);
                assert!(!is_black(pixel), "frame {}: ({x}, {y}) is black", frame.sequence());
            }
        }
    }
}

/// On software renderers the core puts mpv in its dumb mode only for a player that set neither it nor a scaler, even
/// to mpv's default.
#[test]
fn own_rendering_options_are_kept_on_software_renderers() {
    if skipped() || !software() {
        return;
    }
    let default = rig(Autoplay::Off);
    assert_eq!(default.player.get_property::<String>("gpu-dumb-mode").unwrap(), "yes");
    let default_scale = default.player.get_property::<String>("option-info/scale/default-value").unwrap();

    for (name, value) in [("scale", "spline36"), ("scale", default_scale.as_str()), ("gpu-dumb-mode", "no")] {
        let player = Player::builder()
            .video_output(VideoOutput::Render)
            .mpv_option("ao", "null")
            .mpv_option(name, value)
            .build()
            .unwrap();
        let (renderer, frames) = renderer(&player);
        let own = Rig { renderer, frames, player };
        assert_eq!(own.player.get_property::<String>(name).unwrap(), value);
        assert_ne!(own.player.get_property::<String>("gpu-dumb-mode").unwrap(), "yes", "with {name}={value}");
    }
}

// ------------------------------------------------------------------------------------------------------------------
// Media loaded before the renderer

#[test]
fn renderer_created_after_load() {
    if skipped() {
        return;
    }
    let player = render_player(Autoplay::On);
    let events = player.subscribe();
    // A seekable file: mpv re-selects the video track with a seek to the current position, which a generated source
    // cannot do, so its video would resume at the demuxer's read-ahead, more than a second later.
    player.load(fixture("testsrc-h264-aac.mp4")).unwrap();
    wait_loaded(&events);
    wait_position_past(&events, Duration::from_millis(500));
    let before = player.state().position.unwrap();
    let start = Instant::now();

    let (renderer, frames) = renderer(&player);
    renderer.set_size(160, 120);
    wait_frame(&renderer, &frames, &player, "the video to appear", shows_video);

    // Where playback would be had nothing happened, give or take the one second the spec allows.
    let position = player.state().position.unwrap();
    let expected = before + start.elapsed();
    assert!(
        position + Duration::from_secs(1) >= before && position <= expected + Duration::from_secs(1),
        "playback continues from about where it was: {before:?} before, {position:?} after, {expected:?} expected"
    );
    wait_position_past(&events, position + Duration::from_millis(300));
    assert!(!player.state().ended);
}

#[test]
fn media_without_audio_loaded_before_the_renderer() {
    if skipped() {
        return;
    }
    // The fixture has no audio, so its load ends at once when its video output cannot start.
    let player = render_player(Autoplay::On);
    let events = player.subscribe();
    player.load(h264_fixture()).unwrap();
    let deadline = Instant::now() + TIMEOUT;
    loop {
        match events.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Some(Event::Ended(EndReason::Error(_))) => break,
            Some(_) => {}
            None => panic!("timed out waiting for the load to fail without a renderer"),
        }
    }

    let (renderer, frames) = renderer(&player);
    renderer.set_size(160, 120);
    wait_loaded(&events);
    wait_frame(&renderer, &frames, &player, "the video to appear", shows_video);
}

#[test]
fn media_without_audio_loaded_just_before_the_renderer() {
    if skipped() {
        return;
    }
    // Whether the load fails before or after the renderer exists, the video appears.
    for _ in 0..5 {
        let player = render_player(Autoplay::On);
        player.load(h264_fixture()).unwrap();
        let (renderer, frames) = renderer(&player);
        renderer.set_size(160, 120);
        wait_frame(&renderer, &frames, &player, "the video to appear", shows_video);
    }
}

// ------------------------------------------------------------------------------------------------------------------
// Lifetimes

#[test]
fn drop_renderer_while_playing() {
    if skipped() {
        return;
    }
    let rig = rig(Autoplay::On);
    let events = rig.player.subscribe();
    rig.renderer.set_size(160, 120);
    rig.player.load(url(TEST_SOURCE_WITH_AUDIO)).unwrap();
    rig.wait_frame("a video frame", shows_video);

    let Rig { renderer, player, .. } = rig;
    bounded_drop(renderer, "the surface renderer");

    let now = player.state().position.unwrap_or_default();
    wait_position_past(&events, now + Duration::from_millis(300));
    player.pause().unwrap();
    let deadline = Instant::now() + TIMEOUT;
    while !player.state().paused {
        let left = deadline.saturating_duration_since(Instant::now());
        assert!(events.recv_timeout(left).is_some(), "timed out waiting for the pause");
    }
    assert!(player.get_property::<bool>("pause").unwrap());
}

#[test]
fn drop_player_before_renderer() {
    if skipped() {
        return;
    }
    let rig = rig(Autoplay::On);
    rig.renderer.set_size(160, 120);
    rig.player.load(url(TEST_SOURCE)).unwrap();
    rig.wait_frame("a video frame", shows_video);

    let Rig { renderer, player, .. } = rig;
    bounded_drop(player, "the last player");
    assert!(renderer.latest().is_some(), "the renderer still answers");
    bounded_drop(renderer, "the surface renderer after the player");
}

#[test]
fn frame_and_fd_outlive_the_renderer() {
    if skipped() {
        return;
    }
    let rig = rig(Autoplay::Off);
    rig.renderer.set_size(160, 120);
    rig.player.load(url(TEST_SOURCE)).unwrap();
    let frame = rig.wait_frame("the first frame", shows_video);
    let duplicate = frame.fd().try_clone_to_owned().unwrap();

    let Rig { renderer, player, .. } = rig;
    drop(renderer);
    drop(player);
    assert_eq!((frame.width(), frame.height()), (160, 120));
    // Imported only now, after the renderer, its buffers and its GBM device are gone.
    assert!(
        Picture::of(&frame).is_upright_test_pattern(),
        "the dma-buf imports and holds the frame's picture"
    );
    // A duplicate of the fd keeps the memory too.
    let size = dma_buf_size(std::os::fd::AsFd::as_fd(&duplicate));
    drop(frame);
    assert_eq!(dma_buf_size(std::os::fd::AsFd::as_fd(&duplicate)), size);
}

#[test]
fn no_notification_after_drop() {
    if skipped() {
        return;
    }
    let rig = rig(Autoplay::On);
    rig.renderer.set_size(160, 120);
    rig.player.load(url(TEST_SOURCE_WITH_AUDIO)).unwrap();
    rig.wait_frame("a video frame", shows_video);

    let Rig { renderer, frames, player } = rig;
    drop(renderer);
    // Notifications sent before the drop may still be queued. After them the channel must be closed: the callback,
    // which holds the only sender, is gone, so it cannot be called again.
    let deadline = Instant::now() + TIMEOUT;
    loop {
        match frames.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(()) => {}
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => panic!("the notification outlived the renderer"),
        }
    }
    drop(player);
}

// ------------------------------------------------------------------------------------------------------------------
// Sharing

#[test]
fn import_into_another_context() {
    if skipped() {
        return;
    }
    let rig = rig(Autoplay::Off);
    rig.renderer.set_size(200, 150);
    rig.player.load(url(TEST_SOURCE)).unwrap();
    let frame = rig.wait_frame("the first frame", shows_video);

    let picture = Picture::of(&frame);
    assert_eq!((picture.width, picture.height), (200, 150), "the import has the frame's size");
    assert!(picture.is_upright_test_pattern(), "the imported texture holds the published picture");
}

#[test]
fn single_plane() {
    if skipped() {
        return;
    }
    let rig = rig(Autoplay::Off);
    rig.renderer.set_size(161, 121);
    let frame = rig.wait_frame("a frame", |_| true);
    assert_eq!(frame.fourcc(), u32::from_le_bytes(*b"AB24"), "DRM_FORMAT_ABGR8888");
    assert!(frame.stride() >= 161 * 4, "a row holds the width: stride {}", frame.stride());
    // One plane: its rows fit the dma-buf, which has nothing a second plane would need.
    let size = dma_buf_size(frame.fd());
    assert!(
        u64::from(frame.offset()) + u64::from(frame.stride()) * 121 <= size,
        "the plane fits the dma-buf: offset {}, stride {}, size {size}",
        frame.offset(),
        frame.stride()
    );
}

// ------------------------------------------------------------------------------------------------------------------
// Hardware decoding

/// Plays the H.264 fixture until it shows video, and returns the hardware decoder mpv reports.
fn play_h264() -> String {
    let rig = rig(Autoplay::On);
    rig.renderer.set_size(160, 120);
    rig.player.load(h264_fixture()).unwrap();
    let frame = rig.wait_frame("the H.264 video", shows_video);
    assert!(Picture::of(&frame).is_upright_test_pattern(), "the decoded video is drawn upright");
    rig.player.get_property::<String>("hwdec-current").unwrap()
}

#[test]
fn h264_frames_are_published() {
    if skipped() {
        return;
    }
    // Hardware or software decoding, depending on the GPU; frames are published either way.
    eprintln!("hwdec-current: {}", play_h264());
}

#[test]
fn hardware_decoding_on_the_gpu() {
    if !std::env::var("PLAYER_TEST_GL").is_ok_and(|value| value == "hardware") {
        eprintln!("skipped: needs a GPU with a hardware H.264 decoder; set PLAYER_TEST_GL=hardware");
        return;
    }
    assert!(!skipped(), "PLAYER_TEST_GL=hardware, but no DRM render node can be used");
    let decoder = play_h264();
    assert!(
        !decoder.is_empty() && decoder != "no" && !decoder.ends_with("-copy"),
        "a zero-copy hardware decoder is active: {decoder:?}"
    );
}
