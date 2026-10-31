## Why

`PlayerBuilder` defaults to `VideoOutput::Null`, a leftover from slice 1, when the player could only play audio. Every app that shows video, including every `player-gpui` app, must add `.video_output(VideoOutput::Render)`. An app that forgets it gets a black element with audio and only a `log::warn!`. The crate is a video player, so its main use should need no extra line.

## What Changes

- **BREAKING (behaviour):** `PlayerBuilder` defaults to `VideoOutput::Render` instead of `VideoOutput::Null`, and `Render` becomes the enum's `#[default]`. Apps that show video drop `.video_output(VideoOutput::Render)`. Apps that want no video output pass `.video_output(VideoOutput::Null)`.
- Since `Render` is now the default, the platform hardware decoding defaults apply when no video output is set: `hwdec=videotoolbox` on macOS, `auto-safe` on Windows and `vaapi,nvdec` on Linux. A raw `hwdec` option still overrides them. `Null` keeps `hwdec=no`.
- Apps that build a player without a video output and never create a render context or surface renderer see one difference. mpv starts with `vo=libmpv`, its video output fails to start, and mpv deselects the video track. Media with audio plays its audio as before. Media without audio ends at once (the existing `RenderContext` rule: "media loaded while no render context exists plays without video"). Under `Null` it played through. Headless and audio-only apps keep the old behaviour with `.video_output(VideoOutput::Null)`.
- Docs, examples and tests stop passing `Render` where it is now the default. Tests and examples that depend on `Null` behaviour pass it explicitly: the headless playback and `ui::Controls` tests on video-only `lavfi` sources, `player-gpui`'s control tests, the "rejects a player without render output" tests and the `listen` example.

No new API. The libmpv options involved are `vo` (`libmpv` vs `null`) and `hwdec`, both set in `PlayerBuilder::build` as today.

Crates touched:
- `crates/player`: the default lives in the core builder. Every frontend shares it, so a future `crates/egui` gets the same default.
- `crates/gpui`: docs, the `play` example and the tests only. No element code changes.

`rust-sak`: no infrastructure is needed, so nothing applies.

Platforms: macOS, Windows and Linux alike. No platform is left out.

Estimated size: about 10 tasks, one change. It is not part of a series.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `playback-core`: "Player creation with validated options". An unset video output defaults to the render video output instead of none, and the hardware decoding defaults follow that.

## Impact

- **API:** `VideoOutput`'s `Default` impl and `PlayerBuilder::default()` change. No signatures change.
- **Downstream apps:** apps that relied on the implicit `Null` (headless, audio-only, servers) must add `.video_output(VideoOutput::Null)` to keep video-only media playing through and hardware decoding off. Apps that show video need no change, but can drop the explicit `Render`.
- **Code:** `crates/player/src/builder.rs` (enum default, builder default, doc comment, tests). The core doc comments and examples (`lib.rs`, `ui/mod.rs`, `macos`/`windows`/`linux` module docs, `render.rs`). `crates/player/examples/listen.rs`. The core integration tests (`playback.rs`, `source_file.rs`, `ui_controls.rs`, `surface.rs`, `surface/macos.rs`, `surface/linux.rs`, `common/mod.rs`) and unit tests (`player.rs`, `ui/controls.rs`). `crates/gpui` docs (`lib.rs`), `examples/play.rs` and tests (`video.rs`, `element.rs`, `controls/tests.rs`). `README.md`.
- **Specs:** `openspec/specs/playback-core/spec.md`.
