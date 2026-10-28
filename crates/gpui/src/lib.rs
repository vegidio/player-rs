//! # player_gpui
//!
//! The [GPUI](https://gpui.rs) front end of `player-rs`: a video player component with the look and behaviour of
//! [Video.js](https://videojs.org) for apps built with GPUI-Kit.
//!
//! [`VideoPlayer`] shows a player's video. Controls come in a later release.
//!
//! ```no_run
//! use gpui_kit::*;
//! use player::prelude::*;
//! use player_gpui::VideoPlayer;
//!
//! struct Movie {
//!     player: Player,
//! }
//!
//! impl Movie {
//!     fn new() -> player::Result<Self> {
//!         let player = Player::builder().video_output(VideoOutput::Render).autoplay(Autoplay::On).build()?;
//!         player.load(Source::file("movie.mp4")?)?;
//!         Ok(Self { player })
//!     }
//! }
//!
//! impl Render for Movie {
//!     fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
//!         div().size_full().child(VideoPlayer::new("movie", &self.player).size_full())
//!     }
//! }
//! ```
//!
//! - **GPUI-Kit version.** This crate is built on `gpui-kit` 0.8 and the app must use the same minor version, or it
//!   ends up with two copies of GPUI whose types do not match. The app's own `gpui-kit` features, `gpui-fast` among
//!   them, apply to this crate too.
//! - **Player.** It must be built with `VideoOutput::Render`. It may load media before the element is first rendered.
//!   Otherwise the element stays black and logs a warning.
//! - **Repainting.** The element repaints for each new frame and not while paused. As for any GPUI view that
//!   repaints, each frame also re-renders the views the element is in, from the window's root down. Keep those
//!   views cheap, and show expensive parts next to the video as cached views (`Entity::cached`).
//! - **Sharpness.** Frames are rendered at the element's size in device pixels.
//! - **Platforms.** Video is shown on macOS. On Windows and Linux the element is black while audio and control keep
//!   working, until those platforms get video surfaces.

// GPUI's derive macros name the `gpui` crate; through this alias they resolve to the GPUI that GPUI-Kit re-exports.
extern crate gpui_kit as gpui;

mod element;
#[cfg(target_os = "macos")]
mod video;

pub use element::VideoPlayer;

#[cfg(test)]
mod tests {
    /// Links libmpv through `player-rs` alone, without linker flags of this crate's own.
    #[test]
    fn links_libmpv_through_player() {
        assert_eq!(player::client_api_version(), player::BINDINGS_CLIENT_API_VERSION);
    }
}
