//! Helpers shared by the integration tests.
// Each test crate uses only some of them.
#![allow(dead_code)]

use player::prelude::*;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Generated 160x120 video, long enough not to end during a test.
pub const TEST_SOURCE: &str = "av://lavfi:testsrc=duration=30:size=160x120:rate=25";

/// The same video with a sine tone, for the scenarios where playback must go on without video: without audio, mpv
/// ends the media once video is gone.
pub const TEST_SOURCE_WITH_AUDIO: &str =
    "av://lavfi:testsrc=duration=30:size=160x120:rate=25[out0];sine=frequency=440:duration=30[out1]";

/// How long to wait for anything expected before failing.
pub const TIMEOUT: Duration = Duration::from_secs(15);

/// A player with [`VideoOutput::Render`] and null audio.
pub fn render_player(autoplay: Autoplay) -> Player {
    Player::builder()
        .video_output(VideoOutput::Render)
        .autoplay(autoplay)
        .mpv_option("ao", "null")
        .build()
        .expect("the player builds")
}

/// Waits for the first event matching `predicate` and returns it; fails after [`TIMEOUT`].
pub fn wait_for(events: &Events, what: &str, mut predicate: impl FnMut(&Event) -> bool) -> Event {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match events.recv_timeout(left) {
            Some(event) if predicate(&event) => return event,
            Some(_) => {}
            None => panic!("timed out waiting for {what}"),
        }
    }
}

/// Waits until the player's position is past `after`, reading events; fails after [`TIMEOUT`].
pub fn wait_position_past(events: &Events, after: Duration) {
    let what = format!("the position to pass {after:?}");
    wait_for(
        events,
        &what,
        |event| matches!(event, Event::PositionChanged(Some(position)) if *position > after),
    );
}

/// Runs `drop` and checks that it returns within one second.
pub fn bounded_drop<T>(value: T, what: &str) {
    let start = Instant::now();
    drop(value);
    let elapsed = start.elapsed();
    assert!(elapsed < Duration::from_secs(1), "dropping {what} took {elapsed:?}");
}

/// Writes a mono, 16-bit, 8 kHz PCM WAV of `seconds` seconds of a 440 Hz tone to `dir/name` and returns its path.
///
/// The 44-byte header is written by hand, so no audio crate is needed. Unlike mpv's generated `lavfi` sources, the
/// file has a known duration and seeks backwards.
pub fn write_wav(dir: &Path, name: &str, seconds: u32) -> PathBuf {
    const RATE: u32 = 8000;
    let samples = RATE * seconds;
    let data_len = samples * 2;

    let mut wav = Vec::with_capacity(44 + data_len as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // channels
    wav.extend_from_slice(&RATE.to_le_bytes());
    wav.extend_from_slice(&(RATE * 2).to_le_bytes()); // byte rate
    wav.extend_from_slice(&2u16.to_le_bytes()); // block align
    wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for n in 0..samples {
        let phase = 2.0 * std::f64::consts::PI * 440.0 * f64::from(n) / f64::from(RATE);
        wav.extend_from_slice(&((phase.sin() * 8000.0) as i16).to_le_bytes());
    }

    let path = dir.join(name);
    std::fs::write(&path, wav).expect("the WAV file is written");
    path
}
