//! What the pixel tests of Windows and Linux share: windows of a headless app drawn by GPUI's own renderer, and read
//! back.

use super::SurfaceRenderer;
use super::tests::{playing, views};
use crate::VideoPlayer;
use gpui_kit::*;
use player::Player;
use std::time::{Duration, Instant};

/// Opens a hidden `width` x `height`-point window showing `view`.
pub(crate) fn open<V: Render + 'static>(
    cx: &mut HeadlessAppContext,
    width: f32,
    height: f32,
    view: impl FnOnce() -> V,
) -> WindowHandle<V> {
    cx.open_window(size(px(width), px(height)), |_, cx| cx.new(|_| view())).unwrap()
}

/// A window's pixels, as RGBA rows.
pub(crate) struct Picture {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pixels: Vec<u8>,
}

/// Draws the window and reads back what GPUI's renderer makes of it.
pub(crate) fn screenshot<V: 'static>(cx: &mut HeadlessAppContext, window: WindowHandle<V>) -> Picture {
    cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx)).unwrap();
    let image = cx.capture_screenshot(window.into()).unwrap();
    Picture { width: image.width(), height: image.height(), pixels: image.into_raw() }
}

pub(crate) fn rgb(picture: &Picture, x: u32, y: u32) -> [u8; 3] {
    let at = ((y * picture.width + x) * 4) as usize;
    [picture.pixels[at], picture.pixels[at + 1], picture.pixels[at + 2]]
}

/// Whether `actual` is within 3% of `expected` in each channel.
pub(crate) fn close(actual: [u8; 3], expected: [u8; 3]) -> bool {
    actual.iter().zip(expected).all(|(&a, e)| f64::from(a.abs_diff(e)) <= 0.03 * 255.0)
}

/// Checks a 320x180-pixel `image` of a 4:3 picture whose quadrants are, from the top left, red, green, blue and
/// yellow: upright, and letterboxed into 240x180. `what` names the image in failures.
pub(crate) fn assert_quadrants_letterboxed(image: &Picture, what: &str) {
    assert_eq!((image.width, image.height), (320, 180));
    for ((x, y), colour) in [((100, 45), [255, 0, 0]), ((220, 45), [0, 255, 0]), ((100, 135), [0, 0, 255])]
        .into_iter()
        .chain([((220, 135), [255, 255, 0])])
    {
        assert!(close(rgb(image, x, y), colour), "{what}: ({x}, {y}) is {:?}", rgb(image, x, y));
    }
    for (x, y) in [(20, 90), (38, 10), (282, 170), (300, 90)] {
        assert_eq!(rgb(image, x, y), [0, 0, 0], "{what}: ({x}, {y}) is in a bar");
    }
}

/// Five 32x60 solid fields side by side, red, green, blue, white and grey, over a 160x60 yellow field, with a tone.
pub(crate) const FIELDS_SOURCE: &str = "av://lavfi:color=c=0xFF0000:s=32x60:r=25:d=30[r];\
    color=c=0x00FF00:s=32x60:r=25:d=30[g];color=c=0x0000FF:s=32x60:r=25:d=30[b];\
    color=c=0xFFFFFF:s=32x60:r=25:d=30[w];color=c=0x808080:s=32x60:r=25:d=30[k];\
    color=c=0xFFFF00:s=160x60:r=25:d=30[y];[r][g][b][w][k]hstack=inputs=5[top];[top][y]vstack=inputs=2[out0];\
    sine=frequency=440:duration=30[out1]";

struct Movie(Player);

impl Render for Movie {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(VideoPlayer::new("movie", &self.0).skin(crate::Skin::None).size_full())
    }
}

/// The whole path: mpv's frames through the core's renderer and the element into GPUI's renderer, which draws the
/// fields upright, in their colours and letterboxed.
pub(crate) fn assert_video_drawn(cx: &mut HeadlessAppContext) {
    cx.allow_parking();
    let player = playing(FIELDS_SOURCE);
    // 320x180 points: 640x360 pixels, in which the 160x120 video fills 480x360, three pixels per video pixel.
    let window = open(cx, 320., 180., || Movie(player.clone()));

    // Until a frame at the window's size is drawn: the first ones may be of another size, or black.
    let deadline = Instant::now() + Duration::from_secs(15);
    let image = loop {
        cx.run_until_parked();
        // In a `HeadlessAppContext` window, the view's frame notifications do not redraw it, unlike on the real
        // platform (see the `play` example) and in `TestAppContext` (the repaint tests in `video.rs`). This test
        // checks pixels, not repainting, so it redraws the whole window.
        cx.update_window(window.into(), |_, window, _| window.refresh()).unwrap();
        let image = screenshot(cx, window);
        let frame = views()
            .iter()
            .filter_map(WeakEntity::upgrade)
            .find_map(|view| cx.update(|cx| view.read(cx).renderer().and_then(SurfaceRenderer::latest)));
        let sized = frame.is_some_and(|frame| (frame.width(), frame.height()) == (640, 360));
        if sized && !close(rgb(&image, 320, 270), [0, 0, 0]) {
            break image;
        }
        assert!(Instant::now() < deadline, "timed out waiting for the video");
        std::thread::sleep(Duration::from_millis(10));
    };

    let fields = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 255], [128, 128, 128]];
    for (field, expected) in fields.into_iter().enumerate() {
        let actual = rgb(&image, 128 + 96 * field as u32, 90);
        assert!(close(actual, expected), "field {field}: {actual:?}, expected {expected:?}");
    }
    let bottom = rgb(&image, 320, 270);
    assert!(close(bottom, [255, 255, 0]), "the yellow field is at the bottom: {bottom:?}");
    for (x, y) in [(40, 180), (600, 180), (40, 20), (600, 340)] {
        assert!(close(rgb(&image, x, y), [0, 0, 0]), "({x}, {y}) is in a bar: {:?}", rgb(&image, x, y));
    }
}
