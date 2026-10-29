## Context

What already exists:
- **`player::windows::SurfaceRenderer` (slice 6)** has the same shape as `player::macos`: builder, `set_size`, `latest`, `on_frame`.
  - Each `Frame` gives a BGRA D3D11 texture through `shared_handle() -> BorrowedHandle<'_>` (an NT handle), plus its size, sequence number and `AdapterLuid`.
  - Frames are published only once the GPU has finished them, so a consumer opens the handle and samples it with no synchronisation of its own.
  - A ring of 3 slots means that, per renderer, at most 3 distinct textures are ever in flight.
  - Creation fails with `Error::Render` when the GL driver lacks `WGL_NV_DX_interop2` (CI's Mesa), or when GL and the DXGI adapter differ.
- **`player_gpui::VideoPlayer` (slices 4, 5)**:
  - On macOS, a keyed `Slot` holds a `VideoView` (`crates/gpui/src/video.rs`). It builds the renderer, notifies on `on_frame` through a `flume::bounded(1)` channel, and calls `set_size` at prepaint. At paint it calls `window.paint_surface(ObjectFit::Contain.get_bounds(..), frame.pixel_buffer().clone())`.
  - The view and its `Slot` field are `cfg(target_os = "macos")`.
  - On Windows and Linux, the element is a black `div` with the controls over it.
- **CI**:
  - Lint (fmt and clippy) runs only on Linux x64, so Windows-only code is not linted there today.
  - The Windows legs build and test `player-rs` and `player-gpui`.
  - x64 has Mesa (no interop), and arm64 skips GL.

What was verified in `gpui-pre` 0.3.10 and `gpui-pre-windows` 0.3.10 (snapshots of zed@`08cbb88`, and what GPUI-Kit 0.8.1 resolves to here):
- **`PaintSurface`** is `{ order, bounds, content_mask, #[cfg(macos/ios)] image_buffer }`.
  - The scene collects it on every platform, and `PrimitiveBatch::Surfaces` reaches `DirectXRenderer::draw_surfaces`, which returns `Ok(())` without drawing.
  - `Window::paint_surface` is `cfg(macos/ios)`, and so are `SurfaceSource::Surface` and `surface()`.
- **`gpui-pre` depends on `windows` 0.62 on Windows**, the version slice 6 chose, so the fork adds no crate.
- **`gpui-pre-windows`**:
  - Draws with D3D11 at feature level 11.1, 11.0 or 10.1 through a DirectComposition `B8G8R8A8_UNORM` swap chain.
  - Its devices (`DirectXDevices { adapter, device, device_context, .. }`) are recreated in `handle_device_lost`.
  - Each primitive kind has a `PipelineState<T>`: a structured buffer of instances, a vertex and pixel shader, and draws of 4-vertex strips per instance.
  - `draw_polychrome_sprites` binds a shader-resource view with `GlobalParams` and a linear sampler. It is the closest model for a surface draw.
- **Shaders.** Debug builds compile `shaders.hlsl` at run time. Release builds compile it with the Windows SDK's `fxc.exe` in `build.rs`, from a fixed list of modules (`quad`, `shadow`, ..., `polychrome_sprite`), so a new module must be added to that list.
- **Offscreen rendering.** `Window::render_to_image()` exists. Under `test-support`, `DirectXRenderer::render_to_image` draws the scene into its render target and reads it back without presenting. The headless test platform takes a renderer from `gpui_platform::current_headless_renderer`. This makes pixel tests of the Windows draw path possible.
- **Versions are pinned.** `gpui-pre-windows` pins `gpui-pre = "=0.3.10"`, and `gpui-kit` 0.8.1 depends on `gpui-pre` through `gpui-base`. So a `[patch.crates-io]` fork must keep version 0.3.10 and the package names `gpui-pre` and `gpui-pre-windows`.

See proposal.md for why this slice exists. Required behaviour is in `specs/gpui-player`. This document covers how.

## Spike outcomes (task 1.1)

Run locally on 2026-10-29 (NVIDIA GeForce RTX 5090, Windows 11 26300), in a scratch crate outside any repository. The CI half (WARP) was folded into task 4.1's run by the user's choice, so no throwaway branch was pushed.

| Check | Outcome | Effect |
|---|---|---|
| `OpenSharedResource1` of a slice-6-style BGRA `MISC_SHARED \| MISC_SHARED_NTHANDLE` texture, consumer device at FL 11.1 | works | none |
| Same, consumer device forced to FL 10.1 | works | none |
| WARP producer, WARP consumer, at 11.1 and 10.1 | works | none |
| Hardware texture opened on a WARP device (wrong adapter) | fails with `E_INVALIDARG`, no crash | confirms the skip-and-warn path |
| `DuplicateHandle` per paint | 0.37 µs | none |
| Duplicate + `OpenSharedResource1` + SRV per draw | 14 µs | **the cache is dropped** (decision 3) |
| Duplicate + `CompareObjectHandles` over 3 cached entries | 0.66 µs | not needed |
| Headless renderer for `render_to_image` on Windows | **none**: `gpui-pre-platform`'s `current_headless_renderer()` returns `Ok(None)` off macOS and Linux, and `TestWindow::render_to_image` bails without one | **the fork adds one** (decision 8) |
| `gpui_kit::` path of a new root item of `gpui-pre` | `gpui-kit` does `pub use ::gpui::*;`, so `gpui_kit::WINDOWS_SURFACES_REQUIRE_PATCHED_GPUI` | none |
| GPUI's renderer on CI's WARP adapter | moved to task 4.1 | the pixel tests skip with a message if the headless renderer cannot start |

## Slices

The `add-player` series. **This change is slice 7.**

| # | Change | Delivers | Depends on | Status |
|---|---|---|---|---|
| 1 | `add-player-1-core` | `player::Player` handle: builder, sources, load, playback control, property/command access, state snapshot, event subscription; headless CI tests | `add-workspace-scaffold` | done |
| 2 | `add-player-2-render` | mpv OpenGL render API in core over a host-provided GL context: `VideoOutput::Render`, `RenderContext`, `State.video_size`; software-GL render tests on all six CI legs | 1 | done |
| 3 | `add-player-3-ui-core` | `player::ui`, the toolkit-agnostic UI layer (`ui::Options`, `ui::Controls`, `View`, actions, Video.js hotkeys, activity, bar maths); `State.buffered`, `Player::scrub` | 1 | done |
| 4 | `add-player-4-gpui-macos` | macOS video on screen: `player::macos::SurfaceRenderer` (CGL, IOSurface NV12 ring, fenced publication), `hwdec=videotoolbox` default, `Player::ptr_eq`; `player_gpui::VideoPlayer` (black on Windows/Linux) and the `play` example | 2 | done |
| 5 | `add-player-5-gpui-controls` | Video.js controls in GPUI over `VideoPlayer`: control bar, big play button, spinner, error display, fading, pointer/keyboard/focus, actions, window fullscreen, fluid aspect ratio, `player_gpui::Assets`/`Icons` | 3, 4 | done |
| 6 | `add-player-6-windows-surface` | Windows surfaces in core: `player::windows::SurfaceRenderer` (pooled render thread, hidden-window WGL context, D3D11 device on a chosen adapter, BGRA ring of NT-handle textures fed through `WGL_NV_DX_interop2` and a GPU copy, GPU-complete publication), `Frame` with shared handle and adapter LUID, `hwdec=auto-safe` default on Windows, "created after load" code shared with macOS; tests and CI | 2, 4 | done |
| 7 | `add-player-7-gpui-windows` | Windows video in GPUI: a fork of `gpui-pre` + `gpui-pre-windows` 0.3.10 (untouched upstream as the first commit, then one change: a Windows `PaintSurface` payload, `Window::paint_surface`/`surface()` on Windows, `draw_surfaces` opening the shared handle on GPUI's device with a `surface` shader, and a marker item for a readable error when the patch is missing); `[patch.crates-io]` in this workspace; an opt-in `windows-video` feature in `player-gpui` that wires `VideoView` on Windows (black and a one-time warning when creation fails); README setup steps; CI pixel test of the fork's draw path; manual checks | 5, 6 | **current** |
| 8+ | (to be planned) | Linux video surfaces (EGL/dma-buf into GPUI's renderer, after a spike); the `gpui-fast` fork for apps on that backend; dropping the `[patch]` if GPUI-Kit's GPUI ever gains Windows surfaces on its own; menus (rate list, tracks), captions, poster drawing, skip buttons, touch behaviour; hiding the cursor when inactive (needs a GPUI API); HDR surfaces if GPUI gains them; precise GL/D3D adapter matching on hybrid laptops | 6, 7 | pending |

Changes from the plan in slice 6's table:
- **The fork does not expose GPUI's adapter LUID** (decision 4). Slice 6 planned it so the element could pass the LUID to `adapter()`. The core's default adapter rule is already GPUI's rule, and adding a field to `GpuSpecs` would break every platform's struct literal in GPUI.
- **A CI pixel test of the fork's draw path is added.** It does not need GL, so it runs on Mesa's leg (decision 8).
- **No upstream PR** (decision 9). The user decided not to contribute to `zed-industries/zed`, so the `[patch]` stays until GPUI-Kit's GPUI gains Windows surfaces some other way. Dropping it is listed under 8+ for that case only.

## Goals / Non-Goals

**Goals:**
- A minimal, self-contained GPUI change that makes `paint_surface` work on Windows for any shared D3D11 texture, with no player-specific type and no `unsafe` for the caller.
- `player-gpui` reaching on Windows the behaviour macOS has: the same element, sizing, repaint, keyed state, error handling and tests, differing only in the renderer type and the paint call.
- Zero-copy from mpv to the swap chain: the frame is sampled straight from the shared texture by GPUI's shader.
- Apps that do not opt in keep building against stock crates.io GPUI.

**Non-Goals:**
- Any change in `crates/player`.
- The `gpui-fast` backend (`gpui-fast-windows`). It is deferred to 8+, and the docs say that `windows-video` needs GPUI-Kit's default `gpui-pre` backend.
- Exposing GPUI's device or adapter LUID, keyed-mutex synchronisation, NV12 or HDR surfaces on Windows.
- Publishing the fork to crates.io. It is consumed only through `[patch]` with a git source.
- Windows arm64 verification beyond building. Locally there is no arm64 machine, and CI has no GL.

## Decisions

### 1. Where the fork lives and how it is shaped

- **A new GitHub repository, assumed `vegidio/gpui-pre`** (to be confirmed by the user in task 2.5, before anything is pushed). It is a Cargo workspace holding `gpui-pre/` and `gpui-pre-windows/`, unpacked from the crates.io 0.3.10 `.crate` files.
  - **Commit 1** is those sources untouched, the published (normalised) manifests included, so that a diff against crates.io is empty. The repository adds only a root workspace manifest (which patches `gpui-pre` to the sibling for the fork's own builds), a README, `.gitignore` and a `.gitattributes` that turns off line-ending conversion. Task 2.1 found that `Cargo.toml.orig` cannot serve as the manifest: it inherits everything from Zed's workspace (`workspace = true`).
  - **Commit 2** is the Windows surface change, and nothing else.
  - **The tag** is `0.3.10-surfaces.1`. This workspace's `[patch.crates-io]` uses `{ git = "https://github.com/vegidio/gpui-pre", tag = "0.3.10-surfaces.1" }` for both packages.
- **Why not fork `zed-industries/zed`.** `[patch.crates-io]` matches by package name and version. Zed's crates are named `gpui` and `gpui_windows`, at different versions, inside a large workspace with path dependencies. Repackaging them to match `gpui-pre` 0.3.10 is what the `gpui-pre` snapshot already did.
- **Why only two crates.** Every other `gpui-pre-*` crate (`-platform`, `-macros`, `-util`, ...) is unchanged and keeps coming from crates.io. The two patched crates keep version 0.3.10, so the `=0.3.10` pins resolve to them.
- During development, task 2.1's local checkout is used with a path `[patch]`. The switch to the git tag happens when it is pushed.

### 2. The GPUI API (in `gpui-pre`)

```rust
// scene.rs
pub struct PaintSurface {
    pub order: DrawOrder, pub bounds: Bounds<ScaledPixels>, pub content_mask: ContentMask<ScaledPixels>,
    #[cfg(any(target_os = "macos", target_os = "ios"))] pub image_buffer: CFRetained<CVPixelBuffer>,
    #[cfg(target_os = "windows")] pub shared_texture: SharedTexture,
}

/// A Direct3D 11 texture shared through an NT handle (`D3D11_RESOURCE_MISC_SHARED_NTHANDLE`), which GPUI opens on its
/// own device. The producer must have finished writing it before it is painted.
#[cfg(target_os = "windows")] #[derive(Clone, Debug)]
pub struct SharedTexture(Arc<OwnedHandle>);
#[cfg(target_os = "windows")]
impl SharedTexture {
    pub fn new(handle: OwnedHandle) -> Self;
    pub fn duplicate(handle: BorrowedHandle<'_>) -> io::Result<Self>;   // DuplicateHandle via try_clone_to_owned
    pub fn with_size(self, width: u32, height: u32) -> Self;            // only for surface()'s ObjectFit
}

// window.rs
#[cfg(target_os = "windows")]
pub fn paint_surface(&mut self, bounds: Bounds<Pixels>, texture: SharedTexture);

// elements/surface.rs
pub enum SurfaceSource { #[cfg(macos/ios)] Surface(CFRetained<CVPixelBuffer>), #[cfg(windows)] SharedTexture(SharedTexture) }
#[cfg(windows)] impl From<SharedTexture> for SurfaceSource
#[cfg(any(macos, ios, windows))] pub fn surface(source) -> Surface   // the Windows arm takes its size from the texture desc at paint
                                                                      // (ObjectFit needs it), read once and kept in the SharedTexture
/// Marker: present only in the patched GPUI with Windows surfaces. See player-gpui's README.
#[cfg(target_os = "windows")] pub const WINDOWS_SURFACES_REQUIRE_PATCHED_GPUI: () = ();
```

- **A type that owns an NT handle, not a borrowed handle or a raw `HANDLE`.**
  - The scene keeps a `PaintSurface` until the next frame replaces it, which outlives any borrow from the caller.
  - An owned handle keeps the texture alive in the kernel, as slice 6 verified, whatever the producer does meanwhile.
  - `duplicate` makes it safe to call from a borrowed handle, which is exactly what `player::windows::Frame` hands out. `DuplicateHandle` is one cheap system call per paint.
  - `Arc` keeps `PaintSurface: Clone` cheap, because the scene clones primitives into `surfaces`.
- **No `windows` crate type in the API.** It uses std `OwnedHandle`/`BorrowedHandle` (`std::os::windows::io`), so it does not tie GPUI's public API to a `windows` version. This matches slice 6's choice.
- **The texture's size for `surface()`.** `Surface`'s layout needs the content size, which macOS reads from the `CVPixelBuffer`. On Windows, GPUI cannot know it without opening the handle. `SharedTexture::with_size(width, height)` therefore records it, and `surface()` uses `ObjectFit` on it. `paint_surface` (what `player-gpui` uses) takes explicit bounds and does not need it.
- **The marker** is a `const` that `player-gpui` names under `windows-video` (decision 6). On stock GPUI it does not exist, so the first error is "cannot find value `WINDOWS_SURFACES_REQUIRE_PATCHED_GPUI` in crate `gpui_kit`", which reads as the instruction.

### 3. Drawing (in `gpui-pre-windows`)

```
draw_surfaces(surfaces):
  for surface in surfaces:
    srv = open(&surface.shared_texture, &devices)   // OpenSharedResource1 + CreateShaderResourceView, every draw
      on failure: warn unless already warned since the last success, skip this surface
    update the surface pipeline's instance buffer with { bounds, content_mask }   // one instance per draw
    bind srv + linear sampler + GlobalParams; draw 4-vertex strip
```

- **A `surface` shader module in `shaders.hlsl`.** The vertex stage expands bounds to a quad, as `polychrome_sprite` does. The pixel stage samples the BGRA texture with a linear sampler, writes RGB with alpha 1, and discards outside the content mask. It is added to `build.rs`'s `fxc` module list and to `ShaderModule`, so release builds and debug builds (run-time compile) both get it.
- **Colours.** Both the texture and the swap chain are `B8G8R8A8_UNORM` and GPUI blends in gamma space, so sampling and writing passes the bytes through with no conversion, and colours are unchanged. Slice 6 already verified that the texture holds mpv's colours.
- **Opening a texture.** The renderer casts its device to `ID3D11Device1`, calls `OpenSharedResource1(handle)` to get an `ID3D11Texture2D`, and creates an SRV. `ID3D11Device1` comes from the D3D11.1 runtime that Windows 10+ has, at any feature level, including GPUI's 10.1 fallback. Task 1.1 checks this.
- **No cache: opened on every draw.** Task 1.1 measured duplicate + open + SRV at 14 µs per draw on the local GPU, far under the 0.1 ms bar, so the user chose to drop the planned cache to keep the fork's diff small.
  - The opened texture and its SRV are local to the draw and released after it. The D3D11 runtime defers their destruction until the GPU is done with them.
  - With nothing kept between frames, `handle_device_lost` needs no change.
  - The rejected alternatives were a cache keyed by `Arc::ptr_eq`, then `CompareObjectHandles` (0.66 µs per lookup, with 8-frame eviction and clearing on device loss), and a key the caller supplies (it leaks a caching concern into the API). Either can come back if a slower driver shows the open in profiles.
- **Warning once.** The renderer keeps one flag. The first failure logs a `warn!` and sets it. Later failures stay silent, and a successful open clears it. So a texture that keeps failing warns once, not once per frame, as the spec requires, and a new failure after recovery is reported again.
- **No synchronisation in GPUI.** The producer contract, stated on `SharedTexture`, is "finished before painted". `player::windows` meets it by publishing only GPU-complete frames (slice 6, decision 3). A keyed mutex was rejected in slice 6.
- **Failures never fail the frame.** An open failure, such as a texture from another adapter or a closed resource, is warned about as above and skips that surface, so the rest of the window draws, as the spec requires.

### 4. Adapter: no LUID API

`player::windows::SurfaceRenderer`'s default adapter is "the first `EnumAdapters` entry that creates a D3D11 device", which is the rule `gpui-pre-windows::get_adapter` uses. The two therefore agree without an API, except in these cases:
- **After a GPUI device loss**, GPUI may land on another adapter, for example when the GPU was removed. Opening then fails, the surface is skipped with a warning, and the video is black until the element is rebuilt.
- **When GPUI's debug-layer path picks differently.** That happens only in debug builds with the SDK layers installed, and it uses the same enumeration.

Exposing the LUID would need a new `Window` method or a `GpuSpecs` field. A field breaks every platform's struct literal, and a method is more fork surface for a case that falls back to a logged black frame. If it turns out to be needed, 8+ can add `Window::adapter_luid()`.

### 5. `player-gpui` on Windows

- **`video.rs` is split** into a platform-neutral `VideoView` and a small per-platform `surface` module:
  ```rust
  #[cfg(target_os = "macos")]  mod surface { pub(crate) type Renderer = player::macos::SurfaceRenderer; fn build(..) /* Nv12, gl_renderer() */; fn paint(window, bounds, &Frame) /* pixel_buffer */ }
  #[cfg(all(windows, feature = "windows-video"))] mod surface { pub(crate) type Renderer = player::windows::SurfaceRenderer; fn build(..); fn paint(window, bounds, &Frame) { window.paint_surface(fitted, SharedTexture::duplicate(frame.shared_handle())?) } }
  ```
  - The view (channel, task, prepaint sizing, `ObjectFit::Contain`, warn-once on build failure) and its tests are shared.
  - The `cfg(target_os = "macos")` attributes on `Slot.video` and in `element.rs` become `cfg(video)`, where `video` is a `cfg` alias set in `build.rs`: `cargo:rustc-cfg=video` on macOS, and on Windows with `CARGO_FEATURE_WINDOWS_VIDEO`. It is declared with `cargo::rustc-check-cfg`. This keeps the condition in one place instead of repeating `any(target_os = "macos", all(windows, feature = "windows-video"))` everywhere.
- **A duplicate failure** in `paint` (rare, for example handle exhaustion) logs once and paints nothing for that frame, so the element stays black.
- **The marker** is in `lib.rs`, under `cfg(all(windows, feature = "windows-video"))`, with a doc comment pointing at the README: `const _: () = gpui_kit::WINDOWS_SURFACES_REQUIRE_PATCHED_GPUI;`. Task 1.1 confirms that `gpui_kit` re-exports GPUI's root items under that path; otherwise the path names the re-export GPUI-Kit uses.
- **The feature** is `windows-video = ["dep:flume"]`. `flume` moves to `cfg(any(target_os = "macos", windows))` as an optional dependency on Windows. It is a no-op on macOS and Linux, as the spec requires.
- **`gpui-fast`.** If the app enables GPUI-Kit's `gpui-fast`, GPUI-Kit's `gpui` re-export is `gpui-fast`, which has no Windows `paint_surface` and no marker. The build then fails on the marker too, which is correct: this backend is not covered. The README says so.

### 6. Readable error without the patch

What was considered:
- **The marker `const` (chosen).** It needs nothing extra, the error points at an item whose name says what is missing, and its doc comment links to the setup steps.
- **A `build.rs` probe of `cargo metadata` for the patched source.** It is fragile in build scripts, because it re-enters Cargo, and slow.
- **`compile_error!` behind a feature the fork sets.** Not possible: a dependency cannot set its dependents' cfgs.

### 7. Dependencies

| Crate | Where | Why, and alternatives |
|---|---|---|
| Fork of `gpui-pre`, `gpui-pre-windows` 0.3.10 | `[patch.crates-io]`, this workspace | The only way to get Windows surfaces into the GPUI that GPUI-Kit 0.8.1 uses, as chosen by the user in slice 6. The rejected alternatives were hole-punching through DirectComposition (it needs a transparent window and no backgrounds behind the player) and `paint_image` from a CPU readback (it breaks zero-copy). |
| `flume` (existing) | `player-gpui`, Windows, optional under `windows-video` | The same frame-signal channel as macOS. |
| `windows` 0.62 | `player-gpui`, Windows **dev**-dependency | The CI pixel test creates a D3D11 device and a shared BGRA texture (decision 8). It is the version GPUI and `player-rs` already use, so nothing new is built. |
| `gpui-pre-windows` `=0.3.10` (patched), `test-support` | `player-gpui`, Windows **dev**-dependency | Builds the fork's `DirectXHeadlessRenderer` for the pixel tests (decision 8). It is already in the graph through GPUI-Kit, so nothing new is built. The alternative was forking `gpui-pre-platform` too. |

`rust-sak`: none of its modules (`crypto`, `fetch`, `fs`, `github`, `image`, `memo`, `o11y`, `sysinfo`, `tui`) covers GPUI, Direct3D or texture sharing. `fs` stays a dev-dependency of `player-gpui` (test media temp dirs, slice 5). Nothing is added to `rust-sak`.

### 8. Testing

- **A Windows headless renderer in the fork.** Task 1.1 found that GPUI has none on Windows (`current_headless_renderer()` returns `Ok(None)`), so neither pixel test below could run as first designed. The user chose to add one to `gpui-pre-windows`, under `test-support`, as part of commit 2:
  - `gpui_windows::DirectXHeadlessRenderer` implements `PlatformHeadlessRenderer`. It owns a hidden window, used only to give the swap chain an HWND and never shown, and a `DirectXRenderer` built on GPUI's own `DirectXDevices` with DirectComposition off.
  - `render_scene_to_image` resizes it and calls the existing `render_to_image`, which draws without presenting and reads the target back. `render_scene` draws without presenting.
  - Tests plug it into `HeadlessAppContext::with_platform(.., || Ok(Some(Box::new(DirectXHeadlessRenderer::new()?))))`, as GPUI-Kit's macOS and Linux tests plug in `current_headless_renderer`.
  - `gpui-pre-platform` is not forked. Patching its `current_headless_renderer` too would be a third crate. Instead, `player-gpui` takes `gpui-pre-windows = "=0.3.10"` (with `test-support`) as a Windows **dev**-dependency to build the renderer. Apps still patch only two crates.
  - If it cannot start, for example on an adapter that GPUI's renderer rejects, the pixel tests skip with a message. Task 4.1 records whether it runs on CI's WARP.
- **The fork's own tests** (`gpui-pre-windows`, run locally in the fork repository). A `render_to_image` test builds a scene with a `PaintSurface` from a shared BGRA texture filled with 4 solid quadrants, using its own D3D11 device on adapter 0, `MISC_SHARED | MISC_SHARED_NTHANDLE`, and `UpdateSubresource`. It checks:
  - the quadrants' colours and their placement in the bounds
  - clipping by a content mask
  - that a surface whose handle cannot be opened is skipped while a quad drawn after it still appears
  - that repeated paints of the same texture through new duplicates keep drawing it, as `player-gpui` paints it
- **`player-gpui` pixel test** (`cfg(all(windows, feature = "windows-video"))`, runs on CI x64). The same kind of shared texture is drawn through `window.paint_surface` in a headless window and checked with `Window::render_to_image()`.
  - It does not need GL or mpv, so it runs on Mesa's leg, and it proves the patched draw path on CI, which the core renderer cannot do there.
  - Whether the fork's headless renderer runs on CI's WARP adapter is checked in task 4.1. If it does not, the test skips on CI with a message, and that gap is recorded here.
- **The existing `VideoView` harness tests** (state survives parent re-renders, player switch, element removal, two players, bounds × scale, black when the player cannot render) become `cfg(video)`. On Windows they run with `--features windows-video`.
  - When the renderer fails with slice 6's "cannot share textures with Direct3D 11" error (Mesa), they return early with a message, using the same rule as `crates/player/tests/surface/windows.rs`.
  - "Stays black when the player cannot render" runs everywhere.
  - A new test checks that the warning is logged once on Windows without interop. On CI it is the scenario that runs.
- **A real-video pixel test**, locally on the GPU: a 160x120 solid-colour-fields clip through `VideoPlayer` with controls off, checked with `render_to_image` within 3% of each field's colour, upright and letterboxed in a 16:9 element.
- **The non-video test** in `element.rs` (`builds_and_draws_black`) becomes `cfg(not(video))`. It keeps running on Linux, and on Windows without the feature.
- **Manual checks** (task 5.1) with `examples/play.rs` on the local NVIDIA GPU:
  - upright, colours and letterboxing
  - controls over the video, fullscreen and resize
  - no repaint when idle
  - 4K HEVC CPU use in a release build, which exercises the `fxc`-compiled shader
  - two players
  - closing while playing

### 9. No upstream PR

- **Dropped by the user after the manual checks.** The plan was a task 6.1 that ported commit 2 to `zed-industries/zed` and opened a PR. The user decided not to contribute to Zed, so the task was removed.
- **What it means.** The fork and the `[patch]` are the lasting way to get Windows video, not a stopgap. Each GPUI version that GPUI-Kit moves to needs a new fork tag (see Risks).

### 10. CI

- **Lint.** A Windows x64 clippy step: `cargo clippy -p player-gpui --all-targets --features windows-video -- -D warnings`. Today clippy runs only on Linux, which would leave all of this slice's code unlinted.
- **Windows legs.** Build and test `player-gpui` with `--features windows-video`, using the git `[patch]`.
  - x64: the pixel test (decision 8) and the "no interop" warning scenario run, and the video harness tests skip on Mesa.
  - arm64: only the build, as `PLAYER_TEST_SKIP_GL` skips GL, and the pixel test runs if WARP allows it.
- **Release `fxc` path.** CI tests in debug, so the release shader compile is checked by `cargo build --release -p player-gpui --features windows-video --example play` on the x64 leg.
- **Other legs.** macOS and Linux are unchanged. The feature is a no-op there, and `cargo build --features windows-video` on Linux proves it.

### Backend / UI separation, threading, lifetimes, zero-copy

- **Separation.**
  - `crates/player` is not touched, and still has no GUI types.
  - `crates/gpui` gains no D3D, GL or `unsafe`, only the cfg'd paint call. All Direct3D in the UI path is in GPUI's renderer (the fork), where it belongs to the toolkit.
  - A future egui-wgpu crate would open `frame.shared_handle()` through wgpu-hal's DX12 `OpenSharedHandle` and reuse the core unchanged.
- **Threads.**
  - Unchanged from macOS: `on_frame` runs on the render thread and only `try_send`s, and the frame task notifies on GPUI's foreground executor.
  - `paint_surface`, `DuplicateHandle` and the renderer's open and draw all run on GPUI's main thread during its frame.
- **Lifetimes.**
  - `player::windows::Frame` (an `Arc` of the slot's handle) lives only through paint.
  - The `SharedTexture` duplicate lives in the scene until the next frame, and the texture GPUI opens from it lives through that draw.
  - Each of these keeps the kernel texture alive independently, so dropping the renderer, a ring rebuild on resize, or the element's removal never invalidates a texture GPUI is about to sample (slice 6, "handle outlives renderer").
- **Unsafe and FFI.** The only new `unsafe` is in the fork's renderer: COM calls through `windows`. Each block has a `// SAFETY:` note. `player-gpui`'s test helper uses `windows` COM calls (`unsafe`) in test code only.
- **Zero-copy.** mpv renders on the GPU, the core copies on the GPU into the shared slot (slice 6), and GPUI's pixel shader samples that slot into the swap chain. No frame reaches CPU memory. CPU readback happens only in tests (`render_to_image`).
- **GPUI-Kit vs raw GPUI.** GPUI-Kit has no video or surface component. `paint_surface` and `canvas` are raw GPUI, as on macOS. The controls stay on GPUI-Kit components (slice 5).

### Implementation notes

Found while implementing, and not covered by the decisions above:

- **A clamping sampler for surfaces.** GPUI's shared sampler wraps at the edges. A frame scaled up, for example one of the old size during a resize, would then blend its opposite edge into its border pixels. The fork therefore adds a second sampler, the same linear sampler with `CLAMP` addressing, used only by the surface pipeline.
- **`gpui-pre-windows`'s own tests did not build.** Its `cfg(test)` code needs `gpui`'s `test-support`, which only the crate's feature enabled, so `cargo test -p gpui-pre-windows` failed to compile on crates.io's sources. Commit 2 adds `gpui` with `test-support` as a dev-dependency, as Zed's crates do.
- **Upstream failures the fork leaves alone.**
  - `platform::tests::test_clipboard` fails on the local machine with the untouched sources too.
  - `gpui-pre`'s published examples do not build outside Zed's workspace.
  - The fork is checked with `cargo test -p gpui-pre-windows`, where all tests but that one pass, and `cargo clippy -p gpui-pre --lib` and `-p gpui-pre-windows --lib --tests`.
- **A `HeadlessAppContext` window is not redrawn by a view's notifications.** In the real-video pixel test, the view received every frame's notification, yet re-rendered only once, so the screenshots stayed black. The core's frames were checked by reading them back, and they had the right colours. The same view repaints per frame under `TestAppContext` (the repaint tests) and on the real platform (the `play` example re-renders at the video's frame rate). The test therefore refreshes the window before each draw, and says why. It checks pixels, not repainting.
- **`builds_and_draws_black` was flaky.** The controls hear player events from mpv's thread, and the test scheduler panicked when one arrived. It now allows parking, as the video tests do. This was already the case before this slice. It surfaced because the test now runs on Windows without the feature, and keeps running on Linux.
- **The CI-bound tests with Mesa, run locally** (task 3.3). Mesa 26.2.4's `opengl32.dll` and `libgallium_wgl.dll` were placed next to the test binaries, with `GALLIUM_DRIVER=llvmpipe`, as on CI:
  - the six frame-dependent video tests returned early with "the OpenGL driver cannot share textures with Direct3D 11"
  - `warns_once_without_interop`, `stays_black_when_the_player_cannot_render` and the shared-texture pixel test ran and passed
  - GPUI itself still drew on the local GPU, so WARP is checked by task 4.1

### CI results (task 4.1)

Run [40895014557](https://github.com/vegidio/player-rs/actions/runs/40895014557) on `add-player-7-gpui-windows` (commit `88be9f3`), with the git `[patch]` on tag `0.3.10-surfaces.1`. Both lint jobs and all six legs passed.

- **Attempt 1:** Linux x64 failed in `controls::tests::seek_bar_arrow_keys`. The +5 s seek landed at 34.65 s, 0.35 s outside the test's 0.3 s tolerance. This is a slice-5 controls test that this slice does not touch, and it also timed out once locally before any Windows video code was involved. Rerunning only that job passed (attempt 2). The flake is left for a later change.
- **`player-gpui` lib test counts:**

  | Leg | Tests |
  |---|---|
  | Windows x64 | 51, and 59 with `windows-video` |
  | Windows arm64 | 51, and 59 with `windows-video` |
  | macOS arm64 | 56 |
  | macOS x64 | 56 |
  | Linux x64 | 51 |
  | Linux arm64 | 51 |

- **WARP.** The runners have no GPU, so GPUI's renderer runs on the Microsoft Basic Render Driver. `shared_texture_is_drawn_upright_and_letterboxed` ran and passed on both Windows legs. The fork's headless renderer and the surface shader therefore work on WARP, and no skip was needed.
- **Windows x64 (Mesa):** the six interop-dependent video tests skipped with "the OpenGL driver cannot share textures with Direct3D 11", and `warns_once_without_interop` ran and passed.
- **Windows arm64:** the six frame-dependent tests skipped through `PLAYER_TEST_SKIP_GL`, as did the interop-warning test.
- **macOS:** both legs ran the six `VideoView` harness tests through the shared view and the macOS `surface` module.
- **Release build.** The release `play` example built on Windows x64, so `fxc` compiles the `surface` shader.
- **Linux.** Linux x64's `--features windows-video` build passed, so the feature is a no-op there.

### Manual results (task 5.1)

Release build of `examples/play.rs` with `--features windows-video` (commit `90a33a6`), so the `surface` shader is `fxc`-compiled.

- **Machine:**
  - GPU: NVIDIA GeForce RTX 5090, driver 616.92
  - OS: Windows 11 Pro 10.0.26300
  - 32 logical CPUs
  - Display: 2560x1440
- **How it was driven.** A PowerShell script moved and resized the window with `SetWindowPos`, sent real mouse and keyboard input (`mouse_event`, `keybd_event`), closed the window with `WM_CLOSE`, took screenshots of the screen, and read the example's renders-per-second output. CPU is the process's processor time over 6–10 s; libmpv runs in the process, so it includes mpv's own threads.
- **Clips** were made with ffmpeg:
  - 1440x1080 4:3 H.264 at 30 fps (`testsrc`), with a seconds counter for orientation
  - 3840x2160 HEVC at 60 fps (`testsrc2`, `hevc_nvenc`, 40 Mb/s), with AAC audio

| Check | Result |
|---|---|
| Upright, colours, letterboxing | Pass. The 4:3 clip is upright, with the counter readable, and its colours are right. It is pillarboxed and centred at 1280x720 (frame at x 197–1082 in a 1260-wide client), letterboxed at 700x900 and at 1600x500, and pillarboxed at 2560x1440. |
| Controls over the video | Pass. The control bar draws over the bottom of the frame, and shows on hover. A click on the video pauses, `k` resumes, and `f` toggles fullscreen. |
| Fullscreen | Pass. `f` takes the window to 0,0 at 2560x1440, and again restores 100,100 at 1280x720. `fullscreen: true` and then `false` were printed. |
| Window resize | Pass. 30 resizes 33 ms apart, from 800x450 to 1380x740, then a return to 1280x720. The frame stayed fitted, and playback carried on. |
| No repaint while paused | Pass. 0 renders a second for 4–6 s at a time while paused. There were 4 renders in the second fullscreen was left, then 0 again. Resuming went back to 31–32 a second. |
| 4K HEVC at its frame rate | Pass. 60–62 renders a second, with `hwdec-current: nvdec`. The on-screen timecode read 15.567 s at frame 934, which is 15.567 × 60. |
| CPU, 4K60 HEVC | 9% of one core windowed at 1600x950, and 16% fullscreen at 2560x1440. That is 0.3% and 0.5% of the machine. GPU load was 7%, with the decoder at 5%. |
| Two players side by side | Pass. The 4K60 HEVC player and the 4:3 H.264 player each ran in a 1270x720 window. They held 60–62 and 31–32 renders a second, at 14% and 5% of one core. Both pictures were correct and fitted. |
| Closing while playing | Pass. `WM_CLOSE` during playback exited in 196 ms for the 4:3 clip, whose exit code the script could not read, and in 400 ms for the 4K clip, with code 0. In the two-player run, both exited with code 0. Nothing was printed to stderr in any of these. |

- **Not covered:** audio was not listened to, and AMD, Intel and Windows arm64 remain unverified, as the README says.

## Risks / Trade-offs

- [With no upstream PR, the `[patch]` is permanent, and the fork must follow every GPUI version GPUI-Kit moves to] → Commit 2 is one self-contained commit, so a new tag is a replay of it (see the drift risk below). If GPUI ever adds Windows surfaces itself, with a different API, `player-gpui` only calls `paint_surface`, so switching costs one cfg'd call site.
- [`[patch]` friction for apps: every app must copy two lines, and a GPUI-Kit upgrade to a new GPUI version silently stops the patch from applying (Cargo warns "patch was not used")] → The marker turns that silent case into a build error under `windows-video`. The README lists the exact lines per `player-gpui` version, and the fork gets a new tag per GPUI version that GPUI-Kit moves to.
- [The fork's headless renderer does not run on CI's WARP, so the pixel test cannot run on CI] → Task 4.1 checks it. If it cannot run, the test skips with a message, the fork's tests and the local GPU run cover the path, and the gap is recorded here.
- [`OpenSharedResource1` fails on GPUI's 10.1-feature-level devices or on some drivers] → Task 1.1 found it works at 10.1 and on WARP. A failure is skipped with one warning (the spec), never a crash.
- [Opening per draw costs more on another driver than the 14 µs measured on NVIDIA] → The open is one call per video per frame, and the rejected cache (decision 3) can be added back without an API change.
- [Hybrid-GPU laptops or a GPUI device loss put GPUI on another adapter than the renderer] → The texture fails to open, and the video is black with a warning (decision 4). The slice 6 README guidance on steering both to one GPU applies. Precise matching is deferred to 8+.
- [Release builds need `fxc.exe` from the Windows SDK] → GPUI already requires it for its own shaders, so nothing new. CI builds the release example to prove the new module compiles.
- [The fork drifts from crates.io if GPUI-Kit moves to `gpui-pre` 0.3.11+] → A new fork tag is cut by replaying commit 2. It is one commit by construction, which keeps that cheap.
- [Repainting per frame re-renders ancestor views (slice 4's GPUI cost)] → This is unchanged from macOS and already documented.

## Migration Plan

Additive. Apps that do nothing see no change: Windows stays black and builds against stock GPUI. To get Windows video, an app:
1. Enables `player-gpui`'s `windows-video` feature.
2. Adds the two `[patch.crates-io]` lines from the README.
3. Stays on GPUI-Kit's default `gpui-pre` backend.

Rollback is removing the feature and the patch.
