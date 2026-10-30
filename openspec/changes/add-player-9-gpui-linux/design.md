## Context

What already exists:
- **`player::linux::SurfaceRenderer` (slice 8)** has the same shape as the macOS and Windows renderers: builder, `set_size`, `latest`, `on_frame`.
  - Each `Frame` gives `fd() -> BorrowedFd<'_>` (a single-plane dma-buf), `fourcc()` (`DRM_FORMAT_ABGR8888`, R, G, B, A bytes), `modifier()`, `stride()`, `offset()`, `device() -> DrmDevice`, its size and a sequence number.
  - Frames are published only once the GPU has finished them. A ring of 3 slots means that, per renderer, at most 3 distinct buffers are in flight, and a published buffer is unchanged until two newer frames are published.
  - The builder takes `device(DrmDevice)` and `modifiers(..)`, defaulting to the first DRM node that builds and `[LINEAR]`.
  - Creation fails with `Error::Render` "no usable DRM render node is available" when `/dev/dri` has none (WSL2 without `vgem`).
  - On `vgem` with llvmpipe, only the **card** node works (dumb buffers), so the default device there is `card0` (226:0), not a render node (slice 8, 13a).
- **`player-gpui` (slices 4, 5, 7)**:
  - `VideoView` (`crates/gpui/src/video.rs`) is shared. It builds the renderer, notifies on `on_frame` through `flume::bounded(1)`, calls `set_size` at prepaint and paints with `ObjectFit::Contain`.
  - Per-platform `video/macos.rs` and `video/windows.rs` give `build`, `paint` and their own tests.
  - `build.rs` sets the `video` cfg on macOS and Windows.
  - Windows video is always on. `lib.rs` names `gpui_kit::WINDOWS_SURFACES_REQUIRE_PATCHED_GPUI`, and the workspace patches `gpui-pre` and `gpui-pre-windows` with tag `0.3.10-surfaces.1` of `vegidio/gpui-pre`. That repository has two commits: the crates.io sources, untouched, then the Windows change.
- **CI** loads `vgem` on both Linux legs (slice 8), with `LIBGL_ALWAYS_SOFTWARE=1`. The Linux surface tests run in full there. Lint runs on Linux x64 and Windows x64.

What was verified in `gpui-pre-wgpu` 0.3.10 and wgpu-hal 30.0.1 for this design:
- **Instance and adapter.** `WgpuContext::instance` uses `Backends::VULKAN | Backends::GL`.
  - Adapters are sorted by `ZED_DEVICE_ID`, then the compositor's GPU, then device type (discrete, integrated, other, virtual, CPU), then Vulkan before GL.
  - The first adapter that configures the window's surface wins. After a device loss, `recover` creates a new context that rejects software adapters.
  - One context is shared by every window.
- **Device.** `create_device` requests only `DUAL_SOURCE_BLENDING` (when available) with downlevel limits.
- **Surfaces.**
  - `draw` has `PrimitiveBatch::Surfaces(_surfaces) => {}`.
  - A `surfaces` pipeline and bind-group layout exist (`SurfaceParams` uniform, `t_y`, `t_cb_cr`, `s_surface`), and `fs_surface` converts NV12. Nothing feeds it.
- **Swap chain.** The swap-chain format prefers `Bgra8Unorm`, then `Rgba8Unorm`, then any non-sRGB format, so GPUI blends in gamma space, as on Windows.
- **Headless.** Under `test-support`, `WgpuHeadlessRenderer` exists. It shares one device per process, and `gpui-pre-platform::current_headless_renderer` returns it on Linux. Pixel tests are therefore possible without a fork of the platform crate.
- **wgpu-hal Vulkan.**
  - `vulkan::Device::texture_from_dmabuf_fd(OwnedFd, &desc, modifier, stride, offset)` needs `Features::VULKAN_EXTERNAL_MEMORY_DMA_BUF`. wgpu-hal exposes that feature only when `VK_KHR_external_memory_fd`, `VK_EXT_external_memory_dma_buf` and `VK_EXT_image_drm_format_modifier` are all present, and it enables those extensions when they are supported.
  - `texture_from_raw` wraps a `vk::Image` the caller made, with an optional drop callback.
  - `ash` 0.38 is in the graph.
- **wgpu-hal GLES.**
  - `gles::Device::texture_from_raw(NonZeroU32, &desc, drop_callback)` wraps a GL texture name, and without a drop callback wgpu deletes it under the context lock.
  - `gles::AdapterContext` exposes `egl_instance()`, `raw_display()` and `lock()`, which makes wgpu's context current.
  - `khronos-egl` 6.0 and `glow` 0.17 are in the graph.
- **Pins.** `gpui-pre-wgpu` pins `gpui-pre = "=0.3.10"`. `gpui-pre-linux` builds the renderer, but has no surface code to change, so it is not forked.
- **Drivers on hand** (slice 8, 13a):
  - lavapipe has `VK_KHR_external_memory_fd` and `VK_EXT_external_memory_dma_buf`, but neither `VK_EXT_image_drm_format_modifier` nor `VK_EXT_physical_device_drm`. A `VK_IMAGE_TILING_LINEAR` image bound to imported dma-buf memory read back correctly on WSL2.
  - WSL2's `dzn` (Vulkan on D3D12) and `d3d12` (GL) cannot import dma-bufs. wgpu hides `dzn` anyway, as a non-conformant Vulkan driver (spike, task 1.1). `d3d12`'s EGL advertises `EGL_EXT_image_dma_buf_import(_modifiers)` and reports `vgem`'s render node, but `eglCreateImage` fails with `EGL_BAD_ALLOC` (task 2.4).
  - llvmpipe's EGL has `EGL_EXT_image_dma_buf_import(_modifiers)` on the GBM platform. It has not been checked on the X11 or Wayland platforms that GPUI's GL backend uses.

See proposal.md for why. Required behaviour is in `specs/gpui-player`. This document covers how.

## Slices

The `add-player` series. **This change is slice 9.**

| # | Change | Delivers | Depends on | Status |
|---|---|---|---|---|
| 1 | `add-player-1-core` | `player::Player` handle: builder, sources, load, playback control, property/command access, state snapshot, event subscription; headless CI tests | `add-workspace-scaffold` | done |
| 2 | `add-player-2-render` | mpv OpenGL render API in core over a host-provided GL context: `VideoOutput::Render`, `RenderContext`, `State.video_size`; software-GL render tests on all six CI legs | 1 | done |
| 3 | `add-player-3-ui-core` | `player::ui`, the toolkit-agnostic UI layer (`ui::Options`, `ui::Controls`, `View`, actions, Video.js hotkeys, activity, bar maths); `State.buffered`, `Player::scrub` | 1 | done |
| 4 | `add-player-4-gpui-macos` | macOS video on screen: `player::macos::SurfaceRenderer` (CGL, IOSurface NV12 ring, fenced publication), `hwdec=videotoolbox` default, `Player::ptr_eq`; `player_gpui::VideoPlayer` (black on Windows/Linux) and the `play` example | 2 | done |
| 5 | `add-player-5-gpui-controls` | Video.js controls in GPUI over `VideoPlayer`: control bar, big play button, spinner, error display, fading, pointer/keyboard/focus, actions, window fullscreen, fluid aspect ratio, `player_gpui::Assets`/`Icons` | 3, 4 | done |
| 6 | `add-player-6-windows-surface` | Windows surfaces in core: `player::windows::SurfaceRenderer` (pooled render thread, hidden-window WGL context, D3D11 device on a chosen adapter, BGRA ring of NT-handle textures, GPU-complete publication), `hwdec=auto-safe` default on Windows, "created after load" code shared with macOS | 2, 4 | done |
| 7 | `add-player-7-gpui-windows` | Windows video in GPUI: fork of `gpui-pre` + `gpui-pre-windows` 0.3.10 (Windows `PaintSurface` payload, `paint_surface`/`surface()` on Windows, `draw_surfaces` with a `surface` shader, a marker item, a headless Direct3D renderer); `[patch.crates-io]`; Windows video always on in `player-gpui`; README setup; CI pixel test | 5, 6 | done |
| 8 | `add-player-8-linux-surface` | Linux surfaces in core: `player::linux::SurfaceRenderer` (EGL on GBM of a chosen DRM node, surfaceless GL, ring of 3 single-plane RGBA GBM buffers, GPU-complete publication), `Frame` with dma-buf fd and layout, modifier list with LINEAR default, `hwdec=vaapi,nvdec`; software verification through `vgem`; CI with `vgem` | 2, 6 | done |
| 9 | `add-player-9-gpui-linux` | Linux video in GPUI: the fork gains `gpui-pre-wgpu` (Linux `DmaBufTexture` payload, `paint_surface`/`surface()` on Linux, an import description and a marker in `gpui-pre`; `VULKAN_EXTERNAL_MEMORY_DMA_BUF` requested; `draw_surfaces` importing each dma-buf, cached per buffer, through `texture_from_dmabuf_fd`, a LINEAR `VK_IMAGE_TILING_LINEAR` fallback, or an EGLImage on wgpu's GL backend; an RGBA surface shader); tag `0.3.10-surfaces.2` with three `[patch]` entries; Linux video always on in `player-gpui`, on GPUI's device and modifiers when known; README setup; headless pixel tests on lavapipe and llvmpipe | 5, 7, 8 | **current** |
| 10+ | (to be planned) | The `gpui-fast` fork for apps on that backend; menus (rate list, tracks), captions, poster drawing, skip buttons, touch behaviour; hiding the cursor when inactive (needs a GPUI API); HDR surfaces if GPUI gains them; precise GL/D3D adapter matching on hybrid laptops; Linux hardware verification (Mesa Intel/AMD, NVIDIA proprietary, VA-API/NVDEC zero-copy, tiled modifiers, `texture_from_dmabuf_fd` itself) | 8, 9 | pending |

Changes from the plan in slice 8's table:
- **No `linux-video` feature.** Linux video is always on, and Linux builds need the patch, as Windows ended up (slice 7's late change). The user chose this.
- **A LINEAR Vulkan import without modifiers** is added next to `texture_from_dmabuf_fd`. The user chose this, so that lavapipe, the only Vulkan driver here that imports dma-bufs, can test GPUI's Vulkan path.
- **The headless pixel tests run on lavapipe and on llvmpipe's GL**, not on lavapipe through wgpu-hal's modifier import, which lavapipe lacks.

## Goals / Non-Goals

**Goals:**
- A self-contained GPUI change that makes `paint_surface` work on Linux for any single-plane dma-buf, on wgpu's Vulkan and GL backends. It needs no player-specific type and no `unsafe` for the caller.
- `player-gpui` reaching on Linux the behaviour of macOS and Windows: the same element, sizing, repaint, keyed state, warn-once and tests, differing only in the renderer setup and the paint call.
- Zero-copy from mpv to the swap chain. GPUI samples the very memory mpv drew into.
- Both backends tested by pixel tests on hardware we have: lavapipe (Vulkan, LINEAR fallback) and llvmpipe (GL).

**Non-Goals:**
- Any change in `crates/player`.
- Multi-plane layouts, NV12 surfaces on Linux, HDR, explicit sync (`sync_file`), and queue-family ownership transfers beyond what wgpu-hal's import does.
- A non-LINEAR import without `VK_EXT_image_drm_format_modifier`. Tiled buffers need the modifier extension, and the core's default is LINEAR anyway.
- The `gpui-fast` backend, and forking `gpui-pre-linux` or `gpui-pre-platform`.
- Verification on real Linux GPUs (10+).

## Decisions

### 1. The fork: one repository, a new tag

- **Same repository, `vegidio/gpui-pre`.** Two new commits on top of the Windows commit:
  - **Commit 3** adds `gpui-pre-wgpu/`, unpacked from the crates.io 0.3.10 `.crate`, untouched, and adds it as a workspace member.
  - **Commit 4** is the Linux surface change in `gpui-pre` and `gpui-pre-wgpu`, and nothing else.
- **Tag `0.3.10-surfaces.2`.** This workspace's `[patch.crates-io]` points `gpui-pre`, `gpui-pre-windows` and `gpui-pre-wgpu` at it.
  - Windows apps keep working on the new tag, because commit 2 is unchanged.
  - Apps that build for Windows only need just the two Windows lines. Apps that build for Linux need `gpui-pre` and `gpui-pre-wgpu`. The README gives all three, since most apps build for both.
- **Why not fork `gpui-pre-linux` too.** Its X11 and Wayland windows only construct `WgpuRenderer`, so nothing in it has to change. The import description (decision 2) lives in `gpui-pre`, which the wgpu crate can reach.
- During development, a local checkout is used through a path `[patch]`. The switch to the git tag happens when the tag is pushed, with the user's go-ahead.

### 2. The GPUI API (in `gpui-pre`, `cfg(target_os = "linux")`)

```rust
// scene.rs
pub struct PaintSurface { .., #[cfg(target_os = "linux")] pub dma_buf: DmaBufTexture }

/// A single-plane dma-buf that GPUI imports on its own device. The producer must have finished writing it before it
/// is painted, and must not write it again while it may still be drawn.
#[derive(Clone, Debug)]
pub struct DmaBufTexture { fd: Arc<OwnedFd>, layout: DmaBufLayout }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DmaBufLayout { pub width: u32, pub height: u32, pub fourcc: u32, pub modifier: u64, pub stride: u32, pub offset: u32 }
impl DmaBufTexture {
    pub fn new(fd: OwnedFd, layout: DmaBufLayout) -> Self;
    pub fn duplicate(fd: BorrowedFd<'_>, layout: DmaBufLayout) -> io::Result<Self>;   // try_clone_to_owned
}

/// What GPUI's renderer can import, once it has a device: None before, and on backends that report nothing.
#[derive(Clone, Debug, Default)]
pub struct DmaBufImport { pub render_node: Option<(u32, u32)>, pub modifiers: Vec<u64> }   // modifiers for ABGR8888
pub fn dma_buf_import() -> Option<DmaBufImport>;
#[doc(hidden)] pub fn set_dma_buf_import(import: Option<DmaBufImport>);   // called by gpui-pre-wgpu

// window.rs, elements/surface.rs
pub fn paint_surface(&mut self, bounds: Bounds<Pixels>, texture: DmaBufTexture);    // Linux
pub enum SurfaceSource { .., #[cfg(target_os = "linux")] DmaBuf(DmaBufTexture) }   // surface() fits by layout's size

/// Marker: present only in the patched GPUI with Linux surfaces. See player-gpui's README.
pub const LINUX_SURFACES_REQUIRE_PATCHED_GPUI: () = ();
```

- **An owned, duplicated fd**, as slice 7's `SharedTexture` owns an NT handle:
  - The scene keeps the primitive past any borrow.
  - An open fd keeps the buffer's memory alive whatever the producer does (slice 8, "descriptor outlives renderer").
  - `dup` is one cheap system call per paint.
- **Only std and plain integers in the API.** There are no `wgpu`, `ash`, EGL or GBM types, so GPUI's public API is not tied to those crates.
- **The layout is plain data**, exactly what both imports need. `fourcc` is checked at import: only `ABGR8888` (R, G, B, A bytes) is accepted, and anything else is a skipped surface with a warning.
- **`dma_buf_import()` is process-wide**, because the wgpu context is shared by every window. A per-window method would need `PlatformWindow`, which lives in the unforked `gpui-pre-linux`. The wgpu crate sets it when it creates a context, including after device loss, and clears it to `None` when the context has no import support.
- `render_node` is `(major, minor)` as plain integers, so `gpui-pre` does not depend on `player-rs`. `player-gpui` converts it to `DrmDevice`.

### 3. Device features and what GPUI reports (in `gpui-pre-wgpu`)

- **`create_device`** adds `VULKAN_EXTERNAL_MEMORY_DMA_BUF` to `required_features` when `adapter.features()` has it. wgpu-hal then enables the external-memory and modifier extensions.
  - Without the feature, but with `VK_EXT_external_memory_dma_buf` and `VK_KHR_external_memory_fd` supported (lavapipe), the fallback (decision 4) needs those two extensions enabled. wgpu-hal enables any supported `external_memory_dma_buf` and `external_memory_fd` on its own (`adapter.rs`, the optional extension list). The spike confirms this for lavapipe; if not, the fallback is reported unavailable and the device is unchanged.
- **The import description**, built once per context:
  - **Vulkan.**
    - `render_node` comes from `VkPhysicalDeviceDrmPropertiesEXT` (render major and minor), when `VK_EXT_physical_device_drm` is supported.
    - `modifiers` come from `VkDrmFormatModifierPropertiesListEXT` for `R8G8B8A8_UNORM`, keeping those with one plane and `SAMPLED_IMAGE`, and LINEAR always last. They are read only when the modifier extension is enabled.
    - With only the fallback, it is `{ render_node, modifiers: [LINEAR] }`, with `render_node` as above (`None` on lavapipe, which lacks `VK_EXT_physical_device_drm`).
    - With neither (`dzn`, which wgpu hides anyway), it is `None`.
  - **GL.**
    - `modifiers` come from `eglQueryDmaBufModifiersEXT(ABGR8888)`, without the external-only ones.
    - The description is taken from what the driver advertises, with no per-driver exceptions: WSL2's `d3d12` GL gets one too (`renderD128`, `[LINEAR]`), and its first import fails, so its video is black with one warning, the spec's warn-once case (the user's choice, over a driver denylist or a trial import in the probe).
    - `render_node` comes from `EGL_EXT_device_drm_render_node` through `eglQueryDisplayAttribEXT(EGL_DEVICE_EXT)`, when present. On WSL2 with `vgem`, llvmpipe's EGL reports `renderD128`, on which the core cannot render with llvmpipe (no dumb buffers), so `player-gpui`'s fallback to the core's defaults is taken there (decision 5). On CI's stock Mesa, EGL runs on Mesa's software device and reports no node.
    - Without `EGL_EXT_image_dma_buf_import`, it is `None`.

### 4. Importing and drawing (in `gpui-pre-wgpu`)

```
draw_surfaces(surfaces, pass):
  for surface in surfaces:
    key = (fstat(fd).st_ino, layout)              // a dma-buf's inode is unique to its buffer
    entry = cache.get(key) or import(surface)      // on failure: warn unless already warned since last success; skip
    entry.last_used = frame_index
    write SurfaceParams { bounds, content_mask } into the entry's uniform buffer; set the entry's bind group; draw 4 vertices
  evict entries unused for 8 frames
```

- **Vulkan, with the modifier extension.** It calls `texture_from_dmabuf_fd(fd.try_clone_to_owned(), desc, modifier, stride, offset)` as wgpu-hal intends. `desc` is `Rgba8Unorm`, the layout's size and `TEXTURE_BINDING`.
- **Vulkan, LINEAR fallback.** This is used when the device lacks `VULKAN_EXTERNAL_MEMORY_DMA_BUF`, its two dma-buf extensions are enabled, and `modifier == LINEAR`. It is about 100 lines of `ash` through `device.as_hal::<Vulkan>()`, and checks `stride` and `offset` against `vkGetImageSubresourceLayout`:
  1. Create a `VK_IMAGE_TILING_LINEAR` `R8G8B8A8_UNORM` image with `VkExternalMemoryImageCreateInfo(DMA_BUF)`.
  2. Read `vkGetMemoryFdPropertiesKHR` and import the fd (`VkImportMemoryFdInfoKHR`, dedicated allocation).
  3. Bind the memory, and wrap the image with `texture_from_raw`, whose drop callback frees the image and memory.
  4. If the driver's linear row pitch differs from the buffer's `stride` (at the padded width above), or `offset != 0`, reject the import with a warning, since a linear image cannot take an explicit pitch without the modifier extension.
- **GL.** On wgpu's own EGL display, with its context current (`AdapterContext::lock()`):
  1. `eglCreateImage(EGL_LINUX_DMA_BUF_EXT)` with the fd, fourcc, plane-0 offset and pitch, and the modifier attributes when the modifier is not LINEAR or the modifiers extension is present.
  2. Generate a texture and call `glEGLImageTargetTexture2DOES`, loaded through `eglGetProcAddress`.
  3. Destroy the EGLImage at once, because the texture keeps its storage.
  4. Wrap it with `gles::Device::texture_from_raw` and no drop callback, so wgpu deletes it under its lock.
- **LINEAR buffers are imported as wide as their rows** (spike, task 1.1), on all three paths: the texture is `stride / 4` pixels wide (when `stride` is a multiple of 4), so its row pitch is the buffer's stride, and the shader samples only the layout's width.
  - lavapipe's linear images have rows aligned to 64 bytes, while `vgem`'s (GBM's) buffers have rows aligned to 256 bytes, so at the layout's own width the pitch check rejects every width that is not a multiple of 64. At the padded width the pitches match for every size tested.
  - Mesa 25.2.8's software EGL device (CI's stock Mesa) ignores `EGL_DMA_BUF_PLANE0_PITCH_EXT` and draws rows `width * 4` bytes apart. At the padded width, the two agree.
  - It is no copy: the texture is the buffer's memory, and the padding columns are never sampled (the texture coordinate is scaled by `width / texture_width` and clamped half a texel inside the last column, so linear filtering never reaches them).
  - Buffers with other modifiers keep their own width; their pitch is given explicitly.
- **Then, on every backend**, `create_texture_from_hal`, a view, and a bind group on a new surface layout: a uniform, one texture and a clamping linear sampler (slice 7's implementation note on sampler wrap). They are kept in the cache entry.
- **A new `vs_rgba_surface` / `fs_rgba_surface`** in `shaders.wgsl`, which native GL compiles too (with `shaders_storage.wgsl`; `shaders_webgl.wgsl` is for wasm only, as the spike confirmed from `create_pipelines`). It samples the texture at the scaled and clamped coordinate, writes RGB with alpha 1, and clips outside the content mask as the NV12 shader does.
  - Its parameters (bounds, content mask, coordinate scale and clamp) are written for all of a frame's surfaces into one uniform buffer and bound with a dynamic offset, so the same buffer painted twice in a frame draws correctly. The cached bind group holds only the texture and the sampler.
  - The sampler is the renderer's existing linear, clamp-to-edge atlas sampler.
  - The NV12 pipeline is left untouched, so the fork's diff stays additive.
  - Colours pass through unchanged: an `Rgba8Unorm` source into a non-sRGB swap chain, blending in gamma space, as on Windows.
- **Why a cache, unlike Windows** (slice 7 dropped its cache at 14 µs per open):
  - A Vulkan import allocates device memory and creates an image, a view and a bind group. Allocations count against `maxMemoryAllocationCount`, and their cost on real drivers is unknown here.
  - The core's ring means only 3 buffers per renderer recur, so a small cache hits on almost every frame.
  - The key is the buffer's inode, not the fd, because every paint duplicates a new fd for the same buffer.
  - Entries hold their own import (Vulkan memory, or a GL texture), which keeps the buffer alive independently of the fd.
  - After a resize, the core frees its old ring, and the cache drops those imports after 8 unused frames.
  - The cache lives in the renderer's resources, so a device loss drops it with them.
- **Warning once**, as slice 7 does: one flag, set by the first failure and cleared by a successful import (not by a cache hit, so one bad surface next to a good one warns once). A failure never fails the frame, and the rest of the window draws. A device that cannot import at all (`dma_buf_import()` is `None`) fails each surface the same way, with a warning naming the adapter.
- **No synchronisation in GPUI.** The producer publishes GPU-complete frames, and a slot is not rewritten until two newer frames are published (slice 8, decision 5). GPUI takes the newest frame every paint, so it never samples a slot being written.

### 5. `player-gpui` on Linux

- **`video/linux.rs`**, beside `macos.rs` and `windows.rs`:
  ```rust
  pub(crate) use player::linux::{Frame, SurfaceRenderer};
  pub(super) fn build(player, on_frame) -> player::Result<SurfaceRenderer>  // GPUI's node and modifiers, else defaults
  pub(super) fn paint(window, bounds, &Frame)  // DmaBufTexture::duplicate(frame.fd(), layout) -> window.paint_surface
  ```
- **`build`** (spec "Render device and layout from GPUI"):
  - When `gpui::dma_buf_import()` has a `render_node`, it builds with `device(..)` and `modifiers(import.modifiers)`.
  - If that fails, for example on a render node where llvmpipe has no dumb buffers, or when GPUI reports no node, it builds with the core's defaults (first node that builds, `[LINEAR]`).
  - When GPUI reports modifiers but no node, the modifiers are not passed, because they may not apply to the core's device. LINEAR is the one layout that crosses devices.
  - The returned error, if any, is the defaults' error. `VideoView` already logs it once ("no usable DRM render node is available" on WSL2 without `vgem`).
- **`paint`**: a `dup` failure logs once and paints nothing, as on Windows.
- **`build.rs`** sets `video` on Linux too, so the `cfg(video)` view, the slot and the harness tests are compiled on Linux, and `builds_and_draws_black` (`cfg(not(video))`) no longer runs on Linux.
- **`flume`** moves to `cfg(any(target_os = "macos", windows, target_os = "linux"))`.
- **The marker** sits in `lib.rs` under `cfg(target_os = "linux")`: `const _: () = gpui_kit::LINUX_SURFACES_REQUIRE_PATCHED_GPUI;`. Its doc comment points at the README.
- **`gpui-fast`**: GPUI-Kit's `gpui` is then `gpui-fast`, which has no marker, so the build fails there. That is correct, and the README says so.

### 6. Readable error without the patch

The same mechanism as Windows (slice 7, decision 6): a marker `const` named in `lib.rs` before any module. On stock GPUI, the first error is "cannot find value `LINUX_SURFACES_REQUIRE_PATCHED_GPUI` in crate `gpui_kit`". Task 3.1 checks that it comes first on Linux, ahead of the `paint_surface` and `DmaBufTexture` errors.

### 7. Dependencies

| Crate | Where | Why, and alternatives |
|---|---|---|
| Fork of `gpui-pre`, `gpui-pre-windows`, `gpui-pre-wgpu` 0.3.10, tag `0.3.10-surfaces.2` | `[patch.crates-io]`, this workspace | The only way to get Linux surfaces into the GPUI that GPUI-Kit 0.8.1 uses, as slice 7 did for Windows. Rejected: a CPU readback into `paint_image` (breaks zero-copy), and a transparent hole over a child Wayland or X11 surface (no backgrounds behind the player, and it differs per display server). |
| `ash` 0.38, `khronos-egl` 6.0, `glow` 0.17 | the fork's `gpui-pre-wgpu`, Linux | Raw Vulkan for the LINEAR fallback and the import description, and EGL and GL for the GL import. These are the versions wgpu-hal 30 already uses, so nothing new is built. Rejected: forking wgpu-hal to add a non-modifier path to `texture_from_dmabuf_fd` (a fourth patched crate in every app). |
| `flume` (existing) | `player-gpui`, now also Linux | The same frame-signal channel as the other platforms. |
| `gpui-pre-wgpu` `=0.3.10` (patched), `test-support` | `player-gpui`, Linux **dev**-dependency | `WgpuHeadlessRenderer` for the pixel tests, as Windows takes `gpui-pre-windows`. It is already in the graph. |

`rust-sak`: none of its modules (`crypto`, `fetch`, `fs`, `github`, `image`, `memo`, `o11y`, `sysinfo`, `tui`) covers GPUI, Vulkan, EGL or dma-bufs. `fs` stays a dev-dependency of `player-gpui`. Nothing is added to `rust-sak`.

### 8. Testing

- **Spike (task 1.1)**, on WSL2 with `vgem`, in a scratch crate, settled (outcome below):
  - which adapter GPUI picks on WSL2 and on CI
  - how to steer it to lavapipe (`VK_DRIVER_FILES=…/lvp_icd.json`) or to GL llvmpipe (Vulkan ICDs hidden, `LIBGL_ALWAYS_SOFTWARE=1`)
  - that wgpu-hal enables lavapipe's two dma-buf extensions without the modifier feature
  - that a LINEAR import of a core frame through `ash` and `texture_from_raw` samples correctly
  - that llvmpipe's EGL on GPUI's X11 or Wayland display (and on the headless renderer's display) imports the dma-buf
  - which WGSL file the GL backend compiles
  - whether lavapipe's linear row pitch matches GBM's stride for the test sizes
  - **Gate:** if neither backend can import on WSL2, stop and ask the user.
- **`player-gpui` pixel tests** (`video/linux.rs`, `cfg(test)`), on `WgpuHeadlessRenderer`. Each one returns early with a message when the core renderer cannot be built ("no usable DRM render node") or GPUI reports no import (`dma_buf_import()` is `None`):
  - **Draw path.** A core renderer plays a static four-quadrant lavfi picture. Its frame is painted through `paint_surface` with `fit`, then checked for quadrant colours, placement, black bars, and repeated paints of the same buffer (the cache). This replaces Windows' test-made shared texture, because no test-only dma-buf allocator exists without GBM FFI, and the core already is one.
  - **Real video.** The solid-colour-fields clip through `VideoPlayer` with controls off, checked within 3%, upright and letterboxed in a 16:9 element, with `scale=bilinear` (slice 8, 13a).
  - **Warn once.** A buffer GPUI cannot import is painted for several frames, and exactly one warning is logged. A test-only bad layout (wrong fourcc) makes the import fail on any driver.
- **The `VideoView` harness tests** now run on Linux. They return early on "no usable DRM render node", as the core tests do.
- **Both backends on CI and locally.** The Linux test step runs `player-gpui`'s tests twice:
  - once with `VK_DRIVER_FILES` set to lavapipe (Vulkan, LINEAR fallback)
  - once with no Vulkan ICD (`VK_DRIVER_FILES=/dev/null`: wgpu's GL on llvmpipe)

  `ZED_DEVICE_ID` cannot steer between them: every software adapter reports device 0.

  The modifier path (`texture_from_dmabuf_fd`) has no driver here and stays unverified (10+).
- **The fork's own tests** are integration tests (`gpui-pre-wgpu/tests/`), because the crate's unit-test target does not build outside Zed's repository: upstream test modules `include_bytes!` Zed's font assets from `../../../assets`. They run locally in the fork repository with `cargo test -p gpui-pre-wgpu --test '*'`:
  - the cache (`surface_cache.rs`, compiled into the test with `#[path]`): hits, eviction after 8 unused frames, a failed make not kept, a new cache empty (the cache lives in the device's resources, so a device loss drops it)
  - the RGBA shader with a wgpu-made texture wider than the drawn width: colours, the padding never sampled, clipping to the content mask
  - `DmaBufTexture::duplicate` giving an fd that outlives the original (`gpui-pre`'s unit-test target has the same asset problem)

  The dma-buf paths are tested from `player-gpui`, which has the producer.
- **Manual checks (task 5.1)**: `examples/play.rs` in WSLg on WSL2 with `vgem`, on lavapipe and on GL llvmpipe (and on GPUI's `d3d12` GL adapter, expecting black and one warning):
  - upright, colours, letterboxing
  - controls over the video, fullscreen, resize
  - no repaint when idle
  - two players
  - closing while playing
  - the fd count staying flat over a minute (the cache)

#### Spike outcome (task 1.1)

WSL2: kernel 6.18.33.2-microsoft-standard-WSL2, Mesa 26.2.5 (kisak PPA), `vgem` loaded, `LIBGL_ALWAYS_SOFTWARE=1`. The core rendered a four-quadrant picture on `card0` (226:0), LINEAR, at 160x120, 161x121, 333x187, 1366x768 and 1920x1080 (strides 768, 768, 1536, 5632 and 7680). Each frame was imported through wgpu 30.0.1's `as_hal` exactly as decision 4 describes, sampled by a shader into an `Rgba8Unorm` target and compared with the frame read through `mmap`. CI: throwaway branch `spike/add-player-9-gpui-dmabuf` (runs 41018705157, 41018986383 and 41019237917; deleted afterwards), `ubuntu-latest` x64 and `ubuntu-24.04-arm`, `vgem`, with Ubuntu's Mesa 25.2.8 and with the kisak PPA's Mesa 26.2.6.

| Question | Outcome |
|---|---|
| GPUI's adapter by default | wgpu hides `dzn` (non-conformant). With `LIBGL_ALWAYS_SOFTWARE=1`, which the `vgem` setup needs anyway, the adapters are lavapipe (Vulkan, `Cpu`) and llvmpipe (GL, `Cpu`), and GPUI's order picks lavapipe, windowed and headless. Without it, GL is Mesa's `d3d12` (`Other`), which ranks above lavapipe and cannot import dma-bufs (`eglCreateImage` fails, though the extension is advertised), so GPUI picks it and video is black with one warning. |
| Steering | lavapipe: the default above, or `VK_DRIVER_FILES=/usr/share/vulkan/icd.d/lvp_icd.json`. GL llvmpipe: `VK_DRIVER_FILES=/dev/null` hides every Vulkan driver, with `LIBGL_ALWAYS_SOFTWARE=1` (without it, GL is `d3d12`). `ZED_DEVICE_ID` cannot tell them apart (device 0). |
| lavapipe's dma-buf extensions without the modifier feature | Enabled by wgpu-hal: `VK_KHR_external_memory_fd` and `VK_EXT_external_memory_dma_buf` on, `VK_EXT_image_drm_format_modifier` and `VK_EXT_physical_device_drm` absent, `VULKAN_EXTERNAL_MEMORY_DMA_BUF` off. Same on CI. |
| LINEAR import through `ash` and `texture_from_raw` | Samples exactly (max difference 0) at every size **at the padded width** (`stride / 4`). At the layout's own width, lavapipe's row pitch (`width * 4` rounded up to 64 bytes: 640, 704, 1344, 5504) differs from GBM's stride (rounded up to 256 bytes) unless the width is a multiple of 64, so decision 4 now pads. CI with Mesa 25.2.8: `vkAllocateMemory` fails with `ERROR_INVALID_EXTERNAL_HANDLE` for every buffer, on both legs. CI with Mesa 26.2.6: as on WSL2. |
| EGLImage import on wgpu's GL display | Exact at every size, headless (surfaceless platform), windowed on Wayland (WSLg) and on X11 (WSLg, and Xvfb on CI). `EGL_EXT_image_dma_buf_import_modifiers` present, ABGR8888 modifiers `[LINEAR]`, render node `renderD128` on WSL2. CI with Mesa 25.2.8: EGL is on Mesa's software device (no render node), and the pitch is ignored, so only buffers whose stride is `width * 4` came out right; at the padded width every size is exact. CI with Mesa 26.2.6: as on WSL2. |
| WGSL file on the GL backend | `shaders.wgsl` with `shaders_storage.wgsl`, as on Vulkan (`create_pipelines` uses `WEBGL_SHADERS` only for wasm's WebGL). |

Changes made from it: LINEAR imports at the padded width on every path (decision 4); the CI Linux legs install Mesa 26 from the kisak PPA, the user's choice, so lavapipe can import there (decision 9); `d3d12` GL, not `dzn`, is WSL2's no-import case (proposal and spec unchanged: both are Mesa's Direct3D 12 drivers); the fork's tests are integration tests (above). The gate passed: both backends import on WSL2.

### 9. CI

- **Lint.** Linux x64 clippy now covers `player-gpui`'s Linux video code (`--all-targets`, with the git `[patch]`).
- **Linux legs:**
  - load `vgem`, as slice 8 does
  - install Mesa 26 from the kisak PPA (`ppa:kisak/kisak-mesa`: `libegl-mesa0`, `libgl1-mesa-dri`, `libgbm1`, `mesa-vulkan-drivers` with lavapipe), the user's choice after the spike: Ubuntu's Mesa 25.2.8 lavapipe cannot import dma-bufs. The core's tests then run on Mesa 26 too, as they do locally.
  - run `player-gpui`'s tests on both backends (decision 8)
- **Other legs.** Windows and macOS are unchanged, apart from using the new tag.

### Backend / UI separation, threading, lifetimes, zero-copy

- **Separation.**
  - `crates/player` is not touched, and still has no GUI types.
  - `crates/gpui` gains no EGL, GBM, Vulkan or `unsafe` code, only the cfg'd build and paint. All import code in the UI path is in GPUI's renderer (the fork).
  - A future egui-wgpu crate would call the same wgpu-hal import, or copy the fork's fallback, on `frame.fd()`, and reuse the core unchanged.
- **GPUI-Kit vs raw GPUI.** GPUI-Kit has no video or surface component, so `paint_surface` and `canvas` are raw GPUI, as on macOS and Windows. The controls stay on GPUI-Kit (slice 5).
- **Threads.**
  - `on_frame` runs on the core's render thread and only `try_send`s, and the view notifies on GPUI's foreground executor.
  - `paint_surface`, `dup`, the import and the draw run on GPUI's main thread during its frame.
  - `set_dma_buf_import` is called on the thread that creates the context, and `dma_buf_import()` reads it from the main thread, behind a `Mutex`.
- **Lifetimes.**
  - `player::linux::Frame` (an `Arc` of the slot's fd) lives only through paint.
  - The `DmaBufTexture` duplicate lives in the scene until the next frame.
  - A cache entry's Vulkan memory or GL texture holds its own reference to the buffer until eviction.
  - Each of these keeps the memory alive on its own, so dropping the renderer, a ring rebuild or the element's removal never frees a buffer GPUI is about to sample.
- **Unsafe and FFI.** The new `unsafe` code is all in the fork's `gpui-pre-wgpu`: `as_hal`, `texture_from_dmabuf_fd`, `texture_from_raw`, `ash` calls, and EGL and GL calls. Each block has a `// SAFETY:` note.
- **Zero-copy.** mpv draws into the dma-buf (slice 8), and GPUI's shader samples the same memory into the swap chain. On the software test drivers this memory is system RAM, which is a property of llvmpipe and lavapipe, not a copy. CPU readback happens only in tests.

## Risks / Trade-offs

- [No available driver runs `texture_from_dmabuf_fd`, so the modifier path ships untested] → It is wgpu-hal's own, documented import, used exactly as specified. The fallback and GL paths share everything after the import. The README lists it as unverified, and 10+ tests it on hardware.
- [The LINEAR fallback relies on the driver's linear row pitch matching GBM's stride] → Importing at the padded width makes them match on lavapipe for every size tested (spike). It is still checked per import and rejected with a warning when they differ. Drivers with the modifier extension never use the fallback.
- [The padded width depends on `stride` being a multiple of 4 and the buffer holding `stride * height` bytes] → Both hold for any RGBA buffer by definition of its stride. Otherwise the layout's own width is used.
- [Mesa 25's software EGL ignores the pitch, and its lavapipe cannot import dma-bufs] → The padded width covers the first. The second leaves lavapipe black on Mesa 25 (one warning); CI uses Mesa 26. Apps on real GPUs use hardware drivers, not these.
- [GPUI picks `d3d12` GL on WSL2 without `LIBGL_ALWAYS_SOFTWARE`, so such a run is black] → Expected, and covered by the spec's warn-once scenario. GPUI does report an import description there (decision 3), so the Linux tests in both repositories fail on `d3d12` GL rather than skip; they need the `vgem` setup's `LIBGL_ALWAYS_SOFTWARE=1`, with which GPUI picks lavapipe. The README documents steering with `VK_DRIVER_FILES`.
- [A third-party PPA in CI] → It only replaces Mesa on the test runners, matches the local setup, and is not needed by apps.
- [Image layout and queue-family ownership of imported memory (`UNDEFINED`, external queue) are not transferred explicitly] → For LINEAR memory written before import, contents are preserved in practice on Mesa, and wgpu-hal's own import does the same. 10+ revisits it on hardware.
- [The cache keeps evicted buffers' memory for up to 8 frames after a resize] → At most one ring's worth per renderer, briefly. The manual check watches the fd count.
- [Three `[patch]` lines, and a new tag per GPUI version] → The same friction as Windows, made louder by the marker. Commits 2 and 4 replay onto a new snapshot.
- [Linux apps that used `player-gpui` without a patch now fail to build] → This is the intended, documented break (the user's choice). The error names the marker, and the README gives the lines.

## Migration Plan

Linux apps using `player-gpui` must add the `[patch.crates-io]` lines for `gpui-pre` and `gpui-pre-wgpu` at tag `0.3.10-surfaces.2`, and stay on GPUI-Kit's default `gpui-pre` backend. Windows apps move their two lines to the new tag (the old tag keeps working for Windows). macOS apps need nothing. Rollback is pinning the previous `player-gpui` release, on which Linux shows black.
