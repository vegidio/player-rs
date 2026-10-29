## 1. Spike (local, throwaway)

- [x] 1.1 In a scratch checkout of the two crates from crates.io (not committed anywhere), with a throwaway test on the local GPU and on the CI Windows x64 leg (a throwaway branch `spike/add-player-7-gpui-surface`), check:
  - `OpenSharedResource1` of a slice-6-style BGRA NT-handle texture on GPUI's device, also at feature level 10.1 and on WARP
  - the cost of opening per draw vs `CompareObjectHandles` lookup
  - whether `Window::render_to_image()` with GPUI's headless Windows renderer runs on CI's WARP adapter
  - the `gpui_kit::` path that re-exports a new root item of `gpui-pre`

  Record the outcomes in design.md, and adjust decisions 3, 5 and 8 if one fails. Delete the branch afterwards. Verify: the outcome table is in design.md, and the branch is gone.

## 2. GPUI fork (separate repository, ../gpui-pre)

- [x] 2.1 Create the fork workspace with `gpui-pre` and `gpui-pre-windows` 0.3.10 unpacked from crates.io as commit 1, untouched (design decision 1). Point this workspace's `[patch.crates-io]` at it by path. Verify: `cargo build -p player-gpui` on Windows uses the patched sources (`cargo tree -i gpui-pre` shows the path), and `cargo test -p player-rs -p player-gpui` still passes.
- [x] 2.2 In `gpui-pre`, add `SharedTexture`, the Windows `PaintSurface` field, `Window::paint_surface` on Windows, the `SurfaceSource::SharedTexture` variant with `surface()` on Windows, and the marker const (decision 2), with doc comments stating the "finished before painted" contract. Verify: the fork builds on Windows, macOS and Linux (macOS and Linux through CI, task 4.1), and `cargo doc` has no warnings on Windows.
- [x] 2.3 In `gpui-pre-windows`, add the `surface` HLSL module and pipeline, add it to `build.rs`'s `fxc` list and to `ShaderModule`, and implement `draw_surfaces` opening each texture per draw, with a warn-once skip on failure (decision 3, no cache after task 1.1). Add `DirectXHeadlessRenderer` under `test-support` (decision 8). Verify: debug and release builds compile the shader.
- [x] 2.4 Add the fork's `render_to_image` tests (decision 8): quadrant colours and placement, content-mask clipping, an unopenable surface skipped while later primitives draw, and repeated paints through new duplicates. Verify: `cargo test -p gpui-pre-windows` passes locally on the GPU.
- [x] 2.5 With the user's confirmation of the repository name and go-ahead to push, create the GitHub repository, push both commits, tag `0.3.10-surfaces.1`, and switch this workspace's `[patch.crates-io]` to the git tag. Verify: a clean `cargo build -p player-gpui` on Windows fetches the tag, and `git diff` between commit 1 and the crates.io sources is empty.

## 3. `crates/gpui` (`player-gpui`)

- [x] 3.1 Add the `windows-video` feature (`dep:flume` on Windows), the `video` cfg alias from `build.rs` with `rustc-check-cfg`, the marker reference in `lib.rs`, and the Windows `windows` 0.62 and `gpui-pre-windows` dev-dependencies (decisions 5, 7 and 8). Verify: `cargo build -p player-gpui` succeeds on Windows with and without the feature. With the feature and the `[patch]` temporarily removed, the first error names `WINDOWS_SURFACES_REQUIRE_PATCHED_GPUI`. `cargo build --features windows-video` on Linux (CI) is a no-op.
- [x] 3.2 Split `video.rs` into the shared `VideoView` and the per-platform `surface` module, add the Windows one (`player::windows::SurfaceRenderer`, `paint_surface` with `SharedTexture::duplicate`, warn once on duplicate failure), and switch `element.rs`'s macOS cfgs to `cfg(video)`. Verify: the macOS harness tests still pass on the CI macOS legs, and the `play` example shows video on the local Windows GPU.
- [x] 3.3 Make the `VideoView` harness tests run on Windows with the feature, returning early with a message on "cannot share textures with Direct3D 11". Add the warn-once test for Windows without interop, the CI pixel test through `paint_surface` with a shared texture the test creates, and the local real-video pixel test (solid-colour fields within 3%, upright, letterboxed) (decision 8). Verify: all pass locally on the GPU with `--features windows-video`, and the CI-bound ones pass with Mesa's DLLs placed as on CI.
- [x] 3.4 Update the crate docs (`lib.rs` setup section, `VideoPlayer` docs) and the README: Windows video, the `windows-video` feature, the exact `[patch.crates-io]` lines, the `gpui-pre`-only backend, the readable build error, the hybrid-GPU note, and the platform table. Verify: `cargo doc -p player-gpui --features windows-video` has no warnings on Windows, and the README's `[patch]` snippet matches the workspace's.

## 4. CI

- [x] 4.1 Add the Windows x64 clippy step with `--features windows-video`. Build and test `player-gpui` with the feature on both Windows legs, build the release `play` example on x64 (the `fxc` path), and add a Linux build with the feature (decision 10). Run the manual `test` workflow on the change branch. Verify: lint and all six legs pass. Record the run id, the per-leg test counts and the pixel-test outcome on WARP in design.md.

## 5. Verification

- [x] 5.1 Manual check with `examples/play.rs` on the local NVIDIA GPU, in a release build:
  - upright, colours and letterboxing
  - the controls over the video, fullscreen and window resize
  - no repaint while paused
  - a 4K HEVC clip at its frame rate, with CPU use measured
  - two players side by side
  - closing while playing

  Note the GPU, driver and results in design.md. Verify: the results table is in design.md, and the README marks AMD, Intel and arm64 as unverified.
- [x] 5.2 Run `cargo fmt --check`, `cargo clippy --all-targets` (with and without `windows-video`) and `cargo test` on Windows. Verify: all clean.

## Workflow follow-up

- Archive the change after `/openspec-verify-change` passes.
- Then plan slice 8+: Linux surfaces and the `gpui-fast` fork.
