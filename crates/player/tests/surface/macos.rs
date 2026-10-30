//! macOS: [`SurfaceRenderer`] into IOSurface-backed pixel buffers. Apple's software OpenGL renderer is used, as on CI;
//! set `PLAYER_TEST_GL=hardware` to use the GPU instead.

use super::*;
use crate::gl::{self, Gl};
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferGetBaseAddressOfPlane, CVPixelBufferGetBytesPerRowOfPlane, CVPixelBufferGetHeight,
    CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
};
use player::macos::{GlRenderer, SurfaceFormat};
use std::sync::mpsc;

fn gl_renderer() -> GlRenderer {
    if std::env::var("PLAYER_TEST_GL").is_ok_and(|value| value == "hardware") {
        GlRenderer::Hardware
    } else {
        GlRenderer::Software
    }
}

/// Whether the scenarios are skipped: GL tests are skipped.
pub(crate) fn skipped() -> bool {
    gl::skipped()
}

/// Creates a BGRA surface renderer for `player`, notifying on the returned channel.
pub(crate) fn renderer(player: &Player) -> (SurfaceRenderer, Receiver<()>) {
    renderer_with(player, SurfaceFormat::Bgra)
}

/// Creates a surface renderer of `format` for `player`, notifying on the returned channel.
fn renderer_with(player: &Player, format: SurfaceFormat) -> (SurfaceRenderer, Receiver<()>) {
    let (sender, frames) = mpsc::channel();
    let renderer = SurfaceRenderer::builder(player)
        .format(format)
        .gl_renderer(gl_renderer())
        .on_frame(move || {
            let _ = sender.send(());
        })
        .build()
        .expect("the surface renderer is created");
    (renderer, frames)
}

/// A [`Rig`] whose renderer has `format`.
fn rig_with(format: SurfaceFormat, autoplay: Autoplay) -> Rig {
    let player = render_player(autoplay);
    let (renderer, frames) = renderer_with(&player, format);
    Rig { renderer, frames, player }
}

impl Picture {
    /// Reads a frame, decoding NV12 with the full-range BT.601 matrix GPUI uses.
    pub(crate) fn of(frame: &Frame) -> Picture {
        let buffer = frame.pixel_buffer();
        let (width, height) = (CVPixelBufferGetWidth(buffer), CVPixelBufferGetHeight(buffer));
        assert_eq!(
            (width, height),
            (frame.width() as usize, frame.height() as usize),
            "the buffer has the frame's size"
        );
        let rgb = match frame.format() {
            SurfaceFormat::Bgra => {
                let rows = read_plane(buffer, 0, width * 4, height);
                rows.iter()
                    .flat_map(|row| row.as_chunks::<4>().0.iter().map(|&[b, g, r, _]| [r, g, b]))
                    .collect()
            }
            SurfaceFormat::Nv12 => {
                let luma = read_plane(buffer, 0, width, height);
                let chroma = read_plane(buffer, 1, width, height / 2);
                (0..height)
                    .flat_map(|y| (0..width).map(move |x| (x, y)))
                    .map(|(x, y)| decode(luma[y][x], chroma[y / 2][x / 2 * 2], chroma[y / 2][x / 2 * 2 + 1]))
                    .collect()
            }
            format => panic!("unexpected format {format:?}"),
        };
        Picture { width, height, rgb }
    }

    /// The mean absolute difference per channel, from 0 to 255.
    fn difference(&self, other: &Picture, flip: bool) -> f64 {
        assert_eq!((self.width, self.height), (other.width, other.height));
        let mut total = 0u64;
        for y in 0..self.height {
            let other_y = if flip { self.height - 1 - y } else { y };
            for x in 0..self.width {
                let (a, b) = (self.pixel(x, y), other.pixel(x, other_y));
                total += a.iter().zip(b).map(|(a, b)| u64::from(a.abs_diff(b))).sum::<u64>();
            }
        }
        total as f64 / (self.rgb.len() * 3) as f64
    }
}

/// Reads `rows` rows of `bytes` bytes from a plane, top row first.
fn read_plane(buffer: &CVPixelBuffer, plane: usize, bytes: usize, rows: usize) -> Vec<Vec<u8>> {
    // SAFETY: the buffer is locked for reading around the accesses, which stay within the plane.
    unsafe {
        assert_eq!(CVPixelBufferLockBaseAddress(buffer, CVPixelBufferLockFlags::ReadOnly), 0);
        let base = CVPixelBufferGetBaseAddressOfPlane(buffer, plane).cast::<u8>();
        let stride = CVPixelBufferGetBytesPerRowOfPlane(buffer, plane);
        let data = (0..rows)
            .map(|row| std::slice::from_raw_parts(base.add(row * stride), bytes).to_vec())
            .collect();
        CVPixelBufferUnlockBaseAddress(buffer, CVPixelBufferLockFlags::ReadOnly);
        data
    }
}

/// GPUI's decode: full-range BT.601.
fn decode(y: u8, cb: u8, cr: u8) -> [u8; 3] {
    let (y, cb, cr) = (f64::from(y), f64::from(cb) - 128.0, f64::from(cr) - 128.0);
    [y + 1.402 * cr, y - 0.3441 * cb - 0.7141 * cr, y + 1.772 * cb]
        .map(|channel| channel.round().clamp(0.0, 255.0) as u8)
}

#[test]
fn rejects_a_player_without_render_output() {
    let player = Player::builder().mpv_option("ao", "null").build().unwrap();
    let result = SurfaceRenderer::builder(&player).gl_renderer(gl_renderer()).build();
    let Err(Error::Render(message)) = result else {
        panic!("a surface renderer was created: {result:?}")
    };
    assert!(message.contains("VideoOutput::Render"), "{message}");

    player.set_volume(0.5).unwrap();
    assert_eq!(player.get_property::<f64>("volume").unwrap(), 50.0);
}

#[test]
fn second_renderer_fails_and_the_first_keeps_working() {
    let rig = rig_with(SurfaceFormat::Bgra, Autoplay::Off);
    let second = SurfaceRenderer::builder(&rig.player).gl_renderer(gl_renderer()).build();
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
    let gl = Gl::new();
    let player = render_player(Autoplay::Off);
    // SAFETY: `gl` is current on this thread, where the render context is used and dropped, and outlives it.
    let mut render = unsafe { RenderContext::new(&player, gl.loader(), || {}) }.expect("the render context is created");

    let result = SurfaceRenderer::builder(&player).gl_renderer(gl_renderer()).build();
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
    let (renderer, frames) = renderer_with(&player, SurfaceFormat::Bgra);
    renderer.set_size(160, 120);
    player.load(url(TEST_SOURCE)).unwrap();
    wait_frame(&renderer, &frames, &player, "the surface renderer to show video", shows_video);
}

#[test]
fn latest_frame_from_another_thread() {
    let rig = rig_with(SurfaceFormat::Nv12, Autoplay::On);
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
        });
    });
}

#[test]
fn odd_size_with_nv12_rounds_up() {
    let rig = rig_with(SurfaceFormat::Nv12, Autoplay::Off);
    rig.renderer.set_size(321, 181);
    let frame = rig.wait_frame("a frame", |_| true);
    assert_eq!((frame.width(), frame.height()), (322, 182));
    assert_eq!(Picture::of(&frame).height, 182);
}

#[test]
fn zero_size_stops_publication() {
    let rig = rig_with(SurfaceFormat::Bgra, Autoplay::On);
    let events = rig.player.subscribe();
    rig.renderer.set_size(160, 120);
    rig.player.load(url(TEST_SOURCE)).unwrap();
    rig.wait_frame("a video frame", shows_video);

    rig.renderer.set_size(0, 120);
    // Let a frame drawn before the change land, then nothing more may be published.
    let _ = rig.frames.recv_timeout(Duration::from_millis(200));
    let sequence = rig.renderer.latest().unwrap().sequence();
    let now = rig.player.state().position.unwrap_or_default();
    wait_position_past(&events, now + Duration::from_millis(400));
    assert_eq!(rig.renderer.latest().unwrap().sequence(), sequence, "a frame was published at zero size");

    rig.renderer.set_size(160, 120);
    rig.wait_frame("frames again", |frame| frame.sequence() > sequence);
}

#[test]
fn black_with_nothing_loaded() {
    for format in [SurfaceFormat::Bgra, SurfaceFormat::Nv12] {
        let rig = rig_with(format, Autoplay::Off);
        rig.renderer.set_size(160, 120);
        let picture = Picture::of(&rig.wait_frame("a frame", |_| true));
        let lit = picture.rgb.iter().filter(|pixel| !is_black(**pixel)).count();
        assert_eq!(lit, 0, "{format:?}: the frame is black, first pixel: {:?}", picture.rgb[0]);
    }
}

/// The first frame of `source` from a new player and renderer of `format`, at 160x120.
fn first_frame(source: &str, format: SurfaceFormat) -> Picture {
    let rig = rig_with(format, Autoplay::Off);
    rig.renderer.set_size(160, 120);
    rig.player.load(url(source)).unwrap();
    Picture::of(&rig.wait_frame("the first frame", |frame| Picture::of(frame).colours() >= 4))
}

#[test]
fn nv12_colours_match_bgra() {
    let bgra = first_frame(FIELDS_SOURCE, SurfaceFormat::Bgra);
    let nv12 = first_frame(FIELDS_SOURCE, SurfaceFormat::Nv12);
    for field in 0..5 {
        let (x, y) = (16 + 32 * field, 60);
        let (expected, actual) = (bgra.pixel(x, y), nv12.pixel(x, y));
        let close = expected.iter().zip(actual).all(|(e, a)| f64::from(e.abs_diff(a)) <= 0.03 * 255.0);
        assert!(close, "field {field}: NV12 decodes to {actual:?}, BGRA is {expected:?}");
    }
}

#[test]
fn nv12_orientation_matches_bgra() {
    let bgra = first_frame(TEST_SOURCE, SurfaceFormat::Bgra);
    let nv12 = first_frame(TEST_SOURCE, SurfaceFormat::Nv12);
    assert!(nv12.is_upright_test_pattern(), "the NV12 frame is upright");
    let (same, mirrored) = (nv12.difference(&bgra, false), nv12.difference(&bgra, true));
    assert!(same < 0.03 * 255.0, "the NV12 frame matches the BGRA one, mean difference {same:.1}");
    assert!(
        same * 2.0 < mirrored,
        "the NV12 frame is not mirrored: {same:.1} upright, {mirrored:.1} mirrored"
    );
}

#[test]
fn renderer_created_after_load() {
    let player = render_player(Autoplay::On);
    let events = player.subscribe();
    // A seekable file: mpv re-selects the video track with a seek to the current position, which a generated source
    // cannot do, so its video would resume at the demuxer's read-ahead, more than a second later.
    player.load(fixture("testsrc-h264-aac.mp4")).unwrap();
    wait_loaded(&events);
    wait_position_past(&events, Duration::from_millis(500));
    let before = player.state().position.unwrap();
    let start = Instant::now();

    let (renderer, frames) = renderer_with(&player, SurfaceFormat::Nv12);
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
fn drop_renderer_while_playing() {
    let rig = rig_with(SurfaceFormat::Nv12, Autoplay::On);
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
    let rig = rig_with(SurfaceFormat::Nv12, Autoplay::On);
    rig.renderer.set_size(160, 120);
    rig.player.load(url(TEST_SOURCE)).unwrap();
    rig.wait_frame("a video frame", shows_video);

    let Rig { renderer, player, .. } = rig;
    bounded_drop(player, "the last player");
    assert!(renderer.latest().is_some(), "the renderer still answers");
    bounded_drop(renderer, "the surface renderer after the player");
}

#[test]
fn frame_outlives_its_renderer() {
    let rig = rig_with(SurfaceFormat::Nv12, Autoplay::Off);
    rig.renderer.set_size(160, 120);
    rig.player.load(url(TEST_SOURCE)).unwrap();
    let frame = rig.wait_frame("the first frame", shows_video);

    let Rig { renderer, player, .. } = rig;
    drop(renderer);
    drop(player);
    assert_eq!((frame.width(), frame.height()), (160, 120));
    assert!(Picture::of(&frame).is_upright_test_pattern(), "the frame's pixels can still be read");
}

#[test]
fn software_renderer_switches_to_bilinear_scaling() {
    if gl_renderer() != GlRenderer::Software {
        return;
    }
    let rig = rig_with(SurfaceFormat::Nv12, Autoplay::Off);
    for option in ["scale", "dscale", "cscale"] {
        assert_eq!(rig.player.get_property::<String>(option).unwrap(), "bilinear", "{option}");
    }
    // Scaled up, the picture is drawn rather than left black.
    rig.renderer.set_size(320, 240);
    rig.player.load(url(TEST_SOURCE)).unwrap();
    rig.wait_frame("a scaled video frame", shows_video);
}

/// Plays the H.264 fixture through an NV12 renderer on `gl_renderer` until it shows video, and returns the hardware
/// decoder mpv reports.
fn play_h264(gl_renderer: GlRenderer) -> String {
    let player = render_player(Autoplay::On);
    let (sender, frames) = mpsc::channel();
    let renderer = SurfaceRenderer::builder(&player)
        .format(SurfaceFormat::Nv12)
        .gl_renderer(gl_renderer)
        .on_frame(move || {
            let _ = sender.send(());
        })
        .build()
        .expect("the surface renderer is created");
    renderer.set_size(160, 120);
    player.load(h264_fixture()).unwrap();
    let frame = wait_frame(&renderer, &frames, &player, "the H.264 video", shows_video);
    assert!(Picture::of(&frame).is_upright_test_pattern(), "the decoded video is drawn upright");
    player.get_property::<String>("hwdec-current").unwrap()
}

#[test]
fn software_renderer_publishes_h264_frames() {
    // VideoToolbox may stay active (its interop works with the software renderer on Apple Silicon) or mpv may fall
    // back to software decoding (as under Rosetta); frames are published either way.
    let decoder = play_h264(GlRenderer::Software);
    eprintln!("hwdec-current with the software renderer: {decoder}");
}

#[test]
fn videotoolbox_on_the_gpu() {
    if gl_renderer() != GlRenderer::Hardware {
        eprintln!("skipped: needs a GPU; set PLAYER_TEST_GL=hardware");
        return;
    }
    assert_eq!(play_h264(GlRenderer::Hardware), "videotoolbox");
}

#[test]
fn media_without_audio_loaded_before_the_renderer() {
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

    let (renderer, frames) = renderer_with(&player, SurfaceFormat::Nv12);
    renderer.set_size(160, 120);
    wait_loaded(&events);
    wait_frame(&renderer, &frames, &player, "the video to appear", shows_video);
}

#[test]
fn media_without_audio_loaded_just_before_the_renderer() {
    // Whether the load fails before or after the renderer exists, the video appears.
    for _ in 0..5 {
        let player = render_player(Autoplay::On);
        player.load(h264_fixture()).unwrap();
        let (renderer, frames) = renderer_with(&player, SurfaceFormat::Nv12);
        renderer.set_size(160, 120);
        wait_frame(&renderer, &frames, &player, "the video to appear", shows_video);
    }
}
