//! What a control or key does: [`Action`], and the [`Request`]s only the frontend can carry out.

use std::time::Duration;

/// A user-level operation that a control or key triggers, carried out by [`Controls::perform`](super::Controls::perform).
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
    /// Selects the configured playback rate after the current one, wrapping around, or the first one if the current
    /// rate is not in the list. Does nothing with no rates configured.
    CycleRate,
    /// Enters or leaves fullscreen; returned to the frontend as [`Request::ToggleFullscreen`].
    ToggleFullscreen,
}

/// Something only the frontend can do, such as controlling its window, returned by the [`Controls`](super::Controls)
/// methods that may need it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Request {
    /// Enter fullscreen, or leave it if the player is fullscreen.
    ToggleFullscreen,
}
