## Why

`crates/player` links libmpv but cannot play anything yet: its only API is `client_api_version()`. Every frontend (first GPUI-Kit, later others such as egui) needs one toolkit-agnostic way to load media, control playback and observe its state, so that an app moving to another frontend only changes its GUI code. This is the first slice of the `add-player` series and delivers that core handle, headless and testable on CI, before any rendering or UI exists.

## What Changes

- Add `player::Player`, a cheap `Clone` + `Send` + `Sync` handle to one mpv instance. It is the only type apps use to drive playback, from any frontend and any thread.
- Add `Player::builder()...build()?`: initial autoplay (`Autoplay::{Off, On, Muted}`, as in Video.js `autoplay`), volume (`0.0..=1.0`, as in Video.js), muted, video output (`VideoOutput::Null` for now; the render API output arrives in slice 2) and raw mpv options for anything not yet typed. `build()` rejects invalid values instead of clamping them.
- Add `Source` (`Source::url`, `Source::file`) and `Player::load`.
- Add runtime control: play, pause, toggle pause, stop, seek (absolute and relative), volume, mute and playback rate. Runtime setters clamp out-of-range values, as Video.js does.
- Add an escape hatch for every mpv feature not yet typed: get/set any property and run any command, through the core crate, so frontends never touch libmpv.
- Add playback state: a `State` snapshot (position, duration, paused, volume, muted, rate, buffering, idle) kept up to date from observed mpv properties, and an event stream any number of subscribers can consume from any thread or async executor.
- Add the crate's error type (`player::Error`, `player::Result`) and a `prelude`, following the `media-rs` conventions.
- Add a headless example that plays a file or URL from the command line.
- Keep `client_api_version()` and `BINDINGS_CLIENT_API_VERSION` unchanged.

## Capabilities

### New Capabilities

- `playback-core`: the toolkit-agnostic player handle: construction and option validation, sources and loading, playback control, property and command access, the state snapshot, events, errors and shutdown.

### Modified Capabilities

None. (`libmpv-build`, from `add-workspace-scaffold`, is unchanged.)

## Impact

- **Crates**: only `crates/player`. Everything here is backend behaviour that every frontend shares, so it belongs in the core crate, which stays free of any GUI dependency. `crates/gpui` is untouched; it consumes this API in slice 4.
- **libmpv APIs**: `mpv_create`, `mpv_set_option_string`, `mpv_initialize`, `mpv_terminate_destroy`, `mpv_command`, `mpv_command_async`, `mpv_get_property`, `mpv_set_property`, `mpv_observe_property`, `mpv_wait_event`, `mpv_wakeup`, `mpv_error_string`, `mpv_free`. Commands: `loadfile`, `stop`, `seek`. Properties: `pause`, `volume`, `mute`, `speed`, `time-pos`, `duration`, `paused-for-cache`, `idle-active`. Options: `vo`, `ao`, `volume`, `mute`, `pause`, `idle`, `input-default-bindings`, `osc`.
- **Public API**: new types `Player`, `PlayerBuilder`, `Autoplay`, `VideoOutput`, `Source`, `Seek`, `State`, `Event`, `EndReason`, `Events`, `Error`, `Result`, and a `prelude` module. Nothing is removed.
- **Dependencies**: `thiserror` (error enum, as in `media-rs`) and `flume` (an event channel that works for both blocking threads and async executors). Both are justified in design.md.
- **rust-sak**: nothing in it covers this change. It has no mpv, channel or player functionality, and the core crate must stay publishable, which rules out a git dependency anyway. Nothing is added to `rust-sak`.
- **Platforms**: macOS, Windows and Linux, all six CI legs, with no platform-specific code. Without a GUI, no video is shown (`vo=null`); audio plays through mpv's default output. The tests use `ao=null` so CI needs no sound device.
- **Size**: estimated 14 tasks, the first slice of the `add-player` series (see design.md). Later slices add the render API, the toolkit-agnostic UI layer and the GPUI frontend.
