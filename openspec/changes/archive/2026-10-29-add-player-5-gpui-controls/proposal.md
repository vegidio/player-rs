## Why

`player_gpui::VideoPlayer` shows video, but a GPUI app still gets no controls: no play button, seek bar, volume, time label or hotkeys. Slice 3 already implements every Video.js control rule in the toolkit-agnostic `player::ui` layer. This is slice 5 of the `add-player` series. It draws that layer's `ui::View` in GPUI and feeds GPUI input back into `ui::Controls`, so a GPUI app gets a working Video.js player from one element.

## What Changes

- **Video.js control bar over the video**, drawn from `ui::View` in Video.js's default-skin order and look:
  - play / pause / replay button
  - volume panel: a mute button with a 0–3 level icon, and a horizontal volume bar that expands on hover or focus
  - seek bar: played and buffered fractions, a hover time label, and Video.js press / drag / release scrubbing
  - remaining-time label, as Video.js's default skin shows it (current time and duration are hidden there)
  - playback-rate button, shown only when rates are configured. A click cycles the rate; the rate menu comes in a later slice.
  - fullscreen button
- **Overlays**:
  - the big play button, centred as in Video.js 8's default skin
  - the loading spinner
  - the error display
- **Fading and idle behaviour**: the control bar fades in over 0.1 s and out over 1 s, following `ui::View`'s visibility. `ui::Controls` is ticked at `View::next_deadline`, so inactivity hides the bar with no input.
- **Pointer input**: movement, presses, leaving and control-bar hover go to `ui::Controls`. A click on the video toggles play, and a double-click toggles fullscreen.
- **Keyboard**:
  - Clicking the player focuses it.
  - The seek bar and volume bar take focus separately, and Tab moves through the controls.
  - Key-down events are translated to `ui::Key` + `ui::Modifiers` and passed to `ui::Controls::key` with the matching `ui::Focus`. Keys it does not handle propagate to the app.
  - The `ui::Options` hotkeys therefore apply as configured: remapped, disabled, or with Ctrl/Alt/Cmd left to the app.
  - Public GPUI actions `TogglePlay`, `ToggleMute` and `ToggleFullscreen` let app menus drive the focused player.
- **Fullscreen**: GPUI cannot make one element fill the screen, so the player's fullscreen is window fullscreen (`Window::toggle_fullscreen`).
  - The fullscreen button, `f` and double-click toggle it. Escape leaves it, as a browser does.
  - The button shows enter or exit to match.
  - `.on_fullscreen_change(...)` tells the app, so it can lay out the player alone while fullscreen.
- **Element API**:
  - `VideoPlayer::options(ui::Options)` defaults to `ui::Options::default()`, which has controls on.
  - Options are read when the element's state is created or its player changes, as Video.js reads its options at construction.
  - With an aspect ratio set, the element's height follows its width, as in Video.js's fluid mode.
- **Icons**: the controls use Lucide icons through GPUI-Kit. The default GPUI-Kit icon bundle lacks the volume and replay icons. This crate therefore exports:
  - `player_gpui::Assets`: those icons plus a fallback to GPUI-Kit's default bundle
  - `player_gpui::Icons`: the extra icons alone, for apps that compose their own `AssetSource`
- **Not planned any more**: the `player_gpui::init(cx)` that slice 4's table planned. Keys go through `ui::Controls::key`, and GPUI actions need no registration, so there is nothing for it to do.
- **Known gap**: the cursor is not hidden while the user is inactive. GPUI has no public way to hide the cursor on demand: there is no hidden `CursorStyle`, and its own hiding happens only on typing. This is documented and left to a later slice.

## Capabilities

### New Capabilities

- `gpui-controls`: the Video.js controls of the GPUI player element. Covers:
  - control bar contents, look and fading
  - seek and volume bar interaction
  - big play button, spinner and error overlays
  - pointer and keyboard input, focus and actions
  - window fullscreen
  - options and fluid sizing
  - icon assets
  - platform coverage, and the hidden-cursor gap

### Modified Capabilities

None. The `gpui-player` requirements (video element, sharpness, repainting, keyed state, platform coverage) are unchanged. The controls are drawn over the video, and their requirements are in `gpui-controls`.

## Impact

- **Crates**: only `crates/gpui`.
  - All behaviour comes from `player::ui` (slice 3), so this slice is presentation and input translation only. That is exactly what the project puts in a GUI crate.
  - Nothing here would be shared by an egui frontend: GPUI keystroke translation, GPUI layout, GPUI-Kit icons, window fullscreen.
  - `crates/player` needs no change.
- **libmpv APIs**: none directly. Everything goes through `ui::Controls`, which drives the `Player`.
- **Public API** (`player_gpui`):
  - `VideoPlayer::options` and `on_fullscreen_change`
  - the `TogglePlay`, `ToggleMute` and `ToggleFullscreen` actions
  - the `Assets` and `Icons` asset sources
  - **Behaviour change**: a `VideoPlayer` without `.options(...)` now shows controls, the `ui::Options` default. Pass `ui::Options::builder().controls(false)` for bare video.
- **App setup**: the app must call `gpui_kit::init(cx)` and register `player_gpui::Assets`, or compose `player_gpui::Icons` into its own asset source. Without them, the GPUI-Kit components panic for lack of a theme, and the icons are blank.
- **Dependencies**:
  - `gpui-kit` gains its `component` and `assets` features, which most apps already have on (they are its defaults), with the version raised to `0.8.1` for the `icon_assets!` macro.
  - No other crate is added.
- **rust-sak**: nothing in it applies. Its modules are `crypto`, `fetch`, `fs`, `github`, `image`, `memo`, `o11y`, `sysinfo` and `tui`; none draws GPUI or translates GUI input. Nothing is added to it.
- **Platforms**: the controls, input and window fullscreen work on macOS, Windows and Linux. On Windows and Linux they sit over the black video area from slice 4, and audio and control work fully. Their tests run headless on all six CI legs. The hidden cursor is a gap on all three.
- **Size**: 15 tasks. This is slice 5 of the `add-player` series (see design.md). It depends on slices 3 and 4.
