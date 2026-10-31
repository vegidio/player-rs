//! The UI layer driving a real headless player (null audio, no video output), as a frontend would: every event from
//! the frontend's subscription is forwarded to the controller. Media is a WAV file written into a temporary
//! directory, for a known duration and backward seeks, or mpv's generated `lavfi` source where no duration is
//! wanted. Time-based behaviour (inactivity, the spinner delay) is checked by passing `Instant`s offset by the test,
//! so nothing sleeps; waiting on the player is always done on events with a deadline.

mod common;

use common::TIMEOUT;
use player::prelude::*;
use player::ui::{self, Action, Controls, Focus, Hotkeys, Key, Modifiers, PlayIcon, Request, View};
use rust_sak::fs::{TempDir, mk_temp_dir};
use std::time::{Duration, Instant};

/// A player, a subscription and a controller fed from it.
struct Harness {
    player: Player,
    events: Events,
    controls: Controls,
    _dir: Option<TempDir>,
}

impl Harness {
    fn new(autoplay: Autoplay, options: ui::Options) -> Self {
        let player = Player::builder()
            .video_output(VideoOutput::Null)
            .autoplay(autoplay)
            .mpv_option("ao", "null")
            .build()
            .expect("the player builds");
        let events = player.subscribe();
        let controls = Controls::new(&player, options, Instant::now());
        Self { player, events, controls, _dir: None }
    }

    /// Loads a WAV file of `seconds` seconds and waits until it has loaded.
    fn load_wav(&mut self, seconds: u32) {
        let dir = mk_temp_dir("player-rs-").unwrap();
        let path = common::write_wav(dir.path(), "tone.wav", seconds);
        self._dir = Some(dir);
        self.load(Source::file(path).unwrap());
    }

    fn load(&mut self, source: Source) {
        self.player.load(source).unwrap();
        self.wait_event("the source to load", |event, _| *event == Event::Loaded);
    }

    /// Forwards events to the controller until `predicate` holds for the player's state and the view.
    fn wait(&mut self, what: &str, predicate: impl Fn(&State, &View) -> bool) -> View {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let view = self.controls.view(Instant::now());
            if predicate(&self.player.state(), &view) {
                return view;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            match self.events.recv_timeout(left) {
                Some(event) => self.controls.handle_event(&event, Instant::now()),
                None => panic!("timed out waiting for {what}; last state: {:?}", self.player.state()),
            }
        }
    }

    /// Forwards events until `predicate` holds for one of them, and returns it.
    fn wait_event(&mut self, what: &str, predicate: impl Fn(&Event, &State) -> bool) -> Event {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let Some(event) = self.events.recv_timeout(left) else {
                panic!("timed out waiting for {what}")
            };
            self.controls.handle_event(&event, Instant::now());
            if predicate(&event, &self.player.state()) {
                return event;
            }
        }
    }

    /// Forwards events until `wanted` arrives, and returns it without forwarding it, so the test can give its time.
    fn next(&mut self, wanted: Event) -> Event {
        let what = format!("{wanted:?}");
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let Some(event) = self.events.recv_timeout(left) else {
                panic!("timed out waiting for {what}")
            };
            if event == wanted {
                return event;
            }
            self.controls.handle_event(&event, Instant::now());
        }
    }

    /// Forwards every event already queued.
    fn drain(&mut self) {
        while let Some(event) = self.events.recv_timeout(Duration::from_millis(100)) {
            self.controls.handle_event(&event, Instant::now());
        }
    }

    fn key(&mut self, key: Key, focus: Focus) -> ui::KeyOutcome {
        self.controls.key(key, Modifiers::NONE, focus, Instant::now()).unwrap()
    }

    fn perform(&mut self, action: Action) -> Option<Request> {
        self.controls.perform(action, Instant::now()).unwrap()
    }

    fn wait_seeked_near(&mut self, seconds: f64, tolerance: f64) {
        self.wait_event(&format!("a seek to {seconds} s"), |event, state| {
            *event == Event::Seeked && state.position.is_some_and(|p| near(p, seconds, tolerance))
        });
    }
}

fn options() -> ui::Options {
    ui::Options::default()
}

fn near(position: Duration, seconds: f64, tolerance: f64) -> bool {
    (position.as_secs_f64() - seconds).abs() <= tolerance
}

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

fn secs(start: Instant, seconds: f64) -> Instant {
    start + Duration::from_secs_f64(seconds)
}

#[test]
fn play_pause_hotkey() {
    let mut h = Harness::new(Autoplay::On, options());
    h.load_wav(10);
    h.wait("playback", |s, _| !s.paused);

    let outcome = h.key(Key::Char('k'), Focus::Player);
    assert_eq!(outcome, ui::KeyOutcome { handled: true, request: None });
    h.wait("the player to pause", |s, _| s.paused);

    assert!(h.key(Key::Space, Focus::Player).handled);
    h.wait("playback to resume", |s, _| !s.paused);
}

#[test]
fn fullscreen_and_mute_hotkeys() {
    let mut h = Harness::new(Autoplay::Off, options());
    h.load_wav(2);
    // The playback fields only: the buffered position keeps changing on its own as the cache fills.
    let playback = |s: State| (s.idle, s.position, s.paused, s.volume, s.muted, s.rate, s.ended);
    let before = playback(h.player.state());

    let outcome = h.key(Key::Char('f'), Focus::Player);
    assert_eq!(outcome, ui::KeyOutcome { handled: true, request: Some(Request::ToggleFullscreen) });
    h.drain();
    assert_eq!(playback(h.player.state()), before, "fullscreen is the frontend's");

    assert!(h.key(Key::Char('m'), Focus::Player).handled);
    h.wait("the player to mute", |s, _| s.muted);
    assert!(h.key(Key::Char('M'), Focus::Player).handled);
    h.wait("the player to unmute", |s, _| !s.muted);
}

#[test]
fn modified_keys_are_unhandled() {
    let mut h = Harness::new(Autoplay::Off, options());
    h.load_wav(2);
    for modifiers in [Modifiers { ctrl: true, ..Modifiers::NONE }, Modifiers { platform: true, ..Modifiers::NONE }] {
        let outcome = h.controls.key(Key::Char('m'), modifiers, Focus::Player, Instant::now()).unwrap();
        assert_eq!(outcome, ui::KeyOutcome { handled: false, request: None }, "{modifiers:?}");
        let outcome = h.controls.key(Key::Char('f'), modifiers, Focus::Player, Instant::now()).unwrap();
        assert!(!outcome.handled);
    }
    h.drain();
    assert!(!h.player.state().muted);
}

#[test]
fn disabled_and_remapped_hotkeys() {
    let disabled = ui::Options::builder().hotkeys(Hotkeys::disabled()).build().unwrap();
    let mut h = Harness::new(Autoplay::Off, disabled);
    h.load_wav(2);
    assert!(!h.key(Key::Char('m'), Focus::Player).handled);
    h.drain();
    assert!(!h.player.state().muted);

    let remapped = ui::Options::builder().hotkeys(Hotkeys::default().mute([Key::Char('x')])).build().unwrap();
    let mut h = Harness::new(Autoplay::Off, remapped);
    h.load_wav(2);
    assert!(!h.key(Key::Char('m'), Focus::Player).handled);
    assert!(h.key(Key::Char('x'), Focus::Player).handled);
    h.wait("the player to mute", |s, _| s.muted);
}

#[test]
fn seek_bar_keys() {
    let mut h = Harness::new(Autoplay::Off, options());
    h.load_wav(10);

    // Digits go to tenths of the duration.
    assert!(h.key(Key::Char('3'), Focus::SeekBar).handled);
    h.wait_seeked_near(3.0, 0.1);
    assert!(h.key(Key::Right, Focus::SeekBar).handled);
    h.wait_seeked_near(8.0, 0.1);
    assert!(h.key(Key::Down, Focus::SeekBar).handled);
    h.wait_seeked_near(3.0, 0.1);
    assert!(h.key(Key::Home, Focus::SeekBar).handled);
    h.wait_seeked_near(0.0, 0.1);

    // Without slider focus, arrows are not hotkeys.
    let outcome = h.key(Key::Right, Focus::Player);
    assert!(!outcome.handled);
    h.drain();
    assert!(near(h.player.state().position.unwrap(), 0.0, 0.1));

    // Space and Enter toggle play on the seek bar.
    assert!(h.key(Key::Enter, Focus::SeekBar).handled);
    h.wait("playback", |s, _| !s.paused);
}

#[test]
fn volume_bar_keys() {
    let mut h = Harness::new(Autoplay::Off, options());
    h.perform(Action::SetVolume(0.5));
    h.wait("the volume to be 0.5", |s, _| approx(s.volume, 0.5));

    assert!(h.key(Key::Down, Focus::VolumeBar).handled);
    h.wait("the volume to be 0.4", |s, _| approx(s.volume, 0.4));
    assert!(h.key(Key::Right, Focus::VolumeBar).handled);
    h.wait("the volume to be 0.5", |s, _| approx(s.volume, 0.5));

    h.perform(Action::SetVolume(1.0));
    h.wait("the volume to be 1.0", |s, _| approx(s.volume, 1.0));
    assert!(h.key(Key::Up, Focus::VolumeBar).handled);
    h.drain();
    assert!(approx(h.player.state().volume, 1.0), "the volume is clamped to 1.0");

    // Stepping while muted unmutes.
    h.player.set_muted(true).unwrap();
    h.wait("the player to mute", |s, _| s.muted);
    h.key(Key::Left, Focus::VolumeBar);
    h.wait("the player to unmute at 0.9", |s, _| !s.muted && approx(s.volume, 0.9));
}

#[test]
fn video_clicks() {
    let mut h = Harness::new(Autoplay::Off, options());
    h.load_wav(10);

    h.controls.video_clicked(Instant::now()).unwrap();
    h.wait("playback", |s, _| !s.paused);
    h.controls.video_clicked(Instant::now()).unwrap();
    h.wait("the player to pause", |s, _| s.paused);

    assert_eq!(h.controls.video_double_clicked(Instant::now()), Some(Request::ToggleFullscreen));
}

#[test]
fn video_clicks_with_controls_or_clicks_off() {
    let hidden = ui::Options::builder().controls(false).build().unwrap();
    let no_clicks = ui::Options::builder().click(false).double_click(false).build().unwrap();
    for options in [hidden, no_clicks] {
        let mut h = Harness::new(Autoplay::Off, options);
        h.load_wav(2);
        h.controls.video_clicked(Instant::now()).unwrap();
        assert_eq!(h.controls.video_double_clicked(Instant::now()), None);
        h.drain();
        assert!(h.player.state().paused);
    }
}

#[test]
fn toggle_mute_from_zero() {
    let mut h = Harness::new(Autoplay::Off, options());
    h.perform(Action::SetVolume(0.6));
    h.wait("the volume to be 0.6", |s, _| approx(s.volume, 0.6));
    h.perform(Action::SetVolume(0.0));
    h.wait("the volume to be 0", |s, v| s.volume == 0.0 && v.volume_level == 0);

    h.perform(Action::ToggleMute);
    h.wait("the volume to be restored", |s, _| approx(s.volume, 0.6) && !s.muted);

    // A plain toggle flips mute and keeps the volume.
    h.perform(Action::ToggleMute);
    let view = h.wait("the player to mute", |s, _| s.muted);
    assert_eq!((view.volume_fraction, view.volume_level), (0.0, 0));
    assert!(approx(h.player.state().volume, 0.6));

    // From a tiny last volume, at least 0.1 is restored.
    h.perform(Action::SetVolume(0.05));
    h.wait("the volume to be 0.05", |s, _| approx(s.volume, 0.05) && !s.muted);
    h.perform(Action::SetVolume(0.0));
    h.wait("the volume to be 0", |s, _| s.volume == 0.0);
    h.perform(Action::ToggleMute);
    h.wait("the volume to be 0.1", |s, _| approx(s.volume, 0.1));
}

#[test]
fn volume_drag_unmutes() {
    let mut h = Harness::new(Autoplay::Off, options());
    h.player.set_muted(true).unwrap();
    h.wait("the player to mute", |s, _| s.muted);
    h.controls.volume_drag(0.4, Instant::now()).unwrap();
    let view = h.wait("the volume to be 0.4, unmuted", |s, _| approx(s.volume, 0.4) && !s.muted);
    assert_eq!(view.volume_level, 2);
}

#[test]
fn rate_cycling() {
    let rates = ui::Options::builder().playback_rates([0.5, 1.0, 1.5, 2.0]).build().unwrap();
    let mut h = Harness::new(Autoplay::Off, rates);
    assert_eq!(h.controls.view(Instant::now()).rate.as_deref(), Some("1x"));

    h.perform(Action::CycleRate);
    let view = h.wait("the rate to be 1.5", |s, _| approx(s.rate, 1.5));
    assert_eq!(view.rate.as_deref(), Some("1.5x"));

    h.perform(Action::SetRate(2.0));
    h.wait("the rate to be 2", |s, _| approx(s.rate, 2.0));
    h.perform(Action::CycleRate);
    h.wait("the rate to wrap to 0.5", |s, _| approx(s.rate, 0.5));

    // A rate not in the list cycles to the first one.
    h.perform(Action::SetRate(1.25));
    h.wait("the rate to be 1.25", |s, _| approx(s.rate, 1.25));
    h.perform(Action::CycleRate);
    h.wait("the rate to be 0.5", |s, _| approx(s.rate, 0.5));

    // Rates round-tripped through mpv still match their entries.
    let odd = ui::Options::builder().playback_rates([0.3, 1.1, 1.7]).build().unwrap();
    let mut h = Harness::new(Autoplay::Off, odd);
    for expected in [0.3, 1.1, 1.7, 0.3] {
        h.perform(Action::CycleRate);
        h.wait(&format!("the rate to be {expected}"), |s, _| approx(s.rate, expected));
    }
}

#[test]
fn no_rates_no_rate_control() {
    let mut h = Harness::new(Autoplay::Off, options());
    assert_eq!(h.controls.view(Instant::now()).rate, None);
    h.perform(Action::CycleRate);
    h.drain();
    assert!(approx(h.player.state().rate, 1.0));
}

#[test]
fn drag_while_playing() {
    let mut h = Harness::new(Autoplay::On, options());
    h.load_wav(10);
    h.wait("playback", |s, _| !s.paused);

    h.controls.seek_press(0.1, Instant::now()).unwrap();
    h.wait("the drag to pause playback", |s, _| s.paused);
    h.controls.seek_drag(0.5, Instant::now()).unwrap();
    let view = h.controls.view(Instant::now());
    assert_eq!(view.played, 0.5, "the played fraction follows the pointer");
    assert!(h.player.state().paused);
    h.drain();

    h.controls.seek_release(0.5, Instant::now()).unwrap();
    h.wait_seeked_near(5.0, 0.1);
    h.wait("playback to resume", |s, _| !s.paused);
}

#[test]
fn drag_while_paused() {
    let mut h = Harness::new(Autoplay::Off, options());
    h.load_wav(10);

    h.controls.seek_press(0.2, Instant::now()).unwrap();
    h.controls.seek_drag(0.7, Instant::now()).unwrap();
    h.drain();
    h.controls.seek_release(0.7, Instant::now()).unwrap();
    h.wait_seeked_near(7.0, 0.1);
    h.drain();
    assert!(h.player.state().paused);
}

#[test]
fn drag_to_the_end() {
    let mut h = Harness::new(Autoplay::Off, options());
    h.load_wav(2);

    h.controls.seek_press(1.0, Instant::now()).unwrap();
    h.drain();
    h.controls.seek_release(1.0, Instant::now()).unwrap();
    // Seeking to the very end would end the media; the drag lands 0.1 s before it.
    h.wait_seeked_near(1.9, 0.05);
    assert!(!h.player.state().ended);
}

/// mpv's generated sources report a growing duration estimate as soon as they are read, so no source keeps the
/// duration unknown once loaded; a player with no media has none either. The loaded-but-unknown case is covered by
/// the controller's unit tests.
#[test]
fn drag_with_unknown_duration() {
    let mut h = Harness::new(Autoplay::Off, options());
    assert_eq!(h.player.state().duration, None);

    h.controls.seek_hover(Some(0.5));
    h.controls.seek_press(0.5, Instant::now()).unwrap();
    h.controls.seek_drag(0.8, Instant::now()).unwrap();
    h.controls.seek_release(0.8, Instant::now()).unwrap();
    while let Some(event) = h.events.recv_timeout(Duration::from_millis(300)) {
        assert_ne!(event, Event::Seeking, "the seek bar sought with no duration");
    }
    let view = h.controls.view(Instant::now());
    assert!(!view.seekable);
    assert_eq!((view.played, view.hover), (0.0, None));
    assert_eq!(h.player.state().position, None);
}

#[test]
fn loading_other_media_ends_a_drag() {
    let mut h = Harness::new(Autoplay::Off, options());
    h.load_wav(10);
    h.controls.seek_press(0.6, Instant::now()).unwrap();
    h.drain();

    h.load_wav(10);
    h.drain();
    assert!(h.controls.view(Instant::now()).played < 0.1, "the played fraction follows the new media");

    // The release belongs to the old media's drag, so it does not seek the new media.
    h.controls.seek_release(0.6, Instant::now()).unwrap();
    while let Some(event) = h.events.recv_timeout(Duration::from_millis(300)) {
        assert_ne!(event, Event::Seeking, "the release sought the new media");
    }
    assert!(h.player.state().position.is_some_and(|p| p < Duration::from_secs(1)));
}

#[test]
fn seek_hover_label() {
    let mut h = Harness::new(Autoplay::Off, options());
    h.load_wav(10);
    h.controls.seek_hover(Some(0.5));
    let hover = h.controls.view(Instant::now()).hover.expect("the hover preview is shown");
    assert_eq!((hover.fraction, hover.label.as_str()), (0.5, "0:05"));
    h.controls.seek_hover(None);
    assert_eq!(h.controls.view(Instant::now()).hover, None);
}

#[test]
fn start_state_across_load_play_and_end() {
    let poster = ui::Options::builder().poster("poster.jpg").build().unwrap();
    let mut h = Harness::new(Autoplay::Off, poster);
    h.load_wav(2);

    let view = h.controls.view(Instant::now());
    assert!(view.big_play_button);
    assert_eq!(view.poster.as_deref(), Some("poster.jpg"));
    assert_eq!(view.play_icon, PlayIcon::Play);
    assert!(!view.control_bar_visible, "the control bar is hidden before playback starts");
    assert_eq!(
        (view.current_time.as_str(), view.duration.as_str(), view.remaining.as_str()),
        ("0:00", "0:02", "-0:02")
    );

    // The big play button plays.
    h.perform(Action::Play);
    let view = h.wait("playback to start", |s, v| !s.paused && v.play_icon == PlayIcon::Pause);
    assert!(!view.big_play_button);
    assert_eq!(view.poster, None);
    assert!(view.control_bar_visible);

    let view = h.wait("the media to end", |s, v| s.ended && s.paused && v.play_icon == PlayIcon::Replay);
    assert!(view.control_bar_visible);
    assert!(!view.big_play_button);

    // Replay restarts from the beginning.
    h.perform(Action::TogglePlay);
    h.wait("playback to restart", |s, v| {
        !s.paused
            && !s.ended
            && s.position.is_some_and(|p| p < Duration::from_secs(1))
            && v.play_icon == PlayIcon::Pause
    });

    // New media shows the start state again.
    h.player.pause().unwrap();
    h.load_wav(2);
    let view = h.wait("the start state", |_, v| v.big_play_button);
    assert_eq!(view.poster.as_deref(), Some("poster.jpg"));
}

#[test]
fn autoplay_starts() {
    let mut h = Harness::new(Autoplay::On, options());
    h.load_wav(10);
    let view = h.wait("playback", |s, _| !s.paused);
    assert!(!view.big_play_button);
    assert!(view.control_bar_visible);
}

#[test]
fn spinner_delay() {
    let mut h = Harness::new(Autoplay::Off, options());
    h.load_wav(10);
    h.drain();

    // A real seek, with the event times set by the test: one that completes within 0.3 s never shows the spinner.
    let start = Instant::now();
    h.perform(Action::SeekTo(Duration::from_secs(1)));
    let seeking = h.next(Event::Seeking);
    h.controls.handle_event(&seeking, start);
    let view = h.controls.view(secs(start, 0.2));
    assert!(!view.spinner);
    assert_eq!(view.next_deadline, Some(secs(start, 0.3)));
    let seeked = h.next(Event::Seeked);
    h.controls.handle_event(&seeked, secs(start, 0.25));
    assert!(!h.controls.view(secs(start, 0.5)).spinner);

    // A longer wait shows the spinner from 0.3 s after it began until it ends.
    h.perform(Action::SeekTo(Duration::from_secs(2)));
    let seeking = h.next(Event::Seeking);
    let start = Instant::now();
    h.controls.handle_event(&seeking, start);
    assert!(!h.controls.view(secs(start, 0.29)).spinner);
    assert!(h.controls.view(secs(start, 0.3)).spinner);
    let seeked = h.next(Event::Seeked);
    h.controls.handle_event(&seeked, secs(start, 1.0));
    assert!(!h.controls.view(secs(start, 1.0)).spinner);
}

#[test]
fn unplayable_source() {
    let mut h = Harness::new(Autoplay::On, options());
    h.player.load(Source::url("av://lavfi:no-such-filter").unwrap()).unwrap();
    let view = h.wait("the error", |_, v| v.error.is_some());
    assert!(!view.error.unwrap().is_empty());
    let view = h.wait("the player to become idle", |s, _| s.idle);
    assert!(!view.control_bar_visible && !view.big_play_button && !view.spinner);

    // Loading other media clears the error.
    h.load_wav(2);
    let view = h.wait("the error to clear", |_, v| v.error.is_none());
    assert!(view.control_bar_visible, "autoplay started the new media");
}

#[test]
fn control_bar_and_cursor_visibility_over_time() {
    let mut h = Harness::new(Autoplay::On, options());
    h.load_wav(30);
    h.wait("playback", |s, _| !s.paused);

    let start = Instant::now();
    h.controls.pointer_moved(start);
    let view = h.controls.view(start);
    assert!(view.control_bar_visible && !view.cursor_hidden);
    assert_eq!(view.next_deadline, Some(secs(start, 2.0)));

    // Two seconds without input while playing hide both.
    let view = h.controls.view(secs(start, 2.0));
    assert!(!view.control_bar_visible && view.cursor_hidden);
    assert_eq!(view.next_deadline, None);

    // Input shows them again, for another two seconds.
    h.controls.pointer_moved(secs(start, 3.0));
    assert!(h.controls.view(secs(start, 4.9)).control_bar_visible);
    assert!(!h.controls.view(secs(start, 5.0)).control_bar_visible);

    // Hovering the control bar keeps it visible; it hides two seconds after the pointer leaves.
    h.controls.control_bar_hovered(true, secs(start, 6.0));
    assert!(h.controls.view(secs(start, 16.0)).control_bar_visible);
    h.controls.control_bar_hovered(false, secs(start, 16.0));
    assert!(h.controls.view(secs(start, 17.9)).control_bar_visible);
    assert!(!h.controls.view(secs(start, 18.0)).control_bar_visible);

    // Holding a pointer button keeps it visible.
    h.controls.pointer_pressed(secs(start, 20.0));
    assert!(h.controls.view(secs(start, 30.0)).control_bar_visible);
    h.controls.pointer_released(secs(start, 30.0));
    assert!(!h.controls.view(secs(start, 32.0)).control_bar_visible);

    // Leaving the player ends a hold.
    h.controls.pointer_pressed(secs(start, 40.0));
    h.controls.pointer_left(secs(start, 41.0));
    assert!(!h.controls.view(secs(start, 43.0)).control_bar_visible);

    // Paused media keeps the controls and the cursor, however long.
    h.player.pause().unwrap();
    h.wait("the player to pause", |s, _| s.paused);
    let view = h.controls.view(secs(start, 100.0));
    assert!(view.control_bar_visible && !view.cursor_hidden);
}

#[test]
fn zero_inactivity_timeout() {
    let never = ui::Options::builder().inactivity_timeout(Duration::ZERO).build().unwrap();
    let mut h = Harness::new(Autoplay::On, never);
    h.load_wav(10);
    h.wait("playback", |s, _| !s.paused);

    let start = Instant::now();
    let view = h.controls.view(secs(start, 10.0));
    assert!(view.control_bar_visible && !view.cursor_hidden);
    assert_eq!(view.next_deadline, None);
}
