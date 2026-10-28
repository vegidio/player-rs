//! Playback behaviour against media mpv generates itself (`av://lavfi:testsrc`), with null audio and no video output,
//! so no media files, display or sound device are needed. The generated source has no duration until it has been read
//! and cannot seek backwards, so those scenarios use a WAV file written into a temporary directory instead. Waiting is
//! always done on events with a deadline, never with sleeps, so slow CI machines cannot cause false failures.

mod common;

use player::prelude::*;
use rust_sak::fs::{TempDir, mk_temp_dir};
use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, Thread};
use std::time::{Duration, Instant};

/// Two seconds of generated video.
const TEST_SOURCE: &str = "av://lavfi:testsrc=duration=2:size=160x120:rate=25";

/// How long to wait for any expected event before failing.
const TIMEOUT: Duration = Duration::from_secs(15);

fn player(autoplay: Autoplay) -> Player {
    Player::builder()
        .autoplay(autoplay)
        .mpv_option("ao", "null")
        .build()
        .expect("the player builds")
}

fn source() -> Source {
    Source::url(TEST_SOURCE).unwrap()
}

/// A two-second WAV file, deleted when the returned directory is dropped.
fn wav_source() -> (TempDir, Source) {
    let dir = mk_temp_dir("player-rs-").unwrap();
    let path = common::write_wav(dir.path(), "tone.wav", 2);
    (dir, Source::file(path).unwrap())
}

/// Waits for the first event matching `predicate` and returns it.
fn wait_for(events: &Events, what: &str, mut predicate: impl FnMut(&Event) -> bool) -> Event {
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

/// Waits until the player's state satisfies `predicate`, checking after every event.
fn wait_state(player: &Player, events: &Events, what: &str, predicate: impl Fn(&State) -> bool) -> State {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let state = player.state();
        if predicate(&state) {
            return state;
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if events.recv_timeout(left).is_none() {
            panic!("timed out waiting for {what}; last state: {:?}", player.state());
        }
    }
}

/// Loads the generated test source and waits until it has loaded.
fn load(player: &Player, events: &Events) {
    load_source(player, events, source());
}

/// Loads `source` and waits until it has loaded.
fn load_source(player: &Player, events: &Events, source: Source) {
    player.load(source.clone()).unwrap();
    match wait_for(events, "the source to load", |e| matches!(e, Event::Loaded | Event::Ended(_))) {
        Event::Loaded => {}
        Event::Ended(EndReason::Error(message)) => {
            panic!("mpv cannot play {source} ({message}); is libavfilter/libavdevice missing from the archive?")
        }
        other => panic!("unexpected {other:?} while loading"),
    }
}

/// Waits until the position is past `after`.
fn wait_position_past(events: &Events, after: Duration) -> Duration {
    let event = wait_for(
        events,
        &format!("the position to pass {after:?}"),
        |e| matches!(e, Event::PositionChanged(Some(position)) if *position > after),
    );
    let Event::PositionChanged(Some(position)) = event else { unreachable!() };
    position
}

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn load_with_autoplay_off() {
    let player = player(Autoplay::Off);
    let events = player.subscribe();
    let (_dir, wav) = wav_source();
    load_source(&player, &events, wav);

    let state = player.state();
    let duration = state.duration.expect("the duration is known once loaded");
    assert!(duration.abs_diff(Duration::from_secs(2)) < Duration::from_millis(100), "{duration:?}");
    assert!(state.paused);
    assert!(!state.idle);
    assert!(state.position.unwrap_or_default() < Duration::from_millis(100), "{state:?}");
    assert_eq!(state.video_size, None, "audio-only media has no video size");
}

#[test]
fn autoplay_on() {
    let player = player(Autoplay::On);
    let events = player.subscribe();
    load(&player, &events);

    wait_position_past(&events, Duration::from_millis(200));
    let state = player.state();
    assert!(!state.paused);
    assert_eq!(state.video_size, None, "no video is shown with VideoOutput::Null");
}

#[test]
fn autoplay_muted() {
    let player = player(Autoplay::Muted);
    let events = player.subscribe();
    load(&player, &events);

    assert!(player.state().muted);
    wait_position_past(&events, Duration::from_millis(200));
    assert!(player.state().muted);
}

#[test]
fn unplayable_source() {
    let player = player(Autoplay::On);
    let events = player.subscribe();
    player.load(Source::url("av://lavfi:no-such-filter").unwrap()).unwrap();

    let event = wait_for(&events, "the load to fail", |e| matches!(e, Event::Ended(_)));
    let Event::Ended(EndReason::Error(message)) = event else {
        panic!("expected an error, got {event:?}")
    };
    assert!(!message.is_empty());
    wait_state(&player, &events, "the player to become idle", |s| s.idle);
}

#[test]
fn load_replaces_current_media() {
    let player = player(Autoplay::On);
    let events = player.subscribe();
    load(&player, &events);

    player.load(source()).unwrap();
    wait_for(&events, "the first media to be replaced", |e| *e == Event::Ended(EndReason::Replaced));
    wait_for(&events, "the second media to load", |e| *e == Event::Loaded);
}

#[test]
fn pause_and_resume() {
    let player = player(Autoplay::On);
    let events = player.subscribe();
    load(&player, &events);
    wait_position_past(&events, Duration::from_millis(100));

    player.pause().unwrap();
    wait_state(&player, &events, "the player to pause", |s| s.paused);
    let paused_at = player.state().position.unwrap();

    // Nothing advances while paused: watch the events for a while.
    while let Some(event) = events.recv_timeout(Duration::from_millis(400)) {
        if let Event::PositionChanged(Some(position)) = event {
            assert!(position <= paused_at, "advanced to {position:?} while paused at {paused_at:?}");
        }
    }
    assert_eq!(player.state().position, Some(paused_at));

    player.play().unwrap();
    wait_position_past(&events, paused_at);
}

#[test]
fn toggle_pause() {
    let player = player(Autoplay::Off);
    let events = player.subscribe();
    load(&player, &events);

    player.toggle_pause().unwrap();
    wait_state(&player, &events, "playback to start", |s| !s.paused);
    player.toggle_pause().unwrap();
    wait_state(&player, &events, "playback to pause", |s| s.paused);
}

#[test]
fn absolute_seek() {
    let player = player(Autoplay::Off);
    let events = player.subscribe();
    load(&player, &events);

    player.seek(Seek::To(Duration::from_secs(1))).unwrap();
    wait_for(&events, "the seek to complete", |e| *e == Event::Seeked);
    let position = player.state().position.expect("the position is known after a seek");
    assert!(position.abs_diff(Duration::from_secs(1)) <= Duration::from_millis(100), "{position:?}");
}

#[test]
fn relative_seek() {
    let player = player(Autoplay::Off);
    let events = player.subscribe();
    let (_dir, wav) = wav_source();
    load_source(&player, &events, wav);

    player.seek(Seek::To(Duration::from_secs(1))).unwrap();
    wait_for(&events, "the first seek to complete", |e| *e == Event::Seeked);
    player.seek(Seek::By(-0.5)).unwrap();
    wait_for(&events, "the second seek to complete", |e| *e == Event::Seeked);
    let position = player.state().position.unwrap();
    assert!(position.abs_diff(Duration::from_millis(500)) <= Duration::from_millis(100), "{position:?}");
}

#[test]
fn relative_seek_past_the_end() {
    let player = player(Autoplay::On);
    let events = player.subscribe();
    load(&player, &events);

    player.seek(Seek::By(60.0)).unwrap();
    wait_for(&events, "the end of the media", |e| *e == Event::Ended(EndReason::Eof));
    assert!(!player.state().idle);
}

#[test]
fn stop() {
    let player = player(Autoplay::On);
    let events = player.subscribe();
    load(&player, &events);

    player.stop().unwrap();
    wait_for(&events, "the media to stop", |e| *e == Event::Ended(EndReason::Stopped));
    let state = wait_state(&player, &events, "the player to become idle", |s| s.idle);
    assert_eq!((state.position, state.duration), (None, None));
}

#[test]
fn control_with_no_media() {
    let player = player(Autoplay::Off);
    player.play().unwrap();
    player.pause().unwrap();
    player.seek(Seek::To(Duration::from_secs(1))).unwrap();
    player.seek(Seek::By(1.0)).unwrap();

    let state = player.state();
    assert!(state.idle);
    assert_eq!((state.position, state.duration), (None, None));
}

#[test]
fn play_to_the_end_then_play_again() {
    let player = player(Autoplay::On);
    let events = player.subscribe();
    load(&player, &events);

    // Media plays until its end, then stays loaded and paused there, as in Video.js.
    wait_for(&events, "the end of the media", |e| *e == Event::Ended(EndReason::Eof));
    let state = wait_state(&player, &events, "the player to pause at the end", |s| s.paused);
    assert!(state.ended);
    assert!(!state.idle);
    assert!(state.duration.is_some());

    // Playing again restarts from the beginning.
    player.play().unwrap();
    let state = wait_state(&player, &events, "playback to restart", |s| {
        !s.paused && !s.ended && s.position.is_some_and(|p| p < Duration::from_secs(1))
    });
    let restarted_at = state.position.unwrap();
    wait_position_past(&events, restarted_at);
}

#[test]
fn ended_resets_on_next_load_without_keep_open() {
    let player = Player::builder()
        .autoplay(Autoplay::On)
        .mpv_option("ao", "null")
        .mpv_option("keep-open", "no")
        .build()
        .unwrap();
    let events = player.subscribe();
    load(&player, &events);

    wait_for(&events, "the end of the media", |e| *e == Event::Ended(EndReason::Eof));
    assert!(player.state().ended);

    load(&player, &events);
    assert!(!player.state().ended);
}

#[test]
fn state_follows_events() {
    let player = player(Autoplay::On);
    let events = player.subscribe();
    load(&player, &events);

    for _ in 0..10 {
        let event = wait_for(&events, "a position change", |e| matches!(e, Event::PositionChanged(Some(_))));
        let Event::PositionChanged(Some(position)) = event else { unreachable!() };
        let snapshot = player.state().position;
        assert!(
            snapshot.is_some_and(|p| p >= position),
            "snapshot {snapshot:?} is older than event {position:?}"
        );
    }
}

#[test]
fn two_subscribers() {
    let player = player(Autoplay::Off);
    let (first, second) = (player.subscribe(), player.subscribe());
    load(&player, &first);
    wait_for(&second, "the loaded event on the second subscriber", |e| *e == Event::Loaded);
}

#[test]
fn late_subscriber() {
    let player = player(Autoplay::On);
    let events = player.subscribe();
    load(&player, &events);

    let late = player.subscribe();
    let event = wait_for(&late, "an event on the late subscriber", |e| {
        assert_ne!(*e, Event::Loaded, "the late subscriber received the earlier loaded event");
        matches!(e, Event::PositionChanged(_))
    });
    assert!(matches!(event, Event::PositionChanged(_)));
}

#[test]
fn dropped_subscriber() {
    let player = player(Autoplay::On);
    let (dropped, kept) = (player.subscribe(), player.subscribe());
    drop(dropped);

    load(&player, &kept);
    wait_position_past(&kept, Duration::from_millis(200));
}

#[test]
fn async_subscriber() {
    let player = player(Autoplay::Off);
    let events = player.subscribe();
    player.load(source()).unwrap();

    let (event, polls) = block_on(events.recv_async());
    assert!(event.is_some());
    // Polled once before the event and once after it woke the task; a busy loop would poll thousands of times. The
    // margin covers spurious unparks.
    assert!(polls <= 5, "polled {polls} times");
}

#[test]
fn raw_property_and_command_access() {
    let player = player(Autoplay::Off);
    let events = player.subscribe();

    player.set_rate(1.5).unwrap();
    assert_eq!(player.get_property::<f64>("speed").unwrap(), 1.5);
    assert_eq!(player.get_property::<i64>("playlist-count").unwrap(), 0);
    assert!(player.get_property::<String>("mpv-version").unwrap().starts_with("mpv"));
    player.set_property("title", String::from("player-rs")).unwrap();
    assert_eq!(player.get_property::<String>("title").unwrap(), "player-rs");

    let Err(Error::Property { name, message }) = player.get_property::<f64>("no-such-property") else {
        panic!("an unknown property was read")
    };
    assert_eq!(name, "no-such-property");
    assert!(!message.is_empty());

    player.command(&["set", "volume", "50"]).unwrap();
    wait_state(&player, &events, "the volume to be 0.5", |s| approx(s.volume, 0.5));

    let Err(Error::Command { name, .. }) = player.command(&["no-such-command"]) else {
        panic!("an unknown command succeeded")
    };
    assert_eq!(name, "no-such-command");
}

#[test]
fn copies_control_the_same_instance() {
    let player = player(Autoplay::On);
    let copy = player.clone();
    let events = player.subscribe();
    load(&player, &events);

    copy.pause().unwrap();
    wait_state(&player, &events, "the player to pause", |s| s.paused);
    assert!(copy.state().paused);
}

#[test]
fn control_from_another_thread() {
    let player = player(Autoplay::Off);
    let events = player.subscribe();

    let copy = player.clone();
    thread::spawn(move || copy.set_volume(0.3).unwrap()).join().unwrap();

    wait_state(&player, &events, "the volume to be 0.3", |s| approx(s.volume, 0.3));
    assert!(approx(player.get_property::<f64>("volume").unwrap(), 30.0));
}

#[test]
fn drop_while_playing() {
    let player = player(Autoplay::On);
    let copy = player.clone();
    let events = player.subscribe();
    load(&player, &events);
    wait_position_past(&events, Duration::from_millis(100));

    let started = Instant::now();
    drop(copy);
    drop(player);
    let elapsed = started.elapsed();
    assert!(elapsed < Duration::from_secs(1), "dropping took {elapsed:?}");

    // The event stream ends once the queue is drained.
    while events.recv_timeout(TIMEOUT).is_some() {}
}

/// Runs `future` on this thread, parking it until the future's waker is called, and counts the polls.
fn block_on<F: Future>(future: F) -> (F::Output, usize) {
    struct Unpark(Thread);

    impl Wake for Unpark {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }

    let polls = AtomicUsize::new(0);
    let waker = Waker::from(Arc::new(Unpark(thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = pin!(future);
    let deadline = Instant::now() + TIMEOUT;
    loop {
        polls.fetch_add(1, Ordering::Relaxed);
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return (output, polls.into_inner());
        }
        assert!(Instant::now() < deadline, "timed out waiting for the future");
        thread::park_timeout(deadline.saturating_duration_since(Instant::now()));
    }
}
