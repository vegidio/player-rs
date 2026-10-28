//! # player
//!
//! A video player for Rust built on [libmpv](https://mpv.io), statically linked.
//!
//! The static libraries are downloaded and linked at build time from
//! [`vegidio/binaries-mpv`](https://github.com/vegidio/binaries-mpv). The raw FFI bindings are pre-generated and
//! committed (`src/sys/bindings.rs`), so no build-time `bindgen`/`libclang` is needed.
//!
//! [`Player`] is the toolkit-agnostic handle every frontend drives: it loads media, controls playback, and reports
//! state and events. Code written against it stays the same whichever GUI toolkit shows the video.
//!
//! ```no_run
//! use player::prelude::*;
//!
//! let player = Player::builder().autoplay(Autoplay::Muted).volume(0.8).build()?;
//! player.load(Source::url("https://example.com/movie.mp4")?)?;
//!
//! for event in player.subscribe() {
//!     match event {
//!         Event::Loaded => println!("duration: {:?}", player.state().duration),
//!         Event::Ended(reason) => {
//!             println!("ended: {reason:?}");
//!             break;
//!         }
//!         _ => {}
//!     }
//! }
//! # Ok::<(), player::Error>(())
//! ```
//!
//! - Media ends as in Video.js: it stays loaded and paused at its end, and [`Player::play`] restarts it.
//! - Every mpv feature without a typed method yet is reachable through [`Player::get_property`],
//!   [`Player::set_property`] and [`Player::command`].
//! - [`Events`] can be read from a blocking thread or awaited in any async executor.
//!
//! [`client_api_version`] reports the client API version of the linked library, and [`BINDINGS_CLIENT_API_VERSION`]
//! the one the [`sys`] bindings were generated from.
#![warn(missing_docs)]
// FFI guardrail: an `unsafe fn` body must still wrap its unsafe operations in an explicit
// `unsafe {}` block, so the unsafe surface stays visible rather than blanket-covered.
#![deny(unsafe_op_in_unsafe_fn)]

pub mod sys;

pub mod error;
pub mod prelude;

mod builder;
mod event;
mod mpv;
mod player;
mod source;

pub use builder::{Autoplay, PlayerBuilder, VideoOutput};
pub use error::{Error, Result};
pub use event::{EndReason, Event, Events, State};
pub use mpv::PropertyValue;
pub use player::{Player, Seek};
pub use source::Source;

/// The client API version of the headers the [`sys`] bindings were generated from, as `(major, minor)`.
pub const BINDINGS_CLIENT_API_VERSION: (u16, u16) = split_version(sys::MPV_CLIENT_API_VERSION);

/// Returns the client API version of the linked libmpv as `(major, minor)`.
///
/// It equals [`BINDINGS_CLIENT_API_VERSION`] unless the committed bindings are stale.
pub fn client_api_version() -> (u16, u16) {
    // SAFETY: `mpv_client_api_version` takes no arguments, has no preconditions and is thread-safe.
    split_version(unsafe { sys::mpv_client_api_version() })
}

/// Splits libmpv's `(major << 16) | minor` encoding.
const fn split_version(version: std::ffi::c_ulong) -> (u16, u16) {
    ((version >> 16) as u16, (version & 0xffff) as u16)
}
