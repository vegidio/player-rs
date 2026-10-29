## Context

What already exists:
- **`RenderContext` (slice 2)** draws a player's video into any FBO of a host GL context. It has one `unsafe` constructor, whose contract is "this GL context is current on every call and on drop". An `ActiveFlag` allows one render context per player.
- **`player::macos::SurfaceRenderer` (slice 4)** wraps it safely:
  - It has its own render thread and offscreen CGL context, and a ring of 3 IOSurface slots.
  - Signals are a `Mutex<Signals { mpv_frame, size, shutdown }>` plus a `Condvar`.
  - The GPU is fenced before publication, then `report_swap` and `on_frame` run.
  - When a renderer is created after load, it re-selects `vid` (`reselect_video` in `macos/mod.rs`) and reloads audio-less media (the event thread's `reload_async`).
- **The tests' GL helper** (`tests/common/gl.rs`) already makes WGL contexts on a hidden window through `glutin`. CI's Windows x64 leg uses Mesa's software `opengl32.dll` (mesa-dist-win 26.2.4). Windows arm64 skips the GL tests.

What was verified for this design:
- **GPUI on Windows** (`gpui-pre-windows` 0.3.10 and `gpui-fast-windows` 0.1.12):
  - It renders with D3D11 through a DirectComposition swap chain in `B8G8R8A8_UNORM`.
  - It picks the first adapter from `IDXGIFactory6::EnumAdapters` that creates a D3D11 device.
  - Its device is `pub(crate)`.
  - `PaintSurface` has no Windows payload, and `draw_surfaces` is an empty stub. See proposal.md for why.
  - It uses `windows` 0.62.
- **The static libmpv for Windows** (binaries-mpv 26.10.1, `mpv.pc`) links `opengl32` and has no EGL/ANGLE. So libmpv's render API is reachable only through WGL. mpv's own d3d11 VO (libplacebo, spirv-cross-hlsl) exists, but the render API cannot use it.

Required behaviour is in `specs/video-surface` and the `playback-core` delta. This document covers how.

## Slices

The `add-player` series. **This change is slice 6.**

| # | Change | Delivers | Depends on | Status |
|---|---|---|---|---|
| 1 | `add-player-1-core` | `player::Player` handle: builder, sources, load, playback control, property/command access, state snapshot, event subscription; headless CI tests | `add-workspace-scaffold` | done |
| 2 | `add-player-2-render` | mpv OpenGL render API in core over a host-provided GL context: `VideoOutput::Render`, `RenderContext`, `State.video_size`; software-GL render tests on all six CI legs | 1 | done |
| 3 | `add-player-3-ui-core` | `player::ui`, the toolkit-agnostic UI layer (`ui::Options`, `ui::Controls`, `View`, actions, Video.js hotkeys, activity, bar maths); `State.buffered`, `Player::scrub` | 1 | done |
| 4 | `add-player-4-gpui-macos` | macOS video on screen: `player::macos::SurfaceRenderer` (CGL, IOSurface NV12 ring, fenced publication), `hwdec=videotoolbox` default, `Player::ptr_eq`; `player_gpui::VideoPlayer` (black on Windows/Linux) and the `play` example | 2 | done |
| 5 | `add-player-5-gpui-controls` | Video.js controls in GPUI over `VideoPlayer`: control bar, big play button, spinner, error display, fading, pointer/keyboard/focus, actions, window fullscreen, fluid aspect ratio, `player_gpui::Assets`/`Icons` | 3, 4 | done |
| 6 | `add-player-6-windows-surface` | Windows surfaces in core: `player::windows::SurfaceRenderer` (render thread, hidden-window WGL context, D3D11 device on a chosen adapter, BGRA ring of NT-handle shared textures through `WGL_NV_DX_interop2`, GPU-complete publication), `Frame` with shared handle and adapter LUID, Windows hwdec default, "created after load" code shared with macOS; tests and CI | 2, 4 | **current** |
| 7 | `add-player-7-gpui-windows` | Windows video in GPUI: a fork of `gpui-pre` + `gpui-pre-windows` 0.3.10 (untouched upstream as the first commit, then one change: a Windows `PaintSurface` payload, `Window::paint_surface` on Windows, `draw_surfaces` opening the shared handle on GPUI's device, and a marker item for a readable error when the patch is missing); `[patch.crates-io]` in this workspace; an opt-in `windows-video` feature in `player-gpui` that wires `VideoView` on Windows (black and a one-time warning when creation fails); README setup steps; manual checks; upstream PRs to zed-industries/zed after verification | 5, 6 | pending |
| 8+ | (to be planned) | Linux video surfaces (EGL/dma-buf into GPUI's renderer, after a spike); the `gpui-fast` fork for apps on that backend; menus (rate list, tracks), captions, poster drawing, skip buttons, touch behaviour; hiding the cursor when inactive (needs a GPUI API); HDR surfaces if GPUI gains them | 6, 7 | pending |

Changes from the plan in slice 5's table:
- **"Windows and Linux surfaces" is split.** Windows comes first, in two slices (6 core, 7 GPUI), and Linux moves to 8+.
- **The GPUI integration is a fork with `[patch]`**, chosen by the user over hole-punching through DirectComposition (it would need a transparent window and no backgrounds behind the player) and over a CPU copy into `paint_image` (it breaks zero-copy). The fork and its upstream PR are slice 7's explicit work.
- **The `gpui-fast` fork is deferred to 8+.** `player-gpui` builds on `gpui-pre`, GPUI-Kit's default backend.

## Goals / Non-Goals

**Goals:**
- A safe, toolkit-agnostic Windows API that turns a `Player` into a stream of D3D11 textures that any D3D11 or D3D12 device on the same adapter can open, with no GL and no `unsafe` for the consumer.
- No CPU copy of frames anywhere in the shipping path. That includes hardware decoding wherever mpv has a zero-copy GL interop for it.
- The same observable behaviour as `player::macos` (sizing, publication, lifetimes, created-after-load), so slice 7's GPUI code mirrors slice 4's.

**Non-Goals:**
- Anything in GPUI or `crates/gpui` (slice 7).
- NV12 surfaces on Windows. NV12 existed on macOS only because stock GPUI required it, and slice 7 writes the import itself.
- A CPU-readback fallback for drivers without `WGL_NV_DX_interop2`. Creation fails instead, and a GUI shows black (slice 7). Adding a fallback would break zero-copy and needs the user's explicit sign-off.
- HDR or 10-bit textures (GPUI's swap chain is 8-bit BGRA), `MPV_RENDER_PARAM_ADVANCED_CONTROL`, and presentation-timed `report_swap`, as on macOS.

## Decisions

### 1. Public API (`player::windows`, `cfg(windows)`)

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct AdapterLuid { pub low: u32, pub high: i32 }   // DXGI_ADAPTER_DESC1::AdapterLuid, no windows-crate type

pub struct SurfaceRenderer { /* Send + Sync */ }
impl SurfaceRenderer {
    pub fn builder(player: &Player) -> SurfaceRendererBuilder;
    pub fn set_size(&self, width: u32, height: u32);   // device pixels
    pub fn latest(&self) -> Option<Frame>;             // never waits on mpv or GL
    pub fn adapter(&self) -> AdapterLuid;
}
impl SurfaceRendererBuilder {
    pub fn adapter(self, AdapterLuid) -> Self;         // default: first DXGI adapter with D3D11, as GPUI picks
    pub fn on_frame(self, impl Fn() + Send + Sync + 'static) -> Self;   // called on the render thread
    pub fn build(self) -> Result<SurfaceRenderer>;     // Error::Render on failure
}

#[derive(Clone)] pub struct Frame { .. }               // Arc around the slot's handle
impl Frame {
    pub fn shared_handle(&self) -> BorrowedHandle<'_>; // std::os::windows::io; NT handle, BGRA8 D3D11 texture
    pub fn adapter(&self) -> AdapterLuid;
    pub fn width(&self) -> u32; pub fn height(&self) -> u32; pub fn sequence(&self) -> u64;
}
```

- **The same shape as `player::macos`.** Builder, `set_size`, `latest`, `on_frame` and `Frame` have the same names and semantics, so slice 7's `VideoView` differs only in its paint call. There is no `format` or `gl_renderer` option:
  - BGRA is the only format.
  - WGL cannot choose a software renderer. The GL implementation is whatever `opengl32.dll` loads: the GPU driver's ICD, or Mesa when its DLL is placed next to the executable, as on CI.
- **An NT handle, not a COM pointer.** `ID3D11Texture2D` is tied to our device, and a consumer on another device cannot use it. An NT handle opens on any D3D11.1 device with `ID3D11Device1::OpenSharedResource1`, and on D3D12 with `OpenSharedHandle`, which egui-wgpu's DX12 backend could use. Handing out a std `BorrowedHandle` keeps the `windows` crate's version out of the public API. The macOS slice could not do that, because GPUI's own `CFRetained<CVPixelBuffer>` type was the contract there.
- **`AdapterLuid`** is two plain integers. The consumer compares it with its own device's adapter. In slice 7, the GPUI fork exposes GPUI's LUID, and the element passes it to `adapter()`.

### 2. Render thread

```
spawn: hidden window + WGL context (current) -> check WGL_NV_DX_interop2
       D3D11 device on the adapter -> wglDXOpenDeviceNV(device)
       glow + RenderContext::new(loader, on_mpv_frame) -> init result -> loop
loop:  wait on Condvar<Signals { mpv_frame, size, shutdown }>
       shutdown -> break
       render.update()
       no size or zero size -> continue
       size changed -> rebuild the ring for the new size
       slot = ring[next]                              // 3 slots, round-robin
       wglDXLockObjectsNV(slot.interop)
       render.render({fbo: slot.fbo, w, h}, flip_y = false)
       wglDXUnlockObjectsNV(slot.interop)             // GL work is ordered before later D3D11 work
       context.End(event_query); context.Flush(); spin GetData(event_query) with yield, 1 s bound
       latest.lock() = Some(Frame { slot.handle.clone(), .., seq += 1 })
       render.report_swap(); on_frame()
exit:  drop RenderContext (GL current) -> unregister interop objects -> wglDXCloseDeviceNV
       -> delete GL objects -> wglMakeCurrent(null) -> wglDeleteContext -> DestroyWindow
```

- **This is the macOS loop, unchanged** except for the lock/unlock pair and the completion wait. The signals, the bounded join on drop, and `report_swap` after publication carry over, with the same reasons as slice 4.
- **Orientation.** D3D11 row 0 is the top. GL writes row 0 at the texture origin, which through the interop maps to D3D row 0. So `flip_y = false` should give an upright D3D picture, as with IOSurface. Task 1.1 checks this, because the interop's row mapping is driver-defined in practice.
- **The hidden window** exists only because WGL needs a device context with a pixel format. It is a message-only-style `WS_POPUP` window on the render thread, never shown, with a class registered once per process.
- **The WGL context** is created with `wglCreateContextAttribsARB` at 3.2 core when available, else a legacy context. mpv accepts either. The loader is `wglGetProcAddress` with a fallback to `GetProcAddress(opengl32)` for the GL 1.1 entry points, which `wglGetProcAddress` does not return.

### 3. Synchronisation: a completion wait, not a keyed mutex

| Option | Verdict |
|---|---|
| **Unlock, then a D3D11 event query on our device, waited on the render thread** | **Chosen.** The consumer needs no synchronisation protocol. It opens the handle and samples it, exactly as GPUI's Metal side samples a fenced IOSurface. The wait costs the render thread only, as `glClientWaitSync` does on macOS. |
| `D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX` | Correct by construction, but the consumer must `AcquireSync`/`ReleaseSync` around every draw on the GUI thread, and could stall GPUI's frame on our render. It would also put the protocol into the GPUI fork's API. |
| No wait | GPUI could sample a half-drawn texture. |

The ring of 3 together with "publish only completed frames" gives the spec's "unchanged until two newer frames are published", as on macOS. A consumer must take a new frame from `latest()` every paint and not keep sampling an old one. The docs state this.

### 4. Adapter selection and the GL driver

- **D3D11 device.** By default: `CreateDXGIFactory2`, then the first `EnumAdapters` entry that creates a D3D11 device. That is GPUI's own rule, so the two match without any API between them. With `adapter(luid)`: the adapter with that LUID, else `Error::Render` naming it.
- **GL's adapter cannot be chosen through WGL.** The GL ICD runs on whichever GPU the driver picks. On hybrid laptops (NVIDIA Optimus, AMD switchable), that can differ from DXGI adapter 0. In that case `wglDXOpenDeviceNV` fails, or worse, works through a cross-adapter copy.
  - Detection: after `wglDXOpenDeviceNV` succeeds, compare `GL_VENDOR` with the adapter's `VendorId`. A mismatch fails creation with `Error::Render`, and the message names both GPUs.
  - Task 1.1 checks the comparison on the hardware available. A precise LUID match through `WGL_NV_gpu_affinity`/`WGL_AMD_gpu_association` is left for later if a user reports a mismatch.
  - The docs tell hybrid-laptop apps how to steer both to one GPU: `NvOptimusEnablement`/`AmdPowerXpressRequestHighPerformance` exports, or the Windows graphics settings.

### 5. Shared textures and the interop

- **Textures.** Each slot is a `D3D11_TEXTURE2D_DESC` with `B8G8R8A8_UNORM`, `BIND_RENDER_TARGET | BIND_SHADER_RESOURCE` and `MISC_SHARED | MISC_SHARED_NTHANDLE`. Its handle comes from `IDXGIResource1::CreateSharedHandle` (`GENERIC_ALL`) and is wrapped in a std `OwnedHandle` inside an `Arc` shared with every `Frame` of that slot.
  - An open NT handle keeps the resource alive, so a frame outlives a ring rebuild and the renderer (spec).
  - Task 2.3 confirms this with the "handle outlives renderer" scenario.
- **Interop.** `wglDXRegisterObjectNV(device, texture, gl_tex, GL_TEXTURE_2D, WGL_ACCESS_WRITE_DISCARD_NV)` registers each slot's texture, and the GL texture is attached to the slot's FBO. mpv renders into that FBO like any other.
  - The GL view of a `B8G8R8A8` texture is an RGBA8 texture whose channels the interop swizzles, so mpv's RGBA output lands as BGRA in D3D. Task 1.1 checks that colours are not swapped.
- **The ring is rebuilt on a size change**, like macOS: unregister, release, create, register.

### 6. Hardware decoding default: `auto-safe`, confirmed by the spike

mpv's zero-copy GL interops on Windows are, as far as known before the spike:
- `nvdec` (CUDA-GL interop, NVIDIA; `ffnvcodec` is linked)
- `dxva2` through `WGL_NV_DX_interop` with D3D9Ex (`dxva2gldx`), if the mpv in binaries-mpv 26.10.1 still has it

`d3d11va`'s zero-copy GL interop needs ANGLE, which this build does not have.

`auto-safe` tries mpv's whitelist of safe hwdecs in order and uses only those that initialise. With a GL render context, that means only non-copy hwdecs with a working GL interop. When none fits, it falls back to software decoding.
- Unlike macOS's explicit `videotoolbox`, there is no single right name across GPU vendors.
- `auto-safe` should not pick a `-copy` variant. Task 1.1 confirms this, along with which decoder each vendor gets.
- If the spike shows `auto-safe` picking a copy-back hwdec, or none on common GPUs, the default changes to an explicit list (for example `nvdec,dxva2`), and task 1.1 updates the `playback-core` delta before anything depends on it.

As before, `mpv_option("hwdec", ..)` overrides the default, and `Null` keeps `no`.

### 7. Shared "created after load" code

`reselect_video` and the renderer's reload signal move from `player::macos` to a crate-private `surface` module compiled on `cfg(any(target_os = "macos", windows))`. Both renderers call it after a successful build, and the event thread's `reload_async` is untouched. Doing this first (task 2.1) means the macOS CI legs prove the move before any Windows code depends on it.

### 8. Dependencies

| Crate | Where | Why, and alternatives |
|---|---|---|
| `windows` 0.62 | `player`, Windows runtime | D3D11, DXGI, WGL (`Win32_Graphics_OpenGL`), window creation and handles. It is the version GPUI uses, so GPUI apps build one copy. Alternatives: `windows-sys` (no COM interfaces, so hand-written D3D11 vtables) and older `winapi` (unmaintained, no D3D11.1). The existing dev-only `windows-sys` 0.52 stays for the tests' helper. |
| `glow` 0.16 | `player`, Windows runtime (already macOS runtime and dev) | Typed GL calls for FBOs and the interop's texture objects, as on macOS. |
| WGL / `WGL_NV_DX_interop2` | `player`, Windows | About 12 functions, loaded through `wglGetProcAddress`. `glutin` (dev-only today) was rejected at runtime: it would add a dependency for one context on a hidden window, and it does not expose the interop extension anyway. |

`rust-sak`: none of its modules (`crypto`, `fetch`, `fs`, `github`, `image`, `memo`, `o11y`, `sysinfo`, `tui`) covers this. `sysinfo` reports GPUs but does not create D3D devices or resolve DXGI adapters by LUID, and a GL-to-D3D11 bridge for mpv is not general-purpose. No `rust-sak` feature is used, and nothing is added to it.

### 9. Backend / UI separation

Everything WGL-, D3D- and libmpv-specific is in `crates/player` behind `cfg(windows)`, with no GUI types. A frontend only:
1. sets a size
2. waits for `on_frame`
3. opens `frame.shared_handle()` on its own device
4. draws it

- **GPUI (slice 7):** the fork's `draw_surfaces` opens it on GPUI's device and samples it.
- **A future egui-wgpu crate:** it would open the handle through wgpu-hal's DX12 device (`OpenSharedHandle`) and reuse this core unchanged.

`crates/gpui` is not touched in this slice.

### 10. FFI, threading and lifetimes

- **`unsafe` lives in `crates/player/src/windows/`.** It covers the WGL and interop calls, COM calls through `windows`, `CreateSharedHandle`, glow's GL calls, and the one `RenderContext::new` call. Each block has a `// SAFETY:` note.
  - All GL, interop and D3D11 immediate-context calls happen on the render thread.
  - Handle duplication and closing are thread-safe.
- **Threads.** These are the same as on macOS:
  - mpv's threads only set a flag and notify.
  - The render thread owns every GL, D3D11 and interop object.
  - `set_size` and `latest` take a mutex held only for a field swap.
- **Lifetimes.**
  - `SurfaceRenderer` → the render thread → `RenderContext` → `Arc<Handle>`, so mpv outlives the renderer in either drop order (slice 2).
  - `Frame`s hold `Arc<OwnedHandle>`, and the texture lives while a handle is open.
  - Teardown order on the render thread: `RenderContext`, then the interop objects, then the interop device, then GL, then the window. The interop objects must be unregistered while both the GL context and the D3D11 device are alive.

### 11. Zero-copy

- **Hardware path:** an interop hwdec (`nvdec` or `dxva2`) hands decoded surfaces to GL. mpv scales and converts on the GPU into the slot's FBO, which is the D3D11 texture itself through the interop. The consumer samples the same memory.
- **Software decoding** uploads inside mpv, as noted in slices 2 and 4. That is mpv's normal path, not a copy this crate adds.
- **CPU readback** happens only in tests. They copy to a `D3D11_USAGE_STAGING` texture on a second device opened from the handle, then `Map` it. That also exercises the consumer path.

### 12. Testing and CI

- **`tests/surface.rs`** gains a `cfg(windows)` section with the same scenarios as macOS: creation errors, no frames without a size, sizes, resize while paused, black with nothing loaded, the upright test pattern, solid colours, letterboxing, created after load, audio-less reload, both drop orders, a frame and its handle outliving the renderer, no `on_frame` after drop, and opening on another device. Scenarios that are BGRA/NV12-specific or about `GlRenderer` stay macOS-only.
- **Hardware decoding.** `PLAYER_TEST_GL=hardware` enables the hwdec test, which asserts that `hwdec-current` is set and has no `-copy` suffix. It runs locally on a GPU, as on macOS.
- **CI: settled by the spike (task 1.1).** As far as known, Mesa's llvmpipe `opengl32.dll` has no `WGL_NV_DX_interop2`. The spike checks llvmpipe, then Mesa's `d3d12` gallium driver on WARP (`GALLIUM_DRIVER=d3d12`, shipped in mesa-dist-win), which runs on D3D12 and may expose the extension.
  - If one of them works, the Windows x64 leg runs the whole Windows section on it.
  - If neither does, the x64 leg runs only the "driver without Direct3D sharing" scenario plus the builder tests, the full section runs locally, and the gap is recorded here and in the README. That choice goes back to the user before task 3.1.
  - Windows arm64 keeps slice 2's GL skip either way.

## Risks / Trade-offs

- [Mesa offers no interop on CI, so the main path is tested only locally] → Task 1.1 tries the `d3d12` driver on WARP first. If that fails, the user decides, and the gap is documented (decision 12). A test-only CPU fallback is deliberately not added, because it would test a path that never ships.
- [Hybrid-GPU laptops: GL on one GPU, GPUI on another] → Detected and reported as `Error::Render` (decision 4), with the docs explaining how to steer the app to one GPU. Precise matching is deferred. Untested beyond the hardware available locally.
- [Driver bugs in `WGL_NV_DX_interop2` with D3D11 (historically on some AMD and Intel drivers)] → Manual verification (task 4.1) on whatever GPUs are available. Others are listed as unverified in the README. A failing driver produces black frames, never a crash, because creation errors are surfaced.
- [`auto-safe` picks a copy-back decoder, or none, on some GPUs] → Task 1.1 measures it, and the default can become an explicit list (decision 6).
- [The interop's row order or channel order differs from what is assumed] → Task 1.1 checks both on real hardware. A flip or swizzle would be a fixed render parameter, not a CPU copy.
- [An `OwnedHandle` keeps a texture alive after the D3D11 device is released, contrary to expectation] → The "handle outlives renderer" test catches it. If needed, `Frame` also holds an `Arc` to the device.
- [8-bit SDR only] → GPUI's swap chain is `B8G8R8A8_UNORM`. mpv tone-maps HDR to SDR, as on macOS.

## Migration Plan

Additive, except for one behaviour change: Windows players built with `VideoOutput::Render` now hardware-decode by default (`auto-safe`, or the spike's replacement). Apps that need software decoding pass `mpv_option("hwdec", "no")`. `player_gpui::VideoPlayer` stays black on Windows until slice 7.
