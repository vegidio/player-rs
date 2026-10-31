//! [`Controls`], the controller a frontend view owns per player, and the [`View`] it describes.

use crate::error::Result;
use crate::event::{EndReason, Event, State};
use crate::player::{Player, Seek};
use crate::ui::action::{Action, Request};
use crate::ui::activity::Activity;
use crate::ui::keys::{self, Focus, Key, Modifiers};
use crate::ui::options::Options;
use crate::ui::time::{format_rate, format_remaining, format_time};
use crate::ui::{SPINNER_DELAY, bars};
use std::time::{Duration, Instant};

/// The Video.js interaction rules for one player view: input and events in, a [`View`] out.
///
/// A frontend creates one per player view, forwards every [`Event`] from its own subscription to
/// [`handle_event`](Self::handle_event), reports input through the other methods, and draws
/// [`view`](Self::view). See the [module docs](super) for the loop.
///
/// Methods that act on the player return the player's error if mpv rejects the request. Every method takes the
/// current time, which is all the layer knows of the clock.
#[derive(Debug)]
pub struct Controls {
    player: Player,
    options: Options,
    activity: Activity,
    /// Playback has started for the current media, Video.js's `vjs-has-started`.
    started: bool,
    seeking_since: Option<Instant>,
    buffering_since: Option<Instant>,
    error: Option<String>,
    /// The last non-zero volume, which the mute control restores from volume 0.
    last_volume: f64,
    drag: Option<Drag>,
    /// The fraction of the seek bar under the pointer.
    hover: Option<f64>,
}

/// A seek-bar drag in progress.
#[derive(Debug, Clone, Copy)]
struct Drag {
    fraction: f64,
    /// Playback resumes on release.
    was_playing: bool,
}

/// What [`Controls::key`] did with a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyOutcome {
    /// The key did something. An unhandled key should go on to the application.
    pub handled: bool,
    /// Something the frontend must do for the key, such as entering fullscreen.
    pub request: Option<Request>,
}

/// The play control's icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayIcon {
    /// Paused: activating the control plays.
    Play,
    /// Playing: activating the control pauses.
    Pause,
    /// The media played to its end: activating the control restarts it.
    Replay,
}

/// The seek bar's hover preview.
#[derive(Debug, Clone, PartialEq)]
pub struct SeekHover {
    /// Where the pointer is along the seek bar, from 0 to 1.
    pub fraction: f64,
    /// The time at that point, as [`format_time`] writes it.
    pub label: String,
}

/// Everything a frontend draws for one player, from [`Controls::view`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct View {
    /// Controls are enabled ([`Options::controls`]). When they are not, no control is drawn.
    pub controls: bool,
    /// The control bar is shown. Fade it in over [`FADE_IN`](super::FADE_IN) and out over
    /// [`FADE_OUT`](super::FADE_OUT).
    pub control_bar_visible: bool,
    /// The pointer cursor is hidden over the player.
    pub cursor_hidden: bool,
    /// The big play button is shown; activating it performs [`Action::Play`].
    pub big_play_button: bool,
    /// The poster to show over the video, before playback starts.
    pub poster: Option<String>,
    /// The loading spinner is shown.
    pub spinner: bool,
    /// The media failed to load, with mpv's message.
    pub error: Option<String>,
    /// The play control's icon.
    pub play_icon: PlayIcon,
    /// The volume bar's fraction: the volume, or 0 while muted.
    pub volume_fraction: f64,
    /// The mute control's icon level, from 0 (muted) to 3 (loud).
    pub volume_level: u8,
    /// The audio is muted.
    pub muted: bool,
    /// The current time label.
    pub current_time: String,
    /// The duration label.
    pub duration: String,
    /// The remaining time label, such as `-1:18`.
    pub remaining: String,
    /// The duration is known, so the seek bar takes pointer input.
    pub seekable: bool,
    /// The seek bar's played fraction. It follows the pointer during a drag.
    pub played: f64,
    /// The seek bar's buffered fraction.
    pub buffered: f64,
    /// The seek bar's hover preview, while the pointer is over it.
    pub hover: Option<SeekHover>,
    /// The rate control's label, such as `1.5x`; `None` when no playback rates are configured and the control is
    /// not offered.
    pub rate: Option<String>,
    /// When the view next changes without input or events; call [`Controls::tick`] and redraw then. `None` when
    /// nothing is pending.
    pub next_deadline: Option<Instant>,
}

impl Controls {
    /// A controller for `player` with `options`, created at `now`, which counts as user activity. The player may
    /// already have media loaded.
    pub fn new(player: &Player, options: Options, now: Instant) -> Self {
        let state = player.state();
        let started = !state.idle && (!state.paused || state.position.is_some_and(|p| !p.is_zero()));
        Self {
            player: player.clone(),
            activity: Activity::new(options.inactivity_timeout(), now),
            options,
            started,
            seeking_since: None,
            buffering_since: None,
            error: None,
            last_volume: state.volume,
            drag: None,
            hover: None,
        }
    }

    /// The options this controller was created with.
    pub fn options(&self) -> &Options {
        &self.options
    }

    /// Takes one of the player's events. Forward every event from the frontend's subscription, including those the
    /// frontend does not use itself, or the view drifts from the player's state.
    pub fn handle_event(&mut self, event: &Event, now: Instant) {
        match event {
            Event::Loaded => {
                // Autoplay unpauses before loading, so no `PausedChanged(false)` follows.
                self.started = !self.player.state().paused;
                self.error = None;
                self.drag = None;
            }
            Event::PausedChanged(false) if !self.player.state().idle => self.started = true,
            Event::Ended(EndReason::Replaced) => {
                self.started = false;
                self.drag = None;
            }
            Event::Ended(EndReason::Error(message)) => self.error = Some(message.clone()),
            Event::Idle => {
                self.started = false;
                self.seeking_since = None;
                self.buffering_since = None;
                self.drag = None;
            }
            Event::VolumeChanged(volume) if *volume > 0.0 => self.last_volume = *volume,
            Event::BufferingChanged(true) => self.buffering_since = self.buffering_since.or(Some(now)),
            Event::BufferingChanged(false) => self.buffering_since = None,
            Event::Seeking => self.seeking_since = self.seeking_since.or(Some(now)),
            Event::Seeked => self.seeking_since = None,
            _ => {}
        }
    }

    /// Updates the controller at [`View::next_deadline`]. Its state already follows from the time each method is
    /// given, so this only records the time; what matters is redrawing [`view`](Self::view) then.
    pub fn tick(&mut self, now: Instant) {
        let _ = now;
    }

    /// Performs `action` on the player, and returns what the frontend must do for it, if anything. Counts as user
    /// activity.
    ///
    /// # Errors
    ///
    /// Returns the player's error if mpv rejects the request.
    pub fn perform(&mut self, action: Action, now: Instant) -> Result<Option<Request>> {
        self.activity.input(now);
        let player = &self.player;
        match action {
            Action::Play => player.play()?,
            Action::Pause => player.pause()?,
            Action::TogglePlay => player.toggle_pause()?,
            Action::SeekTo(position) => player.seek(Seek::To(position))?,
            Action::SeekBy(seconds) => player.seek(Seek::By(seconds))?,
            Action::ToggleMute => {
                let state = player.state();
                if state.volume <= 0.0 {
                    player.set_volume(bars::restored_volume(self.last_volume))?;
                    player.set_muted(false)?;
                } else {
                    player.set_muted(!state.muted)?;
                }
            }
            Action::SetVolume(volume) => self.set_volume(volume)?,
            Action::StepVolume(step) => self.set_volume(self.player.state().volume + step)?,
            Action::SetRate(rate) => player.set_rate(rate)?,
            Action::CycleRate => {
                if let Some(rate) = bars::next_rate(self.options.playback_rates(), player.state().rate) {
                    player.set_rate(rate)?;
                }
            }
            Action::ToggleFullscreen => return Ok(Some(Request::ToggleFullscreen)),
        }
        Ok(None)
    }

    /// Unmutes, as Video.js's volume bar does, then sets the volume.
    fn set_volume(&self, volume: f64) -> Result<()> {
        if self.player.state().muted {
            self.player.set_muted(false)?;
        }
        self.player.set_volume(volume)
    }

    /// Takes a key pressed while `focus` is in the player, and performs what [`keys::map`] maps it to. Any key counts
    /// as user activity, handled or not.
    ///
    /// # Errors
    ///
    /// Returns the player's error if mpv rejects the request.
    pub fn key(&mut self, key: Key, modifiers: Modifiers, focus: Focus, now: Instant) -> Result<KeyOutcome> {
        self.activity.input(now);
        let duration = self.player.state().duration;
        match keys::map(key, modifiers, focus, self.options.hotkeys(), duration) {
            Some(action) => Ok(KeyOutcome { handled: true, request: self.perform(action, now)? }),
            None => Ok(KeyOutcome { handled: false, request: None }),
        }
    }

    /// The pointer moved over the player.
    pub fn pointer_moved(&mut self, now: Instant) {
        self.activity.input(now);
    }

    /// A pointer button went down over the player. The user stays active until it is released.
    pub fn pointer_pressed(&mut self, now: Instant) {
        self.activity.press(now);
    }

    /// A pointer button went up.
    pub fn pointer_released(&mut self, now: Instant) {
        self.activity.release(now);
    }

    /// The pointer left the player. This also ends a hold and the control-bar hover.
    pub fn pointer_left(&mut self, now: Instant) {
        self.activity.leave(now);
        self.hover = None;
    }

    /// The pointer entered (`true`) or left the control bar. The user stays active while it is over it.
    pub fn control_bar_hovered(&mut self, hovered: bool, now: Instant) {
        self.activity.bar_hover(hovered, now);
    }

    /// The video was clicked: toggles play, unless controls are hidden or [`Options::click`] is off.
    ///
    /// # Errors
    ///
    /// Returns the player's error if mpv rejects the request.
    pub fn video_clicked(&mut self, now: Instant) -> Result<()> {
        self.activity.input(now);
        if self.options.controls() && self.options.click() {
            self.perform(Action::TogglePlay, now)?;
        }
        Ok(())
    }

    /// The video was double-clicked: requests fullscreen, unless controls are hidden or [`Options::double_click`] is
    /// off. As in Video.js, the double-click's two clicks should still be reported to
    /// [`video_clicked`](Self::video_clicked), so they toggle play twice.
    pub fn video_double_clicked(&mut self, now: Instant) -> Option<Request> {
        self.activity.input(now);
        (self.options.controls() && self.options.double_click()).then_some(Request::ToggleFullscreen)
    }

    /// The pointer is over the seek bar at `fraction` (from [`bars::fraction`]), or left it (`None`).
    pub fn seek_hover(&mut self, fraction: Option<f64>) {
        self.hover = fraction;
    }

    /// The seek bar was pressed at `fraction`: pauses playing media and seeks quickly to that point. Does nothing
    /// while the duration is unknown.
    ///
    /// # Errors
    ///
    /// Returns the player's error if mpv rejects the request.
    pub fn seek_press(&mut self, fraction: f64, now: Instant) -> Result<()> {
        self.activity.input(now);
        let state = self.player.state();
        let Some(duration) = seekable(&state) else { return Ok(()) };
        let was_playing = playing(&state);
        if was_playing {
            self.player.pause()?;
        }
        self.drag = Some(Drag { fraction, was_playing });
        self.player.scrub(bars::time_at(fraction, duration))
    }

    /// The pointer moved to `fraction` during a seek-bar drag: seeks quickly there. The played fraction follows the
    /// pointer rather than the decoded position until the drag ends.
    ///
    /// # Errors
    ///
    /// Returns the player's error if mpv rejects the request.
    pub fn seek_drag(&mut self, fraction: f64, now: Instant) -> Result<()> {
        self.activity.input(now);
        let (Some(drag), Some(duration)) = (self.drag.as_mut(), seekable(&self.player.state())) else {
            return Ok(());
        };
        drag.fraction = fraction;
        self.player.scrub(bars::time_at(fraction, duration))
    }

    /// The seek bar was released at `fraction`: seeks exactly there while the duration is known, then resumes playback
    /// if the media was playing when the drag began. Loading other media ends a drag, so a release after that does
    /// nothing.
    ///
    /// # Errors
    ///
    /// Returns the player's error if mpv rejects the request.
    pub fn seek_release(&mut self, fraction: f64, now: Instant) -> Result<()> {
        self.activity.input(now);
        let Some(drag) = self.drag.take() else { return Ok(()) };
        if let Some(duration) = seekable(&self.player.state()) {
            self.player.seek(Seek::To(bars::time_at(fraction, duration)))?;
        }
        if drag.was_playing {
            self.player.play()?;
        }
        Ok(())
    }

    /// The volume bar was pressed or dragged to `fraction`: sets that volume and unmutes.
    ///
    /// # Errors
    ///
    /// Returns the player's error if mpv rejects the request.
    pub fn volume_drag(&mut self, fraction: f64, now: Instant) -> Result<()> {
        self.perform(Action::SetVolume(fraction.clamp(0.0, 1.0)), now).map(|_| ())
    }

    /// Everything to draw at `now`. Cheap: it reads the player's state snapshot and never waits on mpv.
    pub fn view(&self, now: Instant) -> View {
        let state = self.player.state();
        let active = self.activity.active(now);
        let playing = playing(&state);
        let controls = self.options.controls();
        let duration = seekable(&state);
        let not_started = !self.started;

        let waiting_since = [self.buffering_since, self.seeking_since].into_iter().flatten().min();
        let spinner_at = waiting_since.map(|since| since + SPINNER_DELAY);
        let activity_deadline = self.activity.deadline().filter(|_| active);
        let next_deadline =
            [spinner_at, activity_deadline].into_iter().flatten().filter(|deadline| *deadline > now).min();

        View {
            controls,
            control_bar_visible: control_bar_visible(controls, self.error.is_some(), self.started, active, playing),
            cursor_hidden: !active && playing,
            big_play_button: controls && not_started && self.error.is_none(),
            poster: self.options.poster().filter(|_| not_started).map(str::to_owned),
            spinner: self.error.is_none() && spinner_at.is_some_and(|at| now >= at),
            error: self.error.clone(),
            play_icon: if state.ended {
                PlayIcon::Replay
            } else if state.paused || state.idle {
                PlayIcon::Play
            } else {
                PlayIcon::Pause
            },
            volume_fraction: bars::volume_fraction(state.volume, state.muted),
            volume_level: bars::volume_level(state.volume, state.muted),
            muted: state.muted,
            current_time: format_time(state.position, state.duration),
            duration: format_time(state.duration, state.duration),
            remaining: format_remaining(state.position, state.duration),
            seekable: duration.is_some(),
            played: match (self.drag, duration) {
                (_, None) => 0.0,
                (Some(drag), Some(_)) => drag.fraction.clamp(0.0, 1.0),
                (None, Some(_)) => bars::played(state.position, state.duration),
            },
            buffered: bars::buffered(state.buffered, state.duration),
            hover: self.hover.zip(duration).map(|(fraction, duration)| SeekHover {
                fraction: fraction.clamp(0.0, 1.0),
                label: format_time(Some(bars::time_at(fraction, duration)), Some(duration)),
            }),
            rate: (!self.options.playback_rates().is_empty()).then(|| format_rate(state.rate)),
            next_deadline,
        }
    }
}

/// Media is loaded, not paused and not at its end.
fn playing(state: &State) -> bool {
    !state.idle && !state.paused && !state.ended
}

/// The duration, if media is loaded and its duration is known and not zero.
fn seekable(state: &State) -> Option<Duration> {
    state.duration.filter(|d| !state.idle && !d.is_zero())
}

/// Video.js's control-bar visibility: hidden with controls off, on error, before playback starts, and while playing
/// with the user inactive.
fn control_bar_visible(controls: bool, error: bool, started: bool, active: bool, playing: bool) -> bool {
    controls && !error && started && (active || !playing)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::VideoOutput;

    fn player() -> Player {
        Player::builder().video_output(VideoOutput::Null).mpv_option("ao", "null").build().unwrap()
    }

    #[test]
    fn control_bar_visibility() {
        // (controls, error, started, active, playing)
        assert!(control_bar_visible(true, false, true, true, true), "active while playing");
        assert!(control_bar_visible(true, false, true, false, false), "paused");
        assert!(!control_bar_visible(true, false, true, false, true), "inactive while playing");
        assert!(!control_bar_visible(true, false, false, true, false), "not started");
        assert!(!control_bar_visible(true, true, true, true, false), "error");
        assert!(!control_bar_visible(false, false, true, true, false), "controls off");
    }

    #[test]
    fn idle_view() {
        let player = player();
        let poster = Options::builder().poster("poster.jpg").build().unwrap();
        let controls = Controls::new(&player, poster, Instant::now());
        let now = Instant::now();
        let view = controls.view(now);
        assert!(view.controls);
        assert!(!view.control_bar_visible, "not started");
        assert!(!view.cursor_hidden);
        assert!(view.big_play_button);
        assert_eq!(view.poster.as_deref(), Some("poster.jpg"));
        assert!(!view.spinner);
        assert_eq!(view.play_icon, PlayIcon::Play);
        assert_eq!((view.current_time.as_str(), view.duration.as_str()), ("-:-", "-:-"));
        assert!(!view.seekable);
        assert_eq!((view.played, view.buffered), (0.0, 0.0));
        assert_eq!(view.rate, None);
        assert_eq!(view.volume_level, 3);
    }

    #[test]
    fn controls_off_view() {
        let player = player();
        let options = Options::builder().controls(false).playback_rates([1.0, 2.0]).build().unwrap();
        let mut controls = Controls::new(&player, options, Instant::now());
        controls.started = true;
        let view = controls.view(Instant::now());
        assert!(!view.controls && !view.control_bar_visible && !view.big_play_button);
        assert_eq!(view.rate.as_deref(), Some("1x"));
    }

    #[test]
    fn error_view() {
        let player = player();
        let mut controls = Controls::new(&player, Options::default(), Instant::now());
        let now = Instant::now();
        controls.handle_event(&Event::Ended(EndReason::Error("loading failed".into())), now);
        let view = controls.view(now);
        assert_eq!(view.error.as_deref(), Some("loading failed"));
        assert!(!view.control_bar_visible && !view.big_play_button && !view.spinner);
    }

    #[test]
    fn spinner_waits() {
        let player = player();
        let mut controls = Controls::new(&player, Options::default(), Instant::now());
        let start = Instant::now();
        controls.handle_event(&Event::Seeking, start);
        assert!(!controls.view(start + Duration::from_millis(299)).spinner);
        assert_eq!(controls.view(start).next_deadline, Some(start + SPINNER_DELAY));
        assert!(controls.view(start + SPINNER_DELAY).spinner);
        controls.handle_event(&Event::Seeked, start + Duration::from_secs(1));
        assert!(!controls.view(start + Duration::from_secs(1)).spinner);
    }

    #[test]
    fn unknown_duration_is_not_seekable() {
        let state = |idle, duration| State { idle, duration, ..player().state() };
        assert_eq!(seekable(&state(false, None)), None, "loaded, duration unknown");
        assert_eq!(seekable(&state(false, Some(Duration::ZERO))), None);
        assert_eq!(seekable(&state(true, Some(Duration::from_secs(2)))), None);
        assert_eq!(seekable(&state(false, Some(Duration::from_secs(2)))), Some(Duration::from_secs(2)));
    }

    #[test]
    fn controls_are_send() {
        fn assert_send<T: Send>() {}
        assert_send::<Controls>();
    }
}
