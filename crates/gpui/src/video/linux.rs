//! Linux: single-plane RGBA dma-bufs, which the patched GPUI imports on its own device with no copy.

use gpui_kit::*;
use player::Player;
use player::linux::DrmDevice;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) use player::linux::{Frame, SurfaceRenderer};

/// Builds on GPUI's render node with the modifiers GPUI can import, when GPUI reports its node. Otherwise, or when the
/// renderer cannot be built there (llvmpipe has no buffers it can draw into on `vgem`'s render node), it builds with
/// the core's defaults: the first node it can render on, and LINEAR buffers, which import on any device.
pub(super) fn build(player: &Player, on_frame: impl Fn() + Send + Sync + 'static) -> player::Result<SurfaceRenderer> {
    // Shared by both attempts: a failed build drops its copy.
    let on_frame: Arc<dyn Fn() + Send + Sync> = Arc::new(on_frame);
    let signal = |on_frame: &Arc<dyn Fn() + Send + Sync>| {
        let on_frame = Arc::clone(on_frame);
        move || on_frame()
    };
    // Modifiers without a node are not passed: they may not apply to the device the core picks.
    if let Some(import) = gpui::dma_buf_import()
        && let Some((major, minor)) = import.render_node
    {
        let device = DrmDevice { major, minor };
        match SurfaceRenderer::builder(player)
            .device(device)
            .modifiers(import.modifiers)
            .on_frame(signal(&on_frame))
            .build()
        {
            Ok(renderer) => return Ok(renderer),
            Err(error) => log::debug!("player_gpui: cannot render on GPUI's render node {device}: {error}"),
        }
    }
    SurfaceRenderer::builder(player).on_frame(signal(&on_frame)).build()
}

pub(super) fn paint(window: &mut Window, bounds: Bounds<Pixels>, frame: &Frame) {
    static WARNED: AtomicBool = AtomicBool::new(false);
    let layout = DmaBufLayout {
        width: frame.width(),
        height: frame.height(),
        fourcc: frame.fourcc(),
        modifier: frame.modifier(),
        stride: frame.stride(),
        offset: frame.offset(),
    };
    // GPUI keeps the buffer through the duplicate for as long as it draws it.
    match DmaBufTexture::duplicate(frame.fd(), layout) {
        Ok(texture) => window.paint_surface(bounds, texture),
        // Only when the process runs out of descriptors, or the like: the frame stays black.
        Err(error) if !WARNED.swap(true, Ordering::Relaxed) => {
            log::warn!("player_gpui: a video frame cannot be shown: {error}");
        }
        Err(_) => {}
    }
}

/// Pixels drawn by GPUI's wgpu renderer offscreen, through the patched GPUI's headless renderer, and the choice of
/// render device. Steer GPUI's adapter with `VK_DRIVER_FILES`; on WSL2, `LIBGL_ALWAYS_SOFTWARE=1` is needed, or GPUI
/// picks Mesa's `d3d12` GL driver, which cannot import dma-bufs.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::VideoPlayer;
    use crate::video::fit;
    use crate::video::pixels::{
        FIELDS_SOURCE, assert_quadrants_letterboxed, assert_video_drawn, close, open, rgb, screenshot,
    };
    use crate::video::tests::{cannot_show_video, playing, render_player, take_warnings};
    use gpui_wgpu::WgpuHeadlessRenderer;
    use player::prelude::*;
    use std::sync::{Mutex, MutexGuard, PoisonError};
    // The glob above brings in GPUI's `test` macro; these are plain tests.
    use std::prelude::v1::test;
    use std::time::{Duration, Instant};

    /// Held by the tests that set or read what GPUI reports it imports: one value for the whole process.
    fn import_lock() -> MutexGuard<'static, ()> {
        static IMPORT: Mutex<()> = Mutex::new(());
        IMPORT.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A headless app whose windows GPUI's wgpu renderer draws, or `None`, with a message, where it cannot start or
    /// cannot import dma-bufs.
    fn headless() -> Option<HeadlessAppContext> {
        if let Err(error) = WgpuHeadlessRenderer::new() {
            eprintln!("skipped: GPUI's wgpu renderer cannot start: {error:#}");
            return None;
        }
        if gpui::dma_buf_import().is_none() {
            eprintln!("skipped: GPUI's renderer cannot import dma-bufs on this adapter");
            return None;
        }
        let renderer = || Ok(Some(Box::new(WgpuHeadlessRenderer::new()?) as Box<dyn PlatformHeadlessRenderer>));
        Some(HeadlessAppContext::with_platform(Arc::new(NoopTextSystem), Arc::new(()), renderer))
    }

    /// A 64x48 picture whose quadrants are, from the top left, red, green, blue and yellow.
    const QUADRANTS_SOURCE: &str = "av://lavfi:color=c=0xFF0000:s=32x24:r=25:d=30[a];\
        color=c=0x00FF00:s=32x24:r=25:d=30[b];color=c=0x0000FF:s=32x24:r=25:d=30[c];\
        color=c=0xFFFF00:s=32x24:r=25:d=30[d];[a][b]hstack[top];[c][d]hstack[bottom];[top][bottom]vstack[out0]";

    /// A 64x48 frame of the quadrants, from a renderer that is then dropped: a frame stays unchanged only until two
    /// newer ones are published, and its buffer outlives the renderer.
    fn quadrants_frame() -> Frame {
        let player = playing(QUADRANTS_SOURCE);
        let renderer = build(&player, || {}).unwrap();
        frame_of(&renderer, 64, 48)
    }

    /// A frame of `renderer` at `width` x `height` that is not black.
    fn frame_of(renderer: &SurfaceRenderer, width: u32, height: u32) -> Frame {
        renderer.set_size(width, height);
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(frame) = renderer.latest()
                && (frame.width(), frame.height()) == (width, height)
                && frame.sequence() > 2
            {
                return frame;
            }
            assert!(Instant::now() < deadline, "timed out waiting for a {width}x{height} frame");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Paints one frame the way the video view does, letterboxed with `fit`, or a copy of it described with `layout`.
    struct Painted {
        frame: Frame,
        layout: Option<DmaBufLayout>,
    }

    impl Render for Painted {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let (frame, layout) = (self.frame.clone(), self.layout);
            div().size_full().bg(black()).child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        let bounds = fit(bounds, frame.width(), frame.height());
                        match layout {
                            None => paint(window, bounds, &frame),
                            Some(layout) => {
                                window.paint_surface(bounds, DmaBufTexture::duplicate(frame.fd(), layout).unwrap())
                            }
                        }
                    },
                )
                .size_full(),
            )
        }
    }

    /// The draw path: a frame of the core's renderer, imported by GPUI's and painted again and again.
    #[test]
    fn frame_is_drawn_upright_and_letterboxed() {
        if cannot_show_video() {
            return;
        }
        let _import = import_lock();
        let Some(mut cx) = headless() else {
            return;
        };
        let frame = quadrants_frame();
        // 160x90 points at the test windows' scale of 2: 320x180 pixels, in which the 4:3 frame fills 240x180.
        let window = open(&mut cx, 160., 90., || Painted { frame, layout: None });
        for paint in 0..3 {
            // Each draw paints the same buffer through a new duplicate of its fd: GPUI imports it once.
            assert_quadrants_letterboxed(&screenshot(&mut cx, window), &format!("paint {paint}"));
        }
    }

    /// A buffer GPUI cannot import stays black and is warned about once, however often it is painted.
    #[test]
    fn warns_once_about_a_buffer_it_cannot_import() {
        if cannot_show_video() {
            return;
        }
        let _import = import_lock();
        let Some(mut cx) = headless() else {
            return;
        };
        let frame = quadrants_frame();
        // `DRM_FORMAT_XRGB8888`: no importer here takes it.
        let layout = DmaBufLayout {
            width: frame.width(),
            height: frame.height(),
            fourcc: u32::from_le_bytes(*b"XR24"),
            modifier: frame.modifier(),
            stride: frame.stride(),
            offset: frame.offset(),
        };
        take_warnings();
        let window = open(&mut cx, 160., 90., || Painted { frame, layout: Some(layout) });
        for _ in 0..5 {
            let image = screenshot(&mut cx, window);
            assert_eq!(rgb(&image, 160, 90), [0, 0, 0], "the buffer is not drawn");
        }
        let warnings = take_warnings();
        let skipped = warnings.iter().filter(|warning| warning.contains("dma-buf surface cannot be drawn")).count();
        assert_eq!(skipped, 1, "one warning: {warnings:?}");
    }

    /// The whole path: mpv's frames through the core's renderer and the element into GPUI's.
    #[test]
    fn video_is_drawn_upright_in_its_colours_and_letterboxed() {
        if cannot_show_video() {
            return;
        }
        let _import = import_lock();
        let Some(mut cx) = headless() else {
            return;
        };
        assert_video_drawn(&mut cx);
    }

    /// The element with its controls, which never hide: the control bar is drawn over the video.
    struct Controlled(Player);

    impl Render for Controlled {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let options = player::ui::Options::builder().inactivity_timeout(Duration::ZERO).build().unwrap();
            div().size_full().child(VideoPlayer::new("movie", &self.0).options(options).size_full())
        }
    }

    #[test]
    fn controls_are_drawn_over_the_video() {
        if cannot_show_video() {
            return;
        }
        let _import = import_lock();
        let Some(mut cx) = headless() else {
            return;
        };
        cx.allow_parking();
        cx.update(gpui_kit::init);
        let player = playing(FIELDS_SOURCE);
        // As above: 640x360 pixels, the video in x 80-560. The 30-point control bar covers y 300-360, over the yellow
        // field and the bars.
        let window = open(&mut cx, 320., 180., || Controlled(player.clone()));

        // The control bar's `rgba(43, 51, 63, 0.7)` over yellow, and over black.
        let bar = |under: [u8; 3]| -> [u8; 3] {
            let over = [43., 51., 63.];
            std::array::from_fn(|i| (0.3 * f64::from(under[i]) + 0.7 * over[i]).round() as u8)
        };
        let deadline = Instant::now() + Duration::from_secs(15);
        let image = loop {
            cx.run_until_parked();
            cx.update_window(window.into(), |_, window, _| window.refresh()).unwrap();
            let image = screenshot(&mut cx, window);
            // Its top edge, above the progress bar in its middle: plain bar.
            if close(rgb(&image, 320, 270), [255, 255, 0]) && close(rgb(&image, 320, 302), bar([255, 255, 0])) {
                break image;
            }
            assert!(Instant::now() < deadline, "timed out waiting for the video under the controls");
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(
            close(rgb(&image, 40, 302), bar([0, 0, 0])),
            "the bar spans the black bars: {:?}",
            rgb(&image, 40, 302)
        );
        assert!(close(rgb(&image, 128, 90), [255, 0, 0]), "the video above the bar: {:?}", rgb(&image, 128, 90));
    }

    /// Puts back what GPUI reported when it is dropped: the renderers made by other tests do not report it again.
    struct Restore(Option<DmaBufImport>);

    impl Drop for Restore {
        fn drop(&mut self) {
            gpui::set_dma_buf_import(self.0.take());
        }
    }

    /// The device and modifier a renderer is built with while GPUI reports `import`.
    fn built_with(import: Option<DmaBufImport>) -> (DrmDevice, u64) {
        gpui::set_dma_buf_import(import);
        let renderer = build(&render_player(), || {}).unwrap();
        (renderer.device(), renderer.modifier())
    }

    /// The device the core picks by itself.
    fn default_device() -> DrmDevice {
        SurfaceRenderer::builder(&render_player()).build().unwrap().device()
    }

    #[test]
    fn builds_on_the_cores_defaults_when_gpui_reports_nothing() {
        if cannot_show_video() {
            return;
        }
        let _import = import_lock();
        let _restore = Restore(gpui::dma_buf_import());
        let expected = (default_device(), player::linux::MODIFIER_LINEAR);
        assert_eq!(built_with(None), expected, "no import");
        // Modifiers without a node may not apply to the core's device: LINEAR is used.
        let tiled = DmaBufImport { render_node: None, modifiers: vec![0x0100_0000_0000_0001] };
        assert_eq!(built_with(Some(tiled)), expected, "modifiers without a node");
    }

    #[test]
    fn builds_on_gpuis_node_and_falls_back_when_it_cannot() {
        if cannot_show_video() {
            return;
        }
        let _import = import_lock();
        let _restore = Restore(gpui::dma_buf_import());
        let default = default_device();
        let linear = vec![player::linux::MODIFIER_LINEAR];

        let on = |device: DrmDevice| DmaBufImport {
            render_node: Some((device.major, device.minor)),
            modifiers: linear.clone(),
        };
        assert_eq!(built_with(Some(on(default))).0, default, "on GPUI's node");
        // A node that does not exist, as one the core cannot render on: the core's own choice.
        let missing = DrmDevice { major: 226, minor: 250 };
        assert_eq!(built_with(Some(on(missing))).0, default, "GPUI's node is unusable");
    }
}
