## 1. Spike (`crates/player`, throwaway branch)

- [ ] 1.1 On a throwaway branch `spike/add-player-6-wgl-interop`, write a throwaway test that runs locally on the Windows GPU and on the CI Windows x64 leg:
  - Create a hidden-window WGL context and a D3D11 device on DXGI adapter 0.
  - Register a `MISC_SHARED_NTHANDLE` BGRA texture through `WGL_NV_DX_interop2`.
  - Have mpv render the 160x120 test pattern into it, open the handle on a second D3D11 device, and read it back.
  - Read `hwdec-current` for `auto-safe`, `nvdec`, `dxva2` and `d3d11va` with `tests/fixtures/testsrc-h264.mp4`.
  - On CI, check llvmpipe and Mesa's `d3d12` driver on WARP for the extension.

  Record the outcome in design.md: orientation, channel order, `GL_VENDOR` vs adapter, decoders, and which CI path works. If `auto-safe` is wrong, update the `playback-core` delta and decision 6. Delete the branch afterwards. Verify: the outcome table is in design.md, and the branch is gone.

## 2. `crates/player`

- [ ] 2.1 Move `reselect_video` and the renderer's reload signal from `player::macos` into a crate-private `surface` module on `cfg(any(target_os = "macos", windows))`, and call it from the macOS renderer. Verify: `cargo test -p player-rs` passes, and the macOS surface tests pass on the CI macOS legs or locally.
- [ ] 2.2 Add `windows` 0.62 (D3D11, DXGI, OpenGL, window and handle features) and runtime `glow` for `cfg(windows)`. Add `windows/wgl.rs`:
  - a hidden-window class and window
  - a context via `wglCreateContextAttribsARB` at 3.2 core, with a legacy fallback
  - a loader through `wglGetProcAddress`, falling back to `opengl32`
  - the `WGL_NV_DX_interop2` entry points, with a missing extension mapped to `Error::Render`

  Verify: a unit test creates and destroys a context on the local GPU and on CI's Mesa.
- [ ] 2.3 Add `windows/d3d.rs`:
  - DXGI adapter selection (default: first with D3D11; or by `AdapterLuid`, with an unknown LUID giving `Error::Render`)
  - D3D11 device creation
  - BGRA `MISC_SHARED | MISC_SHARED_NTHANDLE` slot textures with `CreateSharedHandle` into `Arc<OwnedHandle>`
  - an event-query completion wait

  Verify: unit tests open a slot's handle on a second device, and the texture survives releasing the first device.
- [ ] 2.4 Add `windows/thread.rs`, the render thread from design decision 2:
  - interop device and per-slot registration
  - lock, render, unlock, then the completion wait before publication
  - the ring rebuilt on resize
  - teardown in the order of decision 10, with the bounded join on drop

  Verify: the Windows surface tests for "no size yet", "frames at the set size", "resize while paused" and "nothing loaded" pass locally.
- [ ] 2.5 Add the public `player::windows` API from decision 1 (`SurfaceRenderer`, `SurfaceRendererBuilder` with `adapter` and `on_frame`, `Frame`, `AdapterLuid`), with module docs and a `no_run` example mirroring `player::macos`. Also add the `GL_VENDOR`/adapter mismatch error. Verify: `cargo doc -p player-rs` has no warnings on Windows, and the creation-error tests pass (no render output, second renderer, unknown LUID, recreate after drop).
- [ ] 2.6 In `PlayerBuilder::build`, default `hwdec` to `auto-safe` (or task 1.1's replacement) on `cfg(windows)` with `VideoOutput::Render`, before raw options are applied. Update the builder tests. Verify: the "hardware decoding default on Windows", "overridden" and "elsewhere" tests pass on Windows, macOS and Linux.
- [ ] 2.7 Add the `cfg(windows)` section of `tests/surface.rs` (decision 12), with a test-only staging readback through a second device:
  - upright test pattern, solid colours within 3%, letterboxing
  - created after load, and audio-less reload (both orders)
  - drop orders, frame and handle outliving the renderer, no `on_frame` after drop
  - open on another device

  Verify: all pass locally on the GPU.
- [ ] 2.8 Add the Windows hardware decoding test, gated on `PLAYER_TEST_GL=hardware`. It asserts that `hwdec-current` is set and has no `-copy` suffix, and that frames are published. Verify: it passes locally on the GPU.
- [ ] 2.9 Update the crate docs and the README platform table:
  - Windows surfaces exist in core, and GPUI video comes in slice 7.
  - Linux has no surfaces.
  - arm64 CI skips the tests.
  - Hybrid-GPU guidance (`NvOptimusEnablement` / `AmdPowerXpressRequestHighPerformance`, or Windows graphics settings).
  - Consumers must take `latest()` on every paint.
  - The hwdec default and how to override it.

  Verify: `cargo doc` has no warnings, and the README renders the table.

## 3. CI

- [ ] 3.1 Wire the Windows legs to task 1.1's outcome:
  - x64 runs the Windows surface section on Mesa's `d3d12`/WARP or llvmpipe if one exposes the interop. Otherwise it runs only the "driver without Direct3D sharing" and builder tests, after the user has agreed to that gap.
  - arm64 keeps its GL skip.

  Verify: the workflow diff matches the recorded outcome.
- [ ] 3.2 Run the manual `test` workflow on the change branch. Verify that the lint job and all six legs pass, and record the run id and per-leg test counts in design.md.

## 4. Verification

- [ ] 4.1 Manual check on the local Windows GPU, with the hardware test harness and a 4K HEVC file given by environment variable:
  - `hwdec-current` is a non-copy decoder
  - frames are published at the video rate
  - the process stays low on CPU in a release build
  - dropping while playing returns within 1 s

  Note the GPU, driver version and results in design.md, and mark other vendors as unverified in the README.
- [ ] 4.2 Run `cargo fmt --check`, `cargo clippy --all-targets` and `cargo test` on Windows and verify they are clean, so the change is ready for verification and archive.

## Workflow follow-up

- Archive the change after `/openspec-verify-change` passes.
- Then propose slice 7, `add-player-7-gpui-windows`: the GPUI fork, `[patch]`, the `windows-video` feature, and the upstream PRs after verification.
