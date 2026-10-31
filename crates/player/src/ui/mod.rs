//! The toolkit-agnostic UI layer: everything about the Video.js player experience that is not drawing.
//!
//! [`Controls`] owns the interaction rules of one player view: which keys do what, when the control bar and the
//! cursor hide, how times are written, and what clicks and drags on the seek and volume bars do to playback. A
//! frontend only translates its toolkit's input into calls on it, and draws the [`View`] it returns. Nothing here
//! draws, spawns threads or depends on a GUI toolkit, so every frontend shares the same behaviour.
//!
//! # The frontend loop
//!
//! - **Forward every event.** Pass each [`Event`](crate::Event) from the frontend's own subscription to
//!   [`Controls::handle_event`], including those it does not care about. The controller keeps its own state (whether
//!   playback has started, the spinner's timers, the error) from them, and it drifts from the player's if events are
//!   skipped.
//! - **Report input.** Pointer movement, presses, hover and clicks go to the `pointer_*`, `control_bar_hovered` and
//!   `video_*` methods, keys to [`Controls::key`], seek and volume bar drags to the `seek_*` and
//!   [`volume_drag`](Controls::volume_drag) methods, and buttons to [`Controls::perform`].
//! - **Carry out requests.** Some actions, such as fullscreen, only the frontend can do; they come back as a
//!   [`Request`].
//! - **Draw the view, and tick at its deadline.** [`Controls::view`] describes everything to draw. Its
//!   [`next_deadline`](View::next_deadline) is when the view next changes without input or events, such as the
//!   control bar hiding after inactivity: call [`Controls::tick`] and redraw then.
//!
//! Every method takes the current [`Instant`](std::time::Instant), so the layer has no clock of its own and its tests
//! need no sleeps.
//!
//! ```no_run
//! use player::prelude::*;
//! use player::ui::{self, Focus, Key, Modifiers};
//! use std::time::{Duration, Instant};
//!
//! // `VideoOutput::Render`, the default: the frontend shows the video through a render context or surface renderer.
//! let player = Player::builder().build()?;
//! let options = ui::Options::builder().playback_rates([0.5, 1.0, 1.5, 2.0]).aspect_ratio("16:9").build()?;
//! let mut controls = ui::Controls::new(&player, options, Instant::now());
//! let events = player.subscribe();
//! player.load(Source::url("https://example.com/movie.mp4")?)?;
//!
//! loop {
//!     let now = Instant::now();
//!     let view = controls.view(now);
//!     // Draw `view`, then wait for the next event, input or deadline.
//!     let wait = view.next_deadline.map_or(Duration::from_millis(250), |d| d.saturating_duration_since(now));
//!     match events.recv_timeout(wait) {
//!         Some(event) => controls.handle_event(&event, Instant::now()),
//!         None => controls.tick(Instant::now()),
//!     }
//!     // On a key press from the toolkit:
//!     let outcome = controls.key(Key::Char('k'), Modifiers::NONE, Focus::Player, Instant::now())?;
//!     if !outcome.handled {
//!         // Let the application handle the key.
//!     }
//!     # break;
//! }
//! # Ok::<(), player::Error>(())
//! ```
//!
//! # Differences from Video.js
//!
//! - Global hotkeys (`f`, `m`, `k` and Space) are on by default, because this is a desktop player. Video.js leaves
//!   them off; [`Hotkeys::disabled`] restores that.
//! - Keys pressed with Ctrl, Alt or the platform key (Cmd, Super/Win) are always left unhandled, so application
//!   shortcuts such as Cmd+F keep working. Video.js ignores modifiers. Shift is allowed.
//! - User activity times out at a deadline instead of Video.js's 250 ms polling, so the control bar hides exactly
//!   when the inactivity timeout has passed.
//!
//! The frontend animates visibility changes with [`FADE_IN`] and [`FADE_OUT`], as Video.js's CSS transitions do.

mod action;
mod activity;
pub mod bars;
mod controls;
pub mod keys;
mod options;
mod time;

use std::time::Duration;

pub use action::{Action, Request};
pub use controls::{Controls, KeyOutcome, PlayIcon, SeekHover, View};
pub use keys::{Focus, Key, Modifiers};
pub use options::{AspectRatio, Hotkeys, Options, OptionsBuilder};
pub use time::{format_rate, format_remaining, format_time};

/// How long the control bar takes to fade in when it becomes visible, as in Video.js.
pub const FADE_IN: Duration = Duration::from_millis(100);

/// How long the control bar takes to fade out when it is hidden, as in Video.js.
pub const FADE_OUT: Duration = Duration::from_secs(1);

/// How long playback must have been waiting, for the network or for a seek, before the loading spinner shows, as in
/// Video.js. Shorter waits never show it.
pub const SPINNER_DELAY: Duration = Duration::from_millis(300);
