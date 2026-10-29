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
| 7 | `add-player-7-gpui-windows` | Windows video in GPUI: a fork of `gpui-pre` + `gpui-pre-windows` 0.3.10 (untouched upstream as the first commit, then one change: a Windows `PaintSurface` payload, `Window::paint_surface`/`surface()` on Windows, `draw_surfaces` opening the shared handle on GPUI's device with a `surface` shader, and a marker item for a readable error when the patch is missing); `[patch.crates-io]` in this workspace; an opt-in `windows-video` feature in `player-gpui` that wires `VideoView` on Windows (black and a one-time warning when creation fails); README setup steps; CI pixel test of the fork's draw path; manual checks; upstream PR to zed-industries/zed after verification | 5, 6 | **current** |
| 8+ | (to be planned) | Linux video surfaces (EGL/dma-buf into GPUI's renderer, after a spike); the `gpui-fast` fork for apps on that backend; dropping the `[patch]` once GPUI-Kit ships a GPUI with the upstreamed change; menus (rate list, tracks), captions, poster drawing, skip buttons, touch behaviour; hiding the cursor when inactive (needs a GPUI API); HDR surfaces if GPUI gains them; precise GL/D3D adapter matching on hybrid laptops | 6, 7 | pending |

Changes from the plan in slice 6's table:
- **The fork does not expose GPUI's adapter LUID** (decision 4). Slice 6 planned it so the element could pass the LUID to `adapter()`. The core's default adapter rule is already GPUI's rule, and adding a field to `GpuSpecs` would break every platform's struct literal upstream.
- **A CI pixel test of the fork's draw path is added.** It does not need GL, so it runs on Mesa's leg (decision 8).
- **"Dropping the `[patch]`" is listed under 8+.** It can only happen after an upstream merge and a GPUI-Kit release.

## Goals / Non-Goals

**Goals:**
- A minimal, upstreamable GPUI change that makes `paint_surface` work on Windows for any shared D3D11 texture, with no player-specific type and no `unsafe` for the caller.
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
  - **Commit 1** is those sources untouched, with `Cargo.toml.orig` restored as the manifest where the published one was normalised, so that a diff against crates.io is empty.
  - **Commit 2** is the Windows surface change, and nothing else.
  - **The tag** is `0.3.10-surfaces.1`. This workspace's `[patch.crates-io]` uses `{ git = "https://github.com/vegidio/gpui-pre", tag = "0.3.10-surfaces.1" }` for both packages.
- **Why not fork `zed-industries/zed`.** `[patch.crates-io]` matches by package name and version. Zed's crates are named `gpui` and `gpui_windows`, at different versions, inside a large workspace with path dependencies. Repackaging them to match `gpui-pre` 0.3.10 is what the `gpui-pre` snapshot already did. The upstream PR (decision 9) ports commit 2 to Zed's tree instead.
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
- **The texture's size for `surface()`.** `Surface`'s layout needs the content size, which macOS reads from the `CVPixelBuffer`. On Windows, GPUI cannot know it without opening the handle. `SharedTexture::with_size(width, height)` therefore records it, and `surface()` uses `ObjectFit` on it. `paint_surface` (what `player-gpui` uses) takes explicit bounds and does not need it. If upstream review prefers, `surface()` on Windows can be left out of the PR. It is a small, separable part of commit 2.
- **The marker** is a `const` that `player-gpui` names under `windows-video` (decision 6). On stock GPUI it does not exist, so the first error is "cannot find value `WINDOWS_SURFACES_REQUIRE_PATCHED_GPUI` in crate `gpui_kit`", which reads as the instruction. The marker is fork-only and is not part of the upstream PR.

### 3. Drawing (in `gpui-pre-windows`)

```
draw_surfaces(surfaces):
  for surface in surfaces:
    srv = self.surface_cache.get_or_open(&surface.shared_texture, &devices)   // see below
      on failure: warn once per texture (log::warn!), skip this surface
    update the surface pipeline's instance buffer with { bounds, content_mask }   // one instance per draw
    bind srv + linear sampler + GlobalParams; draw 4-vertex strip
  at the end of each frame: surface_cache.evict_unused()
```

- **A `surface` shader module in `shaders.hlsl`.** The vertex stage expands bounds to a quad, as `polychrome_sprite` does. The pixel stage samples the BGRA texture with a linear sampler, writes RGB with alpha 1, and discards outside the content mask. It is added to `build.rs`'s `fxc` module list and to `ShaderModule`, so release builds and debug builds (run-time compile) both get it.
- **Colours.** Both the texture and the swap chain are `B8G8R8A8_UNORM` and GPUI blends in gamma space, so sampling and writing passes the bytes through with no conversion, and colours are unchanged. Slice 6 already verified that the texture holds mpv's colours.
- **Opening a texture.** The renderer casts its device to `ID3D11Device1`, calls `OpenSharedResource1(handle)` to get an `ID3D11Texture2D`, and creates an SRV. `ID3D11Device1` comes from the D3D11.1 runtime that Windows 10+ has, at any feature level, including GPUI's 10.1 fallback. Task 1.1 checks this.
- **The cache.** Opening a resource and creating an SRV per draw is not free, and a video repaints at its frame rate. Entries are `(OwnedHandle clone of the SharedTexture's Arc, ID3D11ShaderResourceView, last_used_frame)`.
  - Lookup first compares `Arc::ptr_eq`, which hits when the same `SharedTexture` is painted again. It then falls back to `CompareObjectHandles` (kernelbase, Windows 10+) against the cached handles. That catches the same texture painted through a new duplicate, which is what `paint_surface(..., SharedTexture::duplicate(frame.shared_handle()))` produces on every paint.
  - Entries not used for `SURFACE_CACHE_FRAMES = 8` drawn frames are released. A 3-slot ring cycles every 3 frames, so it stays cached. The cache is bounded by what was painted in the last 8 frames.
  - On `handle_device_lost`, the cache is cleared, because its SRVs belong to the lost device.
  - Alternatives were opening on every draw (simplest, but a kernel and driver round-trip per frame per video; task 1.1 measures it, and if it costs under 0.1 ms the cache may be dropped to keep the upstream diff small) and a key the caller supplies (it leaks a caching concern into the API).
- **No synchronisation in GPUI.** The producer contract, stated on `SharedTexture`, is "finished before painted". `player::windows` meets it by publishing only GPU-complete frames (slice 6, decision 3). A keyed mutex was rejected in slice 6.
- **Failures never fail the frame.** An open failure, such as a texture from another adapter or a closed resource, logs one `warn!` per texture and skips that surface, so the rest of the window draws, as the spec requires.

### 4. Adapter: no LUID API

`player::windows::SurfaceRenderer`'s default adapter is "the first `EnumAdapters` entry that creates a D3D11 device", which is the rule `gpui-pre-windows::get_adapter` uses. The two therefore agree without an API, except in these cases:
- **After a GPUI device loss**, GPUI may land on another adapter, for example when the GPU was removed. Opening then fails, the surface is skipped with a warning, and the video is black until the element is rebuilt.
- **When GPUI's debug-layer path picks differently.** That happens only in debug builds with the SDK layers installed, and it uses the same enumeration.

Exposing the LUID would need a new `Window` method or a `GpuSpecs` field. A field breaks every platform's struct literal, and a method is more upstream surface for a case that falls back to a logged black frame. If it turns out to be needed, 8+ can add `Window::adapter_luid()`.

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

`rust-sak`: none of its modules (`crypto`, `fetch`, `fs`, `github`, `image`, `memo`, `o11y`, `sysinfo`, `tui`) covers GPUI, Direct3D or texture sharing. `fs` stays a dev-dependency of `player-gpui` (test media temp dirs, slice 5). Nothing is added to `rust-sak`.

### 8. Testing

- **The fork's own tests** (`gpui-pre-windows`, run locally in the fork repository). A `render_to_image` test builds a scene with a `PaintSurface` from a shared BGRA texture filled with 4 solid quadrants, using its own D3D11 device on adapter 0, `MISC_SHARED | MISC_SHARED_NTHANDLE`, and `UpdateSubresource`. It checks:
  - the quadrants' colours and their placement in the bounds
  - clipping by a content mask
  - that a surface whose handle cannot be opened is skipped while a quad drawn after it still appears
  - cache reuse across frames through duplicated handles (one open for N paints)
- **`player-gpui` pixel test** (`cfg(all(windows, feature = "windows-video"))`, runs on CI x64). The same kind of shared texture is drawn through `window.paint_surface` in a headless window and checked with `Window::render_to_image()`.
  - It does not need GL or mpv, so it runs on Mesa's leg, and it proves the patched draw path on CI, which the core renderer cannot do there.
  - Whether GPUI's headless Windows renderer runs on CI's WARP adapter is task 1.1's question. If it does not, the test skips on CI with a message, and that gap is recorded here.
- **The existing `VideoView` harness tests** (state survives parent re-renders, player switch, element removal, two players, bounds × scale, black when the player cannot render) become `cfg(video)`. On Windows they run with `--features windows-video`.
  - When the renderer fails with slice 6's "cannot share textures with Direct3D 11" error (Mesa), they return early with a message, using the same rule as `crates/player/tests/surface/windows.rs`.
  - "Stays black when the player cannot render" runs everywhere.
  - A new test checks that the warning is logged once on Windows without interop. On CI it is the scenario that runs.
- **A real-video pixel test**, locally on the GPU: a 160x120 solid-colour-fields clip through `VideoPlayer` with controls off, checked with `render_to_image` within 3% of each field's colour, upright and letterboxed in a 16:9 element.
- **The non-video test** in `element.rs` (`builds_and_draws_black`) becomes `cfg(not(video))`. It keeps running on Linux, and on Windows without the feature.
- **Manual checks** (task 4.1) with `examples/play.rs` on the local NVIDIA GPU:
  - upright, colours and letterboxing
  - controls over the video, fullscreen and resize
  - no repaint when idle
  - 4K HEVC CPU use in a release build, which exercises the `fxc`-compiled shader
  - two players
  - closing while playing

### 9. Upstream PR

- **Gated on the user's go-ahead** after task 4.1. Commit 2 is ported to `zed-industries/zed` main, in `crates/gpui` and `crates/gpui_windows`.
  - Zed has moved on since `08cbb88`, so the port is re-applied by hand and built with Zed's `script/clippy` on Windows.
  - The marker `const` is left out of the PR.
  - The PR description explains the use case and the producer contract ("finished before painted").
- **The PR link is recorded here.** Its outcome does not gate this slice's archive. Dropping the `[patch]` waits for an upstream merge and a GPUI-Kit release (8+).

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
  - `paint_surface`, `DuplicateHandle` and the renderer's open, cache and draw all run on GPUI's main thread during its frame.
- **Lifetimes.**
  - `player::windows::Frame` (an `Arc` of the slot's handle) lives only through paint.
  - The `SharedTexture` duplicate lives in the scene until the next frame, and its cache entry lives until 8 frames without use.
  - Each of these keeps the kernel texture alive independently, so dropping the renderer, a ring rebuild on resize, or the element's removal never invalidates a texture GPUI is about to sample (slice 6, "handle outlives renderer").
- **Unsafe and FFI.** The only new `unsafe` is in the fork's renderer: COM calls through `windows` and `CompareObjectHandles`. Each block has a `// SAFETY:` note. `player-gpui`'s test helper uses `windows` COM calls (`unsafe`) in test code only.
- **Zero-copy.** mpv renders on the GPU, the core copies on the GPU into the shared slot (slice 6), and GPUI's pixel shader samples that slot into the swap chain. No frame reaches CPU memory. CPU readback happens only in tests (`render_to_image`).
- **GPUI-Kit vs raw GPUI.** GPUI-Kit has no video or surface component. `paint_surface` and `canvas` are raw GPUI, as on macOS. The controls stay on GPUI-Kit components (slice 5).

## Risks / Trade-offs

- [Upstream review rejects or reshapes the API (for example, it wants a keyed mutex, a D3D12-friendly type, or no `surface()`)] → Apps keep the `[patch]` until it lands. `player-gpui` only calls `paint_surface`, so a reshaped upstream API costs one cfg'd call site in 8+.
- [`[patch]` friction for apps: every app must copy two lines, and a GPUI-Kit upgrade to a new GPUI version silently stops the patch from applying (Cargo warns "patch was not used")] → The marker turns that silent case into a build error under `windows-video`. The README lists the exact lines per `player-gpui` version, and the fork gets a new tag per GPUI version that GPUI-Kit moves to.
- [GPUI's headless renderer does not run on CI's WARP, so the pixel test cannot run on CI] → Task 1.1 checks it. If it cannot run, the test skips with a message, the fork's tests and the local GPU run cover the path, and the gap is recorded here.
- [`OpenSharedResource1` fails on GPUI's 10.1-feature-level devices or on some drivers] → Task 1.1 checks feature level 10.1 on WARP. A failure is skipped with one warning (the spec), never a crash.
- [`CompareObjectHandles` costs more than expected with many cached entries] → The cache holds at most what was painted in 8 frames (3 per video). The `Arc::ptr_eq` fast path comes first. If task 1.1 shows that opening per draw is cheap, the cache goes.
- [Hybrid-GPU laptops or a GPUI device loss put GPUI on another adapter than the renderer] → The texture fails to open, and the video is black with a warning (decision 4). The slice 6 README guidance on steering both to one GPU applies. Precise matching is deferred to 8+.
- [Release builds need `fxc.exe` from the Windows SDK] → GPUI already requires it for its own shaders, so nothing new. CI builds the release example to prove the new module compiles.
- [The fork drifts from crates.io if GPUI-Kit moves to `gpui-pre` 0.3.11+] → A new fork tag is cut by replaying commit 2. It is one commit by construction, which keeps that cheap.
- [Repainting per frame re-renders ancestor views (slice 4's GPUI cost)] → This is unchanged from macOS and already documented.

## Migration Plan

Additive. Apps that do nothing see no change: Windows stays black and builds against stock GPUI. To get Windows video, an app:
1. Enables `player-gpui`'s `windows-video` feature.
2. Adds the two `[patch.crates-io]` lines from the README.
3. Stays on GPUI-Kit's default `gpui-pre` backend.

Rollback is removing the feature and the patch. Once upstream GPUI and GPUI-Kit ship the change, a later slice makes the feature a no-op default and drops the patch from the README.
