//! Playing local files through `Source::file`, using WAV files written into a temporary directory.

mod common;

use player::prelude::*;
use rust_sak::fs::mk_temp_dir;
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(15);

fn player() -> Player {
    Player::builder()
        .autoplay(Autoplay::On)
        .mpv_option("ao", "null")
        .build()
        .expect("the player builds")
}

/// Waits for the media to end and returns why.
fn wait_for_end(events: &Events) -> EndReason {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        match events.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Some(Event::Ended(reason)) => return reason,
            Some(_) => {}
            None => panic!("timed out waiting for the media to end"),
        }
    }
}

#[test]
fn plays_a_local_file() {
    let dir = mk_temp_dir("player-rs-").unwrap();
    let path = common::write_wav(dir.path(), "tone.wav", 1);

    let player = player();
    let events = player.subscribe();
    player.load(Source::file(&path).unwrap()).unwrap();

    assert_eq!(wait_for_end(&events), EndReason::Eof);
    let duration = player.state().duration.expect("the duration is known");
    assert!(duration.abs_diff(Duration::from_secs(1)) < Duration::from_millis(50), "{duration:?}");
}

/// A relative path that looks like a URL (`http://clip.mp4`: the file `clip.mp4` in a directory named `http:`) is
/// played as a local file. Windows does not allow `:` in file names, so this only exists on Unix.
#[cfg(unix)]
#[test]
fn plays_a_file_whose_path_looks_like_a_url() {
    let dir = mk_temp_dir("player-rs-").unwrap();
    std::fs::create_dir(dir.path().join("http:")).unwrap();
    common::write_wav(&dir.path().join("http:"), "clip.mp4", 1);

    // Only this test depends on the working directory; the other test uses an absolute path.
    std::env::set_current_dir(dir.path()).unwrap();
    let source = Source::file("http://clip.mp4").unwrap();

    let player = player();
    let events = player.subscribe();
    player.load(source).unwrap();
    assert_eq!(wait_for_end(&events), EndReason::Eof);
}
