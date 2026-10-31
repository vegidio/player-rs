//! The UI layer driving a real headless player (null audio, no video output), as a frontend would: every event from
//! the frontend's subscription is forwarded to the controller. Media is a WAV file written into a temporary
//! directory, for a known duration and backward seeks, or mpv's generated `lavfi` source where no duration is
//! wanted. Time-based behaviour (inactivity, the spinner delay) is checked by passing `Instant`s offset by the test,
//! so nothing sleeps; waiting on the player is always done on events with a deadline.

mod common;

use common::TIMEOUT;
use player::prelude::*;
use player::ui::{
    self, Action, BarPolicy, Controls, Focus, Hotkeys, Key, Modifiers, Page, Parts, PlayIcon, Request, View,
};
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
        Self::with_policy(autoplay, options, BarPolicy::default())
    }

    fn with_policy(autoplay: Autoplay, options: ui::Options, policy: BarPolicy) -> Self {
        let player = Player::builder()
            .video_output(VideoOutput::Null)
            .autoplay(autoplay)
            .mpv_option("ao", "null")
            .build()
            .expect("the player builds");
        let events = player.subscribe();
        let controls = Controls::new(&player, options, policy, Instant::now());
        Self { player, events, controls, _dir: None }
    }

    /// Loads a WAV file of `seconds` seconds and waits until it has loaded.
    fn load_wav(&mut self, seconds: u32) {
        let dir = mk_temp_dir("player-rs-").unwrap();
        let path = common::write_wav(dir.path(), "tone.wav", seconds);
        self._dir = Some(dir);
        self.load(Source::file(path).unwrap());
    }

    /// Loads a source that cannot be opened and waits for its error.
    fn load_broken(&mut self) {
        self.player.load(Source::url("av://lavfi:no-such-filter").unwrap()).unwrap();
        self.wait("the error", |_, v| v.error.is_some());
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
fn video_clicks_off() {
    let no_clicks = ui::Options::builder().click(false).double_click(false).build().unwrap();
    let mut h = Harness::new(Autoplay::Off, no_clicks);
    h.load_wav(2);
    h.controls.video_clicked(Instant::now()).unwrap();
    assert_eq!(h.controls.video_double_clicked(Instant::now()), None);
    h.drain();
    assert!(h.player.state().paused);
}

#[test]
fn video_clicks_whatever_is_shown() {
    let none = Parts::default()
        .play(false)
        .volume(false)
        .seek_bar(false)
        .current_time(false)
        .time_display(false)
        .fullscreen(false)
        .speed(false)
        .big_play_button(false)
        .spinner(false)
        .error_display(false)
        .seek_hover(false);
    let mut h = Harness::new(Autoplay::Off, ui::Options::builder().parts(none).build().unwrap());
    h.load_wav(10);
    h.controls.video_clicked(Instant::now()).unwrap();
    h.wait("playback", |s, _| !s.paused);
    assert_eq!(h.controls.video_double_clicked(Instant::now()), Some(Request::ToggleFullscreen));
}

#[test]
fn hidden_volume_keeps_the_mute_hotkey() {
    let hidden = ui::Options::builder().parts(Parts::default().volume(false)).build().unwrap();
    let mut h = Harness::new(Autoplay::Off, hidden);
    assert!(!h.controls.view(Instant::now()).shown.volume);
    assert!(h.key(Key::Char('m'), Focus::Player).handled);
    h.wait("the player to mute", |s, _| s.muted);
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

/// A player with `options`, paused on loaded media, whose control bar (and so its menu) is shown before the first play.
fn paused_with_bar(options: ui::Options) -> Harness {
    let policy = BarPolicy { before_start: true, ..BarPolicy::default() };
    let mut h = Harness::with_policy(Autoplay::Off, options, policy);
    h.load_wav(30);
    h
}

/// The Speed page's checked entries, opened from the root page.
fn speed_page_checked(h: &mut Harness) -> Vec<bool> {
    h.perform(Action::OpenPage(Page::Speed));
    let menu = h.controls.view(Instant::now()).settings.menu.expect("the menu is open");
    assert_eq!(menu.page, Page::Speed);
    let checked = menu.items.iter().map(|item| item.checked).collect();
    h.perform(Action::CloseSettings);
    checked
}

/// The Speed item's value on the root page.
fn speed_value(h: &mut Harness) -> Option<String> {
    h.perform(Action::ToggleSettings);
    let menu = h.controls.view(Instant::now()).settings.menu.expect("the menu is open");
    h.perform(Action::CloseSettings);
    menu.items[0].value.clone()
}

#[test]
fn set_the_rate() {
    let mut h = Harness::new(Autoplay::Off, options());
    h.perform(Action::SetRate(1.5));
    h.wait("the rate to be 1.5", |s, v| approx(s.rate, 1.5) && v.rate.as_deref() == Some("1.5x"));
}

#[test]
fn rates_are_chosen_on_the_speed_page() {
    // From the last rate, the first one is chosen by selecting it; no action cycles to it.
    let rates = ui::Options::builder().playback_rates([0.5, 1.0, 1.5, 2.0]).build().unwrap();
    let mut h = paused_with_bar(rates);
    h.perform(Action::SetRate(2.0));
    h.wait("the rate to be 2", |s, _| approx(s.rate, 2.0));
    h.perform(Action::OpenPage(Page::Speed));
    h.perform(Action::Select(0));
    h.wait("the rate to be 0.5", |s, _| approx(s.rate, 0.5));

    // From a rate not in the list.
    let rates = ui::Options::builder().playback_rates([0.5, 1.0, 2.0]).build().unwrap();
    let mut h = paused_with_bar(rates);
    h.perform(Action::SetRate(1.25));
    h.wait("the rate to be 1.25", |s, _| approx(s.rate, 1.25));
    h.perform(Action::OpenPage(Page::Speed));
    h.perform(Action::Select(0));
    h.wait("the rate to be 0.5", |s, _| approx(s.rate, 0.5));

    // Rates round-tripped through mpv still match their entries.
    let odd = ui::Options::builder().playback_rates([0.3, 1.1, 1.7]).build().unwrap();
    let mut h = paused_with_bar(odd);
    for (index, expected) in [0.3, 1.1, 1.7].into_iter().enumerate() {
        h.perform(Action::OpenPage(Page::Speed));
        h.perform(Action::Select(index));
        h.wait(&format!("the rate to be {expected}"), |s, _| approx(s.rate, expected));
        let mut checked = [false; 3];
        checked[index] = true;
        assert_eq!(speed_page_checked(&mut h), checked);
    }
}

#[test]
fn rate_set_without_the_menu() {
    let rates = ui::Options::builder().playback_rates([1.0, 1.5, 2.0]).build().unwrap();
    let mut h = paused_with_bar(rates);
    assert_eq!(speed_value(&mut h).as_deref(), Some("1x"));

    h.perform(Action::SetRate(1.5));
    h.wait("the rate to be 1.5", |s, _| approx(s.rate, 1.5));
    assert_eq!(speed_value(&mut h).as_deref(), Some("1.5x"));
    assert_eq!(speed_page_checked(&mut h), [false, true, false]);

    // A rate not in the list is labelled, and nothing is checked.
    h.perform(Action::SetRate(1.25));
    h.wait("the rate to be 1.25", |s, _| approx(s.rate, 1.25));
    assert_eq!(speed_value(&mut h).as_deref(), Some("1.25x"));
    assert_eq!(speed_page_checked(&mut h), [false, false, false]);
}

#[test]
fn no_rates_no_rate_control() {
    let mut h = paused_with_bar(ui::Options::builder().playback_rates([]).build().unwrap());
    let view = h.controls.view(Instant::now());
    assert_eq!(view.rate, None);
    assert!(!view.settings.button);
    h.perform(Action::ToggleSettings);
    h.perform(Action::OpenPage(Page::Speed));
    assert_eq!(h.controls.view(Instant::now()).settings.menu, None);
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
    assert!(!view.controls_enabled && view.shown.error_display);

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

#[test]
fn shown_before_start_by_policy() {
    let policy = BarPolicy { before_start: true, ..BarPolicy::default() };
    let mut h = Harness::with_policy(Autoplay::Off, options(), policy);
    h.load_wav(10);
    let start = Instant::now();
    assert!(h.controls.view(start).control_bar_visible);
    assert!(h.controls.view(secs(start, 10.0)).control_bar_visible, "paused media keeps it");

    // Video.js's policy hides it until playback starts.
    let mut h = Harness::new(Autoplay::Off, options());
    h.load_wav(10);
    assert!(!h.controls.view(Instant::now()).control_bar_visible);
}

#[test]
fn shown_on_error_by_policy() {
    let policy = BarPolicy { on_error: true, ..BarPolicy::default() };
    let mut h = Harness::with_policy(Autoplay::Off, options(), policy);
    h.load_broken();
    let view = h.wait("the player to become idle", |s, _| s.idle);
    assert!(view.control_bar_visible && !view.controls_enabled);

    assert_eq!(h.perform(Action::Play), None);
    assert_eq!(h.key(Key::Char('k'), Focus::Player).request, None);
    h.drain();
    assert!(h.player.state().paused, "play does nothing");
    assert_eq!(h.perform(Action::ToggleFullscreen), Some(Request::ToggleFullscreen));
}

#[test]
fn time_display_toggle() {
    let mut h = Harness::new(Autoplay::Off, options());
    h.load_wav(120);
    // Half a second past the mark: mpv's default seek may land a little before it, and labels count whole seconds.
    h.player.seek(Seek::To(Duration::from_millis(42_500))).unwrap();
    let view = h.wait("0:42", |_, v| v.current_time == "0:42");
    assert_eq!((view.time_display, view.time_label.as_str()), (ui::TimeDisplay::Remaining, "-1:18"));

    h.perform(Action::ToggleTimeDisplay);
    let view = h.controls.view(Instant::now());
    assert_eq!((view.time_display, view.time_label.as_str()), (ui::TimeDisplay::Duration, "2:00"));
    assert!(h.player.state().paused, "playback does not change");

    // The choice is kept across loaded media.
    h.load_wav(90);
    let view = h.wait("the new duration", |_, v| v.duration == "1:30");
    assert_eq!((view.time_display, view.time_label.as_str()), (ui::TimeDisplay::Duration, "1:30"));

    h.perform(Action::ToggleTimeDisplay);
    assert_eq!(h.controls.view(Instant::now()).time_label, "-1:30");
}

#[test]
fn select_a_speed() {
    let rates = ui::Options::builder().playback_rates([1.0, 1.5, 2.0]).build().unwrap();
    let mut h = Harness::new(Autoplay::On, rates);
    h.load_wav(30);
    h.wait("playback", |s, _| !s.paused);

    h.perform(Action::ToggleSettings);
    h.perform(Action::Select(0));
    let menu = h.controls.view(Instant::now()).settings.menu.unwrap();
    assert_eq!(menu.page, Page::Speed);
    assert!(menu.items[0].checked, "the current rate is checked");

    h.perform(Action::Select(1));
    let view = h.wait("the rate to be 1.5", |s, v| approx(s.rate, 1.5) && v.rate.as_deref() == Some("1.5x"));
    let menu = view.settings.menu.unwrap();
    assert_eq!(menu.page, Page::Root);
    assert_eq!(menu.items[0].value.as_deref(), Some("1.5x"));

    h.perform(Action::OpenPage(Page::Speed));
    let menu = h.controls.view(Instant::now()).settings.menu.unwrap();
    let checked: Vec<_> = menu.items.iter().map(|item| item.checked).collect();
    assert_eq!(checked, [false, true, false]);
}

#[test]
fn keys_in_the_menu() {
    let rates = ui::Options::builder().playback_rates([0.5, 1.0, 1.5, 2.0]).build().unwrap();
    let mut h = Harness::new(Autoplay::On, rates);
    h.load_wav(30);
    h.wait("playback", |s, _| !s.paused);

    h.perform(Action::ToggleSettings);
    for key in [Key::Enter, Key::Down, Key::Enter] {
        assert!(h.key(key, Focus::Menu).handled, "{key:?}");
    }
    h.wait("the rate after the current one", |s, _| approx(s.rate, 1.5));
    assert!(!h.player.state().paused, "Enter does not toggle play");

    let outcome = h.key(Key::Escape, Focus::Menu);
    assert_eq!(outcome, ui::KeyOutcome { handled: true, request: None });
    assert_eq!(h.controls.view(Instant::now()).settings.menu, None);
}

#[test]
fn open_settings_menu_keeps_the_bar() {
    let mut h = Harness::new(Autoplay::On, options());
    h.load_wav(30);
    h.wait("playback", |s, _| !s.paused);
    let start = Instant::now();
    h.controls.perform(Action::ToggleSettings, start).unwrap();
    let view = h.controls.view(secs(start, 10.0));
    assert!(view.control_bar_visible && view.settings.menu.is_some());
    assert_eq!(view.next_deadline, None);
}

#[test]
fn menu_closes_with_the_bar() {
    let mut h = Harness::new(Autoplay::On, options());
    h.load_wav(30);
    h.wait("playback", |s, _| !s.paused);
    h.perform(Action::ToggleSettings);
    assert!(h.controls.view(Instant::now()).settings.menu.is_some());

    // Stopping unloads the media, which hides the bar until playback starts again.
    h.player.stop().unwrap();
    let view = h.wait("the bar to hide", |_, v| !v.control_bar_visible);
    assert_eq!(view.settings.menu, None);
    h.drain();
    h.load_wav(30);
    h.wait("playback", |s, _| !s.paused);
    assert_eq!(h.controls.view(Instant::now()).settings.menu, None, "it stays closed");
}

#[test]
fn retry_after_the_source_becomes_readable() {
    let mut h = Harness::new(Autoplay::Off, options());
    let dir = mk_temp_dir("player-rs-").unwrap();
    let path = dir.path().join("later.wav");
    h.player.load(Source::file(&path).unwrap()).unwrap();
    h.wait("the error", |_, v| v.error.is_some());

    common::write_wav(dir.path(), "later.wav", 10);
    assert_eq!(h.perform(Action::Retry), None);
    h.wait_event("the source to load", |event, _| *event == Event::Loaded);
    let view = h.wait("playback, whatever the autoplay mode", |s, _| !s.paused && !s.idle);
    assert_eq!(view.error, None);
}

#[test]
fn retry_that_fails_again() {
    let mut h = Harness::new(Autoplay::On, options());
    h.load_broken();
    h.drain();
    h.perform(Action::Retry);
    h.wait_event("the error again", |event, _| matches!(event, Event::Ended(EndReason::Error(_))));
    assert!(h.controls.view(Instant::now()).error.is_some());
}

#[test]
fn nothing_to_retry() {
    let mut h = Harness::new(Autoplay::Off, options());
    h.player.reload().unwrap();
    assert_eq!(h.perform(Action::Retry), None);
    h.drain();
    let state = h.player.state();
    assert!(state.idle && state.paused);
}
