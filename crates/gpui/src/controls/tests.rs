//! The controls in headless GPUI windows, driving real players (null audio, no video output) with simulated pointer
//! and keyboard input. Media is a WAV file written into a temporary directory, for a known duration, or mpv's
//! generated `lavfi` source where no duration is wanted.
//!
//! Player events arrive from mpv's threads, so waiting on the player polls with short real sleeps. Activity timeouts
//! and fades run on the test executor's clock instead, which only `advance_clock` moves.

use super::*;
use crate::VideoPlayer;
use gpui_kit::TestAppContext;
use gpui_kit::base::Root;
use gpui_kit::test::TestWindowExt;
use player::prelude::*;
use player::ui::{Hotkeys, PlayIcon};
use rust_sak::fs::{TempDir, mk_temp_dir};
use std::cell::RefCell;
use std::path::PathBuf;
// The glob above brings in GPUI's `test` macro; `#[gpui_kit::test]` expands to Rust's `#[test]`, named here.
use std::prelude::v1::test;
use std::time::Duration;

thread_local! {
    /// Every controls view created on this thread, so tests can find the views their windows created.
    static VIEWS: RefCell<Vec<WeakEntity<ControlsView>>> = const { RefCell::new(Vec::new()) };
}

pub(super) fn created(view: WeakEntity<ControlsView>) {
    VIEWS.with_borrow_mut(|views| views.push(view));
}

fn live_views() -> Vec<Entity<ControlsView>> {
    VIEWS.with_borrow(|views| views.iter().filter_map(WeakEntity::upgrade).collect())
}

/// How long to wait for the player before failing.
const TIMEOUT: Duration = Duration::from_secs(15);

/// Writes a mono, 16-bit, 8 kHz PCM WAV of `seconds` seconds of a 440 Hz tone, with a hand-written header.
fn write_wav(dir: &TempDir, seconds: u32) -> PathBuf {
    const RATE: u32 = 8000;
    let data_len = RATE * seconds * 2;
    let mut wav = Vec::with_capacity(44 + data_len as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    for field in [16u32.to_le_bytes(), [1, 0, 1, 0], RATE.to_le_bytes(), (RATE * 2).to_le_bytes(), [2, 0, 16, 0]] {
        wav.extend_from_slice(&field);
    }
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for n in 0..RATE * seconds {
        let phase = 2.0 * std::f64::consts::PI * 440.0 * f64::from(n) / f64::from(RATE);
        wav.extend_from_slice(&((phase.sin() * 8000.0) as i16).to_le_bytes());
    }
    let path = dir.path().join(format!("tone-{seconds}.wav"));
    std::fs::write(&path, wav).unwrap();
    path
}

/// The media a test plays.
#[derive(Clone, Copy)]
enum Media {
    /// Nothing loaded.
    None,
    /// A WAV file of this many seconds.
    Wav(u32),
    /// A source that cannot be opened.
    Broken,
}

/// How the test's element is sized.
#[derive(Clone, Copy)]
enum Sizing {
    Fixed(f32, f32),
    Width(f32),
}

/// The application's view: one video element, a record of the keys and fullscreen changes that reached it.
struct Parent {
    player: Player,
    options: ui::Options,
    sizing: Sizing,
    keys: Rc<RefCell<Vec<String>>>,
    fullscreen: Rc<RefCell<Vec<bool>>>,
}

impl Render for Parent {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let fullscreen = Rc::clone(&self.fullscreen);
        let element = VideoPlayer::new("video", &self.player)
            .options(self.options.clone())
            .on_fullscreen_change(move |on, _, _| fullscreen.borrow_mut().push(on));
        let element = match self.sizing {
            Sizing::Fixed(width, height) => element.w(px(width)).h(px(height)),
            Sizing::Width(width) => element.w(px(width)),
        };
        let keys = Rc::clone(&self.keys);
        div()
            .size_full()
            .on_key_down(move |event: &KeyDownEvent, _, cx| {
                keys.borrow_mut().push(event.keystroke.unparse(cx.platform_info().os))
            })
            .child(element)
    }
}

struct Harness {
    window: AnyWindowHandle,
    parent: Entity<Parent>,
    player: Player,
    _dir: TempDir,
}

/// The test's player and options.
struct Setup {
    autoplay: Autoplay,
    volume: f64,
    options: ui::Options,
    media: Media,
    sizing: Sizing,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            autoplay: Autoplay::On,
            volume: 1.0,
            options: ui::Options::default(),
            media: Media::Wav(30),
            sizing: Sizing::Fixed(640., 360.),
        }
    }
}

fn player(autoplay: Autoplay, volume: f64) -> Player {
    Player::builder()
        .video_output(VideoOutput::Null)
        .autoplay(autoplay)
        .volume(volume)
        .mpv_option("ao", "null")
        .build()
        .unwrap()
}

impl Harness {
    fn open(cx: &mut TestAppContext, setup: Setup) -> Self {
        cx.executor().allow_parking();
        cx.update(gpui_kit::init);
        let player = player(setup.autoplay, setup.volume);
        let dir = mk_temp_dir("player-gpui-").unwrap();
        let parent = cx.new(|_| Parent {
            player: player.clone(),
            options: setup.options,
            sizing: setup.sizing,
            keys: Rc::default(),
            fullscreen: Rc::default(),
        });
        let window = cx.add_window({
            let parent = parent.clone();
            move |window, cx| Root::new(parent, window, cx)
        });
        let harness = Self { window: window.into(), parent, player, _dir: dir };
        harness.draw(cx);
        harness.load(cx, setup.media);
        harness
    }

    fn load(&self, cx: &mut TestAppContext, media: Media) {
        let source = match media {
            Media::None => return,
            Media::Wav(seconds) => Source::file(write_wav(&self._dir, seconds)).unwrap(),
            Media::Broken => Source::url("av://lavfi:no-such-filter").unwrap(),
        };
        self.player.load(source).unwrap();
        match media {
            Media::Broken => self.wait_view(cx, "the error", |view| view.error.is_some()),
            Media::Wav(_) => self.wait(cx, "the media to load", |state, _| state.duration.is_some()),
            Media::None => {}
        }
    }

    /// The window's single controls view.
    fn view(&self) -> Entity<ControlsView> {
        match &live_views()[..] {
            [view] => view.clone(),
            views => panic!("{} controls views", views.len()),
        }
    }

    fn update<R>(&self, cx: &mut TestAppContext, f: impl FnOnce(&mut Window, &mut App) -> R) -> R {
        cx.update_window(self.window, |_, window, cx| f(window, cx)).unwrap()
    }

    fn draw(&self, cx: &mut TestAppContext) {
        cx.run_until_parked();
        self.update(cx, |window, cx| window.render_frame(cx));
    }

    /// The view last drawn.
    fn drawn(&self, cx: &TestAppContext) -> ui::View {
        cx.read(|cx| self.view().read(cx).view.clone())
    }

    /// Runs GPUI's tasks and draws until `done` holds for the player's state and the view. Polls with short real
    /// sleeps, as events come from mpv's threads.
    fn wait(&self, cx: &mut TestAppContext, what: &str, mut done: impl FnMut(&State, &ui::View) -> bool) {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            self.draw(cx);
            if done(&self.player.state(), &self.drawn(cx)) {
                return;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}: {:?}", self.player.state());
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_view(&self, cx: &mut TestAppContext, what: &str, mut done: impl FnMut(&ui::View) -> bool) {
        self.wait(cx, what, |_, view| done(view));
    }

    fn wait_playing(&self, cx: &mut TestAppContext) {
        self.wait(cx, "playback", |state, view| !state.paused && view.play_icon == PlayIcon::Pause);
    }

    fn wait_paused(&self, cx: &mut TestAppContext) {
        self.wait(cx, "a pause", |state, view| state.paused && view.play_icon != PlayIcon::Pause);
    }

    /// Moves the test clock past the inactivity timeout and the fade-out.
    fn idle(&self, cx: &mut TestAppContext) {
        cx.executor().advance_clock(Duration::from_secs(2) + ui::FADE_OUT + Duration::from_millis(100));
        self.draw(cx);
    }

    fn find(&self, cx: &mut TestAppContext, id: &'static str) -> Option<Bounds<Pixels>> {
        self.update(cx, |window, cx| {
            window.render_frame(cx);
            window.try_find(id).filter(|element| element.visible()).map(|element| element.bounds())
        })
    }

    fn bounds(&self, cx: &mut TestAppContext, id: &'static str) -> Bounds<Pixels> {
        self.find(cx, id).unwrap_or_else(|| panic!("{id} is not drawn"))
    }

    fn click(&self, cx: &mut TestAppContext, id: &'static str) {
        self.update(cx, |window, cx| window.click(id, cx));
        self.draw(cx);
    }

    fn press(&self, cx: &mut TestAppContext, key: &str) {
        self.update(cx, |window, cx| window.press(key, cx));
        self.draw(cx);
    }

    fn dispatch(&self, cx: &mut TestAppContext, event: impl InputEvent) {
        self.update(cx, |window, cx| {
            window.dispatch_event(event.to_platform_input(), cx);
            window.render_frame(cx);
        });
        self.draw(cx);
    }

    /// Moves the pointer to the centre of `id`.
    fn hover(&self, cx: &mut TestAppContext, id: &'static str) {
        let center = self.bounds(cx, id).center();
        self.mouse_move(cx, center, None);
    }

    fn mouse_move(&self, cx: &mut TestAppContext, position: Point<Pixels>, pressed: Option<MouseButton>) {
        let event = MouseMoveEvent { position, pressed_button: pressed, modifiers: Modifiers::default() };
        self.dispatch(cx, event);
    }

    fn mouse_down(&self, cx: &mut TestAppContext, position: Point<Pixels>) {
        let event = MouseDownEvent {
            button: MouseButton::Left,
            position,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        };
        self.dispatch(cx, event);
    }

    fn mouse_up(&self, cx: &mut TestAppContext, position: Point<Pixels>) {
        let event =
            MouseUpEvent { button: MouseButton::Left, position, modifiers: Modifiers::default(), click_count: 1 };
        self.dispatch(cx, event);
    }

    /// Focuses one of the player's focus handles, as a click or Tab would.
    fn focus(&self, cx: &mut TestAppContext, handle: impl FnOnce(&ControlsView) -> FocusHandle) {
        let handle = cx.read(|cx| handle(self.view().read(cx)));
        self.update(cx, |window, cx| window.focus(&handle, cx));
        self.draw(cx);
    }

    fn focused(&self, cx: &mut TestAppContext, handle: impl FnOnce(&ControlsView) -> FocusHandle) -> bool {
        let handle = cx.read(|cx| handle(self.view().read(cx)));
        self.update(cx, |window, _| handle.is_focused(window))
    }

    fn fullscreen(&self, cx: &TestAppContext) -> Fullscreen {
        cx.read(|cx| self.view().read(cx).fullscreen)
    }

    fn window_fullscreen(&self, cx: &mut TestAppContext) -> bool {
        self.update(cx, |window, _| window.is_fullscreen())
    }

    fn reported(&self, cx: &TestAppContext) -> Vec<bool> {
        cx.read(|cx| self.parent.read(cx).fullscreen.borrow().clone())
    }

    fn keys(&self, cx: &TestAppContext) -> Vec<String> {
        cx.read(|cx| self.parent.read(cx).keys.borrow().clone())
    }

    fn change_parent(&self, cx: &mut TestAppContext, change: impl FnOnce(&mut Parent)) {
        cx.update(|cx| {
            self.parent.update(cx, |parent, cx| {
                change(parent);
                cx.notify();
            })
        });
        self.draw(cx);
    }
}

/// The point at `fraction` along a bar drawn at `bounds` with `margin` on each side, at its vertical centre.
fn along(bounds: Bounds<Pixels>, margin: f32, fraction: f32) -> Point<Pixels> {
    let start = bounds.left() + px(margin);
    let width = bounds.size.width - px(2. * margin);
    point(start + width * fraction, bounds.center().y)
}

/// The seek bar's point at `fraction`: its track has 10-pixel margins.
fn seek_at(bounds: Bounds<Pixels>, fraction: f32) -> Point<Pixels> {
    along(bounds, 10., fraction)
}

fn secs(position: Option<Duration>) -> f64 {
    position.map_or(0., |position| position.as_secs_f64())
}

fn options() -> ui::OptionsBuilder {
    ui::Options::builder()
}

// 1.3: keyed state, events and deadlines.

#[gpui_kit::test]
fn player_event_updates_the_view(cx: &mut TestAppContext) {
    let harness = Harness::open(cx, Setup { autoplay: Autoplay::Off, ..Setup::default() });
    assert_eq!(harness.drawn(cx).volume_level, 3);
    harness.player.set_volume(0.2).unwrap();
    harness.wait_view(cx, "the volume level", |view| view.volume_level == 1);
}

#[gpui_kit::test]
fn inactivity_hides_the_bar_while_playing(cx: &mut TestAppContext) {
    let harness = Harness::open(cx, Setup::default());
    harness.wait_playing(cx);
    harness.wait_view(cx, "the bar", |view| view.control_bar_visible);
    cx.executor().advance_clock(Duration::from_millis(1900));
    harness.draw(cx);
    assert!(harness.drawn(cx).control_bar_visible, "still active before the timeout");
    cx.executor().advance_clock(Duration::from_millis(200));
    harness.draw(cx);
    assert!(!harness.drawn(cx).control_bar_visible, "hidden after the timeout, with no input");
}

#[gpui_kit::test]
fn options_are_read_once_per_player(cx: &mut TestAppContext) {
    let harness = Harness::open(cx, Setup { autoplay: Autoplay::Off, ..Setup::default() });
    let first = harness.view().entity_id();
    let rates = options().playback_rates([1.0, 2.0]).build().unwrap();

    // The same player under the same id keeps the first options.
    harness.change_parent(cx, |parent| parent.options = rates.clone());
    assert_eq!(harness.view().entity_id(), first);
    assert_eq!(harness.drawn(cx).rate, None);

    // A different player rebuilds the controls with the options given then.
    let other = player(Autoplay::Off, 1.0);
    harness.change_parent(cx, |parent| parent.player = other.clone());
    harness.wait_view(cx, "the new controls", |view| view.rate.is_some());
    let view = harness.view();
    assert_ne!(view.entity_id(), first);
    assert!(cx.read(|cx| Player::ptr_eq(view.read(cx).player(), &other)));
}

// 1.4: fluid sizing.

#[gpui_kit::test]
fn aspect_ratio_sets_the_height(cx: &mut TestAppContext) {
    let setup = Setup {
        options: options().aspect_ratio("16:9").build().unwrap(),
        sizing: Sizing::Width(800.),
        media: Media::None,
        ..Setup::default()
    };
    let harness = Harness::open(cx, setup);
    assert_eq!(harness.bounds(cx, "controls").size, size(px(800.), px(450.)));
}

#[gpui_kit::test]
fn no_aspect_ratio_keeps_the_given_size(cx: &mut TestAppContext) {
    let setup = Setup { sizing: Sizing::Fixed(640., 480.), ..Setup::default() };
    let harness = Harness::open(cx, setup);
    assert_eq!(harness.bounds(cx, "controls").size, size(px(640.), px(480.)));
}

// 2.1: the control bar.

#[gpui_kit::test]
fn play_control_pauses_plays_and_replays(cx: &mut TestAppContext) {
    let harness = Harness::open(cx, Setup { media: Media::Wav(1), ..Setup::default() });
    harness.wait_playing(cx);
    harness.click(cx, "play");
    harness.wait_paused(cx);
    harness.click(cx, "play");
    harness.wait_playing(cx);

    harness.wait(cx, "the end", |state, view| state.ended && view.play_icon == PlayIcon::Replay);
    harness.click(cx, "play");
    harness.wait(cx, "a restart", |state, _| !state.paused && !state.ended && secs(state.position) < 0.9);
}

#[gpui_kit::test]
fn rate_control_cycles(cx: &mut TestAppContext) {
    let setup = Setup { options: options().playback_rates([1.0, 1.5, 2.0]).build().unwrap(), ..Setup::default() };
    let harness = Harness::open(cx, setup);
    harness.wait_playing(cx);
    assert_eq!(harness.drawn(cx).rate.as_deref(), Some("1x"));
    harness.click(cx, "rate");
    harness.wait(cx, "rate 1.5", |state, view| state.rate == 1.5 && view.rate.as_deref() == Some("1.5x"));
}

#[gpui_kit::test]
fn mute_control_toggles(cx: &mut TestAppContext) {
    let harness = Harness::open(cx, Setup { volume: 0.8, ..Setup::default() });
    harness.wait_playing(cx);
    harness.click(cx, "mute");
    harness.wait(cx, "mute", |state, view| state.muted && view.volume_level == 0);
    harness.click(cx, "mute");
    harness.wait(cx, "unmute", |state, view| !state.muted && state.volume == 0.8 && view.volume_level == 3);
}

#[gpui_kit::test]
fn no_rate_control_without_rates(cx: &mut TestAppContext) {
    let harness = Harness::open(cx, Setup::default());
    harness.wait_playing(cx);
    assert!(harness.find(cx, "play").is_some() && harness.find(cx, "remaining-time").is_some());
    assert!(harness.find(cx, "rate").is_none());
}

#[gpui_kit::test]
fn remaining_time_follows_playback(cx: &mut TestAppContext) {
    let harness = Harness::open(cx, Setup { media: Media::Wav(120), ..Setup::default() });
    harness.wait_playing(cx);
    harness.player.pause().unwrap();
    // Half a second past each mark: mpv's default seek may land a little before it, and the label counts whole seconds.
    for (at, label) in [(10_500, "-1:50"), (20_500, "-1:40")] {
        harness.player.seek(player::Seek::To(Duration::from_millis(at))).unwrap();
        harness.wait_view(cx, label, |view| view.remaining == label);
        assert!(harness.find(cx, "remaining-time").is_some());
    }
}

// 2.2: the seek bar.

#[gpui_kit::test]
fn seek_drag_while_playing(cx: &mut TestAppContext) {
    let harness = Harness::open(cx, Setup { media: Media::Wav(10), ..Setup::default() });
    harness.wait_playing(cx);
    let bar = harness.bounds(cx, "seek-bar");

    harness.mouse_move(cx, seek_at(bar, 0.1), None);
    harness.mouse_down(cx, seek_at(bar, 0.1));
    harness.wait_paused(cx);
    harness.mouse_move(cx, seek_at(bar, 0.5), Some(MouseButton::Left));
    let played = harness.drawn(cx).played;
    assert!((played - 0.5).abs() < 0.01, "the played bar follows the pointer: {played}");
    harness.mouse_up(cx, seek_at(bar, 0.5));
    harness.wait(cx, "playback near 50 %", |state, _| !state.paused && (secs(state.position) - 5.).abs() < 0.5);
}

#[gpui_kit::test]
fn seek_drag_released_above_the_bar(cx: &mut TestAppContext) {
    let harness = Harness::open(cx, Setup { autoplay: Autoplay::Off, media: Media::Wav(10), ..Setup::default() });
    harness.player.play().unwrap();
    harness.wait_playing(cx);
    harness.player.pause().unwrap();
    harness.wait_paused(cx);
    let bar = harness.bounds(cx, "seek-bar");

    harness.mouse_down(cx, seek_at(bar, 0.1));
    let above = point(seek_at(bar, 0.7).x, bar.top() - px(100.));
    harness.mouse_move(cx, above, Some(MouseButton::Left));
    harness.mouse_up(cx, above);
    harness.wait(cx, "the seek to 70 %", |state, _| (secs(state.position) - 7.).abs() < 0.3);
}

#[gpui_kit::test]
fn seek_hover_shows_the_time(cx: &mut TestAppContext) {
    let harness = Harness::open(cx, Setup { autoplay: Autoplay::Off, media: Media::Wav(90), ..Setup::default() });
    harness.player.play().unwrap();
    harness.wait_playing(cx);
    let bar = harness.bounds(cx, "seek-bar");
    harness.mouse_move(cx, seek_at(bar, 0.5), None);
    assert_eq!(harness.drawn(cx).hover.map(|hover| hover.label).as_deref(), Some("0:45"));
    let label = harness.bounds(cx, "seek-label");
    // Video.js places it 3.4em above the track, which is centred in the 30-pixel bar.
    assert!(label.bottom() < bar.center().y - px(5.), "the label is above the track");
    assert!((label.center().x - seek_at(bar, 0.5).x).abs() < px(1.5), "and centred on the pointer");
}

#[gpui_kit::test]
fn seek_knob_marks_the_played_end(cx: &mut TestAppContext) {
    let harness = Harness::open(cx, Setup { media: Media::Wav(90), ..Setup::default() });
    harness.wait_playing(cx);
    harness.player.pause().unwrap();
    harness.wait_paused(cx);
    let bar = harness.bounds(cx, "seek-bar");
    harness.mouse_down(cx, seek_at(bar, 0.5));
    harness.mouse_up(cx, seek_at(bar, 0.5));
    harness.mouse_move(cx, point(bar.center().x, bar.top() - px(100.)), None);
    harness.wait(cx, "the seek to 50 %", |state, _| (secs(state.position) - 45.).abs() < 0.3);
    harness.draw(cx);
    let knob = harness.bounds(cx, "seek-knob");
    assert_eq!(knob.size.width, px(9.));
    assert!((knob.center().x - seek_at(bar, 0.5).x).abs() < px(1.5), "the knob is centred on the played end");
    assert!((knob.center().y - bar.center().y).abs() < px(1.), "and on the track");

    harness.mouse_move(cx, seek_at(bar, 0.2), None);
    harness.draw(cx);
    assert_eq!(harness.bounds(cx, "seek-knob").size.width, px(15.), "it grows with the hovered track");
}

#[gpui_kit::test]
fn seek_bar_ignores_the_pointer_with_unknown_duration(cx: &mut TestAppContext) {
    // mpv reports a duration for every local source, a growing one for generated `lavfi` media, so the duration is
    // unknown here because nothing is loaded. The bar is not drawn before playback starts, so the press is given to
    // the seek bar's handler directly; the other seek tests cover the pointer reaching it.
    let harness = Harness::open(cx, Setup { media: Media::None, ..Setup::default() });
    assert!(!harness.drawn(cx).seekable);
    let view = harness.view();
    view.update(cx, |view, cx| view.seek_pressed(point(px(100.), px(340.)), cx));
    harness.mouse_up(cx, point(px(300.), px(340.)));
    for _ in 0..20 {
        harness.draw(cx);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(cx.read(|cx| view.read(cx).drag), None, "no drag starts");
    assert_eq!(harness.player.state().position, None, "nothing was sought");
}

// 2.3: the volume panel.

#[gpui_kit::test]
fn volume_bar_expands_on_hover(cx: &mut TestAppContext) {
    let harness = Harness::open(cx, Setup::default());
    harness.wait_playing(cx);
    assert!(harness.find(cx, "volume-bar").is_none(), "collapsed");
    harness.hover(cx, "mute");
    assert_eq!(harness.bounds(cx, "volume-bar").size.width, px(50.));
    harness.hover(cx, "seek-bar");
    assert!(harness.find(cx, "volume-bar").is_none(), "collapsed after leaving the panel");
}

#[gpui_kit::test]
fn volume_drag_sets_the_volume_and_unmutes(cx: &mut TestAppContext) {
    let harness = Harness::open(cx, Setup::default());
    harness.wait_playing(cx);
    harness.player.set_muted(true).unwrap();
    harness.wait_view(cx, "mute", |view| view.muted);
    harness.hover(cx, "mute");
    let bar = harness.bounds(cx, "volume-bar");
    harness.mouse_move(cx, along(bar, 4.5, 0.4), None);
    harness.mouse_down(cx, along(bar, 4.5, 0.4));
    harness.mouse_up(cx, along(bar, 4.5, 0.4));
    harness.wait(cx, "volume 0.4", |state, view| {
        !state.muted && (state.volume - 0.4).abs() < 0.01 && view.volume_level == 2
    });
    assert!(!harness.player.state().paused, "a click on a control is not a click on the video");
}

#[gpui_kit::test]
fn volume_drag_continues_outside_the_bar(cx: &mut TestAppContext) {
    let harness = Harness::open(cx, Setup { volume: 0.5, ..Setup::default() });
    harness.wait_playing(cx);
    harness.hover(cx, "mute");
    let bar = harness.bounds(cx, "volume-bar");
    harness.mouse_down(cx, along(bar, 4.5, 0.2));
    harness.wait(cx, "volume 0.2", |state, _| (state.volume - 0.2).abs() < 0.01);
    harness.mouse_move(cx, point(bar.right() + px(200.), bar.top() - px(50.)), Some(MouseButton::Left));
    harness.wait(cx, "volume 1", |state, _| state.volume == 1.0);
    harness.mouse_up(cx, point(bar.right() + px(200.), bar.top() - px(50.)));
}

// 2.4: pointer activity, clicks and fading.

#[gpui_kit::test]
fn video_click_toggles_play(cx: &mut TestAppContext) {
    let harness = Harness::open(cx, Setup::default());
    harness.wait_playing(cx);
    harness.click(cx, "controls");
    harness.wait_paused(cx);
    harness.click(cx, "controls");
    harness.wait_playing(cx);
}

#[gpui_kit::test]
fn control_click_is_not_a_video_click(cx: &mut TestAppContext) {
    let harness = Harness::open(cx, Setup::default());
    harness.wait_playing(cx);
    harness.click(cx, "mute");
    harness.wait_view(cx, "mute", |view| view.muted);
    harness.click(cx, "control-bar");
    harness.click(cx, "remaining-time");
    assert!(!harness.player.state().paused);
}

#[gpui_kit::test]
fn video_click_disabled(cx: &mut TestAppContext) {
    let harness = Harness::open(cx, Setup { options: options().click(false).build().unwrap(), ..Setup::default() });
    harness.wait_playing(cx);
    harness.click(cx, "controls");
    harness.draw(cx);
    assert!(!harness.player.state().paused);
}

#[gpui_kit::test]
fn hidden_bar_takes_no_click_and_movement_shows_it(cx: &mut TestAppContext) {
    let harness = Harness::open(cx, Setup::default());
    harness.wait_playing(cx);
    let mute = harness.bounds(cx, "mute").center();
    harness.idle(cx);
    assert!(!harness.drawn(cx).control_bar_visible);
    assert!(harness.find(cx, "control-bar").is_none(), "faded out");

    // A press where the mute control was, by a pointer resting there, is a click on the video.
    harness.mouse_down(cx, mute);
    harness.mouse_up(cx, mute);
    harness.wait_paused(cx);
    assert!(!harness.player.state().muted);

    // Moved off the bar, which a resting pointer keeps shown.
    harness.mouse_move(cx, point(px(100.), px(100.)), None);
    harness.player.play().unwrap();
    harness.wait_playing(cx);
    harness.idle(cx);
    assert!(!harness.drawn(cx).control_bar_visible);
    harness.mouse_move(cx, point(px(120.), px(100.)), None);
    assert!(harness.drawn(cx).control_bar_visible, "movement shows the bar");
    assert!(harness.find(cx, "control-bar").is_some());
}

#[gpui_kit::test]
fn bar_stays_visible_while_paused(cx: &mut TestAppContext) {
    let harness = Harness::open(cx, Setup::default());
    harness.wait_playing(cx);
    harness.player.pause().unwrap();
    harness.wait_paused(cx);
    harness.idle(cx);
    assert!(harness.drawn(cx).control_bar_visible);
    assert!(harness.find(cx, "control-bar").is_some());
}

// 2.5: overlays.

#[gpui_kit::test]
fn big_play_button_is_centred_and_plays(cx: &mut TestAppContext) {
    let harness = Harness::open(cx, Setup { autoplay: Autoplay::Off, ..Setup::default() });
    let view = harness.drawn(cx);
    assert!(view.big_play_button && !view.control_bar_visible);
    let button = harness.bounds(cx, "big-play");
    assert_eq!(button.size, size(px(90.), px(49.)));
    assert_eq!(button.center(), harness.bounds(cx, "controls").center());
    assert!(harness.find(cx, "control-bar").is_none(), "no bar before playback starts");

    harness.click(cx, "big-play");
    harness.wait_playing(cx);
    harness.wait_view(cx, "the button to hide", |view| !view.big_play_button);
    assert!(harness.find(cx, "big-play").is_none());
    assert!(harness.find(cx, "control-bar").is_some());
}

#[gpui_kit::test]
fn error_display(cx: &mut TestAppContext) {
    let harness = Harness::open(cx, Setup { media: Media::Broken, ..Setup::default() });
    let message = harness.drawn(cx).error.unwrap();
    assert!(!message.is_empty());
    assert_eq!(harness.bounds(cx, "error").size, size(px(640.), px(360.)));
    assert!(harness.find(cx, "control-bar").is_none() && harness.find(cx, "big-play").is_none());
}

#[gpui_kit::test]
fn spinner_is_centred(cx: &mut TestAppContext) {
    let harness = Harness::open(cx, Setup { media: Media::Wav(10), ..Setup::default() });
    harness.wait_playing(cx);
    // A seek that has not finished after the spinner's delay shows the spinner; the view's seeking start is set the
    // way the player's `Seeking` event sets it.
    harness.view().update(cx, |view, cx| {
        let now = view.now();
        view.controls.handle_event(&Event::Seeking, now);
        view.refresh(cx);
    });
    cx.executor().advance_clock(ui::SPINNER_DELAY);
    harness.draw(cx);
    if harness.drawn(cx).spinner {
        assert_eq!(harness.bounds(cx, "spinner").center(), harness.bounds(cx, "controls").center());
    } else {
        // The real `Seeked` already arrived; the spinner is then correctly gone.
        assert!(harness.find(cx, "spinner").is_none());
    }
}

#[gpui_kit::test]
fn controls_off_draws_nothing_and_ignores_input(cx: &mut TestAppContext) {
    let setup =
        Setup { autoplay: Autoplay::Off, options: options().controls(false).build().unwrap(), ..Setup::default() };
    let harness = Harness::open(cx, setup);
    for id in ["big-play", "control-bar", "play", "seek-bar", "error", "spinner"] {
        assert!(harness.find(cx, id).is_none(), "{id}");
    }
    harness.click(cx, "controls");
    harness.update(cx, |window, cx| window.double_click("controls", cx));
    harness.focus(cx, |view| view.focus.clone());
    harness.press(cx, "k");
    harness.press(cx, "f");
    harness.draw(cx);
    assert!(harness.player.state().paused, "clicks and keys do not play");
    assert!(!harness.window_fullscreen(cx));
    assert_eq!(harness.keys(cx), ["k", "f"], "keys go on to the application");
}

// 2.6: keyboard input and focus.

#[gpui_kit::test]
fn hotkey_pauses(cx: &mut TestAppContext) {
    let harness = Harness::open(cx, Setup::default());
    harness.wait_playing(cx);
    harness.focus(cx, |view| view.focus.clone());
    harness.press(cx, "k");
    harness.wait_paused(cx);
    assert!(harness.keys(cx).is_empty(), "a handled key stops at the player");
}

#[gpui_kit::test]
fn seek_bar_arrow_keys(cx: &mut TestAppContext) {
    let harness = Harness::open(cx, Setup { autoplay: Autoplay::Off, media: Media::Wav(60), ..Setup::default() });
    harness.player.play().unwrap();
    harness.wait_playing(cx);
    harness.player.pause().unwrap();
    harness.player.seek(player::Seek::To(Duration::from_secs(30))).unwrap();
    harness.wait(cx, "0:30", |state, _| state.paused && (secs(state.position) - 30.).abs() < 0.3);
    let before = secs(harness.player.state().position);
    harness.focus(cx, |view| view.seek_focus.clone());
    harness.press(cx, "right");
    harness.wait(cx, "five seconds on", |state, _| (secs(state.position) - before - 5.).abs() < 0.3);
}

#[gpui_kit::test]
fn disabled_hotkeys_reach_the_application(cx: &mut TestAppContext) {
    let setup = Setup { options: options().hotkeys(Hotkeys::disabled()).build().unwrap(), ..Setup::default() };
    let harness = Harness::open(cx, setup);
    harness.wait_playing(cx);
    harness.focus(cx, |view| view.focus.clone());
    harness.press(cx, "k");
    harness.draw(cx);
    assert!(!harness.player.state().paused);
    assert_eq!(harness.keys(cx), ["k"]);
}

#[gpui_kit::test]
fn application_shortcuts_reach_the_application(cx: &mut TestAppContext) {
    let harness = Harness::open(cx, Setup::default());
    harness.wait_playing(cx);
    harness.focus(cx, |view| view.focus.clone());
    let shortcut = if cfg!(target_os = "macos") { "cmd-f" } else { "ctrl-f" };
    harness.press(cx, shortcut);
    assert_eq!(harness.keys(cx), [shortcut]);
    assert_eq!(harness.fullscreen(cx), Fullscreen::Off);
    assert!(!harness.window_fullscreen(cx));
}

#[gpui_kit::test]
fn mute_action_mutes(cx: &mut TestAppContext) {
    let harness = Harness::open(cx, Setup::default());
    harness.wait_playing(cx);
    harness.focus(cx, |view| view.focus.clone());
    harness.update(cx, |window, cx| window.dispatch_action(Box::new(crate::ToggleMute), cx));
    harness.wait(cx, "mute", |state, _| state.muted);
}

#[gpui_kit::test]
fn play_and_fullscreen_actions(cx: &mut TestAppContext) {
    let harness = Harness::open(cx, Setup::default());
    harness.wait_playing(cx);
    harness.focus(cx, |view| view.focus.clone());
    harness.update(cx, |window, cx| window.dispatch_action(Box::new(crate::TogglePlay), cx));
    harness.wait_paused(cx);
    harness.update(cx, |window, cx| window.dispatch_action(Box::new(crate::ToggleFullscreen), cx));
    harness.draw(cx);
    assert!(harness.window_fullscreen(cx));
    assert_eq!(harness.reported(cx), [true]);
}

#[gpui_kit::test]
fn big_play_button_focuses_the_player(cx: &mut TestAppContext) {
    let harness = Harness::open(cx, Setup { autoplay: Autoplay::Off, ..Setup::default() });
    harness.click(cx, "big-play");
    harness.wait_playing(cx);
    assert!(harness.focused(cx, |view| view.focus.clone()));
    harness.press(cx, "k");
    harness.wait_paused(cx);
}

#[gpui_kit::test]
fn clicking_the_bar_focuses_the_player(cx: &mut TestAppContext) {
    let harness = Harness::open(cx, Setup::default());
    harness.wait_playing(cx);
    harness.click(cx, "remaining-time");
    assert!(harness.focused(cx, |view| view.focus.clone()));
    harness.click(cx, "mute");
    assert!(!harness.focused(cx, |view| view.focus.clone()), "a control takes focus itself");
}

#[gpui_kit::test]
fn tab_reaches_the_seek_bar(cx: &mut TestAppContext) {
    let harness = Harness::open(cx, Setup::default());
    harness.wait_playing(cx);
    harness.focus(cx, |view| view.focus.clone());
    // Control-bar order: play, mute, the volume bar (collapsed, but focusable), then the seek bar.
    for _ in 0..3 {
        harness.press(cx, "tab");
    }
    assert!(harness.focused(cx, |view| view.volume_focus.clone()), "the volume bar is third");
    assert!(harness.find(cx, "volume-bar").is_some(), "and expands while focused");
    harness.press(cx, "tab");
    assert!(harness.focused(cx, |view| view.seek_focus.clone()));
}

#[gpui_kit::test]
fn space_activates_a_focused_button(cx: &mut TestAppContext) {
    let harness = Harness::open(cx, Setup::default());
    harness.wait_playing(cx);
    harness.focus(cx, |view| view.focus.clone());
    harness.press(cx, "tab");
    harness.press(cx, "tab");
    // The mute control is focused: Space mutes, and does not also toggle play.
    harness.press(cx, "space");
    harness.wait(cx, "mute", |state, _| state.muted);
    assert!(!harness.player.state().paused);
}

// 2.7: window fullscreen.

#[gpui_kit::test]
fn fullscreen_button_and_escape(cx: &mut TestAppContext) {
    let harness = Harness::open(cx, Setup::default());
    harness.wait_playing(cx);
    harness.click(cx, "fullscreen");
    assert!(harness.window_fullscreen(cx));
    assert_eq!(harness.fullscreen(cx), Fullscreen::On);
    assert_eq!(harness.reported(cx), [true]);

    harness.press(cx, "escape");
    assert!(!harness.window_fullscreen(cx));
    assert_eq!(harness.fullscreen(cx), Fullscreen::Off);
    assert_eq!(harness.reported(cx), [true, false]);
    assert!(harness.keys(cx).is_empty(), "Escape stops at the player while it is fullscreen");

    harness.press(cx, "escape");
    assert_eq!(harness.keys(cx), ["escape"], "and goes on to the application otherwise");
}

#[gpui_kit::test]
fn double_click_and_hotkey_enter_fullscreen(cx: &mut TestAppContext) {
    let harness = Harness::open(cx, Setup::default());
    harness.wait_playing(cx);
    harness.update(cx, |window, cx| window.double_click("controls", cx));
    harness.draw(cx);
    assert!(harness.window_fullscreen(cx));
    assert_eq!(harness.reported(cx), [true]);
    // Both clicks toggled play, as in Video.js.
    harness.wait_playing(cx);

    harness.focus(cx, |view| view.focus.clone());
    harness.press(cx, "f");
    assert!(!harness.window_fullscreen(cx));
    harness.press(cx, "f");
    assert!(harness.window_fullscreen(cx));
    assert_eq!(harness.reported(cx), [true, false, true]);
}

#[gpui_kit::test]
fn leaving_by_the_window_is_reported(cx: &mut TestAppContext) {
    let harness = Harness::open(cx, Setup::default());
    harness.wait_playing(cx);
    harness.click(cx, "fullscreen");
    assert_eq!(harness.fullscreen(cx), Fullscreen::On);

    // The window's own control: the window leaves fullscreen and its bounds change.
    harness.update(cx, |window, _| window.toggle_fullscreen());
    cx.simulate_window_resize(harness.window, size(px(1024.), px(768.)));
    harness.draw(cx);
    assert_eq!(harness.fullscreen(cx), Fullscreen::Off, "the enter icon is back");
    assert_eq!(harness.reported(cx), [true, false]);
}

#[gpui_kit::test]
fn entering_in_a_fullscreen_window_leaves_it_fullscreen(cx: &mut TestAppContext) {
    let harness = Harness::open(cx, Setup::default());
    harness.wait_playing(cx);
    harness.update(cx, |window, _| window.toggle_fullscreen());
    harness.click(cx, "fullscreen");
    assert_eq!(harness.fullscreen(cx), Fullscreen::OnWithoutWindow);
    harness.click(cx, "fullscreen");
    assert_eq!(harness.fullscreen(cx), Fullscreen::Off);
    assert!(harness.window_fullscreen(cx), "the player did not make the window fullscreen");
    assert_eq!(harness.reported(cx), [true, false]);
}
