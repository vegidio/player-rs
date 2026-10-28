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
//! # Rendering video
//!
//! A player built with [`VideoOutput::Render`] draws its video through a [`RenderContext`], into an OpenGL
//! framebuffer the application owns. mpv does the decoding, scaling, colour conversion and subtitle rendering on the
//! GPU, and no frame is copied through the CPU. Any toolkit with an OpenGL context can show the result.
//!
//! - [`RenderContext::new`] is `unsafe`: the GL context must be current on the thread that creates, uses and drops
//!   the render context, and must outlive it.
//! - Its `on_frame` callback runs on mpv's threads and must only signal the render thread, which then calls
//!   [`RenderContext::update`] and [`RenderContext::render`].
//! - Create the render context before the first [`Player::load`]: media loaded before it exists plays without video.
//!
//! ```no_run
//! use player::prelude::*;
//! use std::ffi::{CStr, c_void};
//! use std::sync::mpsc;
//!
//! // From the toolkit's GL context, such as glutin's `Display::get_proc_address`.
//! fn get_proc_address(name: &CStr) -> *const c_void {
//!     # let _ = name;
//!     # std::ptr::null()
//!     // ...
//! }
//!
//! let player = Player::builder().video_output(VideoOutput::Render).build()?;
//! let (wake, frames) = mpsc::channel();
//! // SAFETY: the toolkit's GL context is current on this thread, where `render` is used and dropped, and outlives it.
//! let mut render = unsafe {
//!     RenderContext::new(&player, get_proc_address, move || {
//!         let _ = wake.send(());
//!     })?
//! };
//! player.load(Source::url("https://example.com/movie.mp4")?)?;
//!
//! while frames.recv().is_ok() {
//!     if render.update() {
//!         // Into the window's default framebuffer, which OpenGL shows bottom row first.
//!         render.render(RenderTarget { fbo: 0, width: 1280, height: 720 }, true)?;
//!         // Swap the window's buffers, then:
//!         render.report_swap();
//!     }
//! }
//! # Ok::<(), player::Error>(())
//! ```
//!
//! The video's display size, to size the framebuffer and lay out the player, is in [`State::video_size`] and
//! announced by [`Event::VideoSizeChanged`].
//!
//! On macOS, toolkits that draw with Metal (GPUI among them) use `player::macos::SurfaceRenderer` instead. It owns its
//! render thread and OpenGL context, and hands out each frame as an IOSurface-backed `CVPixelBuffer`, with no
//! `unsafe` code. The module exists on macOS only.
//!
//! # The UI layer
//!
//! [`ui`] gives every frontend the Video.js player experience without drawing anything: [`ui::Controls`] takes the
//! player's events and the user's input, acts on the player, and returns a [`ui::View`] to draw. Frontends forward
//! every event to it, tick it at [`ui::View::next_deadline`], and only translate input and paint; see the module
//! docs for the loop and where it deliberately differs from Video.js.
//!
//! [`client_api_version`] reports the client API version of the linked library, and [`BINDINGS_CLIENT_API_VERSION`]
//! the one the [`sys`] bindings were generated from.
#![warn(missing_docs)]
// FFI guardrail: an `unsafe fn` body must still wrap its unsafe operations in an explicit
// `unsafe {}` block, so the unsafe surface stays visible rather than blanket-covered.
#![deny(unsafe_op_in_unsafe_fn)]

pub mod sys;

pub mod error;
#[cfg(target_os = "macos")]
pub mod macos;
pub mod prelude;
pub mod ui;

mod builder;
mod event;
mod mpv;
mod player;
mod render;
mod source;

/// The integration tests' headless GL context, shared with the unit tests that need a real render context.
#[cfg(test)]
#[path = "../tests/common/gl.rs"]
mod test_gl;

pub use builder::{Autoplay, PlayerBuilder, VideoOutput};
pub use error::{Error, Result};
pub use event::{EndReason, Event, Events, State};
pub use mpv::PropertyValue;
pub use player::{Player, Seek};
pub use render::{RenderContext, RenderTarget};
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
