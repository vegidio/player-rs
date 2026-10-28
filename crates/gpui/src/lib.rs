//! # player_gpui
//!
//! The [GPUI](https://gpui.rs) front end of `player-rs`: a video player component with the look and behaviour of
//! [Video.js](https://videojs.org) for apps built with GPUI-Kit.
//!
//! [`VideoPlayer`] shows a player's video with Video.js's controls over it.
//!
//! ```no_run
//! use gpui_kit::*;
//! use player::prelude::*;
//! use player::ui;
//! use player_gpui::VideoPlayer;
//!
//! struct Movie {
//!     player: Player,
//!     options: ui::Options,
//! }
//!
//! impl Movie {
//!     fn new() -> player::Result<Self> {
//!         let player = Player::builder().video_output(VideoOutput::Render).autoplay(Autoplay::On).build()?;
//!         player.load(Source::file("movie.mp4")?)?;
//!         let options = ui::Options::builder().playback_rates([0.5, 1.0, 1.5, 2.0]).build()?;
//!         Ok(Self { player, options })
//!     }
//! }
//!
//! impl Render for Movie {
//!     fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
//!         div().size_full().child(
//!             VideoPlayer::new("movie", &self.player)
//!                 .options(self.options.clone())
//!                 .on_fullscreen_change(|on, _, _| println!("fullscreen: {on}"))
//!                 .size_full(),
//!         )
//!     }
//! }
//!
//! fn main() {
//!     // The controls' icons, with GPUI-Kit's default icons behind them.
//!     gpui_kit::application().with_assets(player_gpui::Assets).run(|cx| {
//!         // The controls' tooltips and spinner need GPUI-Kit's theme.
//!         gpui_kit::init(cx);
//!         let movie = Movie::new().expect("the movie loads");
//!         gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| movie)).expect("the window opens");
//!     });
//! }
//! ```
//!
//! # Setup
//!
//! - **GPUI-Kit version.** This crate is built on `gpui-kit` 0.8.1 with its `component` and `assets` features (its
//!   defaults), and the app must use the same minor version, or it ends up with two copies of GPUI whose types do not
//!   match. The app's own `gpui-kit` features, `gpui-fast` among them, apply to this crate too.
//! - **GPUI-Kit's theme.** The app must call `gpui_kit::init(cx)` before showing a player; the controls' GPUI-Kit
//!   components panic without it.
//! - **Icons.** The app registers [`Assets`], which serves the controls' icons and falls back to GPUI-Kit's default
//!   bundle, in place of `gpui_kit::assets::Assets`. An app with an asset source of its own composes [`Icons`], the
//!   icons that bundle lacks, into it instead. Without either, some control icons are blank.
//! - **Player.** It must be built with `VideoOutput::Render`. It may load media before the element is first rendered.
//!   Otherwise the element stays black and logs a warning.
//!
//! # Controls
//!
//! The controls follow Video.js 8's default skin, and their behaviour is `player::ui`'s, shared with every frontend:
//!
//! - **Control bar**: play/pause/replay, mute with a volume bar that expands while the panel is hovered or the bar
//!   focused, the seek bar (buffered and played parts, a time label on hover, Video.js's press, drag and release
//!   scrubbing), the remaining time, a playback-rate button when [rates](player::ui::OptionsBuilder::playback_rates)
//!   are set, and fullscreen. It fades out after the inactivity timeout while playing, and back in on input.
//! - **Overlays**: the centred big play button before playback starts, the loading spinner, and the error display.
//! - **Pointer**: clicking the video toggles play and double-clicking toggles fullscreen, unless the options turn
//!   them off.
//! - **Keyboard**: clicking the player focuses it, and Tab moves through the controls in control-bar order. Keys go
//!   to `player::ui`'s key map for the focused part: the seek-bar and volume-bar keys, and the hotkeys as configured.
//!   Keys it does not handle, including every key held with Ctrl, Alt or the platform key, go on to the app. The
//!   [`TogglePlay`], [`ToggleMute`] and [`ToggleFullscreen`] actions let the app's menus drive the focused player.
//! - **Options.** [`VideoPlayer::options`] takes `player::ui::Options`. **Controls are on by default**; for bare
//!   video, pass `ui::Options::builder().controls(false).build()?`. As in Video.js, the options are read once: when
//!   the element's state is created and when it is given a different player.
//! - **Fullscreen is the window's.** Video.js makes the player element fill the screen; a GPUI element cannot, so the
//!   fullscreen control, `f`, double-click and the action toggle the window's fullscreen, and Escape leaves it.
//!   [`VideoPlayer::on_fullscreen_change`] tells the app, including when the window leaves fullscreen by other
//!   means, so it can show the player alone while fullscreen. A player that enters while its window is already
//!   fullscreen leaves the window fullscreen when it exits.
//!
//! # Behaviour and gaps
//!
//! - **Repainting.** The element repaints for each new frame and not while paused. As for any GPUI view that
//!   repaints, each frame also re-renders the views the element is in, from the window's root down, and so does each
//!   change of the controls, such as the seek bar moving by a pixel. Keep those views cheap, and show expensive parts
//!   next to the video as cached views (`Entity::cached`).
//! - **Sharpness.** Frames are rendered at the element's size in device pixels.
//! - **Platforms.** Video is shown on macOS. On Windows and Linux the element is black while audio and the controls
//!   keep working, until those platforms get video surfaces.
//! - **Cursor.** Unlike Video.js, the pointer cursor stays visible while the user is inactive: GPUI has no way to hide
//!   it on demand.
//! - **Not yet.** The rate button cycles the rates instead of opening Video.js's menu. Menus (rates, tracks),
//!   captions and the poster come in later releases.

// GPUI's derive macros name the `gpui` crate; through this alias they resolve to the GPUI that GPUI-Kit re-exports.
extern crate gpui_kit as gpui;

mod assets;
mod controls;
mod element;
mod keys;
#[cfg(target_os = "macos")]
mod video;

pub use assets::{Assets, Icons};
pub use controls::{ToggleFullscreen, ToggleMute, TogglePlay};
pub use element::VideoPlayer;

// The README's examples, compiled as doc tests.
#[cfg(doctest)]
#[doc = include_str!("../../../README.md")]
struct Readme;

#[cfg(test)]
mod tests {
    /// Links libmpv through `player-rs` alone, without linker flags of this crate's own.
    #[test]
    fn links_libmpv_through_player() {
        assert_eq!(player::client_api_version(), player::BINDINGS_CLIENT_API_VERSION);
    }
}
