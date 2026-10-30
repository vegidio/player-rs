## Context

What already exists:
- **`RenderContext` (slice 2)** draws a player's video into any FBO of a host GL context. Its one `unsafe` constructor requires that "this GL context is current on every call and on drop". It passes mpv only `API_TYPE` and `OPENGL_INIT_PARAMS` (`mpv/render.rs`), with no display parameters.
- **`player::macos` (slice 4) and `player::windows` (slice 6)** have the same shape: builder, `set_size`, `latest`, `on_frame`, a ring of 3 slots, and publication once the GPU has finished.
  - The shared crate-private `surface` module holds `Shared<F>` (signals, `Condvar`, latest frame) and the "created after load" handling (`reselect_video`, reload signal). It is compiled on `cfg(any(target_os = "macos", windows))`.
  - Windows render threads are pooled, because NVIDIA's WGL driver deadlocks when a thread that used it exits (slice 6, decision 2).
- **The tests' GL helper** (`tests/common/gl.rs`) makes Linux contexts through `glutin` on an EGL *device* (no window, no surface). On CI that is Mesa's llvmpipe (`LIBGL_ALWAYS_SOFTWARE=1`).
- **The Linux libmpv** (binaries-mpv 26.10.1) leaves the display stack dynamic. CI installs `libegl-dev`, `libgbm-dev`, `libdrm-dev`, `libvulkan-dev`, the X11/Wayland libraries and `libvdpau-dev` for the closure of `mpv.pc`. So libEGL and libgbm are already linked into every Linux binary that uses `player-rs`.

What was verified for this design:
- **GPUI on Linux** (`gpui-pre-wgpu` 0.3.10, what GPUI-Kit 0.8.1 resolves to here):
  - It renders with **wgpu 30**. The instance is created with `Backends::VULKAN | Backends::GL`.
  - Adapters are sorted by `ZED_DEVICE_ID`, then the GPU the compositor uses, then discrete before integrated before software, then Vulkan before GL. The first one that configures the window's surface wins.
  - The device is requested with only `DUAL_SOURCE_BLENDING`.
  - A `surfaces` pipeline and bind-group layout exist, but its shader samples NV12 (`t_y`, `t_cb_cr`), and `PrimitiveBatch::Surfaces(_) => {}` draws nothing. `PaintSurface` has no Linux payload.
  - GPUI has a headless renderer on Linux, so slice 9 can run pixel tests.
- **wgpu-hal 30** has `vulkan::Device::texture_from_dmabuf_fd(OwnedFd, desc, modifier, stride, offset)`. It handles **single-plane** dma-bufs only, and needs `Features::VULKAN_EXTERNAL_MEMORY_DMA_BUF` (`VK_KHR_external_memory_fd`, `VK_EXT_external_memory_dma_buf` and `VK_EXT_image_drm_format_modifier`). This is what slice 9 will call, and it is the consumer contract this slice's buffers must meet.
- **The local machine is WSL2** (kernel 6.18, NVIDIA GPU behind `/dev/dxg`):
  - There is no `/dev/dri`, and `CONFIG_UDMABUF` and `CONFIG_DMABUF_HEAPS` are off.
  - `CONFIG_DRM_VGEM=m` is set.
  - Mesa's `d3d12` (GL) and `dzn` (Vulkan) reach the GPU, but neither has dma-buf import or export. lavapipe's ICD is installed.

Required behaviour is in `specs/video-surface` and the `playback-core` delta. This document covers how.

## Slices

The `add-player` series. **This change is slice 8.**

| # | Change | Delivers | Depends on | Status |
|---|---|---|---|---|
| 1 | `add-player-1-core` | `player::Player` handle: builder, sources, load, playback control, property/command access, state snapshot, event subscription; headless CI tests | `add-workspace-scaffold` | done |
| 2 | `add-player-2-render` | mpv OpenGL render API in core over a host-provided GL context: `VideoOutput::Render`, `RenderContext`, `State.video_size`; software-GL render tests on all six CI legs | 1 | done |
| 3 | `add-player-3-ui-core` | `player::ui`, the toolkit-agnostic UI layer (`ui::Options`, `ui::Controls`, `View`, actions, Video.js hotkeys, activity, bar maths); `State.buffered`, `Player::scrub` | 1 | done |
| 4 | `add-player-4-gpui-macos` | macOS video on screen: `player::macos::SurfaceRenderer` (CGL, IOSurface NV12 ring, fenced publication), `hwdec=videotoolbox` default, `Player::ptr_eq`; `player_gpui::VideoPlayer` (black on Windows/Linux) and the `play` example | 2 | done |
| 5 | `add-player-5-gpui-controls` | Video.js controls in GPUI over `VideoPlayer`: control bar, big play button, spinner, error display, fading, pointer/keyboard/focus, actions, window fullscreen, fluid aspect ratio, `player_gpui::Assets`/`Icons` | 3, 4 | done |
| 6 | `add-player-6-windows-surface` | Windows surfaces in core: `player::windows::SurfaceRenderer` (pooled render thread, hidden-window WGL context, D3D11 device on a chosen adapter, BGRA ring of NT-handle textures fed through `WGL_NV_DX_interop2` and a GPU copy, GPU-complete publication), `Frame` with shared handle and adapter LUID, `hwdec=auto-safe` default on Windows, "created after load" code shared with macOS; tests and CI | 2, 4 | done |
| 7 | `add-player-7-gpui-windows` | Windows video in GPUI: a fork of `gpui-pre` + `gpui-pre-windows` 0.3.10 (Windows `PaintSurface` payload, `Window::paint_surface`/`surface()` on Windows, `draw_surfaces` opening the shared handle with a `surface` shader, a marker item); `[patch.crates-io]` in this workspace; opt-in `windows-video` feature in `player-gpui`; README setup; CI pixel test of the fork's draw path | 5, 6 | done |
| 8 | `add-player-8-linux-surface` | Linux surfaces in core: `player::linux::SurfaceRenderer` (render thread, EGL on the GBM platform of a chosen DRM render node, surfaceless GL context, ring of 3 single-plane RGBA GBM buffers imported as EGLImages, GPU-complete publication), `Frame` with dma-buf fd, fourcc, modifier, stride, offset and render-node device number, modifier list with LINEAR default, `hwdec=vaapi,nvdec` with the render node passed to mpv, "created after load" code shared on Linux; software verification through `vgem`; tests and CI | 2, 6 | **current** |
| 9 | `add-player-9-gpui-linux` | Linux video in GPUI: extend the fork with `gpui-pre-wgpu` (and `gpui-pre`'s Linux `PaintSurface` payload and `Window::paint_surface`/`surface()` on Linux); request `VULKAN_EXTERNAL_MEMORY_DMA_BUF` when supported; `draw_surfaces` imports each dma-buf, cached per buffer, through `texture_from_dmabuf_fd` on Vulkan **and** through an EGLImage on wgpu's GL backend (the user's choice), drawn with an RGBA `fs_surface`; GPUI's render node and importable modifiers passed to the core; a new fork tag and `[patch]` entry; an opt-in `linux-video` feature in `player-gpui` (black and one warning when import is impossible, such as `dzn` on WSL2); README setup; headless pixel test (lavapipe) | 5, 7, 8 | pending |
| 10+ | (to be planned) | The `gpui-fast` fork for apps on that backend; menus (rate list, tracks), captions, poster drawing, skip buttons, touch behaviour; hiding the cursor when inactive (needs a GPUI API); HDR surfaces if GPUI gains them; precise GL/D3D adapter matching on hybrid laptops; Linux hardware verification (Mesa Intel/AMD, NVIDIA proprietary, VA-API/NVDEC zero-copy, non-linear modifiers) | 8, 9 | pending |

Changes from the plan in slice 7's table:
- **"Linux video surfaces" is split** into slice 8 (core) and slice 9 (GPUI), following the Windows pattern. The user chose this.
- **The import target is wgpu, not Blade.** GPUI's Linux renderer in this version is wgpu 30. Slice 9 extends the existing fork with `gpui-pre-wgpu`.
- **Slice 9 also imports on wgpu's GL backend.** The user chose this over leaving the GL fallback black.
- **Linux hardware verification moves to 10+.** The only local machine is WSL2, which has no DRM render node for its GPU. The user chose software verification through `vgem` and a documented hardware gap.

## Goals / Non-Goals

**Goals:**
- A safe, toolkit-agnostic Linux API that turns a `Player` into a stream of dma-bufs that Vulkan (wgpu-hal's import) or EGL can import on the same machine, with no GL and no `unsafe` for the consumer.
- No CPU copy of frames in the shipping path, including hardware decoding wherever mpv has a zero-copy EGL interop (VA-API through dma-buf, NVDEC through CUDA-GL).
- The same observable behaviour as `player::macos` and `player::windows`, so slice 9's GPUI code mirrors slice 7's.
- A test path that runs on the user's WSL2 machine, and on CI if possible.

**Non-Goals:**
- Anything in GPUI or `crates/gpui` (slice 9).
- Multi-plane layouts (NV12, or modifiers with auxiliary planes such as AMD DCC or Intel CCS). wgpu-hal imports single-plane dma-bufs only.
- Explicit synchronisation (`sync_file` fences shared with the consumer). The producer publishes only GPU-complete frames, as on the other platforms.
- A CPU-readback fallback when no render node exists. Creation fails instead, and a GUI shows black (slice 9).
- Verification on real Linux GPUs (10+).
- HDR or 10-bit buffers, `MPV_RENDER_PARAM_ADVANCED_CONTROL`, and presentation-timed `report_swap`, as on macOS and Windows.

## Decisions

### 1. Public API (`player::linux`, `cfg(target_os = "linux")`)

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct DrmDevice { pub major: u32, pub minor: u32 }   // a render node's st_rdev, no libc type

pub struct SurfaceRenderer { /* Send + Sync */ }
impl SurfaceRenderer {
    pub fn builder(player: &Player) -> SurfaceRendererBuilder;
    pub fn set_size(&self, width: u32, height: u32);       // device pixels
    pub fn latest(&self) -> Option<Frame>;                 // never waits on mpv or GL
    pub fn device(&self) -> DrmDevice;
    pub fn modifier(&self) -> u64;                         // the layout in use
}
impl SurfaceRendererBuilder {
    pub fn device(self, DrmDevice) -> Self;                // default: first render node that builds
    pub fn modifiers(self, impl IntoIterator<Item = u64>) -> Self;   // default: [DRM_FORMAT_MOD_LINEAR]
    pub fn on_frame(self, impl Fn() + Send + Sync + 'static) -> Self;
    pub fn build(self) -> Result<SurfaceRenderer>;         // Error::Render on failure
}

#[derive(Clone)] pub struct Frame { .. }                   // Arc around the slot's fd
impl Frame {
    pub fn fd(&self) -> BorrowedFd<'_>;                    // std::os::fd; the dma-buf
    pub fn fourcc(&self) -> u32;                           // DRM_FORMAT_ABGR8888 (R,G,B,A bytes)
    pub fn modifier(&self) -> u64;
    pub fn stride(&self) -> u32; pub fn offset(&self) -> u32;
    pub fn device(&self) -> DrmDevice;
    pub fn width(&self) -> u32; pub fn height(&self) -> u32; pub fn sequence(&self) -> u64;
}
```

- **The same shape as `player::windows`.** Only the selection (`device` instead of `adapter`), the layout list and the frame's description differ. There is no `format` option: RGBA is the only format.
- **Exactly what `texture_from_dmabuf_fd` takes.** The consumer duplicates `fd()` (`BorrowedFd::try_clone_to_owned`) and passes it with `modifier`, `stride` and `offset`. The fourcc maps to `Rgba8Unorm`.
- **A std `BorrowedFd`, not a GBM or EGL handle.** It works for Vulkan, EGL, and any other consumer, and keeps every FFI type out of the public API, as slice 6 did with `BorrowedHandle`.
- **`DrmDevice`** is what a consumer's Vulkan device reports through `VK_EXT_physical_device_drm` (`renderMajor`/`renderMinor`), so the consumer can compare it with its own device, or pass its own device to `device()`.

### 2. Render thread

```
spawn: open /dev/dri/renderDn (O_RDWR | O_CLOEXEC) -> gbm_create_device
       eglGetPlatformDisplay(EGL_PLATFORM_GBM_KHR, gbm) -> eglInitialize
       check EGL_EXT_image_dma_buf_import(_modifiers), EGL_KHR_surfaceless_context
       eglBindAPI(OPENGL) -> context 3.2 core (fallback: legacy; then GLES 3) -> make current, no surface
       check GL_OES_EGL_image -> glow + RenderContext::new(loader, drm_display, on_mpv_frame) -> init result -> loop
loop:  wait on Condvar<Signals { mpv_frame, size, shutdown }>
       shutdown -> break
       render.update()
       no size or zero size -> continue
       size changed -> rebuild the ring for the new size
       slot = ring[next]                                   // 3 slots, round-robin
       render.render({fbo: slot.fbo, w, h}, flip_y = false)
       fence = glFenceSync; glFlush; glClientWaitSync(fence, 1 s bound)
       latest.lock() = Some(Frame { slot.fd.clone(), .., seq += 1 })
       render.report_swap(); on_frame()
exit:  drop RenderContext (GL current) -> delete FBOs and textures -> eglDestroyImage
       -> gbm_bo_destroy (fds stay open in Frames) -> make no context current -> eglDestroyContext
       -> eglTerminate -> gbm_device_destroy -> close the node fd
```

- **This is the macOS loop.** Like macOS, and unlike Windows, there is no extra target or GPU copy, because mpv draws straight into the shared buffer. The signals, the bounded join on drop and `report_swap` after publication carry over through the shared `surface` module.
- **Threads are not pooled.** The Windows pool works around a specific NVIDIA WGL deadlock. Nothing similar is known for EGL on Linux, and it cannot be tested locally. If it shows up on hardware later, the Windows pool can be reused.
- **Orientation.** In an EGLImage-backed texture, texel row 0 is the dma-buf's first row. GL writes image row 0 at the texture origin, so `flip_y = false` should give a buffer whose first row is the top, as with IOSurface and the D3D interop. Task 1.1 confirms it.

### 3. Allocation: GBM buffers imported into EGL (chosen by the user)

| Option | Verdict |
|---|---|
| **GBM allocates the buffer on the render node, EGL imports it (`EGL_LINUX_DMA_BUF_EXT`), GL draws into it** | **Chosen.** We choose the format, the modifier and the plane count, so the buffer always fits wgpu-hal's single-plane import. The render node that GBM opens is also the device's identity (`DrmDevice`). This is how Wayland compositors and Mesa's own tests share buffers. |
| GL allocates a texture, `EGL_MESA_image_dma_buf_export` exports it | No control over the modifier or plane count, so the driver could pick a compressed multi-plane layout the consumer cannot import. NVIDIA's support for it is uncertain. |
| The consumer allocates in Vulkan, GL imports it (`GL_EXT_memory_object_fd`) | The consumer would own the ring, which inverts the producer-owned model of macOS and Windows. It also needs Vulkan code in the core and semaphores across the API. |

- **Buffers:** each slot is `gbm_bo_create_with_modifiers2(dev, w, h, GBM_FORMAT_ABGR8888, modifiers, n, GBM_BO_USE_RENDERING)`. When the list is just LINEAR and the driver lacks the modifiers entry point, it falls back to `gbm_bo_create(.., GBM_BO_USE_RENDERING | GBM_BO_USE_LINEAR)`. A buffer with `gbm_bo_get_plane_count != 1` is rejected.
- **Import into GL:** `eglCreateImage(EGL_LINUX_DMA_BUF_EXT)` with the fd, fourcc, plane-0 offset and pitch, and the modifier (`EGL_DMA_BUF_PLANE0_MODIFIER_LO/HI_EXT`). Then `glEGLImageTargetTexture2DOES` on a texture attached to the slot's FBO.
- **The fd** comes from `gbm_bo_get_fd_for_plane(bo, 0)`, is wrapped in an `OwnedFd` in an `Arc`, and is shared with every `Frame` of that slot. An open dma-buf fd keeps the buffer's memory alive after the bo, the GBM device and the EGL display are gone (spec "descriptor outlives renderer"). Task 2.6 tests it.

### 4. Format and layout (modifiers)

- **`ABGR8888`** stores R, G, B, A bytes in memory order. It is GL's natural `RGBA8`, Vulkan's `R8G8B8A8_UNORM`, wgpu's `Rgba8Unorm`, and the format most often importable with any modifier. mpv's RGBA output lands in it unswizzled. Task 1.1 confirms the channel order.
- **Layout negotiation is one-way.** The host passes the modifiers it can import (`modifiers(..)`). The renderer asks GBM to allocate with that list, in order of preference, and uses what GBM picked if that is single-plane. If none works, creation fails (spec). The default is `[LINEAR]` because:
  - every importer that supports dma-buf at all takes LINEAR
  - it imports across devices, which covers a GPUI adapter that differs from the render node (hybrid GPUs, decision 6)
  - its cost is a less efficient memory layout for scanout and sampling. That is acceptable for one video quad per frame, and slice 9 can pass GPUI's single-plane tiled modifiers to remove it.
- **The ring is rebuilt on a size change**, like macOS and Windows.

### 5. Synchronisation: GPU-complete publication

`glFenceSync`, `glFlush`, then `glClientWaitSync` on the render thread before publishing, as on macOS (with `glClientWaitSync`) and Windows (with an event query).
- The consumer needs no synchronisation. It imports and samples, matching the other platforms' contract and slice 9's simplest import.
- Implicit dma-buf fencing in the kernel would also order the GPU work on most drivers, but NVIDIA's driver did not always take part in implicit sync. The CPU wait makes the contract hold everywhere, and costs only the render thread.
- Rejected: exporting a `sync_file` (`EGL_ANDROID_native_fence_sync`) for the consumer to import as a Vulkan semaphore. It is cheaper on the CPU, but it needs a fence in the API and semaphore handling in GPUI, and it can come later without breaking the API.

The ring of 3, together with "publish only completed frames", gives the spec's "unchanged until two newer frames are published". As on the other platforms, a consumer must take a new frame from `latest()` on every paint.

### 6. Render node selection

- **Enumeration:** the `renderD*` entries of `/dev/dri`, sorted by minor number, each identified by `st_rdev` (std `MetadataExt::rdev()`, split into major and minor).
- **Default:** the first node on which the whole thread setup (GBM, EGL, context, one test allocation with the modifier list) succeeds. With `device(d)`: that node or `Error::Render` naming it. With no node, `Error::Render` "no DRM render node is available". That is the WSL2 case without `vgem`, and also CI without `vgem`.
- **GPUI's own rule cannot be reproduced in the core.** Unlike DXGI's "adapter 0", GPUI's choice depends on `ZED_DEVICE_ID`, the compositor's GPU and the device type. Slice 9 therefore passes GPUI's render node, read through `VK_EXT_physical_device_drm` in the fork, to `device()`. Until then, and whenever the two differ, the LINEAR default keeps cross-device import working on most drivers.

### 7. Hardware decoding default: `vaapi,nvdec`, plus the render node for VA-API

- **mpv's zero-copy interops for an EGL GL context on Linux** are:
  - `vaapi`, through `EGL_EXT_image_dma_buf_import` (Mesa on Intel and AMD)
  - `nvdec`, through CUDA-GL interop (NVIDIA)
  - `vdpau`, through `GL_NV_vdpau_interop`, which is obsolete and left out

  The default is the explicit list `hwdec=vaapi,nvdec`: mpv tries VA-API, then NVDEC, uses the first that initialises, and decodes in software otherwise.
  - **Changed during implementation (the user's choice).** The plan was `auto-safe`, as on Windows (slice 6, decision 6), on the assumption that it never picks a `-copy` variant. That assumption is wrong: on WSL2, `auto-safe` picked `nvdec-copy`. It also probes Vulkan decoding, which on WSL2 loads Mesa's `dzn` driver and crashes the process when that driver's thread exits (see 13a), also with a plain `RenderContext`. The explicit list keeps only the zero-copy interops, never probes Vulkan, and never copies frames back.
- **VA-API needs a display.** With the render API, mpv's VA-API interop finds its VA display through the host's native resources: `MPV_RENDER_PARAM_X11_DISPLAY`, `WL_DISPLAY` or `DRM_DISPLAY_V2`. A headless EGL context has no X11 or Wayland display, so the renderer passes `DRM_DISPLAY_V2 { fd: -1, render_fd: <node fd>, .. }` for its render node.
  - This needs a crate-internal extra parameter on `mpv::render::create`, used by `RenderContext::new`'s internal path. The public `RenderContext` API is unchanged.
  - Whether libmpv's VA-API is built in (no `libva-dev` is in CI's list, so it may be static or absent), and whether the parameter is enough, is checked statically by task 1.1. It is verified on hardware in 10+.
- As before, `mpv_option("hwdec", ..)` overrides the default, and `Null` keeps `no`. The builder test's expectation for Linux changes from `no` to `vaapi,nvdec`.

### 8. Shared code

The `surface` module's `cfg` becomes `any(target_os = "macos", windows, target_os = "linux")`. The Linux renderer uses `Shared<Frame>` and the "created after load" handling unchanged, so the Linux "created after load" and "audio-less reload" scenarios exercise the same code as the other platforms.

### 9. Dependencies

| Crate or library | Where | Why, and alternatives |
|---|---|---|
| `glow` 0.16 | `player`, Linux runtime (already macOS and Windows runtime, and dev) | Typed GL calls for FBOs, textures and fences, as on the other platforms. `glEGLImageTargetTexture2DOES` is loaded through `eglGetProcAddress` if `glow` does not expose it. |
| libEGL, libgbm | `player`, Linux, through a hand-written `extern "C"` block (about 20 functions), with EGL extensions loaded through `eglGetProcAddress` | Both are already linked through `mpv.pc`'s closure, so nothing new reaches the link line. Task 2.2 confirms it, and adds `#[link]` attributes if the static archive's closure does not name them. Rejected: `khronos-egl` (its `static`/`dynamic` features add a second linking policy to `build.rs`'s, for a handful of calls), Smithay's `gbm` crate (brings `drm` and `rustix` for 8 functions), and `glutin` at runtime (no GBM platform helpers or EGLImage import, the same reason slice 6 rejected it on Windows). |
| `libc` | not added | `open`, `st_rdev` and `dup` are covered by std (`File`, `MetadataExt::rdev`, `OwnedFd::try_clone`). The major/minor split is plain bit arithmetic on `dev_t`. |

`rust-sak`: no module is used at runtime. `fs` stays a dev-dependency for the test media's temporary directories. `sysinfo` probes GPUs but cannot open DRM render nodes, allocate GBM buffers or make EGL contexts, and that bridge is not general-purpose. Nothing is added to `rust-sak`.

### 10. Backend / UI separation

Everything EGL-, GBM-, DRM- and libmpv-specific is in `crates/player` behind `cfg(target_os = "linux")`, with no GUI types. A frontend only:
1. sets a size
2. waits for `on_frame`
3. imports `frame.fd()` with the frame's fourcc, modifier, stride and offset
4. draws it

- **GPUI (slice 9):** the fork's `draw_surfaces` imports it on Vulkan through `texture_from_dmabuf_fd`, or on GL through an EGLImage.
- **A future egui-wgpu crate:** it would call the same wgpu-hal import and reuse this core unchanged.

`crates/gpui` is not touched, so GPUI-Kit and raw GPUI play no part in this slice. Slice 9 needs raw GPUI (`paint_surface`), as on macOS and Windows.

### 11. FFI, threading and lifetimes

- **`unsafe` lives in `crates/player/src/linux/`.** It covers the GBM and EGL calls, the GL calls through `glow`, the EGLImage target call, and the one `RenderContext::new` call. Each block has a `// SAFETY:` note.
  - Every GBM, EGL and GL call happens on the render thread.
  - Duplicating and closing fds is thread-safe.
- **Threads** are as on macOS:
  - mpv's threads only set a flag and notify.
  - The render thread owns every GBM, EGL and GL object.
  - `set_size` and `latest` take a mutex held only for a field swap.
  - `on_frame` runs on the render thread, never on one of mpv's.
- **Lifetimes.**
  - `SurfaceRenderer` → the render thread → `RenderContext` → `Arc<Handle>`, so mpv outlives the renderer in either drop order (slice 2).
  - `Frame`s hold `Arc<OwnedFd>`, and the dma-buf's memory lives while any fd to it is open.
  - Teardown order (decision 2): `RenderContext` while GL is current, then the GL objects, then the EGLImages, then the bos, then the context and display, then the GBM device, then the node fd.
  - The node fd outlives mpv's use of it, because the `RenderContext` that holds the `DRM_DISPLAY_V2` parameter is dropped first.

### 12. Zero-copy

- **Hardware path:** VA-API surfaces are imported into EGL as dma-bufs by mpv itself, and NVDEC goes through CUDA-GL. mpv scales and converts on the GPU into the slot's FBO, whose texture is the dma-buf. The consumer samples the same memory. There is no copy at all, not even the GPU blit Windows needs.
- **Software decoding** uploads inside mpv, as noted in slices 2, 4 and 6. On the `vgem` test path everything is in system memory, because llvmpipe is a CPU renderer. That is a property of the test driver, not a copy this crate adds.
- **CPU readback** happens only in tests: a second EGL context imports the frame's dma-buf, which also exercises the consumer path, and `glReadPixels` reads it.

### 13. Testing and CI

- **The software path to verify (task 1.1):** `sudo modprobe vgem` creates a DRM device with no GPU behind it. The hypothesis is that Mesa's GBM and EGL on it use the `kms_swrast` driver (llvmpipe rendering into dumb buffers, perhaps forced with `MESA_LOADER_DRIVER_OVERRIDE=kms_swrast`), and that such buffers export and import as LINEAR dma-bufs. The spike records:
  - whether `vgem` exposes a render node, or only a card node, and which one GBM accepts
  - the exact setup steps
  - whether it works on WSL2 and on GitHub's Ubuntu runners (whose kernel may need `linux-modules-extra-$(uname -r)` for `vgem`)
  - orientation and channel order
  - whether lavapipe on the same machine imports the buffer (for slice 9's pixel test)

  If the renderer uses the node's "card" path instead of a render node, decision 6's enumeration takes card nodes as a fallback, and the spec's wording of "render node" covers the DRM device used.
- **`tests/surface.rs`** gains a `cfg(target_os = "linux")` section with the same scenarios as Windows: creation errors (including no render node, unknown device and unusable layouts), no frames without a size, sizes, resize while paused, black with nothing loaded, the upright test pattern, solid colours, letterboxing, created after load, audio-less reload, both drop orders, a frame and its fd outliving the renderer, no `on_frame` after drop, import into another context, and single plane.
  - When creation fails with "no DRM render node is available", these tests return early with a message, as the Windows section does on Mesa. The "without a render node" test runs only when `/dev/dri` has no usable node.
- **Hardware decoding.** `PLAYER_TEST_GL=hardware` enables the Linux hwdec test (non-copy `hwdec-current`, frames published). It cannot run locally. It is written now, so the 10+ hardware check is a test run and not new code.
- **CI** is settled by the spike: the Linux legs either run `modprobe vgem` and the full section, or run only the error and builder tests, with the gap recorded.

### 13a. Spike outcome (task 1.1)

Run on WSL2 (kernel 6.18.33.2-microsoft-standard-WSL2, Mesa 26.2.5 from the kisak PPA), branch `spike/add-player-8-gbm-dmabuf`.

| Question | Outcome |
|---|---|
| Setup | `sudo modprobe vgem` (as root: `wsl -u root -- modprobe vgem` from Windows, since `sudo` needs a terminal), then make the nodes accessible (`chmod 666 /dev/dri/*`, or membership of the `video` and `render` groups). `vgem` creates `card0` (226:0) and `renderD128` (226:128). |
| Driver and override | `LIBGL_ALWAYS_SOFTWARE=1` (or `GALLIUM_DRIVER=llvmpipe`) is required. Without it, Mesa loads `d3d12` for the GBM display on WSL2, and GBM allocation fails. `MESA_LOADER_DRIVER_OVERRIDE=kms_swrast` alone does not help. |
| Node type | **Card node only.** With llvmpipe, GBM allocates through dumb buffers, which the kernel refuses on render nodes (`DRM_IOCTL_MODE_CREATE_DUMB: Permission denied`). On `card0`, everything works. So decision 6's enumeration falls back to card nodes after render nodes, as decision 13 foresaw, and the default node on WSL2 is `card0` (226:0). |
| EGL | EGL 1.5, `EGL_KHR_platform_gbm`, `EGL_EXT_image_dma_buf_import(_modifiers)`, `EGL_KHR_surfaceless_context` and `EGL_KHR_no_config_context` are present. A 3.2 core context is created with no config and no surface. |
| Allocation | `gbm_bo_create_with_modifiers2([LINEAR], GBM_BO_USE_RENDERING)` succeeds: 1 plane, offset 0, stride 768 for 160 px, modifier 0 (LINEAR). |
| Orientation | With `flip_y = false`, mpv's top row lands in the buffer's first row (testsrc's colour bars at the top, its gradient at row 90 of 120). GL row 0 is the buffer's first row. |
| Channel order | `ABGR8888` holds R, G, B, A bytes in memory order, both read through GL and through `mmap` of the dma-buf. |
| Fd outlives bo | After `gbm_bo_destroy` and with only the dma-buf fd kept, a second GBM device and EGL display import the fd and read the picture. |
| lavapipe import | lavapipe has `VK_KHR_external_memory_fd` and `VK_EXT_external_memory_dma_buf`, but **not** `VK_EXT_image_drm_format_modifier` (nor `VK_EXT_physical_device_drm`). So wgpu-hal's `texture_from_dmabuf_fd` cannot run on it. A `VK_IMAGE_TILING_LINEAR` image bound to the imported dma-buf memory reads back correctly. Input for slice 9: its headless pixel test cannot use lavapipe through wgpu-hal's import as is. |
| VA-API | Static check of libmpv 26.10.1: `libva.a`, `libva-drm.a` are in the archive, `ra_hwdec_vaapi` is defined, and the vaapi interop reads the `drm_params_v2` native resource (`vaGetDisplayDRM`). So `DRM_DISPLAY_V2` is the right parameter. Not verifiable without hardware (10+). |
| mpv on llvmpipe (found by the spike, not specific to dma-bufs) | With mpv's default scaler, **YUV** video (yuv420p) renders black, or with wrong colours, on llvmpipe here, in most runs, on the existing plain `RenderContext` path too. RGB sources (`testsrc`) are always right. `scale=bilinear` (or `gpu-dumb-mode=yes`) fixes it. `fbo-format=rgba8` only hides it (white becomes `[51, 255, 0]`). The Linux surface tests therefore set `scale=bilinear` on software renderers. |
| `hwdec=auto-safe` on WSL2 (found while implementing 2.7) | With an H.264 file, `auto-safe` makes FFmpeg probe Vulkan decoding, which loads Mesa's `dzn` (D3D12) Vulkan driver. It has no `VK_KHR_video_decode_queue`, and is unloaded while its `core` thread lives, so the process crashes with SIGSEGV when that thread exits. This also happens with a plain `RenderContext`, so it comes from the new Linux default, not the surface renderer. Hiding `dzn` (`VK_ICD_FILENAMES=.../lvp_icd.json`) or a list without Vulkan (`hwdec=vaapi,nvdec`) avoids it. Also, on WSL2 `auto-safe` picks **`nvdec-copy`** (CUDA works, CUDA-GL interop does not), so decision 7's "never picks a `-copy` variant" is wrong: `auto-safe` includes copy-back decoders. |
| Renderer on vgem without `LIBGL_ALWAYS_SOFTWARE` (WSL2) | Mesa loads `d3d12` for the vgem node. Allocation fails as above, so `build` fails with "no usable DRM render node is available". The render thread then exits, and the same `core`-thread crash follows. This only happens on WSL2 with `vgem` loaded, which is test setup; the README's setup sets `LIBGL_ALWAYS_SOFTWARE=1`. |
| CI (GitHub runners) | **Works on both Linux legs** (run 41011411874; `ubuntu-latest` x64 and `ubuntu-24.04-arm`, kernel 7.0.0-1017-azure, Mesa 25.2.8, llvmpipe with `LIBGL_ALWAYS_SOFTWARE=1`). `sudo modprobe vgem` works as is (no `linux-modules-extra`). The runners already have a `hyperv_drm` `card1`; vgem adds `card0` and `renderD128`. GBM, EGL, mpv rendering and readback work on `card0` and also on `card1`, and fail on `renderD128` (dumb buffers), as on WSL2. Same orientation, channel order and colours. **Difference:** Mesa 25.2.8 reports `DRM_FORMAT_MOD_INVALID` for a buffer allocated with `[LINEAR]`, so the renderer then allocates again with `GBM_BO_USE_LINEAR` and reports LINEAR. mpv's default scaler drew YUV black in 2 of 3 runs on x64 here too, confirming the tests' `scale=bilinear`. The throwaway lavapipe probe crashed on both legs, so CI says nothing about lavapipe import; slice 9 has to check it itself. Task 3.1 therefore loads `vgem` on the Linux legs, and the full Linux surface section runs in CI. |

### 13b. Manual check (task 4.1)

WSL2, kernel 6.18.33.2-microsoft-standard-WSL2, Mesa 26.2.5 (kisak PPA), llvmpipe (LLVM 21.1.8) on `vgem`'s `card0` (226:0), `LIBGL_ALWAYS_SOFTWARE=1`, release build. Media: a 90 s 1920x1080 30 fps H.264 + AAC file made with FFmpeg (`testsrc2`), given by `PLAYER_MANUAL_FILE`, with `scale=bilinear` (see 13a).

| Check | Result |
|---|---|
| Frames at the set size while playing | Yes: 1920x1080 frames, LINEAR, stride 7680, offset 0; 48 frames published in 2 s (24 fps on llvmpipe), sequence numbers in order. Decoding in software (`vaapi,nvdec` find no device). |
| Resize while paused | Yes: a 1280x720 frame is published while paused. |
| A minute of playback with resizes | 62 s, 12 resizes between 1080p, 720p, 480p and 900p, with frames at every size. |
| File descriptors | Flat: 29 after each of the 12 resizes. The 6 fds above the count taken at the first frame (two eventfds, a pipe pair, two sockets) are the audio output's connection, opened as playback starts; no dma-buf fd stays open. |
| Drop while playing | Returns in 24-236 ms (under 1 s). |

The README lists Linux hardware (Mesa on Intel and AMD, NVIDIA, tiled modifiers, VA-API and NVDEC zero-copy decoding) as unverified.

### 14. If the software path does not work

If task 1.1 finds that `vgem` gives no working GBM/EGL path on WSL2, the slice cannot be verified anywhere available. Implementation then stops, and the user decides between real hardware (a native Linux machine, a live USB or a GPU cloud VM) and another approach. This is a gate in task 1.1, not something to work around.

## Risks / Trade-offs

- [The `vgem` software path does not work, or works on WSL2 but not on CI] → Task 1.1 checks it first, with a stop-and-ask gate (decision 14). CI falls back to the error and builder tests, with the gap recorded, as Windows did.
- [Software verification does not prove hardware behaviour: tiled modifiers, VA-API or NVDEC zero-copy, NVIDIA's GBM, implicit sync] → The user accepted a documented gap. The README lists Linux hardware as unverified, and the hwdec test is ready for 10+.
- [LINEAR buffers are slower to render and sample on discrete GPUs] → One quad per frame keeps the cost small. Slice 9 can pass GPUI's single-plane tiled modifiers through `modifiers(..)`.
- [The default render node differs from GPUI's adapter on hybrid machines] → LINEAR imports across devices on most drivers. Slice 9 passes GPUI's node to `device()`.
- [The VA-API interop needs more than `DRM_DISPLAY_V2`, or libmpv lacks VA-API] → mpv then falls back to NVDEC (on NVIDIA) or software decoding, and frames are still published. This is recorded by task 1.1 and fixed in 10+ on hardware.
- [The row order or channel order differs from what is assumed] → Task 1.1 checks it. A flip is a render parameter and a different fourcc is a constant, never a CPU copy.
- [NVIDIA's EGL driver has thread-exit problems like its WGL driver] → It is unknown and cannot be tested here. If it appears, the Windows render-thread pool is reused.
- [wgpu-hal's import assumes a layout or ownership state the buffer does not have] → This is slice 9's risk. It is noted here because the producer contract (single plane, explicit modifier, GPU-complete) was shaped to match `texture_from_dmabuf_fd`.
- [8-bit SDR only] → mpv tone-maps HDR to SDR, as on the other platforms.

## Migration Plan

Additive, except for one behaviour change: Linux players built with `VideoOutput::Render` now hardware-decode by default (`vaapi,nvdec`). Apps that need software decoding pass `mpv_option("hwdec", "no")`. `player_gpui::VideoPlayer` stays black on Linux until slice 9.
