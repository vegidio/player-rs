//! What a control or key does: [`Action`], and the [`Request`]s only the frontend can carry out.

use crate::ui::controls::Page;
use std::time::Duration;

/// A user-level operation that a control or key triggers, carried out by
/// [`Controls::perform`](super::Controls::perform).
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum Action {
    /// Starts or resumes playback, restarting media that has ended.
    Play,
    /// Pauses playback.
    Pause,
    /// Plays if paused, pauses otherwise.
    TogglePlay,
    /// Seeks to this position.
    SeekTo(Duration),
    /// Seeks by this many seconds, backwards if negative.
    SeekBy(f64),
    /// Mutes or unmutes, as Video.js's mute control: at volume 0 it restores the last volume (at least 0.1) instead.
    ToggleMute,
    /// Sets the volume, from `0.0` to `1.0`, and unmutes.
    SetVolume(f64),
    /// Changes the volume by this amount and unmutes.
    StepVolume(f64),
    /// Sets the playback rate.
    SetRate(f64),
    /// Enters or leaves fullscreen; returned to the frontend as [`Request::ToggleFullscreen`].
    ToggleFullscreen,
    /// Loads the last source given to the player again, and plays it whatever the autoplay mode. Does nothing if no
    /// source was ever loaded.
    Retry,
    /// Switches the time display between the remaining time and the duration.
    ToggleTimeDisplay,
    /// Opens the Settings menu on its root page, or closes it if it is open. Does nothing while the Settings button
    /// is not shown.
    ToggleSettings,
    /// Closes the Settings menu.
    CloseSettings,
    /// Opens the Settings menu on this page, if the page's item is shown.
    OpenPage(Page),
    /// Returns the Settings menu from an item's page to its root page.
    Back,
    /// Activates the Settings menu's entry at this index on the current page: on the root page it opens the item's
    /// page; on an item's page it applies the entry and returns to the root page.
    Select(usize),
    /// Moves the Settings menu's keyboard focus to the next entry, wrapping around.
    FocusNext,
    /// Moves the Settings menu's keyboard focus to the previous entry, wrapping around.
    FocusPrevious,
    /// Activates the Settings menu's focused entry, as [`Select`](Self::Select) does.
    ActivateFocused,
}

/// Something only the frontend can do, such as controlling its window, returned by the [`Controls`](super::Controls)
/// methods that may need it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Request {
    /// Enter fullscreen, or leave it if the player is fullscreen.
    ToggleFullscreen,
}
