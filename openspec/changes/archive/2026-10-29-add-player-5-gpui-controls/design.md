## Context

What already exists:
- **`player::ui` (slice 3).** `ui::Controls` owns every Video.js interaction rule:
  - Inputs: `handle_event`, `tick`, `perform`, `key(key, modifiers, focus, now)`, `pointer_moved/pressed/released/left`, `control_bar_hovered`, `video_clicked`, `video_double_clicked`, `seek_hover/press/drag/release`, `volume_drag`.
  - Output: `ui::View`, which describes everything to draw and has a `next_deadline`.
  - Every method takes `now: Instant`; the layer has no clock of its own.
  - `ui::Options` is `Clone` and is read at construction.
  - `ui::bars::fraction(x, start, width)` maps a pointer to a fraction.
  - `Request::ToggleFullscreen` is what fullscreen actions return.
- **`player_gpui::VideoPlayer` (slice 4).** A `RenderOnce` element: a black `div` with the element id.
  - On macOS it adds `video::view`, which keeps a `Slot { view: Entity<VideoView> }` in `window.use_keyed_state(id)`. It replaces the view when `Player::ptr_eq` says the player changed, and renders it `.cached(size_full)`.
  - Each video frame notifies `VideoView`. That re-renders the window's views from the root down, so whatever sits beside the video must be cheap or cached.
- **`Player::subscribe()`** returns `Events`, a flume-backed receiver with `recv_async`.
- **GPUI-Kit 0.8.1** (`gpui-pre` 0.3.10). Verified in source:
  - `component` gives `Icon`, `Spinner`, `Tooltip` and `Slider`. `Slider` emits only value changes, with no press or release, and has no buffered layer or hover label.
  - `assets` gives Lucide icons. The default `Assets` bundle has `play`, `pause`, `maximize`, `minimize` and `loader-circle`, but not `rotate-ccw` or `volume*`. Those are in the full catalogue and can be embedded one by one with `icon_assets!`.
  - Apps register assets with `application().with_assets(...)`, and `gpui_kit::init` initialises the component theme.
  - GPUI has `Window::toggle_fullscreen` and `is_fullscreen`, `with_animation`, `tab_index`, `track_focus`, `on_key_down`, `on_action`, and `Context::observe_window_bounds`.
  - `CursorStyle` has no hidden variant. GPUI hides the cursor only on typing (`CursorHideMode`), through a platform method that is not public.

See proposal.md for why. Required behaviour is in `specs/gpui-controls`. This document covers how.

## Slices

The `add-player` series. **This change is slice 5.**

| # | Change | Delivers | Depends on | Status |
|---|---|---|---|---|
| 1 | `add-player-1-core` | `player::Player` handle: builder, sources, load, playback control, property/command access, state snapshot, event subscription; headless CI tests | `add-workspace-scaffold` | done |
| 2 | `add-player-2-render` | mpv OpenGL render API in core over a host-provided GL context: `VideoOutput::Render`, `RenderContext` (loader, frame-ready callback, render into host FBO, `report_swap`), `State.video_size`; software-GL render tests on all six CI legs | 1 | done |
| 3 | `add-player-3-ui-core` | `player::ui`, the toolkit-agnostic UI layer: `ui::Options`, `ui::Controls` controller and its `View`, `Action`/`Request`, Video.js hotkey map, time formatting, user-activity state machine, seek/volume bar maths; in `playback-core`, `State.buffered` and `Player::scrub` | 1 | done |
| 4 | `add-player-4-gpui-macos` | macOS video on screen: `player::macos::SurfaceRenderer` (own render thread and CGL context, IOSurface NV12 ring, GPU-fenced publication), `hwdec=videotoolbox` default, `Player::ptr_eq`; the `player_gpui::VideoPlayer` element (keyed state, repaint on frame, device-pixel sizing, `paint_surface`; black on Windows/Linux) and the `play` example | 2 | done |
| 5 | `add-player-5-gpui-controls` | Video.js controls in GPUI over `VideoPlayer`, drawn from `ui::View` with input forwarded to `ui::Controls`: control bar (play, volume panel, seek bar, remaining time, rate cycle, fullscreen), big play button, spinner, error display, fading, pointer/keyboard/focus, `TogglePlay`/`ToggleMute`/`ToggleFullscreen` actions, window fullscreen with `on_fullscreen_change`, fluid aspect ratio, `player_gpui::Assets`/`Icons` | 3, 4 | **current** |
| 6+ | (to be planned) | Windows and Linux video surfaces (offscreen WGL/EGL context and texture sharing in core, import in GUI crates, after a spike on GPUI surface support there), per-platform zero-copy hwdec; menus (rate list, tracks), captions, poster drawing, skip buttons, touch behaviour; hiding the cursor when inactive (needs a GPUI API); BGRA/HDR surfaces if GPUI gains them | 4, 5 | pending |

Changes from the plan in slice 4's table:
- **No `player_gpui::init(cx)`.** Keys go through `ui::Controls::key` (decision 5), and GPUI actions register themselves, so an init function would have nothing to do.
- **Fullscreen moves into this slice** (decision 6). The fullscreen button, `f` and double-click are part of Video.js's control bar, and window fullscreen is small.
- **Cursor hiding moves to 6+.** GPUI has no API for it (see Context).

## Goals / Non-Goals

**Goals:**
- Everything the user sees or does in the controls comes from `ui::View` or goes into `ui::Controls`. The GPUI code decides only layout, look and the translation of GPUI input.
- The controls never slow the video path. They do not re-render for video frames, and they re-render for player events only when `ui::View` changed.
- Deterministic headless tests on all six CI legs, with no real-time sleeps for activity timeouts.

**Non-Goals:**
- Menus, captions, poster drawing, skip buttons, touch, live UI: these are slice 6+.
- Theming the controls with the app's GPUI-Kit theme. The look is Video.js's default skin, not the app's.
- Localisation of tooltips and labels. They are English, as Video.js's default language.

## Decisions

### 1. One keyed slot per element, holding a video view and a controls view

`VideoPlayer::render` keeps `Slot { video: Entity<VideoView> (macOS only), controls: Entity<ControlsView> }` under the element id. This moves the keyed state from `video.rs` to `element.rs`. When `Player::ptr_eq` says the player changed, both entities are rebuilt, so options are re-read then. The element is laid out as a root `div` with these layers, bottom to top:
1. the video view (cached)
2. the controls view (cached), which holds the click target, the overlays and the control bar

The controls view is rendered `.cached(size_full)`, so video frames, which re-render the views from the root down, do not re-render it.

The one per-render input that is not an option, the `on_fullscreen_change` callback, is written into `ControlsView` on each element render without notifying it.

*Alternative*: a single entity owning both. Rejected: video frames would then re-render the control tree at the frame rate.

### 2. `ControlsView` owns `ui::Controls` and its clock

`ControlsView` holds:
- `ui::Controls`
- the last drawn `ui::View`
- three focus handles: player, seek bar and volume bar
- the fullscreen state, the fade generation, and the bar bounds measured at prepaint
- two tasks: one for player events and one for the deadline

How it is driven:
- **Events.** The event task loops on `Events::recv_async()` on GPUI's foreground executor. It calls `handle_event(&event, now)`, recomputes `view(now)`, and `cx.notify()`s only if the view differs (`View: PartialEq`).
  - `PositionChanged` arrives about once per video frame, but the view changes only when the played fraction or a label does.
  - All `ui::Controls` calls therefore stay on the GUI thread. `Player` is thread-safe, and nothing here is `unsafe`.
- **Deadline.** After each render, if `view.next_deadline` differs from the armed one, the deadline task is replaced. Its timer calls `tick(now)` and then notifies.
- **Clock.** `now` comes from GPUI's `BackgroundExecutor::now()`, the same executor that runs the deadline timers:
  - in the app, it is the real clock
  - in tests, it is the test executor's fake clock, which `advance_clock` moves, so timers and `Instant`s move together

### 3. Raw GPUI for bars and buttons; GPUI-Kit for icons, tooltips and spinner

- **GPUI-Kit `Icon`** draws every icon, as Lucide SVGs coloured white. **`Tooltip`** gives the control names, and **`Spinner`** is the loading spinner, sized and coloured to Video.js's.
- **The seek bar and the volume bar are raw GPUI** (`div` + `canvas` for bounds). GPUI-Kit's `Slider` has no buffered layer and no hover label. It reports only value changes, while `ui::Controls` needs press, drag and release separately for Video.js's pause-while-scrubbing. Its thumb ring and theme colours are also not Video.js's.
  - Both bars share their drawing and their drag handling:
    1. A `track` helper draws the slider background with its margins and records its bounds at prepaint. The bars put their fills (`fill`) and, for the seek bar, the knob and hover label in it.
    2. Each bar's own `on_mouse_down` starts a drag and reports a press at `bars::fraction(x, bounds.left, bounds.width)`.
    3. Window-level `MouseMoveEvent` and `MouseUpEvent` listeners, in the capture phase, continue the drag outside the bar and the element. They are registered in paint at all times, because the cached view is not repainted when a press starts, and they act only while a drag or a press is in progress.
    4. Moves report `seek_drag` or `volume_drag`. Release reports `seek_release`.
- **Buttons are raw `div`s with an id**, with `on_click`, `tab_index` and a tooltip. GPUI-Kit's `Button` brings themed padding, backgrounds and hover states that would have to be undone to look like Video.js's bare icon buttons.

The look follows Video.js 8's default skin (`video-js.css`), at its 10 px base font size. The values below are to be checked against that stylesheet during implementation:

| Part | Look |
|---|---|
| Control bar | 30 px high, `rgba(43, 51, 63, 0.7)`, full width, at the bottom |
| Buttons | 40 px wide, white icon about 18 px |
| Seek bar | track 3 px, 5 px on hover, in `rgba(115, 133, 159, 0.5)`; buffered `rgba(115, 133, 159, 0.75)`; played white, ending in a white 9 px circle (15 px on hover); hover label white text on `rgba(0, 0, 0, 0.8)` above the pointer |
| Volume panel | bar collapsed to 0 width; expands to about 50 px on panel hover or bar focus; same colours as the seek bar |
| Big play button | about 90×49 px, `rgba(43, 51, 63, 0.7)`, white 2 px border, rounded; centred (Video.js 8; the top-left default and `vjs-big-play-centered` were Video.js 7) |
| Error display | Video.js's modal background, a gradient from `rgba(0, 0, 0, 0.8)` at the top to transparent at the bottom, over the video; white message at 14 px, centred |

### 4. Fading with `with_animation`

When `control_bar_visible` changes, `ControlsView` bumps a fade generation. The bar is wrapped in `with_animation((bar, generation), Animation::new(FADE_IN or FADE_OUT), ...)`, which animates opacity from the current direction's start.

Once a fade-out has finished, the bar is rendered without its hitboxes, so a hidden bar takes no pointer input. The tests check hit-testing, not opacity.

### 5. Keys: translate and forward to `ui::Controls::key`

The root `div` handles keys:
- It `track_focus`es the player handle. A mouse-down anywhere in the element focuses it, unless a focusable control is the target.
- `on_key_down` translates `Keystroke` into `ui::Key` + `ui::Modifiers` in a pure function:
  - `key` names `space`, `enter`, `left`, `right`, `up`, `down`, `home`, `end`, `pageup`, `pagedown` map to the named keys
  - a single character maps to `Key::Char`
  - modifiers map as `control → ctrl`, `alt → alt`, `shift → shift`, `platform → platform`
- `Focus` comes from which handle is focused: seek bar, volume bar, or player.
- If `KeyOutcome::handled` is false, it calls `cx.propagate()`.

Other keyboard pieces:
- **Buttons.** Each button's own `on_key_down` handles Space and Enter first, as a focused button would, and stops propagation.
- **Escape.** It is handled at the root only while the player is fullscreen.
- **Actions.** `actions!(player_gpui, [TogglePlay, ToggleMute, ToggleFullscreen])` is handled with `on_action` at the root through `ui::Controls::perform`. GPUI's action registry is static, so nothing has to be registered.

*Alternative*: GPUI key bindings in a `VideoPlayer` key context, the original slice-4 plan. Rejected: the `ui::Options` hotkeys (remapped, disabled, Ctrl/Alt/Cmd left alone) and the focus-dependent seek and volume keys would have to be re-expressed as a GPUI keymap, which is a second source of truth. The user chose this approach.

### 6. Fullscreen is window fullscreen, tracked per player

`ControlsView` carries a fullscreen state: `Off`, `Entering`, `On`, or `OnWithoutWindow`. The last one covers a player that entered while its window was already fullscreen.
- **`Request::ToggleFullscreen` while `Off`.** If the window is not fullscreen, call `window.toggle_fullscreen()` and become `Entering`. Otherwise become `OnWithoutWindow`. Either way, call the callback with `true`.
- **Leaving.** Toggle the window only if this player made it fullscreen, then call the callback with `false`.
- **Leaving by other means.** `cx.observe_window_bounds` checks `window.is_fullscreen()`:
  - `Entering` becomes `On` once the window reports fullscreen. macOS animates the transition, so `is_fullscreen` lags the toggle.
  - `On` going to not-fullscreen means the window left by other means: become `Off` and call the callback with `false`.

The fullscreen control's icon shows exit for every state but `Off`. The callback is `Rc<dyn Fn(bool, &mut Window, &mut App)>`.

### 7. Fluid sizing

With `options.aspect_ratio()` set, the root gets GPUI's `aspect_ratio(width / height)` style. The app's own style is refined over it, so an explicit height still wins, as CSS would. Without an aspect ratio, nothing is added.

`ui::Options::player_height`'s fallbacks (video size, then 16:9) are not used. In Video.js they apply only with `fluid: true`, which `ui::Options` does not have.

### 8. Icon assets

Video.js's volume levels map to Lucide icons as follows. The fullscreen icons come from GPUI-Kit's default bundle; the others are extras.

| Use | Lucide icon |
|---|---|
| Play | `Play` |
| Pause | `Pause` |
| Replay | `RotateCcw` |
| Volume level 0 | `VolumeX` |
| Volume level 1 | `Volume` |
| Volume level 2 | `Volume1` |
| Volume level 3 | `Volume2` |
| Enter fullscreen | `Maximize` |
| Exit fullscreen | `Minimize` |

The crate exports two asset sources:
- **`player_gpui::Icons`**: `icon_assets!(pub Icons, [RotateCcw, Volume, Volume1, Volume2, VolumeX])`.
- **`player_gpui::Assets`**: an `AssetSource` that loads from `Icons`, then `gpui_kit::assets::Assets`. Its `list` merges both, sorted and deduplicated, as the GPUI-Kit README's composition example does.

The `gpui-kit` dependency becomes `{ version = "0.8.1", default-features = false, features = ["component", "assets"] }`. These are its default features, so apps already have them on and nothing new is compiled for them.

### 9. Backend/UI separation and reuse

`crates/player` is not touched. Everything an egui frontend would need is already in `player::ui`:
- the key map
- the activity rules
- the scrubbing and volume rules
- the labels, the icon level and the visibility

What this slice adds is GPUI-only: keystroke translation, layout, the Lucide asset wiring, window fullscreen and animation. An egui crate would reuse `player::ui` unchanged and write its own versions of exactly these.

The zero-copy video path is unchanged. The controls draw over the cached video view and never touch frames.

### 10. Tests

The tests are GPUI headless tests (`TestAppContext` / `VisualTestContext`) in `crates/gpui`, on all six CI legs:
- **Player setup.** The player uses `ao=null` and an audio-only lavfi source (`av://lavfi:sine=...`) or the WAV fixture pattern from the core tests, so they need no GPU. The video layer is the black `div` off macOS, and the software-GL view on macOS.
- **What they cover:**
  - pointer and key simulation (`simulate_mouse_move`, `simulate_click`, `simulate_keystrokes`), asserting on player state and on test hooks into `ControlsView`: last view, focus, fullscreen state and bar hit-testing
  - activity timeouts, through the injected clock and `advance_clock`
- **Fullscreen.** The test platform's window flips its `is_fullscreen` on `toggle_fullscreen`. The fullscreen tests assert on that, and leave fullscreen by toggling the test window directly.

## Risks / Trade-offs

- **The control bar re-renders about once per video frame while it is visible**, because the played fraction changes. → The tree is small and cached apart from the video. Video.js redraws its seek bar at a similar rate. If profiling shows a cost, round the played fraction to the bar's width in device pixels before comparing views.
- **Lucide icons differ from Video.js's icon font** (the user chose GPUI-Kit). → The sizes, colours and layout are Video.js's, so the difference is limited to the glyph shapes.
- **Apps that skip `gpui_kit::init` or the asset source get panics or blank icons.** → Document both in the crate docs and README, and do both in the example. The `Assets` source makes the asset part a single line.
- **Window fullscreen is not element fullscreen.** → `on_fullscreen_change` lets the app show the player alone. This is documented as a difference from Video.js.
- **`toggle_fullscreen` is asynchronous on macOS.** → Handled by the `Entering` state (decision 6). The test platform toggles synchronously and cannot show the lag, so it is checked by hand on macOS (task 2.8).
- **The cursor stays visible while inactive.** → Documented gap, listed for 6+. Calling `NSCursor` directly would desynchronise GPUI's own cursor-visibility tracking, so it is not done.
- **The skin values come from Video.js's stylesheet as recalled.** → Task 2.1 checks them against `video-js.css` before the look is final, and task 2.8 compares the result by eye.

## Migration Plan

- **Behaviour change.** A `VideoPlayer` without `.options(...)` now shows controls. To keep bare video, pass `ui::Options::builder().controls(false).build()`. This is stated in the crate docs and the README.
- **App setup.** Apps must call `gpui_kit::init(cx)`, which existing GPUI-Kit apps already do, and register `player_gpui::Assets`.
- **Example.** The `play` example drops its own `TogglePause` action and Space binding, because the hotkeys do this now.
