//! # player_gpui
//!
//! The [GPUI](https://gpui.rs) front end of `player-rs`: a video player component with the look and behaviour of
//! [Video.js](https://videojs.org) for apps built with GPUI-Kit.
//!
//! [`VideoPlayer`] shows a player's video with controls over it, drawn by its [`Skin`].
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
//!         let player = Player::builder().autoplay(Autoplay::On).build()?;
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
//!   icons the controls draw, into it instead. Without either, some control icons are blank.
//! - **Font.** None: the Default skin's typeface, Inter, is embedded and registers itself with GPUI's text system the
//!   first time a player is created. It is licensed under the SIL Open Font License 1.1, shipped with the crate in
//!   `assets/fonts/OFL.txt`.
//! - **Player.** It must use `VideoOutput::Render`, the default video output; otherwise the element stays black and
//!   logs a warning. It may load media before the element is first rendered.
//! - **Windows and Linux.** Apps that build for Windows or Linux must patch GPUI in their workspace root with the fork
//!   that adds video surfaces:
//!
//!   ```toml
//!   [patch.crates-io]
//!   gpui-pre = { git = "https://github.com/vegidio/gpui-pre", tag = "0.3.10-surfaces.2" }
//!   gpui-pre-windows = { git = "https://github.com/vegidio/gpui-pre", tag = "0.3.10-surfaces.2" }
//!   gpui-pre-wgpu = { git = "https://github.com/vegidio/gpui-pre", tag = "0.3.10-surfaces.2" }
//!   ```
//!
//!   `gpui-pre` is needed on both, `gpui-pre-windows` for Windows and `gpui-pre-wgpu` for Linux. Without the patch,
//!   the build fails with an error naming `WINDOWS_SURFACES_REQUIRE_PATCHED_GPUI` or
//!   `LINUX_SURFACES_REQUIRE_PATCHED_GPUI`. Only GPUI-Kit's default backend is covered, not `gpui-fast`. macOS needs
//!   no patch. See the [README](https://github.com/vegidio/player-rs#patched-gpui) for the details.
//!
//! # Controls
//!
//! The controls' behaviour is `player::ui`'s, shared with every frontend, and their look is the [`Skin`]'s:
//! [`Skin::Default`], the designed skin, or [`Skin::None`], which draws nothing over the video while clicks and keys
//! keep working. The Default skin has fixed sizes, whatever the player's size, and draws its text in Inter:
//!
//! - **Control bar**: a floating, rounded, translucent pill holding play/pause/replay, mute, the current time, the
//!   seek bar (buffered and played parts, a time label on hover, Video.js's press, drag and release scrubbing), the
//!   time display (the remaining time, or the duration once clicked), Settings while one of its items is offered, and
//!   fullscreen. It is shown before playback starts, fades out after the inactivity timeout while playing, and back
//!   in on input.
//! - **Volume**: a vertical slider in a popover above the mute button, open while the button or the popover is
//!   hovered, during a drag, or while the volume bar has focus.
//! - **Settings menu**: a popover above the Settings button, listing each item with its value (today, Speed); the
//!   Speed page lists the playback rates, with the current one checked. A press elsewhere closes it.
//! - **Parts**: every part is shown by default; [`Parts`](player::ui::Parts) in the options hides any of them.
//! - **Overlays**: no big play button, as in the skin's design; a ring spinner while loading; and an error card with
//!   mpv's message and a Try again button, over the bar, whose controls are disabled except fullscreen.
//! - **Pointer**: clicking the video toggles play and double-clicking toggles fullscreen, unless the options turn
//!   them off.
//! - **Keyboard**: clicking the player focuses it, and Tab moves through the controls in control-bar order. Keys go
//!   to `player::ui`'s key map for the focused part: the seek-bar and volume-bar keys, the Settings menu's keys while
//!   focus is in it (opening it from the keyboard moves focus in, and closing it returns focus to the button), and
//!   the hotkeys as configured.
//!   Keys it does not handle, including every key held with Ctrl, Alt or the platform key, go on to the app. The
//!   [`TogglePlay`], [`ToggleMute`] and [`ToggleFullscreen`] actions let the app's menus drive the focused player.
//! - **Options and skin.** [`VideoPlayer::options`] takes `player::ui::Options`, and [`VideoPlayer::skin`] the skin.
//!   For bare video, use [`Skin::None`]; it replaces the former `controls(false)` option, which also turned off clicks
//!   and keys, now their own options under every skin. Speed is offered by default, with the rates 0.25x to 3x;
//!   `playback_rates([])` offers none. As in Video.js, the options and the skin are read once: when the element's
//!   state is created and when it is given a different player.
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
//! - **Platforms.** Video is shown on macOS, and on Windows and Linux with the patched GPUI (see [Setup](#setup)).
//!   - On Windows, the graphics driver must share OpenGL textures with Direct3D 11 (`WGL_NV_DX_interop2`), on the GPU
//!     GPUI uses; otherwise, as on hybrid-GPU laptops where the two land on different GPUs, the element is black and
//!     one warning is logged.
//!   - On Linux, frames are dma-bufs that GPUI's renderer imports: on Vulkan with `VK_EXT_image_drm_format_modifier`,
//!     on Vulkan with linear buffers only (Mesa's lavapipe), or on OpenGL through EGL. They are rendered on GPUI's
//!     render node, with the layouts it imports, when GPUI reports one, and otherwise on the first usable node with
//!     linear buffers. Without a usable DRM render node (WSL2 without `vgem`), or on a driver that cannot import
//!     dma-bufs (Mesa's `d3d12` on WSL2), the element is black and one warning is logged. Only software drivers are
//!     verified; Linux GPUs and the modifier import are not.
//!
//!   Wherever the element is black, audio and the controls keep working.
//! - **Cursor.** Unlike Video.js, the pointer cursor stays visible while the user is inactive: GPUI has no way to hide
//!   it on demand.
//! - **No backdrop blur.** The Default skin's design blurs the video behind its pill and popovers; GPUI cannot blur
//!   what lies behind an element, so on every platform they keep the design's translucent black without the blur.
//! - **Not yet.** Track menus, captions and the poster come in later releases.

// GPUI's derive macros name the `gpui` crate; through this alias they resolve to the GPUI that GPUI-Kit re-exports.
extern crate gpui_kit as gpui;

/// Windows video needs GPUI patched with Windows surfaces: see the README's "Windows video" section. Stock GPUI lacks
/// this item, so without the patch the build fails here first, with its name.
#[cfg(windows)]
const _: () = gpui_kit::WINDOWS_SURFACES_REQUIRE_PATCHED_GPUI;

/// Linux video needs GPUI patched with Linux surfaces: see the README's "Linux video" section. Stock GPUI lacks this
/// item, so without the patch the build fails here first, with its name.
#[cfg(target_os = "linux")]
const _: () = gpui_kit::LINUX_SURFACES_REQUIRE_PATCHED_GPUI;

mod assets;
mod controls;
mod element;
mod fonts;
mod keys;
#[cfg(video)]
mod video;

pub use assets::{Assets, Icons};
pub use controls::{ToggleFullscreen, ToggleMute, TogglePlay};
pub use element::{Skin, VideoPlayer};

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
