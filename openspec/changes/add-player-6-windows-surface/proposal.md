## Why

Video reaches the screen only on macOS (slice 4). On Windows, `player_gpui::VideoPlayer` paints black while audio plays. This is slice 6 of the `add-player` series, the first of two slices that bring video to Windows. It does the core half: the player renders each frame on the GPU into a Direct3D 11 texture that another D3D11 device can open, without the frame passing through the CPU. Slice 7 then shows those textures in GPUI.

A finding shaped the plan. Stock GPUI cannot show an external GPU texture on Windows at all, in either backend GPUI-Kit 0.8.1 can use (`gpui-pre` 0.3.10 and `gpui-fast` 0.1.12):
- `PaintSurface` has no payload off Apple platforms.
- `Window::paint_surface` is `cfg(macos/ios)`.
- `DirectXRenderer::draw_surfaces` is an empty stub.

The user chose to fork GPUI (`gpui-pre` and `gpui-pre-windows`), add Windows surface support there, and send it upstream once it is verified. Apps then add a `[patch.crates-io]` entry until upstream ships it. The fork is slice 7's work. This slice produces a toolkit-agnostic texture stream that the fork, or a wgpu/D3D frontend, can import. It can be tested on its own, without GPUI.

The libmpv build for Windows renders only through OpenGL (WGL, `opengl32`; no ANGLE/EGL). So the bridge is `WGL_NV_DX_interop2`: mpv draws through GL straight into a D3D11 texture. NVIDIA, AMD and Intel drivers expose this extension.

## What Changes

- **`crates/player`, new Windows-only module `player::windows`** (toolkit-agnostic, no GPUI types), mirroring `player::macos`:
  - `SurfaceRenderer`, a safe type owning a render thread with:
    - a hidden window and an offscreen WGL context
    - its own D3D11 device on a chosen DXGI adapter (by default the first adapter that supports D3D11, the one GPUI picks; or one given by LUID)
    - slice 2's `RenderContext` on that context
    - a small ring of BGRA D3D11 textures, created shareable through NT handles and registered with GL through `WGL_NV_DX_interop2`
  - It renders each frame mpv announces at the size the host sets, waits for the GPU to finish, publishes the newest frame (readable from any thread) and calls a host callback, as on macOS.
  - `Frame` gives the texture's shared NT handle (a std `BorrowedHandle`), its size, sequence number and adapter LUID. No `windows` crate type is in the public API.
  - The format is BGRA only. NV12 existed on macOS only because stock GPUI required it there, and slice 7 writes GPUI's Windows import itself.
  - Creation fails with `Error::Render` when the GL driver lacks `WGL_NV_DX_interop2`, when the GL context and the D3D11 device cannot be shared on the requested adapter, or for the macOS slice's existing reasons.
  - The "video when created after loading" behaviour (re-select `vid`, reload audio-less media) moves from `player::macos` into code shared by both platforms.
  - All `unsafe` stays inside the module.
- **`crates/player`, playback-core**: on Windows, a player built with `VideoOutput::Render` gets a zero-copy hardware decoding default. It is proposed as `hwdec=auto-safe`, and task 1.1's spike confirms or revises it. A raw `mpv_option("hwdec", ...)` overrides it.
- **CI**: the Windows x64 leg runs the Windows surface tests on whatever software path the spike finds (see design.md). Windows arm64 keeps slice 2's skip of OpenGL tests.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `video-surface`: surface renderers become available on Windows.
  - Creation gains adapter selection and the interop/adapter failure cases.
  - Content and encoding gain the Windows BGRA texture.
  - Hardware decoding gains the Windows default.
  - A new requirement covers sharing a frame with another D3D11 device.
  - Platform coverage changes to macOS and Windows, with Linux still absent.
- `playback-core`: "Player creation with validated options" gains the Windows hardware-decoding default for the render output.

## Impact

- **Crates**: `crates/player` only.
  - Everything platform-specific lives in the core: WGL, D3D11/DXGI, the interop, the render thread and the hwdec default. A GUI crate only opens the shared handle on its own device.
  - `crates/gpui` is not touched. `VideoPlayer` stays black on Windows until slice 7.
- **libmpv APIs**: no new render API calls (slice 2's `RenderContext` is reused).
  - New option default on Windows: `hwdec` (proposed `auto-safe`).
  - Properties read in tests and docs: `hwdec-current`, `vid`.
- **Windows APIs** (behind `cfg(windows)`):
  - WGL: `ChoosePixelFormat`, `wglCreateContext`, `wglCreateContextAttribsARB`, `wglMakeCurrent`, `wglGetProcAddress`, and `WGL_NV_DX_interop2` (`wglDXOpenDeviceNV`, `wglDXRegisterObjectNV`, `wglDXLockObjectsNV`, `wglDXUnlockObjectsNV`, and the matching close/unregister calls).
  - D3D11/DXGI: `CreateDXGIFactory2`, `EnumAdapters`, `D3D11CreateDevice`, textures with `D3D11_RESOURCE_MISC_SHARED | D3D11_RESOURCE_MISC_SHARED_NTHANDLE`, `IDXGIResource1::CreateSharedHandle`, event queries.
  - Win32: a hidden window (`CreateWindowExW`) for the WGL device context.
- **Public API**:
  - New `player::windows` module: `SurfaceRenderer`, `SurfaceRendererBuilder`, `Frame`, `AdapterLuid`.
  - Behaviour change: Windows Render players hardware-decode by default.
- **Dependencies** (Windows only): `windows` 0.62, the version GPUI uses, so no second copy is built for GPUI apps. `glow` moves from dev to runtime on Windows. Both are justified in design.md.
- **rust-sak**: nothing in it covers this. Its modules are `crypto`, `fetch`, `fs`, `github`, `image`, `memo`, `o11y`, `sysinfo` and `tui`. `sysinfo` probes GPUs but does not create D3D devices or pick DXGI adapters by LUID, and a GL/D3D11 bridge for mpv is specific to this project. Nothing is added to `rust-sak`.
- **Platforms**:
  - Windows gets surfaces in the core, but no video in GPUI until slice 7.
  - Linux keeps no surface renderer and a black `VideoPlayer`, and the gap stays documented.
  - Windows arm64 has no software OpenGL on CI, so its surface tests skip like slice 2's.
  - Hybrid-GPU laptops where the GL driver and GPUI's adapter differ are a known risk (see design.md).
- **Size**: 14 tasks. This is slice 6 of the `add-player` series (see design.md). It depends on slices 2 and 4.
