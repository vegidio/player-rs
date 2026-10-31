## Context

`PlayerBuilder::default()` sets `video_output: VideoOutput::Null`, and `VideoOutput` derives `Default` with `#[default] Null` (`crates/player/src/builder.rs`). `PlayerBuilder::build` turns the choice into mpv's `vo` option (`null` or `libmpv`). For `Render` only, it also sets the platform's `hwdec` default unless a raw option sets one. `Player::start` takes the output: a `Render` player observes `dwidth`/`dheight` and records loads made without a render context, so a surface renderer can reload video-only media.

Many tests and docs rely on the implicit `Null`. The core playback, `ui::Controls` and unit tests play video-only `lavfi` sources (`testsrc`). Under `Render` with no render context, those sources end at once, because mpv deselects the video track and has nothing left to play. `player-gpui`'s control tests build players without a video output, so the element never creates a surface renderer and needs no GL. Several "rejects a player without render output" tests use the default builder to get a `Null` player.

This change is not sliced. It is one change of about 10 tasks.

## Goals / Non-Goals

**Goals:**
- An unset video output means `Render`, on every platform, with the existing `hwdec` defaults following it.
- Every test keeps testing what it tested before. Tests that need `Null` say so.
- Docs and examples show the shorter form for video apps, and `Null` for audio-only use.

**Non-Goals:**
- Renaming `VideoOutput::Null` or adding variants.
- Warning when a `Render` player loads media with no render context. That is valid for a GUI app that creates its view after loading, and `SurfaceRenderer` already recovers from it.
- Any change to the `player-gpui` element's behaviour, the render path or the controls.

## Decisions

**Change the default in the core builder, not in `player-gpui`.** The user chose this. The alternative was a `player-gpui` builder preset, which would leave the core default as is but give users two ways to build a player. A future `crates/egui` would need the same preset again. The default belongs with the builder in `crates/player`, which knows nothing about toolkits, so every frontend gets it unchanged.

**Change both `VideoOutput`'s `#[default]` and `PlayerBuilder::default()`.** They must agree. `VideoOutput::default()` is public, and a mismatch would surprise anyone who builds the value themselves.

**Tests opt into `Null` explicitly instead of switching their media.** The headless tests, the `ui::Controls` tests (`tests/ui_controls.rs`, `src/ui/controls.rs`), `src/player.rs`'s unit tests, `tests/source_file.rs` and the builder's `default_player`/`embedding_defaults` tests add `.video_output(VideoOutput::Null)` where they play or check `Null` behaviour. The "rejects a player without render output" tests (`tests/surface.rs`, `tests/surface/macos.rs`, `src/render.rs`) and `player-gpui`'s `stays_black_when_the_player_cannot_render` do the same. Their purpose is a `Null` player, and the implicit default would now give them a `Render` one. `player-gpui`'s `controls/tests.rs` passes `Null` too, so that the control tests stay GL-free and keep running where `PLAYER_TEST_SKIP_GL` is set. The alternative was moving the tests to sources with audio (`TEST_SOURCE_WITH_AUDIO`). That would change what they check, timing included, for no gain.

**Add tests for the new default.** `default_player` checks that the default reads `vo=libmpv`. `hardware_decoding_default` checks the platform `hwdec` for a builder with no video output set, alongside the explicit `Render`. A playback test checks the new scenario: a default-output player loads a source with audio, no render context exists, the audio plays and `video_size` stays `None`.

**Docs and examples: drop `Render` where it is now the default, add `Null` where audio-only is the point.** The `player-gpui` crate docs, `examples/play.rs`, the README's GPUI snippet, and the `RenderContext`/surface module docs drop `.video_output(VideoOutput::Render)`. They gain one line saying `Render` is the default. The `listen` example ("audio only") passes `Null`. The README's Usage snippet and the crate-level example in `lib.rs` stay as they are, since media with audio plays the same either way. The README gains one bullet on the default and when to choose `Null`. Tests that already pass `Render` explicitly keep it, which avoids churn and keeps the intent visible.

**The error message stays.** `RenderContext` and `SurfaceRenderer` reject a non-render player with a message naming `VideoOutput::Render`. It is still accurate.

Backend and UI separation: only `crates/player` changes behaviour. `crates/gpui` changes only docs, an example and tests, so a future egui crate would reuse the backend part unchanged. Zero-copy, unsafe and FFI boundaries, threading and resource lifetimes are untouched. No new crates. No `rust-sak` feature applies. GPUI-Kit is not involved beyond the existing tests.

## Risks / Trade-offs

- **[Silent behaviour change for headless apps]** An app that relied on the implicit `Null` now loads video-only media that ends at once. It reports `Ended` with the reason it uses for nothing to play, where it used to play through. It also gets hardware decoding turned on, though no decoder runs without a video output. → The README and the builder docs state the new default and the `Null` opt-in. The proposal marks the change **BREAKING**, so the release notes flag it.
- **[Tests silently testing `Render` instead of `Null`]** Missing one builder call site would turn a `Null` test into a `Render` test. Most such tests would fail loudly (video-only media ending at once), but some might pass by accident. → Task 1.3 lists every `Player::builder()` call without `.video_output(...)`, found by grep, and each one is decided on purpose.
- **[Bookkeeping on every default load]** A default player now records "loaded without a render context" for every load before a renderer exists. → This is the existing `Render` path, and it is already exercised by GPUI apps that load in the view constructor. Nothing new.

## Migration Plan

Apps that show video need no change. Apps that want no video output add `.video_output(VideoOutput::Null)`. The crates use calendar versions, so the release notes for the next version carry the **BREAKING** note. Rolling back means restoring the two defaults. The explicit `Null` calls added to tests stay valid either way.
