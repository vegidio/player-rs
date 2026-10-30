## Why

On Linux, `player_gpui::VideoPlayer` shows black while audio plays and the controls work. Slice 8 gave the core a Linux frame stream (`player::linux::SurfaceRenderer`: single-plane RGBA dma-bufs, published GPU-complete), but stock GPUI cannot show it. In `gpui-pre` 0.3.10 and `gpui-pre-wgpu` 0.3.10, the versions GPUI-Kit 0.8.1 builds on by default:
- `PaintSurface` has no payload off Apple platforms (Windows has one only in our fork).
- `Window::paint_surface` and `surface()` are not available on Linux.
- `WgpuRenderer` draws `PrimitiveBatch::Surfaces(_) => {}`, and its `surface` pipeline samples NV12 planes that nothing feeds.
- The wgpu device is requested with only `DUAL_SOURCE_BLENDING`, so wgpu-hal's dma-buf import (`VULKAN_EXTERNAL_MEMORY_DMA_BUF`) is off.

This is slice 9 of the `add-player` series, the second of the two Linux slices. As slice 7 did for Windows, it extends the existing GPUI fork (`vegidio/gpui-pre`) with Linux surface support, and `player-gpui` uses it to show Linux video.

## What Changes

- **GPUI fork** (`vegidio/gpui-pre`, outside this workspace). `gpui-pre-wgpu` 0.3.10 is imported untouched in its own commit, then one Linux change follows:
  - **`gpui-pre`**:
    - `PaintSurface` gets a Linux payload: a dma-buf texture, an owned fd with its DRM fourcc, modifier, stride, offset and size.
    - `Window::paint_surface` and `surface()` exist on Linux. They take a `DmaBufTexture`, which is built safely from a borrowed fd by duplicating it, so the caller needs no `unsafe`.
    - A process-wide, read-only description of what GPUI's renderer can import: its DRM render node (when Vulkan reports it) and the single-plane modifiers it imports for RGBA.
    - A public marker item, `LINUX_SURFACES_REQUIRE_PATCHED_GPUI`, for a readable build error without the patch.
  - **`gpui-pre-wgpu`**:
    - The device also requests `VULKAN_EXTERNAL_MEMORY_DMA_BUF` when the adapter supports it.
    - `draw_surfaces` imports each dma-buf on GPUI's own device and draws it with a new RGBA surface shader, clipped to the content mask. Imports are cached per buffer and dropped on device loss.
    - **On Vulkan**, there are two import paths. With `VK_EXT_image_drm_format_modifier`, it uses wgpu-hal's `texture_from_dmabuf_fd`. Without it, the fork imports the memory itself into a `VK_IMAGE_TILING_LINEAR` image, for LINEAR buffers only. The user chose this fallback so that the Vulkan path can be tested on lavapipe.
    - **On wgpu's GL backend**, the fork imports the dma-buf as an EGLImage on wgpu's own EGL display and context, and wraps it with `texture_from_raw`.
    - A buffer that cannot be imported is skipped, and a warning is logged once.
  - No CPU copy is made anywhere.
- **`crates/gpui`** (`player-gpui`):
  - On Linux, `VideoPlayer` gets the same video view as macOS and Windows, built on `player::linux::SurfaceRenderer`:
    - the renderer is built on GPUI's render node with GPUI's importable modifiers when GPUI reports them, and otherwise falls back to the core's defaults
    - frames are requested at the element's bounds in device pixels
    - one repaint per published frame, and none while idle
    - letterboxing with `ObjectFit::Contain`
    - keyed state
  - When the renderer cannot be built, the element stays black and logs one warning, as on the other platforms. Examples: no DRM render node (WSL2 without `vgem`), or GPUI on a driver that cannot import (`dzn` on WSL2).
  - **Linux video is always on, as on Windows** (the user's choice, replacing slice 8's planned opt-in `linux-video` feature). Linux builds of `player-gpui` need the patched GPUI. Without the patch, the build fails with an error that names the marker.
- **This workspace**:
  - `[patch.crates-io]` gains `gpui-pre-wgpu`.
  - All three entries move to a new fork tag, `0.3.10-surfaces.2`, which also carries the unchanged Windows commit.
- **Docs**: the README and the crate docs gain Linux setup (three `[patch]` lines, the `gpui-pre` backend only), the Linux behaviour and gaps, and an updated platform table.
- **CI**: Linux legs build and test `player-gpui` with the patch, run the pixel tests on GPUI's headless renderer over a dma-buf, and add a Linux clippy step for the new code.

- **`crates/player`** (added during implementation, the user's choice): mpv's default rendering draws YUV video black, or in wrong colours, on Mesa's llvmpipe in most runs, and the tests' `scale=bilinear` workaround sometimes draws it black or crossed by black lines. Slice 8's spike found the first, but apps had no workaround at all. `RenderContext` creation, which the Linux surface renderer also goes through, reads the GL context's renderer. On llvmpipe or softpipe it turns on mpv's `gpu-dumb-mode` unless the app set it or a scaler. Slice 8's `player::linux` API is otherwise used as it is.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `gpui-player`:
  - "Video element" and "GPUI platform coverage" extend video to Linux.
  - "Windows video setup" becomes "Patched GPUI setup", which covers Windows and Linux.
  - A new requirement, "Linux video unavailable at run time", covers black with one warning when the dma-buf cannot be made or imported.
  - A new requirement, "Render device and layout from GPUI", covers the core renderer following GPUI's device and modifiers.
- `video-render`: a new requirement, "Correct picture on software OpenGL", covers mpv's dumb mode on Mesa's software renderers.

## Impact

- **Crates**:
  - `crates/gpui` gets the Linux video view. It contains only GPUI glue: sizing, frame subscription, the renderer's device and modifiers taken from GPUI, and the `paint_surface` call. There is no EGL, GBM, Vulkan or `unsafe` code in it.
  - `crates/player` gains only the software-renderer scaler default, in `RenderContext` creation. Everything platform-specific stays in the core (slice 8) or in GPUI's renderer (the fork), so a future egui-wgpu crate would reuse the core unchanged and call the same wgpu-hal import.
- **Video.js reference**: the `<video>` area of a Video.js player, letterboxed on black under the control bar, now on Linux as on macOS and Windows.
- **libmpv APIs**: none new. Slice 8's renderer and its `hwdec=vaapi,nvdec` default are used unchanged.
- **Linux APIs, all in the fork**:
  - Vulkan through wgpu-hal: `texture_from_dmabuf_fd`, `texture_from_raw`, `VK_EXT_external_memory_dma_buf`, `VK_KHR_external_memory_fd`, `VK_EXT_image_drm_format_modifier` (modifier list), and `VK_EXT_physical_device_drm` (render node).
  - EGL and GL through wgpu-hal's GLES backend: `eglCreateImage(EGL_LINUX_DMA_BUF_EXT)`, `eglQueryDmaBufModifiersEXT`, `glEGLImageTargetTexture2DOES`.
- **Public API**:
  - `player-gpui`: no Rust API change.
  - Behaviour changes on Linux: video is shown, and the build needs the patch (**BREAKING** for Linux apps that use `player-gpui` without the patch: they must add the `[patch]` lines).
  - The fork additions are additive: `Window::paint_surface`, `surface()` and `DmaBufTexture` on Linux, a `PaintSurface` field, the import description and the marker.
- **Dependencies**:
  - `[patch.crates-io]` gains `gpui-pre-wgpu`, and the tag moves to `0.3.10-surfaces.2`.
  - `flume` becomes a Linux dependency of `player-gpui`.
  - The fork's `gpui-pre-wgpu` uses `ash`, `khronos-egl` and `glow` at the versions wgpu-hal 30 already brings in, so nothing new is built.
- **rust-sak**: nothing in it covers this. Its modules are `crypto`, `fetch`, `fs`, `github`, `image`, `memo`, `o11y`, `sysinfo` and `tui`, and none touches GPUI, Vulkan, EGL or dma-bufs. `fs` stays a dev-dependency of `player-gpui` for test media. Nothing is added to `rust-sak`.
- **Platforms**:
  - Linux x64 and arm64 get video in GPUI.
  - Verification is software-only: WSL2 with `vgem` (llvmpipe and lavapipe), and CI if the spike shows it works there.
  - Real GPUs are unverified: Mesa on Intel or AMD, NVIDIA's proprietary driver, tiled modifiers, and the `texture_from_dmabuf_fd` path itself, which no available driver can run. They stay listed as such, for slice 10+.
  - Only the `gpui-pre` backend is covered. Apps on GPUI-Kit's `gpui-fast` backend fail the build on the marker, as on Windows.
  - macOS still needs no patch.
- **Outward-facing steps** (each needs the user's go-ahead): pushing the fork's new commits and the `0.3.10-surfaces.2` tag.
- **Size**: 17 tasks, two over the project's limit of 15. The two core tasks (5.1 and 5.2, decision 10) were added during implementation, after the manual check found the core's own frames black on llvmpipe. The user chose to keep them in this change rather than split them out: slice 9's Linux pixel tests and manual checks only pass with them, so neither part would leave the project working on its own. This is slice 9 of the `add-player` series (see design.md). It depends on slices 5, 7 and 8.
