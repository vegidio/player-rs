//! The Video.js controls over the video: [`ControlsView`] drives a [`ui::Controls`] with GPUI input, and draws its
//! [`ui::View`] (see `render.rs`).

mod render;
#[cfg(test)]
mod tests;

use crate::keys;
use gpui_kit::*;
use player::Player;
use player::ui::{self, Action, Focus, Request};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Instant;

gpui_kit::actions!(
    player_gpui,
    [
        /// Plays or pauses the focused [`VideoPlayer`](crate::VideoPlayer), as its `k` hotkey does.
        TogglePlay,
        /// Mutes or unmutes the focused [`VideoPlayer`](crate::VideoPlayer), as its `m` hotkey does.
        ToggleMute,
        /// Enters or leaves fullscreen for the focused [`VideoPlayer`](crate::VideoPlayer), as its `f` hotkey does.
        ToggleFullscreen,
    ]
);

/// Told `true` when the player enters fullscreen and `false` when it leaves.
pub(crate) type FullscreenCallback = Rc<dyn Fn(bool, &mut Window, &mut App)>;

/// The player's fullscreen, which is its window's (GPUI cannot make one element fill the screen).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fullscreen {
    Off,
    /// This player asked the window to enter fullscreen, which it has not reported yet: macOS animates the
    /// transition, and `is_fullscreen` lags the toggle.
    Entering,
    /// This player made its window fullscreen.
    On,
    /// The window was already fullscreen when the player entered, so leaving does not toggle it.
    OnWithoutWindow,
}

/// A seek or volume bar drag in progress, which continues outside the bar until the button is released.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Drag {
    Seek,
    Volume,
}

/// The control bar's fade, following [`ui::View::control_bar_visible`].
#[derive(Debug, Clone, Copy, Default)]
struct Fade {
    /// Bumped on each change of visibility, to restart the animation.
    generation: usize,
    /// While fading out, when it ends. Once it has, the bar is not drawn, so it takes no pointer input.
    out_until: Option<Instant>,
}

/// Owns one player's [`ui::Controls`] and draws them. Rendered cached over the video, so video frames do not
/// re-render it; it re-renders when its [`ui::View`] changes, and for its own hover, drag and fullscreen state.
pub(crate) struct ControlsView {
    player: Player,
    controls: ui::Controls,
    /// The view last computed, which `render` draws.
    view: ui::View,
    /// The time every `ui::Controls` call is given: the executor's clock, which tests advance with `advance_clock`.
    clock: BackgroundExecutor,
    focus: FocusHandle,
    seek_focus: FocusHandle,
    volume_focus: FocusHandle,
    fullscreen: Fullscreen,
    pub(crate) on_fullscreen_change: Option<FullscreenCallback>,
    fade: Fade,
    seek_hovered: bool,
    volume_hovered: bool,
    /// A pointer button went down over the player and is not released yet.
    pressed: bool,
    drag: Option<Drag>,
    /// The bars' bounds at the last prepaint, which pointer positions are measured against.
    seek_bounds: Rc<Cell<Bounds<Pixels>>>,
    volume_bounds: Rc<Cell<Bounds<Pixels>>>,
    /// The deadline the deadline task waits for.
    armed: Option<Instant>,
    deadline: Option<Task<()>>,
    _events: Task<()>,
    _window_bounds: Subscription,
}

impl ControlsView {
    pub(crate) fn new(player: &Player, options: ui::Options, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Subscribed before the controller reads the player's state, so no event between the two is missed.
        let events = player.subscribe();
        let clock = cx.background_executor().clone();
        let now = clock.now();
        let controls = ui::Controls::new(player, options, now);
        let view = controls.view(now);

        // Player events are handled on the GUI thread, so `ui::Controls` is only ever used here.
        let events = cx.spawn(async move |this, cx| {
            while let Some(event) = events.recv_async().await {
                let handled = this.update(cx, |this, cx| {
                    this.controls.handle_event(&event, this.clock.now());
                    this.refresh(cx);
                });
                if handled.is_err() {
                    break;
                }
            }
        });
        let window_bounds = cx.observe_window_bounds(window, |this, window, cx| this.window_bounds_changed(window, cx));

        #[cfg(test)]
        tests::created(cx.entity().downgrade());
        Self {
            player: player.clone(),
            controls,
            view,
            clock,
            focus: cx.focus_handle(),
            seek_focus: cx.focus_handle().tab_stop(true),
            volume_focus: cx.focus_handle().tab_stop(true),
            fullscreen: Fullscreen::Off,
            on_fullscreen_change: None,
            fade: Fade::default(),
            seek_hovered: false,
            volume_hovered: false,
            pressed: false,
            drag: None,
            seek_bounds: Rc::default(),
            volume_bounds: Rc::default(),
            armed: None,
            deadline: None,
            _events: events,
            _window_bounds: window_bounds,
        }
    }

    pub(crate) fn player(&self) -> &Player {
        &self.player
    }

    /// The options the controls were created with.
    pub(crate) fn options(&self) -> &ui::Options {
        self.controls.options()
    }

    fn now(&self) -> Instant {
        self.clock.now()
    }

    /// Recomputes the view and re-renders if anything drawn changed. A view that differs only in its deadline is kept
    /// without re-rendering, and the deadline is re-armed.
    ///
    /// Each re-render also re-renders the views the element is in, so the seek bar's fractions are kept to the bar's
    /// resolution in pixels: the played part otherwise changes with every position update. With controls off nothing
    /// is drawn, and nothing re-renders.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let now = self.now();
        let mut view = self.controls.view(now);
        let width = f64::from(f32::from(self.seek_bounds.get().size.width));
        if width > 0. && self.drag.is_none() {
            view.played = (view.played * width).round() / width;
            view.buffered = (view.buffered * width).round() / width;
        }
        // Compared without the deadline, which changes nothing drawn.
        let deadline = std::mem::replace(&mut view.next_deadline, self.view.next_deadline);
        let changed = view != self.view;
        view.next_deadline = deadline;
        if changed && view.controls {
            if view.control_bar_visible != self.view.control_bar_visible {
                self.fade.generation += 1;
                self.fade.out_until = (!view.control_bar_visible).then_some(now + ui::FADE_OUT);
            }
            cx.notify();
        }
        self.view = view;
        self.rearm(cx);
    }

    /// Makes the deadline task wait for the earliest of the view's deadline and the end of a fade-out.
    fn rearm(&mut self, cx: &mut Context<Self>) {
        let wanted = [self.view.next_deadline, self.fade.out_until].into_iter().flatten().min();
        if wanted == self.armed {
            return;
        }
        self.armed = wanted;
        self.deadline = wanted.map(|at| {
            let timer = self.clock.timer(at.saturating_duration_since(self.now()));
            cx.spawn(async move |this, cx| {
                timer.await;
                let _ = this.update(cx, |this, cx| {
                    this.armed = None;
                    this.deadline_reached(cx);
                });
            })
        });
    }

    fn deadline_reached(&mut self, cx: &mut Context<Self>) {
        let now = self.now();
        self.controls.tick(now);
        if self.fade.out_until.is_some_and(|until| now >= until) {
            self.fade.out_until = None;
            cx.notify();
        }
        self.refresh(cx);
    }

    /// Whether the control bar is drawn: while visible, and while fading out.
    fn bar_drawn(&self) -> bool {
        self.view.control_bar_visible || self.fade.out_until.is_some()
    }

    /// Logs a request mpv rejected; there is nobody else to tell.
    fn check(result: player::Result<()>) {
        if let Err(error) = result {
            Self::warn(&error);
        }
    }

    fn warn(error: &player::Error) {
        log::warn!("player_gpui: {error}");
    }

    /// Performs `action`, as a control or an action does.
    fn perform(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        match self.controls.perform(action, self.now()) {
            Ok(Some(Request::ToggleFullscreen)) => self.toggle_fullscreen(window, cx),
            Ok(_) => {}
            Err(error) => Self::warn(&error),
        }
        self.refresh(cx);
    }

    /// An application action for the player: ignored, so it goes on to the application, with controls off.
    fn action(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        if self.controls.options().controls() {
            self.perform(action, window, cx);
        } else {
            cx.propagate();
        }
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.controls.options().controls() {
            return;
        }
        let keystroke = &event.keystroke;
        if keystroke.key == "escape" && !keystroke.modifiers.modified() && self.fullscreen != Fullscreen::Off {
            cx.stop_propagation();
            self.toggle_fullscreen(window, cx);
            return;
        }
        let Some((key, modifiers)) = keys::translate(keystroke) else { return };
        let focus = if self.seek_focus.is_focused(window) {
            Focus::SeekBar
        } else if self.volume_focus.is_focused(window) {
            Focus::VolumeBar
        } else {
            Focus::Player
        };
        match self.controls.key(key, modifiers, focus, self.now()) {
            Ok(outcome) => {
                if outcome.handled {
                    cx.stop_propagation();
                }
                if outcome.request == Some(Request::ToggleFullscreen) {
                    self.toggle_fullscreen(window, cx);
                }
            }
            Err(error) => {
                cx.stop_propagation();
                Self::warn(&error);
            }
        }
        self.refresh(cx);
    }

    fn pointer_moved(&mut self, cx: &mut Context<Self>) {
        self.controls.pointer_moved(self.now());
        self.refresh(cx);
    }

    fn pointer_hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if hovered {
            self.controls.pointer_moved(self.now());
        } else {
            self.controls.pointer_left(self.now());
        }
        self.refresh(cx);
    }

    fn pointer_pressed(&mut self, cx: &mut Context<Self>) {
        self.pressed = true;
        self.controls.pointer_pressed(self.now());
        self.refresh(cx);
    }

    fn bar_hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        self.controls.control_bar_hovered(hovered, self.now());
        self.refresh(cx);
    }

    /// A click on the video area, outside every control. Keyboard "clicks" (Space or Enter on the focused player) are
    /// keys, which the hotkeys handle.
    fn video_clicked(&mut self, event: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.is_keyboard() || !event.standard_click() {
            return;
        }
        let now = self.now();
        Self::check(self.controls.video_clicked(now));
        // As in Video.js, both clicks of a double-click toggle play, and the second also toggles fullscreen.
        if event.click_count() == 2 && self.controls.video_double_clicked(now) == Some(Request::ToggleFullscreen) {
            self.toggle_fullscreen(window, cx);
        }
        self.refresh(cx);
    }

    fn fraction(x: Pixels, bounds: Bounds<Pixels>) -> f64 {
        let (x, left, width) = (f32::from(x), f32::from(bounds.left()), f32::from(bounds.size.width));
        ui::bars::fraction(f64::from(x), f64::from(left), f64::from(width))
    }

    fn seek_fraction(&self, x: Pixels) -> f64 {
        Self::fraction(x, self.seek_bounds.get())
    }

    fn volume_fraction(&self, x: Pixels) -> f64 {
        Self::fraction(x, self.volume_bounds.get())
    }

    fn seek_pressed(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        if self.view.seekable {
            self.drag = Some(Drag::Seek);
            let fraction = self.seek_fraction(position.x);
            Self::check(self.controls.seek_press(fraction, self.now()));
        }
        self.refresh(cx);
    }

    fn seek_hover(&mut self, position: Option<Point<Pixels>>, cx: &mut Context<Self>) {
        let fraction = position.map(|position| self.seek_fraction(position.x));
        if self.seek_hovered != fraction.is_some() {
            self.seek_hovered = fraction.is_some();
            cx.notify();
        }
        self.controls.seek_hover(fraction);
        self.refresh(cx);
    }

    fn volume_pressed(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        self.drag = Some(Drag::Volume);
        let fraction = self.volume_fraction(position.x);
        Self::check(self.controls.volume_drag(fraction, self.now()));
        self.refresh(cx);
    }

    fn volume_panel_hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        self.volume_hovered = hovered;
        cx.notify();
    }

    /// The pointer moved anywhere in the window, from the window-level listener: continues a drag, wherever it is.
    fn window_mouse_moved(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let now = self.now();
        match self.drag {
            Some(Drag::Seek) => {
                let fraction = self.seek_fraction(event.position.x);
                Self::check(self.controls.seek_drag(fraction, now));
            }
            Some(Drag::Volume) => {
                let fraction = self.volume_fraction(event.position.x);
                Self::check(self.controls.volume_drag(fraction, now));
            }
            None => return,
        }
        self.refresh(cx);
    }

    /// A pointer button went up anywhere in the window: ends a drag and a press. Other releases are not the player's.
    fn window_mouse_up(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        let now = self.now();
        let drag = self.drag.take();
        if drag == Some(Drag::Seek) {
            let fraction = self.seek_fraction(event.position.x);
            Self::check(self.controls.seek_release(fraction, now));
        }
        let pressed = std::mem::take(&mut self.pressed);
        if pressed {
            self.controls.pointer_released(now);
        }
        if drag.is_some() {
            // What is drawn depends on the drag: the volume bar is held expanded, and the seek fractions unrounded.
            cx.notify();
        }
        if drag.is_some() || pressed {
            self.refresh(cx);
        }
    }

    /// Enters fullscreen, or leaves it while the player is fullscreen, and tells the application.
    fn toggle_fullscreen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let entering = self.fullscreen == Fullscreen::Off;
        self.fullscreen = match self.fullscreen {
            Fullscreen::Off if window.is_fullscreen() => Fullscreen::OnWithoutWindow,
            Fullscreen::Off => {
                window.toggle_fullscreen();
                // Platforms that switch at once report it now; macOS reports it through the bounds observer.
                if window.is_fullscreen() { Fullscreen::On } else { Fullscreen::Entering }
            }
            Fullscreen::Entering | Fullscreen::On => {
                window.toggle_fullscreen();
                Fullscreen::Off
            }
            Fullscreen::OnWithoutWindow => Fullscreen::Off,
        };
        self.report_fullscreen(entering, window, cx);
        cx.notify();
    }

    /// Follows the window's fullscreen, which also changes without the player: the window's own control, a menu, or
    /// the app.
    fn window_bounds_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match (self.fullscreen, window.is_fullscreen()) {
            (Fullscreen::Entering, true) => self.fullscreen = Fullscreen::On,
            (Fullscreen::On | Fullscreen::OnWithoutWindow, false) => {
                self.fullscreen = Fullscreen::Off;
                self.report_fullscreen(false, window, cx);
                cx.notify();
            }
            _ => {}
        }
    }

    fn report_fullscreen(&self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(callback) = self.on_fullscreen_change.clone() {
            callback(on, window, cx);
        }
    }
}
