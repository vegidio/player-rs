## 1. crates/player - foundations

- [ ] 1.1 Add `thiserror` 2 and `flume` 0.12 to `[dependencies]` and `rust-sak` (git tag `26.10.1`, `default-features = false`, feature `fs`) to `[dev-dependencies]`; add `src/error.rs` (`Error` per design, `Result` alias, mpv code -> message via `mpv_error_string`) and `src/prelude.rs`; verify `cargo build -p player-rs` and `cargo package -p player-rs --list` succeed
- [ ] 1.2 Add the private `mpv` FFI module: `Send + Sync` handle newtype, `CString` conversion helpers, option setting, sealed `PropertyValue` for `bool`/`i64`/`f64`/`String` (get/set with `mpv_free` of returned strings) and sync/async command helpers, each `unsafe` block with a `// SAFETY:` comment; verify `cargo clippy -p player-rs --all-targets -- -D warnings` is clean
- [ ] 1.3 Add `Autoplay`, `VideoOutput` (`#[non_exhaustive]`, `Null`) and `PlayerBuilder` with build-time validation and the option order from design (embedding defaults, typed values, raw `mpv_option`s last), destroying the handle on any init failure; verify unit tests for the default player, out-of-range/NaN volume and an unknown raw option pass
- [ ] 1.4 Add `Source::url` (scheme or `av://` check) and `Source::file` (UTF-8 check, `std::path::absolute`); verify unit tests for valid/empty/schemeless URLs and a non-UTF-8 path (Unix only) pass

## 2. crates/player - player handle and events

- [ ] 2.1 Add `State`, `Event` and `EndReason` (`Clone`, `#[non_exhaustive]`), the `Shared` struct and the event thread: observe the properties in the design table, handle FILE_LOADED/END_FILE/SEEK/PLAYBACK_RESTART, update state before fanning out, prune dropped subscribers; verify `cargo build -p player-rs` and a unit test of END_FILE reason mapping (including `Replaced` via `pending_loads`) pass
- [ ] 2.2 Add `Player` (`Arc<Inner>`) with `builder()`, `state()`, `subscribe()` returning `Events` (`recv`, `try_recv`, `recv_timeout`, `recv_async`, `Iterator`) and the `Drop` order from design (stop flag, `mpv_wakeup`, join, `mpv_terminate_destroy`); verify a test that drops the last clone while lavfi media plays returns within one second
- [ ] 2.3 Add `load` (autoplay applied per load, `loadfile ... replace` via async command), `play` (restart from 0 after `eof-reached`), `pause`, `toggle_pause`, `stop` and `seek` (`absolute+exact` / `relative+exact`) with the idle no-op rule; verify `cargo build -p player-rs`
- [ ] 2.4 Add `set_volume`, `set_muted`, `set_rate` with runtime clamping, plus public `get_property`, `set_property` and `command`; verify unit-level tests for clamping (1.3 -> 1.0, 0.0 -> 0.01) and mute keeping the volume pass
- [ ] 2.5 Write `tests/playback.rs` against `av://lavfi:testsrc` with `ao=null`, covering the spec scenarios for loading and autoplay modes, unplayable source, load replacing media, pause/resume, absolute and relative seeks, stop, control with no media, play to the end and play after the end, state following events, two/late/dropped/async subscribers, raw property/command access and cross-thread control; verify `cargo test -p player-rs --test playback` passes using only `recv_timeout` waits
- [ ] 2.6 Write `tests/source_file.rs`: generate a one-second PCM WAV in a `rust-sak` temp dir and play it via `Source::file`, plus the Unix-only `http:` directory case; verify `cargo test -p player-rs --test source_file` passes
- [ ] 2.7 Add `examples/play.rs` (plays a path or URL from argv, prints events, exits on end) and crate-level docs with the portable usage example from design; verify `cargo run -p player-rs --example play -- <local file>` plays audio and `cargo doc -p player-rs --no-deps` builds without warnings

## 3. Docs and CI

- [ ] 3.1 Update `README.md` (status, a playback usage example, the `play` example, `keep-open`/Video.js end-of-media behaviour) and confirm `.github/workflows/test.yml` runs the new tests on all six legs with no extra setup; verify every README command runs locally
- [ ] 3.2 Run `cargo fmt --check`, `cargo clippy --workspace --exclude xtask --all-targets -- -D warnings` and `cargo test --workspace --exclude xtask` locally on macOS and confirm `cargo package -p player-rs --list` still succeeds with the git dev-dependency
- [ ] 3.3 With the user's approval, push a branch and trigger the manual `test` workflow; verify the lint job and all six legs (macOS arm64/x64, Linux x64/arm64, Windows x64/arm64) pass
