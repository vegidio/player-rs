## 1. Spike (local, throwaway)

- [x] 1.1 On WSL2 with `vgem` loaded, in a scratch crate outside any repository that uses a local clone of `vegidio/gpui-pre` (with `gpui-pre-wgpu` 0.3.10 unpacked next to it) through a path `[patch]`, check the questions in design decision 8:
  - GPUI's adapter by default, and how to steer it to lavapipe or to GL llvmpipe
  - whether wgpu-hal enables lavapipe's dma-buf extensions without the modifier feature
  - a LINEAR import of a core `Frame` through `ash` and `texture_from_raw`, sampled correctly, with lavapipe's row pitch compared to GBM's stride
  - an EGLImage import on wgpu's GL display, windowed and headless
  - which WGSL file the GL backend compiles
  - the same checks on a throwaway CI Linux x64 run (branch `spike/add-player-9-gpui-dmabuf`)

  Record the outcomes in design.md, and adjust decisions 3, 4 and 8 if one fails. **Gate:** if neither backend imports on WSL2, stop and ask the user. Delete the branch afterwards. Verify: the outcome table is in design.md, and the branch is gone.

## 2. GPUI fork (separate repository, ../gpui-pre)

- [x] 2.1 Clone `vegidio/gpui-pre` to `../gpui-pre`, and add `gpui-pre-wgpu` 0.3.10 unpacked from crates.io as commit 3, untouched, as a workspace member (design decision 1). Point this workspace's `[patch.crates-io]` at the checkout by path for all three crates. Verify: `git diff` between commit 3's `gpui-pre-wgpu/` and the crates.io sources is empty, `cargo tree -i gpui-pre-wgpu` shows the path on Linux, and `cargo test -p player-rs -p player-gpui` still passes on Linux.
- [x] 2.2 In `gpui-pre`, add `DmaBufTexture`, `DmaBufLayout`, the Linux `PaintSurface` field, `Window::paint_surface` and `SurfaceSource::DmaBuf` with `surface()` on Linux, `DmaBufImport` with `dma_buf_import()` and the hidden setter, and the `LINUX_SURFACES_REQUIRE_PATCHED_GPUI` marker (decision 2). Doc comments state the "finished before painted, not rewritten while drawn" contract. Verify: the fork builds on Linux and Windows, `cargo doc -p gpui-pre` has no warnings on Linux, and a unit test checks that `duplicate` gives an fd that outlives the original.
- [x] 2.3 In `gpui-pre-wgpu`, request `VULKAN_EXTERNAL_MEMORY_DMA_BUF` when supported, and publish the import description per context for Vulkan (render node, single-plane modifiers, LINEAR-only fallback) and GL (`eglQueryDmaBufModifiersEXT`, render node), including after device loss (decision 3). Verify: on WSL2, `dma_buf_import()` reports `[LINEAR]` with no node on lavapipe, the GL description on llvmpipe, and a GL description on `d3d12` GL (without `LIBGL_ALWAYS_SOFTWARE`) whose first import fails with one warning, each logged at info level.
- [x] 2.4 In `gpui-pre-wgpu`, implement the imports: `texture_from_dmabuf_fd` with the modifier extension, the `ash` LINEAR fallback with the pitch check, and the EGLImage import on GL, LINEAR buffers at the padded width (decision 4). Verify: a fork test imports a core-shaped LINEAR buffer on lavapipe and on GL llvmpipe and reads back its pixels.
- [x] 2.5 In `gpui-pre-wgpu`, add the RGBA surface pipeline and shader, the clamping sampler, `draw_surfaces` with the inode-keyed cache (8-frame eviction, dropped with the device's resources), and the warn-once skip (decision 4). Add the fork's tests for the cache and for the shader with a wgpu-made texture, as integration tests (decision 8). Verify: `cargo test -p gpui-pre-wgpu --test '*'` passes on WSL2 on both backends, and `cargo clippy -p gpui-pre-wgpu --lib --test '*'` is clean.
- [x] 2.6 With the user's go-ahead, push commits 3 and 4 and tag `0.3.10-surfaces.2`. Switch this workspace's `[patch.crates-io]` to the git tag for `gpui-pre`, `gpui-pre-windows` and `gpui-pre-wgpu`. Verify: a clean `cargo build -p player-gpui` on Linux fetches the tag, and the Windows commit is unchanged between the two tags (`git diff 0.3.10-surfaces.1 0.3.10-surfaces.2 -- gpui-pre-windows` is empty).

## 3. `crates/gpui` (`player-gpui`)

- [x] 3.1 Set the `video` cfg on Linux in `build.rs`, and move `flume` to Linux too. Add the Linux marker reference in `lib.rs` and the Linux `gpui-pre-wgpu` (`test-support`) dev-dependency (decisions 5, 6 and 7). Verify: `cargo build -p player-gpui` succeeds on Linux. With the `[patch]` temporarily removed, the first Linux error names `LINUX_SURFACES_REQUIRE_PATCHED_GPUI`.
- [x] 3.2 Add `video/linux.rs`:
  - `build` with GPUI's render node and modifiers, falling back to the core's defaults
  - `paint` through `DmaBufTexture::duplicate`, with warn-once on `dup` failure

  Wire it into `video.rs`'s `surface` module (decision 5). Verify: the `play` example shows video in WSLg on WSL2 with `vgem` when GPUI is steered to lavapipe, and the `VideoView` harness tests pass on Linux, returning early with a message without a render node.
- [x] 3.3 Add the Linux tests from decision 8:
  - the draw-path pixel test (four-quadrant picture from the core renderer, painted with `fit`, repeated paints)
  - the real-video pixel test (solid-colour fields within 3%, upright, letterboxed)
  - the warn-once test with an unimportable layout
  - the "GPUI reports nothing" and "cannot render on GPUI's node" fallback tests for `build`

  Each test returns early, with a message, when there is no render node or GPUI reports no import. Verify: all pass on WSL2 on lavapipe and on GL llvmpipe, and they skip cleanly without `vgem`.
- [x] 3.4 Update the crate docs (`lib.rs` setup and "Behaviour and gaps") and the README:
  - Linux video
  - the three `[patch.crates-io]` lines at `0.3.10-surfaces.2`, and which are needed per platform
  - the `gpui-pre`-only backend and the readable build error
  - the Vulkan, LINEAR-fallback and GL import paths
  - black with one warning on a driver without dma-buf import (`d3d12` on WSL2) or without a render node
  - steering GPUI's adapter for tests (`VK_DRIVER_FILES`)
  - Linux hardware and the modifier import listed as unverified
  - the platform table

  Verify: `cargo doc -p player-gpui` has no warnings on Linux, and the README's `[patch]` snippet matches the workspace's.

## 4. CI

- [x] 4.1 Update the workflow (decision 9):
  - Linux clippy covers `player-gpui` with the new code.
  - The Linux legs install Mesa 26 from the kisak PPA, keep `vgem`, and run `player-gpui`'s tests twice, once on lavapipe and once on GL llvmpipe with no Vulkan ICD.
  - Windows and macOS use the new tag unchanged.

  Verify: the workflow diff matches decision 9, and `actionlint` (or a dry read of the YAML) shows no errors.
- [x] 4.2 Run the manual `test` workflow on the change branch. Verify that both lint jobs and all six legs pass. Record in design.md the run id, the per-leg `player-gpui` test counts and which Linux pixel tests ran or skipped on each backend.

## 5. Core: software OpenGL scaler (added during implementation, decision 10)

- [x] 5.1 In `crates/player`, `RenderContext::with_drm_device` reads `GL_RENDERER` through the caller's loader. On llvmpipe or softpipe, with mpv's `gpu-dumb-mode` and `scale` still at their defaults, it sets `gpu-dumb-mode=yes` before creating mpv's render context (first `scale=bilinear`, replaced after the manual check showed black lines). Document it on `RenderContext` and in the Linux module docs. Verify: the existing render and surface tests pass on llvmpipe.
- [x] 5.2 Drop the surface tests' `software_scaling` workaround and `player-gpui`'s `scale=bilinear`, and add two Linux surface tests: yuv420p colour fields scaled down by an uneven factor, every pixel checked on several frames, and an app's own `scale` or `gpu-dumb-mode` kept. Verify: the new colour test fails without 5.1 in most runs (it is decided per process) and passes 10 runs in a row with it (the core renders with GL, so GPUI's backend does not matter here).

## 6. Verification

- [x] 6.1 Manual check in WSLg on WSL2 with `vgem`, with a release build of `examples/play.rs` and a 1080p H.264 file, on lavapipe and on GL llvmpipe:
  - upright, colours, letterboxing
  - controls over the video, fullscreen, window resize
  - no repaint while paused
  - two players
  - closing while playing
  - the process's fd count flat over a minute with resizes

  Also on GPUI's `d3d12` GL adapter (the default without `LIBGL_ALWAYS_SOFTWARE`): black, one warning, and audio and controls working. Record Mesa, kernel and results in design.md.
- [x] 6.2 Run `cargo fmt --check`, `cargo clippy --all-targets` and `cargo test` for the workspace on Linux (WSL2, with `vgem`, on both backends) and verify they are clean. Run the manual `test` workflow on the branch again, since the core changed after task 4.2, and verify that both lint jobs and all six legs pass, so the change is ready for verification and archive. Record the run in design.md.

## Workflow follow-up

- Archive the change after `/openspec-verify-change` passes.
- Then plan slice 10+ (`gpui-fast` fork, menus, captions, Linux hardware verification, and so on) when it is reached.
