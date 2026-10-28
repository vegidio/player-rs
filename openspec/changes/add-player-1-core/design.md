## Context

`crates/player` links the static libmpv and has committed raw bindings (`player::sys`), but its only safe API is `client_api_version()` (see `add-workspace-scaffold`). See proposal.md for why this slice exists. The specs in `specs/playback-core/spec.md` define the required behaviour. This document covers how.

The overriding constraint is portability: an app must be able to switch frontends (GPUI-Kit now, egui or others later) and change only its GUI code. That is why every playback operation and every observation goes through a core type that knows nothing about any toolkit.

## Slices

The `add-player` series. **This change is slice 1.**

| # | Change | Delivers | Depends on | Status |
|---|---|---|---|---|
| 1 | `add-player-1-core` | `player::Player` handle: builder, sources, load, playback control, property/command access, state snapshot, event subscription; headless CI tests | `add-workspace-scaffold` | **current** |
| 2 | `add-player-2-render` | mpv render API in core: `VideoOutput::Render`, `mpv_render_context`, GL context/FBO handling, frame-ready callback | 1 | pending |
| 3 | `add-player-3-ui-core` | `player::ui`, toolkit-agnostic UI layer: `ui::Options` (controls, poster, playback rates, aspect ratio, hotkeys, idle timeout), `ui::Controls`, `Action` enum, Video.js hotkey map, time formatting, idle-hide state machine, seek-bar maths | 1 | pending |
| 4 | `add-player-4-gpui-macos` | `player_gpui::VideoPlayer` element showing video on macOS (IOSurface into GPUI), UI state keyed by element id | 2 | pending |
| 5 | `add-player-5-gpui-controls` | Video.js control bar in GPUI and `player_gpui::init(cx)` (actions and key bindings), driven by slice 3 | 3, 4 | pending |
| 6+ | (to be planned) | Windows and Linux video surfaces (after a spike on GPUI surface support), menus (rate, tracks), captions, fullscreen, poster, ... | 4, 5 | pending |

Slices 2 and 3 do not depend on each other and may be done in either order. Target app code once the series is complete:

```rust
// portable: unchanged when switching frontends
let player = Player::builder().autoplay(Autoplay::Muted).volume(0.8).build()?;
player.load(Source::url("https://example.com/movie.mp4")?)?;
let ui = ui::Options::builder().playback_rates([0.5, 1.0, 1.5, 2.0]).poster("poster.jpg").build()?;

// frontend-specific: the only lines that change
player_gpui::init(cx);
VideoPlayer::new("main-player", &player).options(ui.clone())
```

## Goals / Non-Goals

**Goals:**
- A public API shape that later slices only extend and never break: builder, handle, state, events.
- Correct resource lifetimes and threading around `mpv_handle` and its event loop.
- Full behaviour coverage on CI without audio devices, displays or media fixtures.

**Non-Goals:**
- Rendering, `vo=libmpv`, GL (slice 2). Any UI configuration or UI logic (slice 3).
- Typed track, chapter, playlist or subtitle APIs. They come later, and until then the raw property/command access covers them.
- Routing mpv's log messages anywhere. mpv's terminal output is off by default in libmpv. A logging facility like `media-rs`'s `log` module is left for a later change.
- `mpv_node` (structured) properties. Only flag, int64, double and string are supported for now.

## Decisions

### Public API

```rust
pub struct Player { inner: Arc<Inner> }        // Clone + Send + Sync

impl Player {
    pub fn builder() -> PlayerBuilder;
    pub fn load(&self, source: Source) -> Result<()>;
    pub fn play(&self) -> Result<()>;   pub fn pause(&self) -> Result<()>;
    pub fn toggle_pause(&self) -> Result<()>;    pub fn stop(&self) -> Result<()>;
    pub fn seek(&self, seek: Seek) -> Result<()>;  // Seek::To(Duration) | Seek::By(f64 seconds, signed)
    pub fn set_volume(&self, v: f64) -> Result<()>; pub fn set_muted(&self, m: bool) -> Result<()>;
    pub fn set_rate(&self, r: f64) -> Result<()>;
    pub fn state(&self) -> State;                 // cheap snapshot clone
    pub fn subscribe(&self) -> Events;
    pub fn get_property<T: PropertyValue>(&self, name: &str) -> Result<T>;
    pub fn set_property<T: PropertyValue>(&self, name: &str, value: T) -> Result<()>;
    pub fn command(&self, args: &[&str]) -> Result<()>;
}

PlayerBuilder: .autoplay(Autoplay) .volume(f64) .muted(bool)
               .video_output(VideoOutput) .mpv_option(name, value) .build() -> Result<Player>
```

- `Autoplay::{Off, On, Muted}` follows the Video.js `autoplay` values that make sense outside a browser. `'play'` and `'any'` exist only because of browser autoplay policies.
- `VideoOutput` is `#[non_exhaustive]`, with only `Null` in this slice, so slice 2 can add `Render` without breaking anything. The choice is made at build time because mpv only accepts `vo` before `mpv_initialize`.
- `PropertyValue` is a sealed trait implemented for `bool`, `i64`, `f64` and `String`, mapped to `MPV_FORMAT_FLAG/INT64/DOUBLE/STRING`.
- `Events` wraps a receiver with `recv()`, `try_recv()`, `recv_timeout()`, `recv_async()` and `Iterator`.
- `Event` and `State` are `Clone` and `#[non_exhaustive]`, so later slices can add events and fields. `EndReason::{Eof, Stopped, Replaced, Error(String)}`.

Builder style, `thiserror` error enum, crate-wide `Result` alias and `prelude` follow `media-rs`. The builder builds the player itself rather than a separate options struct, because creating mpv can fail and the error must reach the caller before any frontend is involved. (A frontend constructor called inside `cx.new(|cx| ...)` could not return it.)

Alternatives considered: a GPUI entity as the main type (rejected because it ties app code to one frontend), and a separate `PlayerOptions` struct (rejected because it is a second type with no extra value now that the builder is fallible).

### Handle, event thread and lifetimes

```
 Player (clone) --+
 Player (clone) --+--> Arc<Inner> { mpv: *mut mpv_handle, shared: Arc<Shared>, thread: JoinHandle }
                                         |
                    event thread --------+--> holds raw mpv ptr + Arc<Shared>, NOT Arc<Inner>
                    loop mpv_wait_event(-1):
                      property-change / file-loaded / end-file / seek / playback-restart
                        -> update Shared.state (RwLock)  -> then fan out Event to subscribers
                      on MPV_EVENT_NONE with Shared.stop set -> exit

 Shared { state: RwLock<State>, subscribers: Mutex<Vec<flume::Sender<Event>>>, stop: AtomicBool, pending_loads: AtomicU32 }
```

- `Inner` is dropped when the last `Player` clone goes. Its `Drop` sets `stop`, calls `mpv_wakeup` so `mpv_wait_event` returns, joins the thread, and only then calls `mpv_terminate_destroy`. That keeps the rule that no other thread is inside an mpv call on the handle when it is destroyed. The event thread never holds an `Arc<Inner>`, so there is no reference cycle and `Drop` is never run on the event thread (which would make the join deadlock).
- Slice 2 adds the render context to `Inner`. It must be freed in `Drop` before `mpv_terminate_destroy`, and this structure leaves room for that.
- `mpv_handle` functions are thread-safe except `mpv_wait_event`, which only the event thread calls. That is why `Player` is `Send + Sync`. The raw pointer is wrapped in a private newtype with a documented `unsafe impl Send + Sync`.
- **State before event**: the event thread writes the new value into `Shared.state` before sending the event. Every subscriber therefore sees a snapshot at least as new as the event it just received, as the spec requires.

### Initialisation and options

Options are applied with `mpv_set_option_string` before `mpv_initialize`, in this order:
1. Embedding defaults: `vo=null` (from `VideoOutput::Null`), `idle=yes`, `keep-open=yes`, `input-default-bindings=no`, `input-vo-keyboard=no`, `osc=no`. libmpv already defaults most of these, but setting them makes the behaviour explicit and independent of the mpv version.
2. Typed builder values: `volume` (`v * 100`), `mute`, `pause` (from autoplay).
3. Raw `mpv_option`s, last, so they can override anything (tests use `ao=null`).

A failing `mpv_set_option_string` becomes `Error::InvalidOption { name, message }`, with the message from `mpv_error_string`. If `mpv_create` returns null or `mpv_initialize` fails, the result is `Error::Init`, and the half-built handle is destroyed.

Typed values are validated in `build()` before mpv is touched: volume must be finite and in `0.0..=1.0`. Runtime setters clamp instead, as decided for the series: volume to `0.0..=1.0`, rate to `0.01..=100.0`, which is mpv's `speed` range.

### Loading, autoplay and end of media

- `load` checks the source, sets `pause` (and, for `Autoplay::Muted`, `mute=yes`) according to the autoplay mode, then sends `loadfile <src> replace` with `mpv_command_async`, so it never blocks on network I/O. Applying autoplay on every load matches Video.js, where `autoplay` applies to each new source.
- **End of media follows Video.js, not mpv's default.** With `keep-open=yes`, mpv keeps the file loaded and paused at its end instead of unloading it. The event thread watches `eof-reached` and emits `Ended(Eof)` when it becomes true. When `play()` finds `eof-reached` set, it seeks to 0 before unpausing, so the media restarts the way Video.js does after `ended`.
- **Replaced vs stopped**: mpv reports `MPV_END_FILE_REASON_STOP` in both cases. `load` increments `pending_loads` before sending `loadfile`. An END_FILE with reason STOP while a load is pending is reported as `Replaced`, and the matching FILE_LOADED or error END_FILE decrements the counter. `stop()` sends `stop`, which ends with `Stopped`.
- Errors that happen while loading arrive as END_FILE with reason ERROR. Its `error` code is turned into a message with `mpv_error_string` and reported as `Ended(Error(msg))`.

### Sources

`Source::url(&str) -> Result<Source>` accepts `scheme://...` with an RFC 3986 scheme, and mpv's `av://` device form. It does not try to fully parse URLs, which would need a URL crate for little gain, since mpv reports bad URLs through `Ended(Error)` anyway. `Source::file(impl AsRef<Path>) -> Result<Source>` rejects non-UTF-8 paths, because mpv takes UTF-8 strings on every platform. It also makes the path absolute (`std::path::absolute`, which does not touch the filesystem), so mpv's protocol detection never treats a relative name like `http://clip.mp4` as a URL.

### Controls with no media

`seek` and similar commands fail inside mpv when nothing is loaded. If `state().idle` is set, `Player` returns `Ok(())` without calling mpv. If the media unloads between that check and the command (a race), the resulting mpv command error is ignored. `play` and `pause` while idle just set the `pause` property, which is harmless, and the next `load` applies autoplay again anyway.

### Seeking

`Seek::To` sends `seek <secs> absolute+exact`. `Seek::By` sends `seek <secs> relative+exact`. Exact seeks match HTML/Video.js `currentTime` semantics, which the spec's 0.1 s tolerance relies on. Keyframe seeks would be faster, but they land on a different position than requested. Faster scrubbing while dragging the seek bar can be added later as a separate option.

### Observed properties to state and events

| mpv property (format) | State field | Event |
|---|---|---|
| `time-pos` (double) | `position: Option<Duration>` | `PositionChanged` |
| `duration` (double) | `duration: Option<Duration>` | `DurationChanged` |
| `pause` (flag) | `paused` | `PausedChanged` |
| `paused-for-cache` (flag) | `buffering` | `BufferingChanged` |
| `volume` (double, /100) | `volume` | `VolumeChanged` |
| `mute` (flag) | `muted` | `MutedChanged` |
| `speed` (double) | `rate` | `RateChanged` |
| `idle-active` (flag) | `idle` | `Idle` (on true) |
| `eof-reached` (flag) | `ended` | `Ended(Eof)` (on true) |

Plus events: `MPV_EVENT_FILE_LOADED` -> `Loaded`, `MPV_EVENT_END_FILE` -> `Ended(..)`, `MPV_EVENT_SEEK` -> `Seeking`, `MPV_EVENT_PLAYBACK_RESTART` -> `Seeked`. Unknown events are ignored.

### Event channel: `flume`

Each `subscribe()` creates an unbounded `flume` channel. Its sender goes into `Shared.subscribers`, and the event thread removes senders whose receiver was dropped. Events are cloned into each subscriber's channel.

| Option | Blocking recv | Async recv (any executor) | Verdict |
|---|---|---|---|
| `std::sync::mpsc` | yes | no | A GPUI or egui frontend would need a forwarding thread per player |
| `crossbeam-channel` | yes | no | Same problem |
| `tokio::sync::broadcast` | partly | Tokio only | Ties the core crate to one runtime |
| `async-channel` | `recv_blocking` | yes | Viable |
| **`flume`** | yes | yes (`recv_async`, `into_stream`) | **Chosen**: blocking and async are both first-class, it is small, and it is widely used |

`async-channel` would work just as well. `flume` was chosen for its fuller synchronous API (`recv_timeout`, iterators), which tests and headless apps use. `rust-sak` has no channel facility, and channels are not infrastructure in the sense of the config's list, so there is nothing to extend there.

### Errors

```rust
#[derive(Debug, thiserror::Error)] #[non_exhaustive]
pub enum Error {
    Init(String),                                   // mpv_create / mpv_initialize
    InvalidOption { name: String, message: String },// builder validation or mpv rejection
    InvalidSource(String),
    Property { name: String, message: String },
    Command { name: String, message: String },
}
```

mpv's messages come from `mpv_error_string`. Strings returned by `mpv_get_property` (MPV_FORMAT_STRING) are copied into a `String` and then released with `mpv_free`.

### Crates introduced

- `thiserror` 2: the error enum, as in `media-rs`.
- `flume` 0.12: see the event channel decision.
- Dev-only: `rust-sak` (git tag `26.10.1`, feature `fs`) for RAII temporary directories in tests. A git dev-dependency without a version is stripped by `cargo publish`, so the crate stays publishable. `tempfile` is not used because `rust-sak` `fs` already wraps it.

### Tests without fixtures

- Most integration tests load `av://lavfi:testsrc=duration=2:size=160x120:rate=25` with `ao=null` and `VideoOutput::Null`. mpv generates the media, so the tests need no files, display or sound device. `libavfilter` and `libavdevice` ship in every `binaries-mpv` archive.
- The local-file tests (`Source::file`, including the `http:` directory case on Unix) write a one-second PCM WAV into a `rust-sak` temp dir. The header is 44 bytes, written by hand, so no audio crate is needed.
- Waiting is always done with `recv_timeout` on events, never with sleeps, so slow CI legs cannot cause false failures.

### Backend / UI separation

Everything in this slice lives in `crates/player`, which has no GUI dependency. `Player` is the type both the GPUI frontend (slice 4) and any future egui frontend use unchanged: they subscribe to `Events` (async in GPUI, polled or blocking in egui), read `state()` while rendering, and call control methods in response to input. No frontend needs anything else from libmpv. GPUI-Kit vs raw GPUI does not apply here: there is no UI in this slice.

### Unsafe/FFI boundaries

All `unsafe` is in a private `mpv` module inside `crates/player`: handle creation and destruction, option/property/command calls (C string conversion with `CString`, so interior NULs become `Error::InvalidOption`, `InvalidSource`, `Property` or `Command`), reading `mpv_event` payloads on the event thread, and `mpv_free` of returned strings. Each block carries a `// SAFETY:` comment. The public API has no `unsafe`. No frames are involved, so the zero-copy rendering path is not affected.

## Risks / Trade-offs

- [Unbounded subscriber channels grow if a subscriber stops reading, since `time-pos` changes many times per second] → This is documented on `subscribe()`: drop `Events` when it is no longer read. Throttling `PositionChanged` (Video.js `timeupdate` fires about every 250 ms) can be added later if needed, without breaking the API.
- [`get_property` and `command` are synchronous and briefly wait for mpv's core lock] → Documented. Frontends read `state()` in render paths, and the control methods that touch the network (`load`) are asynchronous.
- [The `pending_loads` heuristic for `Replaced` could misclassify an END_FILE if the user sends `loadfile` through the raw `command`] → Documented. Raw commands bypass the typed bookkeeping.
- [Exact seeks are slower on long-GOP media] → They are acceptable for this slice. A fast-seek option can come with the seek bar UI.
- [`keep-open=yes` differs from the mpv CLI's default] → This is deliberate, for Video.js parity, and the raw `mpv_option("keep-open", "no")` override still works.
- [The lavfi `testsrc` may not be available in some archive] → The first integration test checks for it and fails with a clear message. All six CI legs run it.
- [A git dev-dependency on `rust-sak` could break `cargo publish` if Cargo's stripping behaviour differs from what we expect] → Run `cargo package -p player-rs --list` as part of verification. If it fails, fall back to a hand-written temp dir helper under `tests/` (not `tempfile`).

## Migration Plan

Not applicable. The change only adds API: `client_api_version()` and `BINDINGS_CLIENT_API_VERSION` stay as they are.
