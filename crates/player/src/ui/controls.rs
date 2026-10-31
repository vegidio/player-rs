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
    policy: BarPolicy,
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
    time_display: TimeDisplay,
    /// The Settings menu, while it is open.
    menu: Option<Menu>,
}

/// The open Settings menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Menu {
    page: Page,
    /// The index of the entry with keyboard focus.
    focused: usize,
}

/// An item of the Settings menu's root page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Item {
    Speed,
}

impl Item {
    /// The page the item opens.
    fn page(self) -> Page {
        match self {
            Self::Speed => Page::Speed,
        }
    }
}

/// When the control bar shows besides Video.js's rules, chosen by the frontend for its skin, as a property of the
/// skin's design. The default, both off, is Video.js's: the bar is hidden before playback first starts and while an
/// error is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BarPolicy {
    /// Show the control bar before playback first starts, for a skin without a big play button.
    pub before_start: bool,
    /// Show the control bar, with its controls disabled, while the media's error is shown.
    pub on_error: bool,
}

/// What the time display shows. Clicking it switches between the two, unlike Video.js, which shows them in separate
/// fixed components.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimeDisplay {
    /// The remaining time, such as `-1:18`.
    #[default]
    Remaining,
    /// The duration, such as `2:00`.
    Duration,
}

impl TimeDisplay {
    fn toggled(self) -> Self {
        match self {
            Self::Remaining => Self::Duration,
            Self::Duration => Self::Remaining,
        }
    }
}

/// A page of the Settings menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Page {
    /// The root page, listing the shown Settings items with their current values.
    Root,
    /// The Speed item's page, listing the playback rates.
    Speed,
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

/// Which parts of the controls are shown: visible in the [`Parts`](super::Parts) options and available. Parts with
/// content of their own are reported in their [`View`] field instead: the big play button, the spinner, the seek
/// hover, the rate and the Settings button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Shown {
    /// The play control.
    pub play: bool,
    /// The volume panel: the mute control and the volume bar.
    pub volume: bool,
    /// The seek bar.
    pub seek_bar: bool,
    /// The current time.
    pub current_time: bool,
    /// The time display.
    pub time_display: bool,
    /// The fullscreen control.
    pub fullscreen: bool,
    /// The error display: the media failed to load, and the part is visible.
    pub error_display: bool,
}

/// The Settings button and its menu.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Settings {
    /// The Settings button is shown: at least one Settings item is. Activating it performs
    /// [`Action::ToggleSettings`].
    pub button: bool,
    /// The menu, while it is open.
    pub menu: Option<SettingsMenu>,
}

/// The open Settings menu.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct SettingsMenu {
    /// The page shown.
    pub page: Page,
    /// The page's entries, in order. Activating one performs [`Action::Select`] with its index.
    pub items: Vec<MenuItem>,
    /// The index of the entry with keyboard focus.
    pub focused: usize,
}

/// An entry of the Settings menu.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct MenuItem {
    /// The entry's label, such as `Speed` on the root page or `1.5x` on the Speed page.
    pub label: String,
    /// On the root page, the item's current value, such as `1.5x`.
    pub value: Option<String>,
    /// On an item's page, the entry is the current value.
    pub checked: bool,
    /// The entry has keyboard focus.
    pub focused: bool,
}

/// Everything a frontend draws for one player, from [`Controls::view`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct View {
    /// The control bar is shown. Fade it in over [`FADE_IN`](super::FADE_IN) and out over
    /// [`FADE_OUT`](super::FADE_OUT).
    pub control_bar_visible: bool,
    /// The controls take input. False while the media's error is shown, when every action but
    /// [`Action::ToggleFullscreen`] and [`Action::Retry`] does nothing; draw them disabled.
    pub controls_enabled: bool,
    /// Which parts of the controls are shown.
    pub shown: Shown,
    /// The pointer cursor is hidden over the player.
    pub cursor_hidden: bool,
    /// The big play button is shown; activating it performs [`Action::Play`].
    pub big_play_button: bool,
    /// The poster to show over the video, before playback starts.
    pub poster: Option<String>,
    /// The loading spinner is shown.
    pub spinner: bool,
    /// The media failed to load, with mpv's message, whether or not the error display is shown.
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
    /// What the time display shows; activating it performs [`Action::ToggleTimeDisplay`].
    pub time_display: TimeDisplay,
    /// The time display's label: [`remaining`](Self::remaining) or [`duration`](Self::duration).
    pub time_label: String,
    /// The duration is known, so the seek bar takes pointer input.
    pub seekable: bool,
    /// The seek bar's played fraction. It follows the pointer during a drag.
    pub played: f64,
    /// The seek bar's buffered fraction.
    pub buffered: f64,
    /// The seek bar's hover preview, while the pointer is over it and the hover part is visible.
    pub hover: Option<SeekHover>,
    /// The current rate's label, such as `1.5x`, while the Speed item is offered: playback rates are configured and
    /// the Speed part is visible.
    pub rate: Option<String>,
    /// The Settings button and its menu.
    pub settings: Settings,
    /// When the view next changes without input or events; call [`Controls::tick`] and redraw then. `None` when
    /// nothing is pending.
    pub next_deadline: Option<Instant>,
}

impl Controls {
    /// A controller for `player` with `options` and the frontend's control-bar `policy`, created at `now`, which
    /// counts as user activity. The player may already have media loaded.
    pub fn new(player: &Player, options: Options, policy: BarPolicy, now: Instant) -> Self {
        let state = player.state();
        let started = !state.idle && (!state.paused || state.position.is_some_and(|p| !p.is_zero()));
        Self {
            player: player.clone(),
            activity: Activity::new(options.inactivity_timeout(), now),
            options,
            policy,
            started,
            seeking_since: None,
            buffering_since: None,
            error: None,
            last_volume: state.volume,
            drag: None,
            hover: None,
            time_display: TimeDisplay::default(),
            menu: None,
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
                self.set_menu(None, now);
            }
            Event::PausedChanged(false) if !self.player.state().idle => self.started = true,
            Event::Ended(EndReason::Replaced) => {
                self.started = false;
                self.drag = None;
                self.set_menu(None, now);
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
        self.close_hidden_menu(now);
    }

    /// Updates the controller at [`View::next_deadline`]. Its state otherwise follows from the time each method is
    /// given; what matters is redrawing [`view`](Self::view) then.
    pub fn tick(&mut self, now: Instant) {
        self.close_hidden_menu(now);
    }

    /// Performs `action` on the player, and returns what the frontend must do for it, if anything. Counts as user
    /// activity. While the media's error is shown, only [`Action::ToggleFullscreen`] and [`Action::Retry`] do
    /// anything.
    ///
    /// # Errors
    ///
    /// Returns the player's error if mpv rejects the request.
    pub fn perform(&mut self, action: Action, now: Instant) -> Result<Option<Request>> {
        self.activity.input(now);
        if self.error.is_some() && !matches!(action, Action::ToggleFullscreen | Action::Retry) {
            return Ok(None);
        }
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
            Action::Retry => {
                if player.reload_last()? {
                    player.play()?;
                }
            }
            Action::ToggleTimeDisplay => self.time_display = self.time_display.toggled(),
            Action::ToggleSettings => match self.menu {
                Some(_) => self.set_menu(None, now),
                None => self.open_page(Page::Root, 0, now),
            },
            Action::CloseSettings => self.set_menu(None, now),
            Action::OpenPage(page) => self.open_page(page, self.default_focus(page), now),
            Action::Back => {
                if let Some(Menu { page: page @ Page::Speed, .. }) = self.menu {
                    self.back_from(page, now);
                }
            }
            Action::Select(index) => self.select(index, now)?,
            Action::FocusNext => self.move_focus(1),
            Action::FocusPrevious => self.move_focus(-1),
            Action::ActivateFocused => {
                if let Some(menu) = self.menu {
                    self.select(menu.focused, now)?;
                }
            }
        }
        Ok(None)
    }

    /// The Speed item is offered: rates are configured and the Speed part is visible.
    fn speed_shown(&self) -> bool {
        self.options.parts().speed && !self.options.playback_rates().is_empty()
    }

    /// The items of the Settings menu's root page that are shown, in order.
    fn root_items(&self) -> Vec<Item> {
        self.speed_shown().then_some(Item::Speed).into_iter().collect()
    }

    /// The Settings button is shown: at least one Settings item is.
    fn settings_shown(&self) -> bool {
        !self.root_items().is_empty()
    }

    /// Whether `page` can be opened: the root page while the Settings button is shown, an item's page while the
    /// item is.
    fn page_shown(&self, page: Page) -> bool {
        match page {
            Page::Root => self.settings_shown(),
            page => self.root_items().iter().any(|item| item.page() == page),
        }
    }

    /// The number of entries on `page`.
    fn entries(&self, page: Page) -> usize {
        match page {
            Page::Root => self.root_items().len(),
            Page::Speed => self.options.playback_rates().len(),
        }
    }

    /// The entry focused when `page` opens: an item's current value, or the first entry.
    fn default_focus(&self, page: Page) -> usize {
        match page {
            Page::Root => 0,
            Page::Speed => bars::rate_index(self.options.playback_rates(), self.player.state().rate).unwrap_or(0),
        }
    }

    /// Opens or closes the Settings menu, holding the user active while it is open.
    fn set_menu(&mut self, menu: Option<Menu>, now: Instant) {
        if self.menu.is_some() || menu.is_some() {
            self.menu = menu;
            self.activity.menu(menu.is_some(), now);
        }
    }

    /// Opens the Settings menu on `page` with `focused` focused, if the page can be opened.
    fn open_page(&mut self, page: Page, focused: usize, now: Instant) {
        if self.page_shown(page) {
            self.set_menu(Some(Menu { page, focused }), now);
        }
    }

    /// Returns from `page` to the root page, focused on the item that opened it.
    fn back_from(&mut self, page: Page, now: Instant) {
        let focused = self.root_items().iter().position(|item| item.page() == page).unwrap_or(0);
        self.open_page(Page::Root, focused, now);
    }

    /// Activates the entry at `index` of the open menu's page.
    fn select(&mut self, index: usize, now: Instant) -> Result<()> {
        let Some(menu) = self.menu else { return Ok(()) };
        match menu.page {
            Page::Root => {
                if let Some(page) = self.root_items().get(index).map(|item| item.page()) {
                    self.open_page(page, self.default_focus(page), now);
                }
            }
            Page::Speed => {
                if let Some(rate) = self.options.playback_rates().get(index).copied() {
                    self.player.set_rate(rate)?;
                    self.back_from(Page::Speed, now);
                }
            }
        }
        Ok(())
    }

    /// Moves the open menu's focus by `step` entries, wrapping around.
    fn move_focus(&mut self, step: isize) {
        let Some(menu) = self.menu else { return };
        let entries = self.entries(menu.page) as isize;
        if entries > 0 {
            let focused = (menu.focused as isize + step).rem_euclid(entries) as usize;
            self.menu = Some(Menu { focused, ..menu });
        }
    }

    /// Closes the Settings menu once the control bar or the Settings button is hidden.
    fn close_hidden_menu(&mut self, now: Instant) {
        if self.menu.is_some() && !(self.settings_shown() && self.bar_visible(&self.player.state(), now)) {
            self.set_menu(None, now);
        }
    }

    fn bar_visible(&self, state: &State, now: Instant) -> bool {
        let shown = BarState {
            error: self.error.is_some(),
            started: self.started,
            active: self.activity.active(now),
            playing: playing(state),
        };
        control_bar_visible(self.policy, shown)
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

    /// The video was clicked: toggles play, unless [`Options::click`] is off. This does not depend on what the
    /// frontend draws.
    ///
    /// # Errors
    ///
    /// Returns the player's error if mpv rejects the request.
    pub fn video_clicked(&mut self, now: Instant) -> Result<()> {
        self.activity.input(now);
        if self.options.click() {
            self.perform(Action::TogglePlay, now)?;
        }
        Ok(())
    }

    /// The video was double-clicked: requests fullscreen, unless [`Options::double_click`] is off. As in Video.js,
    /// the double-click's two clicks should still be reported to [`video_clicked`](Self::video_clicked), so they
    /// toggle play twice.
    pub fn video_double_clicked(&mut self, now: Instant) -> Option<Request> {
        self.activity.input(now);
        self.options.double_click().then_some(Request::ToggleFullscreen)
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
        let parts = self.options.parts();
        let duration = seekable(&state);
        let not_started = !self.started;
        let bar_visible = self.bar_visible(&state, now);
        let remaining = format_remaining(state.position, state.duration);
        let duration_label = format_time(state.duration, state.duration);

        let waiting_since = [self.buffering_since, self.seeking_since].into_iter().flatten().min();
        let spinner_at = waiting_since.map(|since| since + SPINNER_DELAY);
        let activity_deadline = self.activity.deadline().filter(|_| active);
        let next_deadline =
            [spinner_at, activity_deadline].into_iter().flatten().filter(|deadline| *deadline > now).min();

        View {
            control_bar_visible: bar_visible,
            controls_enabled: self.error.is_none(),
            shown: Shown {
                play: parts.play,
                volume: parts.volume,
                seek_bar: parts.seek_bar,
                current_time: parts.current_time,
                time_display: parts.time_display,
                fullscreen: parts.fullscreen,
                error_display: parts.error_display && self.error.is_some(),
            },
            cursor_hidden: !active && playing,
            big_play_button: parts.big_play_button && not_started && self.error.is_none(),
            poster: self.options.poster().filter(|_| not_started).map(str::to_owned),
            spinner: parts.spinner && self.error.is_none() && spinner_at.is_some_and(|at| now >= at),
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
            time_label: match self.time_display {
                TimeDisplay::Remaining => remaining.clone(),
                TimeDisplay::Duration => duration_label.clone(),
            },
            duration: duration_label,
            remaining,
            time_display: self.time_display,
            seekable: duration.is_some(),
            played: match (self.drag, duration) {
                (_, None) => 0.0,
                (Some(drag), Some(_)) => drag.fraction.clamp(0.0, 1.0),
                (None, Some(_)) => bars::played(state.position, state.duration),
            },
            buffered: bars::buffered(state.buffered, state.duration),
            hover: self.hover.filter(|_| parts.seek_hover).zip(duration).map(|(fraction, duration)| SeekHover {
                fraction: fraction.clamp(0.0, 1.0),
                label: format_time(Some(bars::time_at(fraction, duration)), Some(duration)),
            }),
            rate: self.speed_shown().then(|| format_rate(state.rate)),
            settings: Settings {
                button: self.settings_shown(),
                menu: self
                    .menu
                    .filter(|_| bar_visible && self.settings_shown())
                    .map(|menu| self.menu_view(menu, &state)),
            },
            next_deadline,
        }
    }

    fn menu_view(&self, menu: Menu, state: &State) -> SettingsMenu {
        let entry = |index: usize, label: String, value: Option<String>, checked: bool| MenuItem {
            label,
            value,
            checked,
            focused: index == menu.focused,
        };
        let items = match menu.page {
            Page::Root => self
                .root_items()
                .into_iter()
                .enumerate()
                .map(|(index, item)| match item {
                    Item::Speed => entry(index, "Speed".into(), Some(format_rate(state.rate)), false),
                })
                .collect(),
            Page::Speed => {
                let rates = self.options.playback_rates();
                let current = bars::rate_index(rates, state.rate);
                rates
                    .iter()
                    .enumerate()
                    .map(|(index, rate)| entry(index, format_rate(*rate), None, current == Some(index)))
                    .collect()
            }
        };
        SettingsMenu { page: menu.page, items, focused: menu.focused }
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

/// What the control bar's visibility depends on.
#[derive(Debug, Clone, Copy)]
struct BarState {
    error: bool,
    started: bool,
    active: bool,
    playing: bool,
}

/// Video.js's control-bar visibility: hidden on error and before playback starts, unless the policy shows it then,
/// and while playing with the user inactive. A policy that shows it on error does so whether or not playback started.
fn control_bar_visible(policy: BarPolicy, bar: BarState) -> bool {
    if bar.error {
        policy.on_error
    } else {
        (bar.started || policy.before_start) && (bar.active || !bar.playing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::VideoOutput;
    use crate::ui::options::Parts;

    fn player() -> Player {
        Player::builder().video_output(VideoOutput::Null).mpv_option("ao", "null").build().unwrap()
    }

    fn controls(options: Options) -> Controls {
        Controls::new(&player(), options, BarPolicy::default(), Instant::now())
    }

    /// A controller whose media has started and is paused, so the control bar is visible.
    fn started(options: Options) -> Controls {
        let mut controls = controls(options);
        controls.started = true;
        controls
    }

    fn rates(rates: &[f64]) -> Options {
        Options::builder().playback_rates(rates.iter().copied()).build().unwrap()
    }

    fn menu(controls: &Controls) -> Option<SettingsMenu> {
        controls.view(Instant::now()).settings.menu
    }

    fn perform(controls: &mut Controls, action: Action) {
        controls.perform(action, Instant::now()).unwrap();
    }

    #[test]
    fn control_bar_visibility() {
        let bar = |error, started, active, playing| BarState { error, started, active, playing };
        let video_js = BarPolicy::default();
        assert!(control_bar_visible(video_js, bar(false, true, true, true)), "active while playing");
        assert!(control_bar_visible(video_js, bar(false, true, false, false)), "paused");
        assert!(!control_bar_visible(video_js, bar(false, true, false, true)), "inactive while playing");
        assert!(!control_bar_visible(video_js, bar(false, false, true, false)), "not started");
        assert!(!control_bar_visible(video_js, bar(true, true, true, false)), "error");

        let before_start = BarPolicy { before_start: true, ..BarPolicy::default() };
        assert!(control_bar_visible(before_start, bar(false, false, false, false)), "paused before start");
        assert!(!control_bar_visible(before_start, bar(false, false, false, true)), "inactive while playing");
        assert!(!control_bar_visible(before_start, bar(true, true, true, false)), "error");

        let on_error = BarPolicy { on_error: true, ..BarPolicy::default() };
        assert!(control_bar_visible(on_error, bar(true, true, true, false)), "error");
        assert!(control_bar_visible(on_error, bar(true, false, false, false)), "error before playback started");
        assert!(!control_bar_visible(on_error, bar(false, false, true, false)), "not started");
    }

    #[test]
    fn idle_view() {
        let player = player();
        let poster = Options::builder().poster("poster.jpg").build().unwrap();
        let controls = Controls::new(&player, poster, BarPolicy::default(), Instant::now());
        let now = Instant::now();
        let view = controls.view(now);
        assert!(view.controls_enabled);
        assert!(!view.control_bar_visible, "not started");
        assert!(!view.cursor_hidden);
        assert!(view.big_play_button);
        assert_eq!(view.poster.as_deref(), Some("poster.jpg"));
        assert!(!view.spinner);
        assert_eq!(view.play_icon, PlayIcon::Play);
        assert_eq!((view.current_time.as_str(), view.duration.as_str()), ("-:-", "-:-"));
        assert!(!view.seekable);
        assert_eq!((view.played, view.buffered), (0.0, 0.0));
        assert_eq!(view.rate.as_deref(), Some("1x"), "the default rates offer Speed");
        assert_eq!(view.volume_level, 3);
        assert_eq!((view.time_display, view.time_label.as_str()), (TimeDisplay::Remaining, "-0:00"));
        assert!(view.settings.button && view.settings.menu.is_none());
        let shown = view.shown;
        assert!(shown.play && shown.volume && shown.seek_bar && shown.current_time);
        assert!(shown.time_display && shown.fullscreen && !shown.error_display);
    }

    #[test]
    fn hidden_parts() {
        let hidden = Parts::default()
            .play(false)
            .volume(false)
            .seek_bar(false)
            .current_time(false)
            .time_display(false)
            .fullscreen(false)
            .big_play_button(false)
            .error_display(false)
            .seek_hover(false);
        let mut controls = controls(Options::builder().parts(hidden).build().unwrap());
        controls.hover = Some(0.5);
        let view = controls.view(Instant::now());
        let shown = view.shown;
        assert!(!shown.play && !shown.volume && !shown.seek_bar && !shown.current_time);
        assert!(!shown.time_display && !shown.fullscreen);
        assert!(!view.big_play_button, "hidden before the first play");
        assert_eq!(view.hover, None);

        controls.handle_event(&Event::Ended(EndReason::Error("loading failed".into())), Instant::now());
        let view = controls.view(Instant::now());
        assert_eq!(view.error.as_deref(), Some("loading failed"), "reported without the display");
        assert!(!view.shown.error_display);
    }

    #[test]
    fn settings_follows_its_items() {
        let speed_hidden = Options::builder().parts(Parts::default().speed(false)).build().unwrap();
        let view = controls(speed_hidden).view(Instant::now());
        assert!(!view.settings.button);
        assert_eq!(view.rate, None);

        let no_rates = controls(rates(&[])).view(Instant::now());
        assert!(!no_rates.settings.button);
        assert_eq!(no_rates.rate, None);

        let shown = controls(rates(&[1.0, 2.0])).view(Instant::now());
        assert!(shown.settings.button);
        assert_eq!(shown.rate.as_deref(), Some("1x"));
    }

    #[test]
    fn settings_menu_navigation() {
        let mut controls = started(rates(&[0.5, 1.0, 1.5, 2.0]));
        assert_eq!(menu(&controls), None);

        perform(&mut controls, Action::ToggleSettings);
        let root = menu(&controls).unwrap();
        assert_eq!((root.page, root.focused), (Page::Root, 0));
        let speed = MenuItem { label: "Speed".into(), value: Some("1x".into()), checked: false, focused: true };
        assert_eq!(root.items, [speed]);

        // The Speed page opens focused on the current rate, which is checked.
        perform(&mut controls, Action::Select(0));
        let page = menu(&controls).unwrap();
        assert_eq!((page.page, page.focused), (Page::Speed, 1));
        let labels: Vec<_> = page.items.iter().map(|item| item.label.as_str()).collect();
        assert_eq!(labels, ["0.5x", "1x", "1.5x", "2x"]);
        let checked: Vec<_> = page.items.iter().map(|item| item.checked).collect();
        assert_eq!(checked, [false, true, false, false]);
        assert!(page.items[1].focused && page.items.iter().all(|item| item.value.is_none()));

        // Focus wraps both ways.
        for _ in 0..3 {
            perform(&mut controls, Action::FocusNext);
        }
        assert_eq!(menu(&controls).unwrap().focused, 0);
        perform(&mut controls, Action::FocusPrevious);
        assert_eq!(menu(&controls).unwrap().focused, 3);

        perform(&mut controls, Action::Back);
        let root = menu(&controls).unwrap();
        assert_eq!((root.page, root.focused), (Page::Root, 0));
        perform(&mut controls, Action::Back);
        assert_eq!(menu(&controls).unwrap().page, Page::Root, "nothing to go back to");

        perform(&mut controls, Action::OpenPage(Page::Speed));
        assert_eq!(menu(&controls).unwrap().page, Page::Speed);
        perform(&mut controls, Action::ToggleSettings);
        assert_eq!(menu(&controls), None);
        perform(&mut controls, Action::OpenPage(Page::Speed));
        perform(&mut controls, Action::CloseSettings);
        assert_eq!(menu(&controls), None);
    }

    #[test]
    fn settings_menu_needs_the_button() {
        let mut controls = started(rates(&[]));
        perform(&mut controls, Action::ToggleSettings);
        perform(&mut controls, Action::OpenPage(Page::Speed));
        assert_eq!(controls.menu, None);
    }

    #[test]
    fn open_menu_holds_the_user_active() {
        let mut controls = started(Options::default());
        let start = Instant::now();
        controls.perform(Action::ToggleSettings, start).unwrap();
        assert!(controls.activity.active(start + Duration::from_secs(10)));
        controls.perform(Action::ToggleSettings, start + Duration::from_secs(10)).unwrap();
        assert!(!controls.activity.active(start + Duration::from_secs(12)));
    }

    #[test]
    fn menu_closes_with_the_bar_and_new_media() {
        for event in [Event::Idle, Event::Ended(EndReason::Replaced), Event::Loaded] {
            let mut controls = started(Options::default());
            perform(&mut controls, Action::ToggleSettings);
            controls.handle_event(&event, Instant::now());
            assert_eq!(controls.menu, None, "{event:?}");
        }

        // An error hides the bar under Video.js's policy, but not under one that shows it on error.
        let error = Event::Ended(EndReason::Error("loading failed".into()));
        let mut controls = started(Options::default());
        perform(&mut controls, Action::ToggleSettings);
        controls.handle_event(&error, Instant::now());
        assert_eq!(controls.menu, None);

        let on_error = BarPolicy { on_error: true, ..BarPolicy::default() };
        let mut controls = Controls::new(&player(), Options::default(), on_error, Instant::now());
        controls.started = true;
        perform(&mut controls, Action::ToggleSettings);
        controls.handle_event(&error, Instant::now());
        assert!(controls.menu.is_some());
    }

    #[test]
    fn time_display_toggles() {
        let mut controls = controls(Options::default());
        perform(&mut controls, Action::ToggleTimeDisplay);
        let view = controls.view(Instant::now());
        assert_eq!((view.time_display, view.time_label.as_str()), (TimeDisplay::Duration, "-:-"));
        perform(&mut controls, Action::ToggleTimeDisplay);
        assert_eq!(controls.view(Instant::now()).time_display, TimeDisplay::Remaining);
    }

    #[test]
    fn error_view() {
        let player = player();
        let mut controls = Controls::new(&player, Options::default(), BarPolicy::default(), Instant::now());
        let now = Instant::now();
        controls.handle_event(&Event::Ended(EndReason::Error("loading failed".into())), now);
        let view = controls.view(now);
        assert_eq!(view.error.as_deref(), Some("loading failed"));
        assert!(!view.control_bar_visible && !view.big_play_button && !view.spinner);
        assert!(!view.controls_enabled && view.shown.error_display);
    }

    #[test]
    fn error_disables_actions_but_fullscreen_and_retry() {
        let mut controls = started(Options::default());
        controls.handle_event(&Event::Ended(EndReason::Error("loading failed".into())), Instant::now());
        for action in [Action::ToggleTimeDisplay, Action::ToggleSettings, Action::Play, Action::SetVolume(0.2)] {
            assert_eq!(controls.perform(action, Instant::now()).unwrap(), None, "{action:?}");
        }
        assert_eq!(controls.time_display, TimeDisplay::Remaining);
        assert_eq!(controls.menu, None);
        let fullscreen = controls.perform(Action::ToggleFullscreen, Instant::now()).unwrap();
        assert_eq!(fullscreen, Some(Request::ToggleFullscreen));
        // Nothing was ever loaded, so retrying does nothing.
        assert_eq!(controls.perform(Action::Retry, Instant::now()).unwrap(), None);
    }

    #[test]
    fn spinner_waits() {
        let player = player();
        let mut controls = Controls::new(&player, Options::default(), BarPolicy::default(), Instant::now());
        let start = Instant::now();
        controls.handle_event(&Event::Seeking, start);
        assert!(!controls.view(start + Duration::from_millis(299)).spinner);
        assert_eq!(controls.view(start).next_deadline, Some(start + SPINNER_DELAY));
        assert!(controls.view(start + SPINNER_DELAY).spinner);
        controls.handle_event(&Event::Seeked, start + Duration::from_secs(1));
        assert!(!controls.view(start + Duration::from_secs(1)).spinner);

        let hidden = Options::builder().parts(Parts::default().spinner(false)).build().unwrap();
        let mut controls = Controls::new(&player, hidden, BarPolicy::default(), start);
        controls.handle_event(&Event::Seeking, start);
        assert!(!controls.view(start + SPINNER_DELAY).spinner);
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
