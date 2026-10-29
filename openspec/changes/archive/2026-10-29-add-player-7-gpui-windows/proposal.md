## Why

On Windows, `player_gpui::VideoPlayer` shows black while audio plays and the controls work. Slice 6 gave the core a Windows texture stream (`player::windows::SurfaceRenderer`), but stock GPUI cannot show it. In `gpui-pre` 0.3.10 and `gpui-pre-windows` 0.3.10, the versions GPUI-Kit 0.8.1 builds on by default:
- `PaintSurface` has no payload off Apple platforms.
- `Window::paint_surface` and `surface()` are `cfg(macos/ios)`.
- `DirectXRenderer::draw_surfaces` is an empty stub.

This is slice 7 of the `add-player` series, the second of the two Windows slices. As the user decided in slice 6, it forks those two GPUI crates and adds Windows surface support to them. Apps take the fork through `[patch.crates-io]`. `player-gpui` then shows Windows video behind an opt-in feature.

## What Changes

- **GPUI fork** (new repository, outside this workspace): `gpui-pre` and `gpui-pre-windows` at 0.3.10. The first commit is the crates.io sources, untouched. A single change follows it:
  - **`gpui-pre`**:
    - `PaintSurface` gets a Windows payload: a shared Direct3D 11 texture, held by an owned NT handle.
    - `Window::paint_surface` exists on Windows. It takes a `SharedTexture`, which owns an NT handle and is built safely from a borrowed one by duplicating it, so the caller needs no `unsafe`.
    - `SurfaceSource` and `surface()` get the matching Windows variant.
    - A public marker item lets a crate that needs the fork fail with a readable error when the `[patch]` is missing.
  - **`gpui-pre-windows`**:
    - `draw_surfaces` opens each shared texture on GPUI's own D3D11 device (`ID3D11Device1::OpenSharedResource1`) on every draw. It draws the texture into its bounds, clipped to the content mask, with a new `surface` HLSL shader. That shader is added to the release build's `fxc` module list.
    - A texture that cannot be opened is skipped, and a warning is logged once.
    - Under `test-support`, a `DirectXHeadlessRenderer`, because GPUI has no headless renderer on Windows to run pixel tests with.
  - No CPU copy is made anywhere.
- **`crates/gpui`** (`player-gpui`):
  - A new opt-in feature `windows-video`. On Windows, it gives `VideoPlayer` the same video view as macOS, built on `player::windows::SurfaceRenderer`:
    - frames requested at the element's bounds in device pixels
    - a repaint per published frame, and none while idle
    - letterboxing with `ObjectFit::Contain`
    - keyed state
  - When the renderer cannot be created (for example, a driver without `WGL_NV_DX_interop2`, or a hybrid-GPU mismatch), the element stays black and logs one warning, as on macOS.
  - Without the feature, Windows stays black, as today, and builds against stock GPUI.
  - With the feature but without the `[patch]`, the build fails with an error that names the marker. The README explains it.
- **This workspace**: `[patch.crates-io]` for `gpui-pre` and `gpui-pre-windows`, pointing at a tag of the fork, so the example, the tests and CI use it.
- **Docs**: README and crate-doc setup steps for Windows video (the feature, the `[patch]` lines, and the `gpui-pre` backend requirement). The platform table is updated.
- **No upstream PR**: the user decided not to contribute the change to `zed-industries/zed`. The fork and the `[patch]` stay for as long as GPUI-Kit's GPUI cannot draw a shared Direct3D texture on Windows by itself.

`crates/player` is not touched. Slice 6's `player::windows` API is used as it is.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `gpui-player`:
  - "Video element" and "GPUI platform coverage" extend video to Windows when the `windows-video` feature is on and the patched GPUI is used. Windows without the feature stays black.
  - A new requirement covers Windows setup: the opt-in feature, the patched GPUI, the readable build error without the patch, and black with one warning when the graphics driver cannot share the video.

## Impact

- **Crates**:
  - `crates/gpui` gets the Windows video view behind `windows-video`. It contains only GPUI glue: sizing, frame subscription and the `paint_surface` call. There is no D3D, GL or `unsafe` code in it.
  - `crates/player` is unchanged. Everything platform-specific stays in the core (slice 6) or in GPUI's renderer (the fork), so a future egui-wgpu crate would reuse the core unchanged.
- **Video.js reference**: the `<video>` area of a Video.js player, with the video letterboxed on black under the control bar. This is the same look as macOS, now on Windows.
- **libmpv APIs**: none new. Slice 6's renderer and its `hwdec=auto-safe` default are used unchanged.
- **Windows APIs**:
  - In the fork: `ID3D11Device1::OpenSharedResource1`, `DuplicateHandle` (through std `BorrowedHandle::try_clone_to_owned`), and the D3D11 shader-resource views and pipeline GPUI already uses.
  - In `player-gpui`'s tests only: `windows` 0.62 (dev) to make a shared test texture, and `gpui-pre-windows` (dev, patched) for the headless renderer.
- **Public API**:
  - `player-gpui`: a new `windows-video` feature. Its Rust API is unchanged.
  - The fork: `Window::paint_surface` and `surface()` on Windows, a `PaintSurface` field, and the marker item. All are additive.
- **Dependencies**:
  - `[patch.crates-io]` in this workspace, pointing at the fork's git tag.
  - Apps that want Windows video add the same `[patch]` lines. Patches are not inherited from dependencies.
  - `flume` becomes a dependency of `player-gpui` on Windows too, behind the feature.
- **rust-sak**: nothing in it covers this. Its modules are `crypto`, `fetch`, `fs`, `github`, `image`, `memo`, `o11y`, `sysinfo` and `tui`. None of them touches GPUI, Direct3D or texture sharing. `fs` is already a dev-dependency of `player-gpui`, for the test media's temporary directory. Nothing is added to `rust-sak`.
- **Platforms**:
  - Windows x64 and arm64 get video in GPUI, with the feature and the patch. arm64 is unverified locally, and CI has no GL there.
  - Only the `gpui-pre` backend is covered. Apps on GPUI-Kit's `gpui-fast` backend keep black video on Windows. The `gpui-fast` fork is deferred to slice 8+.
  - Linux stays black, as documented.
  - On CI, Mesa has no interop. The core renderer cannot be built there, so the video tests that need it skip. The fork's drawing is still tested on CI with a shared texture the test makes itself (see design.md).
- **Outward-facing steps** (each needs the user's go-ahead): creating and pushing the fork repository and its tag.
- **Size**: 13 tasks. This is slice 7 of the `add-player` series (see design.md). It depends on slices 5 and 6.
