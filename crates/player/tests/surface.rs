//! Rendering through each platform's surface renderer, against media mpv generates itself (`av://lavfi:...`), with
//! null audio. Frames are read back on the CPU and checked for properties of the picture (many colours, black bars,
//! where the test pattern's gradient is) rather than exact images, so different renderers cannot make them flaky.
//! Waiting is always done on channels with a deadline, never with sleeps.
#![cfg(any(target_os = "macos", windows, target_os = "linux"))]

// An outer `#[path]`: with `surface/` next to this file, rustfmt and rustc disagree on where an inline module's
// files are.
#[path = "common/gl.rs"]
mod gl;
mod common {
    pub(crate) use super::gl;
}

#[cfg(target_os = "linux")]
#[path = "surface/linux.rs"]
mod linux;
#[cfg(target_os = "macos")]
#[path = "surface/macos.rs"]
mod macos;
#[cfg(windows)]
#[path = "surface/windows.rs"]
mod windows;

use player::prelude::*;
use std::time::{Duration, Instant};

/// Generated 160x120 video, long enough not to end during a test.
const TEST_SOURCE: &str = "av://lavfi:testsrc=duration=30:size=160x120:rate=25";

/// The same video with a sine tone, for the scenarios where playback must go on without video: without audio, mpv
/// ends the media once video is gone.
const TEST_SOURCE_WITH_AUDIO: &str =
    "av://lavfi:testsrc=duration=30:size=160x120:rate=25[out0];sine=frequency=440:duration=30[out1]";

/// Five 32x120 solid fields side by side: red, green, blue, white and grey.
const FIELDS_SOURCE: &str = "av://lavfi:color=c=0xFF0000:s=32x120:r=25:d=30[r];color=c=0x00FF00:s=32x120:r=25:d=30[g];\
    color=c=0x0000FF:s=32x120:r=25:d=30[b];color=c=0xFFFFFF:s=32x120:r=25:d=30[w];\
    color=c=0x808080:s=32x120:r=25:d=30[k];[r][g][b][w][k]hstack=inputs=5[out0]";

/// How long to wait for anything expected before failing.
const TIMEOUT: Duration = Duration::from_secs(15);

fn render_player(autoplay: Autoplay) -> Player {
    let builder = Player::builder().video_output(VideoOutput::Render).autoplay(autoplay).mpv_option("ao", "null");
    #[cfg(target_os = "linux")]
    let builder = linux::software_scaling(builder);
    builder.build().expect("the player builds")
}

fn url(url: &str) -> Source {
    Source::url(url).unwrap()
}

/// Waits until the player's position is past `after`, reading events; fails after [`TIMEOUT`].
fn wait_position_past(events: &Events, after: Duration) {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match events.recv_timeout(left) {
            Some(Event::PositionChanged(Some(position))) if position > after => return,
            Some(_) => {}
            None => panic!("timed out waiting for the position to pass {after:?}"),
        }
    }
}

fn wait_loaded(events: &Events) {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match events.recv_timeout(left) {
            Some(Event::Loaded) => return,
            Some(Event::Ended(reason)) => panic!("the media ended instead of loading: {reason:?}"),
            Some(_) => {}
            None => panic!("timed out waiting for the media to load"),
        }
    }
}

/// Runs `drop` and checks that it returns within one second.
fn bounded_drop<T>(value: T, what: &str) {
    let start = Instant::now();
    drop(value);
    let elapsed = start.elapsed();
    assert!(elapsed < Duration::from_secs(1), "dropping {what} took {elapsed:?}");
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
