## 1. crates/gpui: setup and plumbing

- [x] 1.1 Raise `gpui-kit` to `0.8.1` with the `component` and `assets` features. Add `player_gpui::Icons` (`icon_assets!` with `RotateCcw`, `Volume`, `Volume1`, `Volume2`, `VolumeX`) and `player_gpui::Assets`, which falls back to `gpui_kit::assets::Assets` and lists both sources merged and deduplicated (design decision 8). Verify with a test that every icon path the controls use loads from `Assets`, and that `cargo build -p player-gpui --all-targets` succeeds.
- [x] 1.2 Add the pure `Keystroke` → `ui::Key` + `ui::Modifiers` translation (design decision 5). Verify with unit tests covering every named key, characters in both cases, unknown keys (→ none) and each modifier.
- [x] 1.3 Move the keyed state to `element.rs` as `Slot { video (macOS), controls }`. Add a cached `ControlsView` that owns `ui::Controls` and an injected clock, has an event task that notifies only when `ui::View` changed, and has a deadline task re-armed after each render. Add `VideoPlayer::options` (design decisions 1–2). Verify that the existing slice-4 element tests still pass, and with new tests that:
  - a player event updates the view
  - `advance_clock` past the inactivity timeout hides the bar while playing
  - a different player under the same id rebuilds the controls with the new options, while the same player keeps the first options
- [x] 1.4 Apply fluid sizing from `options.aspect_ratio()` (design decision 7). Verify with tests: an 800-point-wide element with `16:9` lays out 450 points tall, and an element without an aspect ratio keeps the size it is given.

## 2. crates/gpui: controls

- [x] 2.1 Draw the control bar from `ui::View`: play/pause/replay, mute (level icons), the remaining-time label, the rate button (only with rates) and the fullscreen button. Buttons have tooltips and act through `Controls::perform`. Check the skin values against Video.js 8's `video-js.css` and record the ones used in code comments (design decision 3). Verify with tests that clicking play/pause/replay changes playback, that the rate button cycles `1x → 1.5x`, that two mute clicks restore 0.8, and that no rate button is drawn without rates.
- [x] 2.2 Add the bars' shared `track` helper and drag handling, and the seek bar: track, buffered, played, a hover growth and time label, and press/drag/release with window-level move/up listeners during a drag (design decision 3). Verify with tests: a drag while playing from 10 % to 50 % pauses and then resumes near 50 %; a drag released above the bar ends at the pointer's x; a hover at the middle of 90-second media shows `0:45`; with an unknown duration a click does not seek.
- [x] 2.3 Add the volume panel: a volume bar that expands on panel hover or bar focus, and drags through `volume_drag`. Verify with tests: hover expands it and leaving collapses it; dragging a muted player to 40 % gives volume 0.4, unmuted, at icon level 2; a drag continues outside the bar.
- [x] 2.4 Wire pointer activity, clicks and fading:
  - `pointer_moved/pressed/released/left` and `control_bar_hovered` go to `ui::Controls`
  - a click and a double-click on the video area only reach `video_clicked` and `video_double_clicked`, not clicks on controls
  - the bar fades with `with_animation` (`FADE_IN`/`FADE_OUT`) and has no hitboxes once faded out (design decision 4)

  Verify with tests: a video click toggles play, a click on a control does not, click disabled does nothing, a hidden bar takes no click, and a pointer move shows the bar again.
- [x] 2.5 Draw the overlays: the big play button (centred), the GPUI-Kit `Spinner` centred, and the error display. With controls off, draw none of them and no bar. Verify with tests: the big play button is centred, clicking it starts playback and hides it, the error message appears for an unopenable source with the bar hidden, and controls off draws nothing and ignores clicks.
- [x] 2.6 Add keyboard input and focus (design decision 5):
  - player, seek-bar and volume-bar focus handles; a click focuses the player
  - Tab order in control-bar order
  - `on_key_down` → `Controls::key` with the focused `ui::Focus`, with `cx.propagate()` when unhandled
  - Space/Enter on focused buttons
  - the `TogglePlay`, `ToggleMute` and `ToggleFullscreen` actions

  Verify with tests:
  - `k` pauses
  - the right arrow on the focused seek bar moves +5 s
  - hotkeys disabled lets `k` through to a parent handler
  - `cmd-f` / `ctrl-f` reaches the parent
  - a dispatched `ToggleMute` mutes
  - Tab reaches the seek bar
- [x] 2.7 Add window fullscreen (design decision 6): the `Off`/`Entering`/`On`/`OnWithoutWindow` state, `on_fullscreen_change`, Escape while fullscreen, `observe_window_bounds` for leaving by other means, and the enter/exit icon. Verify with tests on the test window:
  - the button and Escape toggle `is_fullscreen`, with callbacks `true` then `false`
  - a double-click and `f` enter fullscreen
  - toggling the test window out of fullscreen reports `false` and restores the enter icon
  - entering while the window is already fullscreen leaves the window fullscreen on exit
- [x] 2.8 Update the `play` example: register `player_gpui::Assets`, set playback rates `[0.5, 1, 1.5, 2]`, print `on_fullscreen_change`, and drop its own `TogglePause` action and binding. On macOS, run it on a real video and check by hand:
  - the bar's look against a Video.js 8 page side by side
  - fading after 2 s, then reappearing on movement
  - scrubbing
  - the volume panel
  - hotkeys
  - fullscreen in and out through the button, Escape and the window's green button, including the animated transition
- [x] 2.9 On Windows and Linux, verify that the headless control tests pass on their CI legs. Run the `play` example on each (a VM is acceptable) and check that the bar draws over black, play/pause/seek/volume work with audio, and window fullscreen enters and leaves.

## 3. Docs and verification

- [x] 3.1 Update the docs in `crates/gpui/src/lib.rs` and the README:
  - the required setup (`gpui_kit::init`, `player_gpui::Assets` or `Icons`)
  - `options` and `on_fullscreen_change`
  - the default-controls behaviour change and how to opt out
  - window fullscreen versus Video.js element fullscreen
  - the visible-cursor gap
  - the slice 6+ items still missing (menus, captions, poster)

  Verify that `cargo doc -p player-gpui --no-deps` builds without warnings and that the README example compiles as a doc test.
- [x] 3.2 Run `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`, and trigger the CI workflow. Verify that all six legs are green.
