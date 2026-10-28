## Why

`player::Player` can play and render media, but every frontend still has to build the Video.js experience on its own: the control bar's idle hiding, the hotkeys, the time labels, the seek bar's scrubbing and the volume rules. If GPUI does that inside `crates/gpui`, a future egui frontend has to do it again, and the two will drift apart in subtle ways. This is slice 3 of the `add-player` series. It puts all of that UI logic in one toolkit-agnostic layer in the core crate, so slice 5 (the GPUI control bar) and any later frontend only draw what it describes and forward input to it.

## What Changes

- Add a public `player::ui` module to `crates/player`. It contains no drawing and depends on no GUI toolkit.
- Add `ui::Options`, built and validated like `Player::builder()`, holding the Video.js player options that concern the UI:
  - `controls` (default on)
  - `poster`
  - `playbackRates` (default none, which hides the rate button)
  - `aspectRatio` (`"16:9"` form, rejected when malformed, as Video.js does)
  - `inactivityTimeout` (default 2 s, with 0 meaning never)
  - `userActions.click` / `doubleClick` / `hotkeys`
- Add `ui::Action`, the user-level operations a control or key triggers:
  - play, pause, toggle play
  - seek to / by
  - toggle mute, set / step volume
  - set / cycle rate
  - toggle fullscreen
- Add `ui::Request`, for the actions only the frontend can carry out. For now this is fullscreen.
- Add the Video.js hotkey map over a toolkit-agnostic `ui::Key` + modifiers:
  - Global keys: `f` fullscreen, `m` mute, `k` / Space play-pause.
  - Seek-bar focus keys: ←/↓ and →/↑ seek ±5 s, PageDown/PageUp seek ±60 s, Home/End go to start/end, 0–9 jump to 0–90 %, Space/Enter toggle play.
  - Volume-bar focus keys: arrows change the volume by ±10 %.
  - **Deviation from Video.js, chosen deliberately**: global hotkeys are on by default, because this is a desktop player. They can be turned off or remapped. Keys pressed with Ctrl/Alt/Cmd are left to the app.
- Add Video.js time formatting (`m:ss`, or `h:mm:ss` when the duration is an hour or more, and `-:-` when unknown) and the remaining-time label (`-m:ss`).
- Add the Video.js user-activity ("idle") state machine:
  - The control bar hides only once playback has started, the user is inactive and the media is playing.
  - Hovering the control bar or holding a pointer button keeps the user active.
  - The cursor hides while the control bar is hidden.
  - The fade timings (fade-in 0.1 s, fade-out 1 s) are exposed as constants.
- Add seek-bar and volume-bar maths, following Video.js:
  - pointer position to fraction, and fraction to time, never landing exactly on the end while scrubbing
  - played and buffered fractions, and the hover time label
  - the volume icon level (0–3)
  - the mute-toggle rule that restores a volume of at least 10 % when unmuting from 0
- Add `ui::Controls`, the controller one frontend view owns per player. It:
  - takes input: pointer activity and holds, control-bar hover, video click and double-click, keys with their focus, seek and volume drags, button presses, and time ticks
  - takes the player's events
  - runs actions on the `Player`
  - returns a render-ready `ui::View`:
    - what is visible: control bar, cursor, big play button, poster, spinner, error
    - the play / pause / replay icon, the time labels, the played / buffered / hover fractions and the rate label
    - the next deadline at which the frontend should tick it
  - follows Video.js seek-bar dragging: pause while dragging, fast seeks while moving, an exact seek on release, then resume if the media was playing
- **playback-core additions** these need:
  - `State.buffered` with `Event::BufferedChanged`, from mpv's `demuxer-cache-time`, for Video.js's load-progress bar
  - `Player::scrub(Duration)`, a fast keyframe seek for dragging. `Seek` is not `#[non_exhaustive]`, so this is a method rather than a new variant, and nothing breaks.

## Capabilities

### New Capabilities

- `ui-core`: the toolkit-agnostic Video.js UI layer: UI options and their validation, actions and frontend requests, hotkeys, time labels, user activity and control visibility, seek and volume bar behaviour, and the controller's view of the player.

### Modified Capabilities

- `playback-core`:
  - "Playback control" gains fast seeking for scrubbing.
  - "Playback state snapshot" and "Event subscription" gain the buffered position and its change event.

## Impact

- **Crates**: only `crates/player`.
  - Everything here is UI logic that does not depend on a toolkit: which keys do what, when controls hide, how time is written, what a drag on the seek bar does to playback. Putting it in the core crate, which stays free of any GUI dependency, is what lets slice 5 and a later egui frontend share it unchanged.
  - The buffered position and fast seeking are backend features of `Player`, used by the UI layer.
  - `crates/gpui` is untouched; slice 5 consumes this.
- **libmpv APIs**: one new observed property, `demuxer-cache-time` (double), and one new command form, `seek <secs> absolute+keyframes`. Everything else goes through the existing `Player` methods.
- **Public API**: new `player::ui` module (`Options`, `OptionsBuilder`, `AspectRatio`, `Hotkeys`, `Key`, `Modifiers`, `Focus`, `Action`, `Request`, `Controls`, `View`, `PlayIcon`, `format_time`, `format_remaining`, the fade constants), new `State.buffered`, `Event::BufferedChanged` and `Player::scrub`. All are new items or additions to `#[non_exhaustive]` types, so nothing breaks.
- **Dependencies**: none.
- **rust-sak**: nothing in it covers this. Its modules are `crypto`, `fetch`, `fs`, `github`, `image`, `memo`, `o11y`, `sysinfo` and `tui`. `tui` is a terminal progress bar, and `image` decodes images, which the poster will need only once a frontend draws it (slice 6+). None of them has UI state, key mapping or time formatting, and none of that is general infrastructure that belongs there. The core crate must also stay publishable, which rules out a runtime git dependency. Nothing is added to `rust-sak`.
- **Platforms**: macOS, Windows and Linux alike. The layer is pure logic with no platform code, and its tests run headless on all six CI legs.
- **Size**: 14 tasks. This is slice 3 of the `add-player` series (see design.md). It depends only on slice 1, so it does not need slice 2.
