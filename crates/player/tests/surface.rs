//! Rendering through each platform's surface renderer, against media mpv generates itself (`av://lavfi:...`), with
//! null audio. Frames are read back on the CPU and checked for properties of the picture (many colours, black bars,
//! where the test pattern's gradient is) rather than exact images, so different renderers cannot make them flaky.
//! Waiting is always done on channels with a deadline, never with sleeps.
#![cfg(any(target_os = "macos", windows, target_os = "linux"))]

// An outer `#[path]`: with `surface/` next to this file, rustfmt and rustc disagree on where an inline module's
// files are.
mod common;
#[path = "common/gl.rs"]
mod gl;

#[cfg(target_os = "linux")]
#[path = "surface/linux.rs"]
mod linux;
#[cfg(target_os = "macos")]
#[path = "surface/macos.rs"]
mod macos;
#[cfg(windows)]
#[path = "surface/windows.rs"]
mod windows;

#[cfg(target_os = "linux")]
use self::linux as platform;
#[cfg(target_os = "macos")]
use self::macos as platform;
#[cfg(windows)]
use self::windows as platform;
#[cfg(target_os = "linux")]
use player::linux::{Frame, SurfaceRenderer};
#[cfg(target_os = "macos")]
use player::macos::{Frame, SurfaceRenderer};
#[cfg(windows)]
use player::windows::{Frame, SurfaceRenderer};

use common::{TEST_SOURCE, TEST_SOURCE_WITH_AUDIO, TIMEOUT, bounded_drop, render_player, wait_position_past};
use player::prelude::*;
use std::collections::HashSet;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

/// Five 32x120 solid fields side by side: red, green, blue, white and grey.
const FIELDS_SOURCE: &str = "av://lavfi:color=c=0xFF0000:s=32x120:r=25:d=30[r];color=c=0x00FF00:s=32x120:r=25:d=30[g];\
    color=c=0x0000FF:s=32x120:r=25:d=30[b];color=c=0xFFFFFF:s=32x120:r=25:d=30[w];\
    color=c=0x808080:s=32x120:r=25:d=30[k];[r][g][b][w][k]hstack=inputs=5[out0]";

fn url(url: &str) -> Source {
    Source::url(url).unwrap()
}

fn wait_loaded(events: &Events) {
    common::wait_for(events, "the media to load", |event| match event {
        Event::Ended(reason) => panic!("the media ended instead of loading: {reason:?}"),
        event => *event == Event::Loaded,
    });
}

fn is_black([r, g, b]: [u8; 3]) -> bool {
    r < 16 && g < 16 && b < 16
}

/// A committed fixture (see `fixtures/README.md`).
fn fixture(name: &str) -> Source {
    Source::file(format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// The H.264 fixture: mpv's generated sources never reach a hardware decoder.
fn h264_fixture() -> Source {
    fixture("testsrc-h264.mp4")
}

/// A render player and its surface renderer, declared so the renderer is dropped first.
struct Rig {
    renderer: SurfaceRenderer,
    frames: Receiver<()>,
    player: Player,
}

fn rig(autoplay: Autoplay) -> Rig {
    let player = render_player(autoplay);
    let (renderer, frames) = platform::renderer(&player);
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

/// A frame read back as RGB, top row first. Each platform reads it in its own `Picture::of`.
struct Picture {
    width: usize,
    height: usize,
    rgb: Vec<[u8; 3]>,
}

impl Picture {
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

// ------------------------------------------------------------------------------------------------------------------
// Every platform

#[test]
fn recreated_after_drop_publishes_frames() {
    if platform::skipped() {
        return;
    }
    let player = render_player(Autoplay::Off);
    let (first, _frames) = platform::renderer(&player);
    drop(first);
    let (second, frames) = platform::renderer(&player);
    second.set_size(160, 120);
    player.load(url(TEST_SOURCE)).unwrap();
    wait_frame(&second, &frames, &player, "the new renderer to show video", shows_video);
}

#[test]
fn no_frame_without_a_size() {
    if platform::skipped() {
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
    if platform::skipped() {
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
fn resize_while_paused() {
    if platform::skipped() {
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
fn test_pattern_is_upright() {
    if platform::skipped() {
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
fn letterboxing() {
    if platform::skipped() {
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

#[test]
fn no_notification_after_drop() {
    if platform::skipped() {
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
// Linux and Windows. macOS has its own versions of these: it picks its OpenGL renderer, and runs several of them on
// NV12 surfaces.

#[cfg(not(target_os = "macos"))]
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

#[cfg(not(target_os = "macos"))]
#[test]
fn second_renderer_fails_and_the_first_keeps_working() {
    if platform::skipped() {
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

#[cfg(not(target_os = "macos"))]
#[test]
fn zero_size_stops_publication() {
    if platform::skipped() {
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

#[cfg(not(target_os = "macos"))]
#[test]
fn black_with_nothing_loaded() {
    if platform::skipped() {
        return;
    }
    let rig = rig(Autoplay::Off);
    rig.renderer.set_size(160, 120);
    let picture = Picture::of(&rig.wait_frame("a frame", |_| true));
    let lit = picture.rgb.iter().filter(|pixel| !is_black(**pixel)).count();
    assert_eq!(lit, 0, "the frame is black, first pixel: {:?}", picture.rgb[0]);
}

#[cfg(not(target_os = "macos"))]
#[test]
fn solid_colours() {
    if platform::skipped() {
        return;
    }
    let rig = rig(Autoplay::Off);
    rig.renderer.set_size(160, 120);
    rig.player.load(url(FIELDS_SOURCE)).unwrap();
    let picture = Picture::of(&rig.wait_frame("the first frame", |frame| Picture::of(frame).colours() >= 4));
    let fields: [[u8; 3]; 5] = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 255], [128, 128, 128]];
    for (field, expected) in fields.into_iter().enumerate() {
        let actual = picture.pixel(16 + 32 * field, 60);
        let close = expected.iter().zip(actual).all(|(e, a)| f64::from(e.abs_diff(a)) <= 0.03 * 255.0);
        assert!(close, "field {field}: {actual:?}, expected {expected:?}");
    }
}

#[cfg(not(target_os = "macos"))]
#[test]
fn renderer_created_after_load() {
    if platform::skipped() {
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

    let (renderer, frames) = platform::renderer(&player);
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

#[cfg(not(target_os = "macos"))]
#[test]
fn media_without_audio_loaded_before_the_renderer() {
    if platform::skipped() {
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

    let (renderer, frames) = platform::renderer(&player);
    renderer.set_size(160, 120);
    wait_loaded(&events);
    wait_frame(&renderer, &frames, &player, "the video to appear", shows_video);
}

#[cfg(not(target_os = "macos"))]
#[test]
fn media_without_audio_loaded_just_before_the_renderer() {
    if platform::skipped() {
        return;
    }
    // Whether the load fails before or after the renderer exists, the video appears.
    for _ in 0..5 {
        let player = render_player(Autoplay::On);
        player.load(h264_fixture()).unwrap();
        let (renderer, frames) = platform::renderer(&player);
        renderer.set_size(160, 120);
        wait_frame(&renderer, &frames, &player, "the video to appear", shows_video);
    }
}

#[cfg(not(target_os = "macos"))]
#[test]
fn drop_renderer_while_playing() {
    if platform::skipped() {
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

#[cfg(not(target_os = "macos"))]
#[test]
fn drop_player_before_renderer() {
    if platform::skipped() {
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
