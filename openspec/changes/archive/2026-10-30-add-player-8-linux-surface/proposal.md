## Why

Video reaches the screen on macOS (slice 4) and on Windows (slices 6 and 7). On Linux, `player_gpui::VideoPlayer` still paints black while audio plays, and the core has no surface renderer at all. This is slice 8 of the `add-player` series, the first of two slices that bring video to Linux. Like slice 6 for Windows, it does the core half: the player renders each frame on the GPU into a Linux dma-buf that another graphics API (Vulkan or EGL/GL) on the same machine can import, without the frame passing through the CPU. Slice 9 then shows those buffers in GPUI.

Two findings shaped the plan:
- **GPUI's Linux renderer is wgpu 30** (`gpui-pre-wgpu` 0.3.10, Vulkan with a GL fallback), not Blade. wgpu-hal 30 already imports a single-plane dma-buf (`vulkan::Device::texture_from_dmabuf_fd`), so the consumer side in slice 9 is small. That import takes one memory plane with an explicit DRM format modifier, which constrains this slice's buffers.
- **The only local test machine is WSL2**, which reaches the GPU through `/dev/dxg` (D3D12), not DRM. It has no `/dev/dri`, so the dma-buf path cannot run on its GPU. Its kernel has the `vgem` module, though, which may give a software path end to end (Mesa's GBM on llvmpipe). The user chose to verify on that software path, and to document hardware behaviour as unverified.

The libmpv build for Linux renders through OpenGL and links EGL, GBM and libdrm dynamically (they are in the closure of `mpv.pc`), so the producer can be built from them with no new native library.

## What Changes

- **`crates/player`, new Linux-only module `player::linux`** (toolkit-agnostic, no GUI types), mirroring `player::windows`:
  - `SurfaceRenderer`, a safe type with its own render thread, which has:
    - an EGL display on the GBM platform for a DRM render node, and a surfaceless GL context on it
    - slice 2's `RenderContext` on that context
    - a ring of 3 GBM buffers, each a single-plane RGBA8 dma-buf, imported into GL as an EGLImage and drawn into by mpv through an FBO
  - It renders each frame mpv announces at the size the host sets, waits for the GPU to finish, publishes the newest frame (readable from any thread) and calls a host callback, as on macOS and Windows.
  - `Frame` gives the dma-buf's file descriptor (a std `BorrowedFd`), its DRM fourcc, modifier, stride and offset, its size, sequence number and the render node's device number. No FFI type is in the public API.
  - The render node is chosen by device number, defaulting to the first render node on which the renderer can be built. The modifier defaults to LINEAR, and the host can offer a list of modifiers it can import. Only single-plane layouts are used.
  - Creation fails with `Error::Render` when there is no usable render node, when GBM or EGL lacks dma-buf import on it, when no render node has the requested device number, or for the existing reasons.
  - The "video when created after loading" code shared by macOS and Windows is also compiled on Linux.
  - All `unsafe` stays inside the module.
- **`crates/player`, render context (crate-internal)**: the context mpv renders through can be given the render node, as `MPV_RENDER_PARAM_DRM_DISPLAY_V2`, so mpv's VA-API decoder can hand frames to EGL without a copy. `RenderContext`'s public API is unchanged.
- **`crates/player`, playback-core**: on Linux, a player built with `VideoOutput::Render` gets VA-API or NVDEC hardware decoding (`hwdec=vaapi,nvdec`), mpv's two decoders that hand frames to EGL without a copy. A raw `mpv_option("hwdec", ...)` overrides it. (Planned as `auto-safe`; changed during implementation because `auto-safe` probes Vulkan decoding, which crashes on WSL2, and allows copy-back decoders.)
- **CI**: the Linux legs run the Linux surface tests on the software `vgem` path if the spike shows it works on GitHub's runners. Otherwise they run only the "no render node" and builder tests, and the gap is recorded.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `video-surface`: surface renderers become available on Linux.
  - Creation gains render node selection and the Linux failure cases.
  - Content and encoding gain the Linux RGBA dma-buf.
  - Hardware decoding gains the Linux default.
  - A new requirement covers sharing a frame through its dma-buf.
  - Platform coverage changes to macOS, Windows and Linux.
- `playback-core`: "Player creation with validated options" gains the Linux hardware-decoding default for the render output.

## Impact

- **Crates**: `crates/player` only.
  - Everything platform-specific lives in the core: EGL, GBM, the render node, the render thread and the hwdec default. A GUI crate only imports the dma-buf on its own device.
  - `crates/gpui` is not touched. `VideoPlayer` stays black on Linux until slice 9.
- **libmpv APIs**:
  - Slice 2's render context is reused. Newly passed: `MPV_RENDER_PARAM_DRM_DISPLAY_V2` (render node fd).
  - New option default on Linux: `hwdec=vaapi,nvdec`.
  - Properties read in tests and docs: `hwdec-current`, `vid`.
- **Linux APIs** (behind `cfg(target_os = "linux")`):
  - GBM: `gbm_create_device`, `gbm_bo_create_with_modifiers2` and `gbm_bo_create`, `gbm_bo_get_plane_count`, `gbm_bo_get_fd_for_plane`, `gbm_bo_get_stride_for_plane`, `gbm_bo_get_offset`, `gbm_bo_get_modifier`, and the matching destroy calls.
  - EGL: `eglGetPlatformDisplay` (`EGL_PLATFORM_GBM_KHR`), `eglInitialize`, `eglBindAPI`, `eglChooseConfig`, `eglCreateContext`, `eglMakeCurrent` (surfaceless), `eglGetProcAddress`, `eglCreateImage` (`EGL_LINUX_DMA_BUF_EXT`, `EGL_EXT_image_dma_buf_import_modifiers`), `eglDestroyImage`, and teardown.
  - GL: `glEGLImageTargetTexture2DOES`, fence sync.
  - The kernel's DRM render nodes under `/dev/dri`.
- **Public API**:
  - New `player::linux` module: `SurfaceRenderer`, `SurfaceRendererBuilder`, `Frame`, `DrmDevice`.
  - Behaviour change: Linux Render players hardware-decode by default.
- **Dependencies**: no new crate. `glow` moves from dev to runtime on Linux. EGL and GBM are called through a small hand-written FFI against libraries libmpv already links (see design.md).
- **rust-sak**: nothing in it covers this. Its modules are `crypto`, `fetch`, `fs`, `github`, `image`, `memo`, `o11y`, `sysinfo` and `tui`. `sysinfo` probes GPUs but does not open DRM render nodes or allocate GBM buffers, and a GBM/EGL bridge for mpv is specific to this project. `fs` stays a dev-dependency for the test media's temporary directories. Nothing is added to `rust-sak`.
- **Platforms**:
  - Linux gets surfaces in the core, but no video in GPUI until slice 9.
  - Verification is software-only (WSL2 with `vgem`, and CI if the spike shows it works there). Real GPUs (Mesa on Intel or AMD, NVIDIA's proprietary driver), their modifiers and VA-API or NVDEC zero-copy decoding are documented as unverified.
  - WSL2's own GPU path (`/dev/dxg`) has no DRM render node, so on WSL2 without `vgem`, creation fails with the "no render node" error.
- **Size**: 14 tasks. This is slice 8 of the `add-player` series (see design.md). It depends on slices 2 and 6.
