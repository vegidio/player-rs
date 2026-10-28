## Context

`crates/player` has `Player`, a `Clone + Send + Sync` handle whose `state()` reads a snapshot without touching mpv and whose `subscribe()` gives an `Events` stream (slice 1). It also has the OpenGL render API (slice 2), which this slice does not use. `State` and `Event` are `#[non_exhaustive]`. `Seek` is not, so new seek modes cannot be variants. `crates/gpui` is still empty. See proposal.md for why the UI logic goes in the core crate. The `ui-core` spec and the `playback-core` delta define the required behaviour. This document covers how.

What this layer reproduces is defined by Video.js:
- **Options**: `controls`, `poster`, `playbackRates`, `aspectRatio`, `inactivityTimeout`, `userActions.{click,doubleClick,hotkeys}`.
- **Hotkeys**: `Player#handleHotkeys`, and the `handleKeyDown` handlers of `SeekBar` and `VolumeBar`.
- **Time labels**: `formatTime` and `RemainingTimeDisplay`.
- **User activity**: `Player#listenForUserActivity_`, plus the control-bar `mouseenter`/`mouseleave` override.
- **Visibility CSS**: `vjs-has-started`, `vjs-user-inactive`, `vjs-playing`, `vjs-waiting`, `vjs-seeking`, `vjs-ended`, `vjs-error`.
- **Controls**: `MuteToggle#handleClick`, `PlaybackRateMenuButton#handleClick`, and `SeekBar`'s mouse down/move/up.

## Slices

The `add-player` series. **This change is slice 3.**

| # | Change | Delivers | Depends on | Status |
|---|---|---|---|---|
| 1 | `add-player-1-core` | `player::Player` handle: builder, sources, load, playback control, property/command access, state snapshot, event subscription; headless CI tests | `add-workspace-scaffold` | done |
| 2 | `add-player-2-render` | mpv OpenGL render API in core over a host-provided GL context: `VideoOutput::Render`, `RenderContext` (loader, frame-ready callback, render into host FBO, `report_swap`), `State.video_size`; software-GL render tests on all six CI legs | 1 | done |
| 3 | `add-player-3-ui-core` | `player::ui`, toolkit-agnostic UI layer: `ui::Options` (controls, poster, playback rates, aspect ratio, hotkeys, inactivity timeout, click/double-click), `ui::Controls` controller and its `View`, `Action`/`Request`, Video.js hotkey map, time formatting, user-activity state machine, seek/volume bar maths; in `playback-core`, `State.buffered` and `Player::scrub` | 1 | **current** |
| 4 | `add-player-4-gpui-macos` | macOS video on screen: in core, an offscreen CGL context and an IOSurface-backed GL texture/FBO that slice 2's `RenderContext` draws into (toolkit-agnostic, reusable by e.g. egui-wgpu); in `crates/gpui`, the `player_gpui::VideoPlayer` element importing the IOSurface into GPUI, UI state keyed by element id; zero-copy `hwdec=videotoolbox` enabled and verified | 2 | pending |
| 5 | `add-player-5-gpui-controls` | Video.js control bar in GPUI and `player_gpui::init(cx)` (actions and key bindings): draws slice 3's `ui::View` and forwards input to `ui::Controls` | 3, 4 | pending |
| 6+ | (to be planned) | Windows and Linux video surfaces (offscreen WGL/EGL context and texture sharing in core, import in GUI crates, after a spike on GPUI surface support), per-platform zero-copy hwdec, menus (rate list, tracks), captions, fullscreen, poster drawing, skip buttons, touch behaviour, ... | 4, 5 | pending |

Compared with the plan in slice 2's table, this slice grows by two small backend pieces that its UI logic needs: `State.buffered` for the load-progress bar and `Player::scrub` for seek-bar dragging. Neither belongs in a GUI crate, so they come here rather than in slice 5. Slices 3 and 4 do not depend on each other.

## Goals / Non-Goals

**Goals:**
- One place for every Video.js interaction rule, so slice 5 and a later egui frontend only map input in and draw the `View` out.
- Deterministic, headless tests. All timing goes through an `Instant` the caller passes, so no test sleeps or depends on a clock.
- An API that later slices extend (menus, captions, skip buttons, touch) without breaking it: `#[non_exhaustive]` on `Action`, `Request`, `Key`, `Focus` and `View`.

**Non-Goals:**
- Drawing anything: icons, layout, fade animations, poster image decoding (slice 5, 6+).
- Video.js `controlBar` per-component configuration, skip buttons, the rate *menu* list (only the button's click-to-cycle is here), track menus, captions, picture-in-picture, live-stream UI, error modal text, localisation and ARIA labels.
- Touch behaviour (Video.js toggles user activity on tap instead of playing). Pointer and keyboard only for now.
- Throttling position updates for rendering. That is a frontend concern, and `PositionChanged` already exists.
- Focus traversal (Tab order). The frontend owns focus and reports it with each key.

## Decisions

### Module layout and public API

All of it lives in `crates/player/src/ui/` as the public module `player::ui`. It is not re-exported from the crate root or the prelude, so `ui::Options` does not clash with other names and UI code reads as `ui::...`.

```
ui/mod.rs        re-exports, FADE_IN (100 ms), FADE_OUT (1 s), SPINNER_DELAY (300 ms)
ui/options.rs    Options, OptionsBuilder, AspectRatio, Hotkeys
ui/time.rs       format_time(Option<Duration>, guide: Option<Duration>) -> String, format_remaining(..)
ui/keys.rs       Key, Modifiers, Focus, key -> Action mapping
ui/action.rs     Action, Request
ui/activity.rs   Activity (private): the user-activity state machine
ui/bars.rs       pointer fraction, fraction <-> time, played/buffered fractions, volume level, rate label
ui/controls.rs   Controls, View, PlayIcon, SeekHover
```

```rust
let ui = ui::Options::builder()
    .playback_rates([0.5, 1.0, 1.5, 2.0])
    .aspect_ratio("16:9")             // or AspectRatio::new(16.0, 9.0)
    .poster("poster.jpg")
    .inactivity_timeout(Duration::from_secs(2))
    .hotkeys(ui::Hotkeys::default().mute([Key::Char('x')]))
    .build()?;                       // Error::InvalidOption on bad rates / ratio

let mut controls = ui::Controls::new(&player, ui, now);
// frontend loop:
controls.handle_event(&event, now);              // every Event from its own subscription
controls.tick(now);                              // at view.next_deadline
let outcome = controls.key(Key::Char('f'), Modifiers::NONE, Focus::Player, now)?;  // KeyOutcome { handled, request }
controls.perform(ui::Action::TogglePlay, now)?;  // buttons: play toggle, big play, mute, rate
controls.seek_press(0.1, now)?; controls.seek_drag(0.5, now)?; controls.seek_release(0.5, now)?;
let view: ui::View = controls.view(now);         // everything to draw
```

- **The `Options` builder mirrors `PlayerBuilder`.** It is fallible, reports `Error::InvalidOption { name, message }` with the Video.js option name (`playbackRates`, `aspectRatio`), and validates in `build()`. `Options` is `Clone`, so one value can configure many players, as in the slice-1 target example.
- **`Hotkeys`** holds an `enabled` flag and three key lists (fullscreen, mute, play-pause), defaulting to Video.js's `f`, `m` and `k`/Space. `Hotkeys::disabled()` turns them off. Remapping one action replaces its list. Video.js does the same through its `fullscreenKey`, `muteKey` and `playPauseKey` functions.
- **`Request` keeps window control in the frontend.** It is `#[non_exhaustive]`, with only `ToggleFullscreen` for now. `perform`, `key` and `video_double_clicked` return it.

Alternatives considered: re-exporting `ui` types in the prelude (rejected: `Options` and `View` are generic names), and a cargo feature `ui` (rejected: the module is pure Rust with no dependencies and costs little to compile, and a feature would only add a test matrix).

### The controller: input in, `View` out

`Controls` is owned by one frontend view (a GPUI entity, or an egui widget's state). It holds:
- a `Player` clone and the `Options`
- the private `Activity`
- its own small state: `started`, `seeking_since`, `buffering_since`, `error`, `last_volume`, the seek `drag` and the hover fraction

Every method takes `&mut self` and runs on the frontend's UI thread.

- **Events come from the frontend, not from an internal subscription.** The frontend already needs a subscription to know when to redraw. A GPUI task awaits `recv_async()`, and egui polls `try_recv()` each frame. The frontend passes each event to `handle_event`. With a second, internal subscription, the frontend could not tell when it had been drained, so redraws and controller state could disagree. Feeding events in also lets the tests drive the controller.
- **`view(now)` combines `player.state()`, which needs no mpv call, with the controller's own state.** It returns:
  - `controls`, `control_bar_visible`, `cursor_hidden`, `big_play_button`, `poster: Option<String>` (shown only before the media starts), `spinner` and `error`
  - `play_icon` (`Play`, `Pause` or `Replay`)
  - `volume_fraction`, `volume_level` (0..=3) and `muted`
  - `current_time`, `duration` and `remaining` labels
  - `seekable`, `played`, `buffered` and `hover: Option<SeekHover { fraction, label }>`
  - `rate: Option<String>`
  - `next_deadline: Option<Instant>`

  The frontend schedules a `tick` (or simply a redraw) at `next_deadline`. That is the earliest of the inactivity timeout and the spinner delay, and `None` when nothing is pending.
- **Started tracking (`vjs-has-started`).** The rules:
  - On `Loaded`: `started = !state.paused`, since autoplay unpauses before `loadfile`, so no `PausedChanged(false)` follows. The error is cleared.
  - On `PausedChanged(false)` while media is loaded: `started = true`.
  - On `Idle` and on `Ended(Replaced)`: `started = false`.
  - A `Controls` created while media is already loaded starts with `started = !paused || position > 0`.
- **`last_volume`** follows `VolumeChanged` events whose value is non-zero, starting from the initial state. It provides the mute toggle's restore rule (`max(last, 0.1)`), as in `MuteToggle#handleClick`.
- **Spinner**: `buffering_since` is set by `BufferingChanged(true)` and `seeking_since` by `Seeking`. `BufferingChanged(false)`, `Seeked` and `Idle` clear them. The spinner shows when either has lasted `SPINNER_DELAY`, which reproduces Video.js's 0.3 s animation delay on `.vjs-waiting` and `.vjs-seeking`.
- **The error** is set by `Ended(Error(msg))`. It hides the control bar, the spinner and the big play button, as `.vjs-error` does.

### Actions

`Action` is `#[non_exhaustive]`: `Play`, `Pause`, `TogglePlay`, `SeekTo(Duration)`, `SeekBy(f64)`, `ToggleMute`, `SetVolume(f64)`, `StepVolume(f64)`, `SetRate(f64)`, `CycleRate`, `ToggleFullscreen`. `perform` maps each one to `Player` calls:
- `SetVolume` and `StepVolume` unmute first, as `VolumeBar#checkMuted` does.
- `StepVolume` reads the current volume from `state()`.
- `CycleRate` follows `PlaybackRateMenuButton#handleClick`: take the index of the current rate, use `(i + 1) % len`, or index 0 when the rate is not in the list. Rates are compared with a 1e-6 tolerance, so a rate that mpv stores and reports back with a tiny floating-point difference still matches its list entry.
- `ToggleFullscreen` returns `Some(Request::ToggleFullscreen)` and does nothing else.

Every `perform` also counts as user activity.

### Keys

`Key` is `#[non_exhaustive]`: `Char(char)`, `Space`, `Enter`, `Left`, `Right`, `Up`, `Down`, `Home`, `End`, `PageUp`, `PageDown`. `Modifiers { ctrl, alt, shift, platform }`, where `platform` is Cmd on macOS and Super/Win elsewhere. `Focus` is `#[non_exhaustive]`: `Player`, `SeekBar`, `VolumeBar`. The frontend translates its own key events; slice 5 does that for GPUI keystroke names.

Mapping, in order:
1. Any of `ctrl`, `alt` or `platform` set: unhandled. This deliberately differs from Video.js, whose `keycode.isEventKey` ignores modifiers, so that Cmd+F or Ctrl+M stay with the app. Shift is allowed, and `Char` matching ignores case, as with Video.js key codes.
2. `Focus::SeekBar`:
   - ←/↓ and →/↑: `SeekBy(∓5)`
   - PageDown/PageUp: `SeekBy(∓60)`, which is `stepSeconds` × `pageMultiplier` (5 × 12)
   - Home: `SeekTo(0)`; End: `SeekTo(duration)`
   - `'0'..='9'`: `SeekTo(duration × d/10)`
   - Space/Enter: `TogglePlay`

   Keys that need the duration are unhandled while it is unknown.
3. `Focus::VolumeBar`: ←/↓ and →/↑ map to `StepVolume(∓0.1)`.
4. Hotkeys enabled: look the key up in the three lists.
5. Otherwise unhandled.

The mapping is a pure function, `keys::map(key, modifiers, focus, &Hotkeys, duration) -> Option<Action>`, which `Controls::key` calls before `perform`. It is also public, so slice 5 can generate GPUI key bindings from the same table. A handled key counts as user activity. Video.js reports activity on any `keydown`, and so does `Controls::key`, handled or not.

### User activity

`Activity` follows `listenForUserActivity_`, but with deadlines instead of Video.js's 250 ms polling interval. That makes it deterministic and avoids a timer while nothing is pending.

```
state: last_input: Instant, held: bool (pointer button down), over_bar: bool, timeout: Duration
input(now)            -> last_input = now                       (move, key, click, perform, leave)
press(now)/release()  -> held = true/false, plus input(now)
bar_hover(b, now)     -> over_bar = b, plus input(now)
active(now)           =  timeout.is_zero() || held || over_bar || now < last_input + timeout
deadline()            =  None if timeout is zero, held or over_bar; else Some(last_input + timeout)
```

`Controls` exposes this as `pointer_moved`, `pointer_pressed`, `pointer_released`, `pointer_left` (Video.js `mouseleave` reports activity and ends a hold) and `control_bar_hovered`. Visibility is then derived:
- **Control bar hidden** when `!controls || error || !started || (!active && playing)`.
- **Cursor hidden** when `!active && playing`.

"Playing" means loaded, not paused and not ended. The fade timings are the constants `FADE_IN` and `FADE_OUT`, taken from Video.js's `transition: visibility 0.1s, opacity 0.1s` and `1s` rules. The frontend animates them.

### Seek bar and scrubbing

- **Pointer to fraction.** `bars::fraction(x, start, width)` clamps to `0..=1`, and returns 0 for a zero width.
- **Fraction to time.** `bars::time_at(fraction, duration)` gives `fraction × duration`, moved back by 0.1 s when it equals the duration (`SeekBar#calculateDistance` / `handleMouseMove`).
- **Played fraction.** `position / duration`. While a drag is active it is the drag fraction, so the bar follows the pointer even while mpv catches up, as Video.js uses its cached scrub time.
- **Buffered fraction.** `min(buffered, duration) / duration`. It is 0 when unknown.
- **Hover label.** `seek_hover(Some(fraction))` stores the fraction. `View.hover` writes `format_time(time_at(f), duration)`.
- **Drag.**
  - `seek_press(f)` records `was_playing = !paused && !ended`, pauses, and scrubs to `time_at(f)`.
  - `seek_drag(f)` scrubs.
  - `seek_release(f)` seeks exactly with `Seek::To(time_at(f))` and plays if `was_playing`.

  These are inert while the duration is unknown.
- **`Player::scrub(Duration)`** sends `seek <secs> absolute+keyframes`, with the same idle no-op rule as `seek`. Keyframe seeks are what make dragging fluid on long-GOP media; mpv's own OSC uses the same flag. Rapid scrubs do not queue up, because mpv coalesces pending seeks and executes only the latest. The final exact seek on release gives the precise landing point the spec requires.

Alternatives: exact seeks while dragging (rejected: they stall on long-GOP 4K media, which this project targets), and a `Seek::Keyframe` variant (rejected: `Seek` is not `#[non_exhaustive]`, so adding a variant would break exhaustive matches downstream).

### Buffered position

A new observed property, `demuxer-cache-time` (`MPV_FORMAT_DOUBLE`, seconds of media time), goes into the existing `Property` table. It becomes `State.buffered: Option<Duration>` and `Event::BufferedChanged(Option<Duration>)`, through the same `set` helper as `duration`, so nothing new is needed in the event thread. mpv leaves the property unavailable when it has no demuxer cache for the media, and that becomes `None`. Video.js shows buffered *ranges*. mpv's single cached-ahead end is what matters for a forward load-progress bar, and `demuxer-cache-state` (with `seekable-ranges`) is an `mpv_node`, which the crate does not decode yet (slice 1 non-goal).

### Time formatting

`format_time` is a direct port of Video.js's `defaultImplementation`. Seconds are floored, negative values become 0, and unknown or infinite times produce dashes in the guide's number of fields. Hours are shown when `h > 0 || guide_h > 0`. Minutes are zero-padded when hours are shown or `guide_m >= 10`. `format_remaining(position, duration)` is `"-" + format_time(floor(duration) - floor(position), duration)`, as `RemainingTimeDisplay` computes it. The rate label is `format!("{rate}x")`. Rust's `Display` for `f64` already prints `1` and `1.5`, the way JavaScript's number-to-string does.

### Aspect ratio

`AspectRatio` stores positive finite `f64` width and height. Parsing accepts Video.js's pattern, `^\d+(\.\d+)?:\d+(\.\d+)?$`, and rejects zero. `Options::player_height(width, video_size)` uses the configured ratio, then the video's `(w, h)`, then 16:9, which is Video.js's fluid default before metadata. The video's own letterboxing inside the player stays mpv's job (slice 2).

### Backend / UI separation

Everything is in `crates/player` and uses only `std` and the existing `Player`, `State` and `Event`:
- no GUI types: keys, focus and pointer positions are this crate's own plain types
- no drawing
- no threads, since `Controls` runs wherever the frontend calls it

A future egui frontend reuses the whole layer unchanged. It maps `egui::Key` to `ui::Key`, the hovered rect to fractions, and `ctx.input` time to `Instant`, then paints the `View`. GPUI-Kit vs raw GPUI does not apply to this slice: it has no GUI code. Slice 5 makes that choice for the control bar.

### Threading, FFI and lifetimes

- **No new `unsafe`.** The only FFI-adjacent changes are a new row in the existing observed-property table and a new command string through the existing safe `command` helper.
- **Threading.** `Controls` is `Send` and is used through `&mut self` from one thread: the frontend's UI thread. It calls `Player` methods synchronously. Those take mpv's core lock briefly, as any button click in slice 5 would anyway. Slice 2's render rules still apply: `view()` only reads `state()`, so calling it from a render path never waits on mpv.
- **Lifetimes.** `Controls` holds a `Player` clone. Dropping the last `Controls` and `Player` shuts the player down as before, and `Controls` adds no threads or callbacks.

### Zero-copy

No frames are involved. The rendering path from slice 2 is untouched.

### rust-sak and crates

No `rust-sak` feature is used: none covers UI state, key mapping or time formatting, and this is not general-purpose infrastructure that should go there. No new crates are added. Formatting and parsing are a few lines of `std`. A regex crate for the aspect-ratio pattern is not worth a dependency.

### Testing

- **Unit tests** (in-crate, no mpv) cover `format_time` against a table taken from Video.js's own `format-time.test.js` cases, plus:
  - `format_remaining` and the rate labels
  - `AspectRatio` parsing and `player_height`
  - options validation
  - the full key map, including modifiers, remapping and disabled hotkeys
  - the `Activity` state machine, with synthetic `Instant`s
  - seek and volume bar maths
  - the cycle and mute-restore rules
- **Integration tests** (`tests/ui_controls.rs`) drive a real headless `Player` (`ao=null`, `VideoOutput::Null`) through `Controls`. They feed it the player's own events through `recv_timeout`, and pass `Instant`s offset by the test, so idle timeouts need no sleeps. They use the two-second WAV the existing tests write, for seekable media with a known duration. They cover:
  - clicks, hotkeys and slider keys acting on the player
  - drag (pause, scrub, exact landing, resume)
  - drag to the end
  - started and replay icon state across load, play and end
  - spinner and error from an unplayable source
  - visibility over time
- The buffered-position test loads the WAV with `cache=yes`, which forces mpv's demuxer cache on for a local file.

## Risks / Trade-offs

- [`demuxer-cache-time` may stay unavailable for local files with mpv's default `cache=auto`, so the buffered bar shows nothing for them, while browsers show local media as fully buffered] → The test forces `cache=yes`. Task 1.1 checks what default-cache local playback reports and records it in the docs. If it is `None`, `View.buffered` stays 0, which is accurate (nothing is known to be cached). A later change can show local media as fully buffered if that turns out to matter.
- [Frontends forget to forward an event, and `Controls`' started, spinner or error state drifts from the player's] → Document that every event must be forwarded. Make `handle_event` take every `Event` variant, ignoring those it does not use, so forwarding everything is the easy path. The integration tests forward exactly what a frontend would.
- [Deadline-based activity differs from Video.js's 250 ms polling by up to 250 ms] → Video.js's actual hide time varies by up to that much anyway. The spec states the timeout, not the polling.
- [Ignoring modified keys differs from Video.js] → This is deliberate (the hotkeys question) and stated in the spec. Shift still works.
- [Global hotkeys on by default differ from Video.js] → This is the user's decision for a desktop player, stated in the proposal and spec. `Hotkeys::disabled()` restores Video.js behaviour.
- [Keyframe scrubbing lands up to a GOP away during the drag] → Only the preview position is affected. Release always does an exact seek.
- [Rate comparisons with floating point] → A 1e-6 tolerance in `CycleRate`, covered by a unit test that round-trips rates through mpv.

## Migration Plan

Not applicable. Everything is added. `State` and `Event` are `#[non_exhaustive]`, and `Player::scrub` is a new method.
