## 1. Spike (`crates/player`, throwaway branch)

- [ ] 1.1 On a throwaway branch `spike/add-player-8-gbm-dmabuf`, write a throwaway test that runs on the local WSL2 machine after `sudo modprobe vgem`:
  - Open the vgem DRM node (render node, or card node if that is all GBM accepts), create a GBM device, an EGL display on `EGL_PLATFORM_GBM_KHR` and a surfaceless GL context. Note the driver Mesa loads, and any override it needs (for example `MESA_LOADER_DRIVER_OVERRIDE=kms_swrast`).
  - Allocate a LINEAR `ABGR8888` buffer, import it as an EGLImage, have mpv render the 160x120 test pattern and solid colour fields into it with `flip_y = false`, and read it back through a second EGL context that imports the same fd.
  - Check that lavapipe imports the same fd through `VK_EXT_external_memory_dma_buf` (input for slice 9).
  - Statically check whether libmpv 26.10.1 for Linux has VA-API, and whether its interop reads `MPV_RENDER_PARAM_DRM_DISPLAY_V2`.
  - Push the branch and run the test on the CI Linux x64 leg with `vgem` loaded (installing `linux-modules-extra-$(uname -r)` if needed).

  Record the outcome in design.md: setup steps, node type, orientation, channel order, lavapipe import, VA-API, and the CI result. **Gate:** if no software path works on WSL2, stop and ask the user (design decision 14). Delete the branch afterwards. Verify: the outcome table is in design.md, and the branch is gone.

## 2. `crates/player`

- [x] 2.1 Compile the shared `surface` module on Linux too. Add a crate-internal optional `DRM_DISPLAY_V2` parameter to `mpv::render::create`, reachable from the crate-internal path of `RenderContext::new`, with the public API unchanged. Verify: `cargo test -p player-rs` passes on Linux, and the existing render tests are unchanged.
- [x] 2.2 Add runtime `glow` for `cfg(target_os = "linux")`. Add `linux/egl.rs` and `linux/gbm.rs`, containing:
  - the hand-written `extern "C"` blocks for libEGL and libgbm, with extensions loaded through `eglGetProcAddress`
  - render node enumeration (`/dev/dri`, sorted by minor, `DrmDevice` from `st_rdev`)
  - the GBM device, the EGL display, and a 3.2 core context with legacy and GLES 3 fallbacks, made current with no surface
  - the extension checks (`EGL_EXT_image_dma_buf_import(_modifiers)`, `EGL_KHR_surfaceless_context`, `GL_OES_EGL_image`), each mapped to `Error::Render`

  Confirm the link line already has libEGL and libgbm through `mpv.pc`, and add `#[link]` attributes only if not. Verify: a unit test creates and destroys a context on the `vgem` node, and enumeration returns no nodes without `vgem`.
- [x] 2.3 Add the slot allocation in `linux/gbm.rs`:
  - `gbm_bo_create_with_modifiers2` with the modifier list, falling back to `gbm_bo_create` with `GBM_BO_USE_LINEAR`
  - rejection of buffers whose plane count is not 1
  - the plane-0 fd into `Arc<OwnedFd>`, with stride, offset and modifier
  - EGLImage import into a GL texture and FBO

  Verify: unit tests allocate a slot on `vgem`, import its fd into a second context, and read it back after the bo and the GBM device are destroyed.
- [x] 2.4 Add `linux/thread.rs`, the render thread from design decision 2:
  - setup with the `DRM_DISPLAY_V2` parameter
  - render, fence and `glClientWaitSync` before publication
  - the ring rebuilt on resize
  - teardown in the order of decision 11, with the bounded join on drop
  - default node selection by first successful build (decision 6)

  Verify: the Linux surface tests for "no size yet", "frames at the set size", "resize while paused" and "nothing loaded" pass on WSL2 with `vgem`.
- [x] 2.5 Add the public `player::linux` API from decision 1 (`SurfaceRenderer`, `SurfaceRendererBuilder` with `device`, `modifiers` and `on_frame`, `Frame`, `DrmDevice`), with module docs and a `no_run` example mirroring `player::windows`. Verify: `cargo doc -p player-rs` has no warnings on Linux, and the creation-error tests pass on WSL2: no render output, second renderer, unknown device, unusable layouts, recreate after drop, and no render node (run without `vgem`).
- [x] 2.6 In `PlayerBuilder::build`, default `hwdec` to `vaapi,nvdec` on Linux with `VideoOutput::Render` (changed from `auto-safe` during implementation, see design decision 7), update the `VideoOutput::Render` docs, and change the builder test's Linux expectation. Verify: the "hardware decoding default on Linux", "overridden" and "elsewhere" tests pass on Linux, and the macOS and Windows expectations are unchanged.
- [x] 2.7 Add the `cfg(target_os = "linux")` section of `tests/surface.rs` (decision 13), with a test-only readback that imports the frame's fd into a second EGL context:
  - upright test pattern, solid colours within 3%, letterboxing
  - created after load, and audio-less reload (both orders)
  - drop orders, frame and fd outliving the renderer, no `on_frame` after drop
  - import into another context, single plane, default LINEAR

  The tests return early when no render node is available. Verify: all pass on WSL2 with `vgem`, and all skip cleanly without it.
- [x] 2.8 Add the Linux hardware decoding test, gated on `PLAYER_TEST_GL=hardware`. It asserts that `hwdec-current` is set and has no `-copy` suffix, and that frames are published. Verify: it compiles and is skipped without the variable. Running it on hardware is 10+ work.
- [x] 2.9 Update the crate docs and the README:
  - Linux surfaces exist in core, and GPUI video comes in slice 9.
  - The platform table.
  - Software-only verification, with Mesa on Intel/AMD, NVIDIA and VA-API/NVDEC zero-copy listed as unverified.
  - The `vgem` setup for running the tests (from task 1.1), and WSL2's "no render node" behaviour.
  - Consumers must take `latest()` on every paint and pass their render node and importable modifiers.
  - The hwdec default and how to override it.

  Verify: `cargo doc` has no warnings, and the README renders the table.

## 3. CI

- [ ] 3.1 Wire the Linux legs to task 1.1's outcome:
  - If `vgem` works on the runners, load it (and install its modules package if needed) before the tests, so the full Linux surface section runs.
  - Otherwise, only the "no render node" and builder tests run, and the gap is recorded in design.md and the README after the user agrees.

  Verify: the workflow diff matches the recorded outcome.
- [ ] 3.2 Run the manual `test` workflow on the change branch. Verify that the lint job and all six legs pass, and record the run id and per-leg test counts in design.md.

## 4. Verification

- [x] 4.1 Manual check on WSL2 with `vgem`, in a release build, with a 1080p H.264 file given by environment variable:
  - frames are published at the set size while it plays
  - resizing and pausing work
  - dropping while playing returns within 1 s
  - the file descriptor count does not grow over a minute of playback and resizes

  Note the Mesa version, kernel and results in design.md, and confirm the README lists hardware as unverified.
- [ ] 4.2 Run `cargo fmt --check`, `cargo clippy --all-targets` and `cargo test` on Linux (WSL2) and verify they are clean, then confirm macOS and Windows still build in CI, so the change is ready for verification and archive.

## Workflow follow-up

- Archive the change after `/openspec-verify-change` passes.
- Then propose slice 9, `add-player-9-gpui-linux`: extend the fork with `gpui-pre-wgpu` (Vulkan and GL dma-buf import, RGBA surface shader), a new fork tag and `[patch]` entry, and the `linux-video` feature in `player-gpui`.
