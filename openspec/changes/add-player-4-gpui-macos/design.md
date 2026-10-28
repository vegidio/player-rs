## Context

What already exists:
- `crates/player` has `Player` (slice 1) and `RenderContext` (slice 2). `RenderContext` draws a player's video into any FBO of a host GL context, through one `unsafe` constructor whose contract is "this GL context is current on every call and on drop". An `ActiveFlag` allows one render context per player, and the context keeps mpv alive through its own `Arc<Handle>`.
- `player::ui` (slice 3) is done and not used here.
- `crates/gpui` is an empty crate that depends on `player-rs`.
- The macOS render tests already create raw CGL contexts (`tests/common/gl.rs`: hardware, or Apple's software renderer `kCGLRendererGenericFloatID`).

See proposal.md for why this slice exists and for the GPUI finding that shapes it. Required behaviour is in `specs/video-surface`, `specs/gpui-player` and the `playback-core` delta. This document covers how.

What was verified in GPUI-Kit 0.8's GPUI (`gpui-pre` 0.3.9; `gpui-fast` 0.1.10 is the same):
- `surface(source)` and `Window::paint_surface(bounds, CFRetained<CVPixelBuffer>)` exist only on macOS and iOS.
- `MetalRenderer::draw_surfaces` `assert_eq!`s the pixel format to `kCVPixelFormatType_420YpCbCr8BiPlanarFullRange`. It samples plane 0 as `R8Unorm` and plane 1 as `RG8Unorm` through a `CVMetalTextureCache`, and converts with a fixed full-range BT.601 matrix: `R = Y + 1.402(Cr−½)`, `G = Y − 0.3441(Cb−½) − 0.7141(Cr−½)`, `B = Y + 1.772(Cb−½)`.
- The Metal layer is `BGRA8Unorm`. There is no HDR or wide-gamut output.
- `Window::use_keyed_state(id, cx, init) -> Entity<S>` keeps state while an element with that id renders in consecutive frames, and notifies the rendering view when the state entity notifies.
- `canvas(prepaint, paint)` provides bounds and paint access without a custom `Element` impl. `ObjectFit::get_bounds` is public.
- The crates use `objc2-core-video` and `objc2-core-foundation` 0.3.2.

## Slices

The `add-player` series. **This change is slice 4.**

| # | Change | Delivers | Depends on | Status |
|---|---|---|---|---|
| 1 | `add-player-1-core` | `player::Player` handle: builder, sources, load, playback control, property/command access, state snapshot, event subscription; headless CI tests | `add-workspace-scaffold` | done |
| 2 | `add-player-2-render` | mpv OpenGL render API in core over a host-provided GL context: `VideoOutput::Render`, `RenderContext` (loader, frame-ready callback, render into host FBO, `report_swap`), `State.video_size`; software-GL render tests on all six CI legs | 1 | done |
| 3 | `add-player-3-ui-core` | `player::ui`, toolkit-agnostic UI layer: `ui::Options`, `ui::Controls` controller and its `View`, `Action`/`Request`, Video.js hotkey map, time formatting, user-activity state machine, seek/volume bar maths; in `playback-core`, `State.buffered` and `Player::scrub` | 1 | done |
| 4 | `add-player-4-gpui-macos` | macOS video on screen: in core, `player::macos::SurfaceRenderer` (own render thread, offscreen CGL context, IOSurface-backed `CVPixelBuffer` ring in BGRA or NV12 via a GPU conversion pass, GPU-fenced publication, video re-selected when created after load), `hwdec=videotoolbox` default for Render on macOS, `Player::ptr_eq`; in `crates/gpui`, the `player_gpui::VideoPlayer` element (keyed state, repaint on frame, device-pixel sizing, `paint_surface`; black on Windows/Linux) and a manual-test example | 2 | **current** |
| 5 | `add-player-5-gpui-controls` | Video.js control bar in GPUI over `VideoPlayer`, and `player_gpui::init(cx)` (actions and key bindings): draws slice 3's `ui::View` and forwards input to `ui::Controls` | 3, 4 | pending |
| 6+ | (to be planned) | Windows and Linux video surfaces (offscreen WGL/EGL context and texture sharing in core, import in GUI crates, after a spike on GPUI surface support on those platforms, which today have no `surface()` at all), per-platform zero-copy hwdec, menus (rate list, tracks), captions, fullscreen, poster drawing, skip buttons, touch behaviour, and, if GPUI gains them, BGRA/HDR surfaces to lift the NV12/SDR limits | 4, 5 | pending |

Changes from the plan in slice 3's table:
- The IOSurface is NV12, produced by a GPU pass. GPUI cannot take BGRA (see Context).
- The core type owns its render thread and GL context, so it has a safe API.
- `Player::ptr_eq` and the hwdec default are added here, because the element and the zero-copy goal need them.

## Goals / Non-Goals

**Goals:**
- A safe, toolkit-agnostic macOS API that turns a `Player` into a stream of GPU surfaces. GPUI uses the NV12 format, and an egui-wgpu or Metal app could use BGRA, with no GL or `unsafe` in either.
- No CPU copy of frames anywhere in the shipping path, including with VideoToolbox decoding.
- A video frame repaints only what GPUI must repaint for any notifying view: the element and the views it is inside, once per frame (corrected in task 2.2, see the GPUI element section).
- Deterministic tests on CI's software renderer, with real hardware decoding checked locally.

**Non-Goals:**
- Any controls, input, `init(cx)`, poster or spinner (slice 5 and later).
- Windows and Linux video (slice 6+). HDR or EDR output, and 10-bit surfaces: GPUI cannot show them.
- Matching the CGL renderer to GPUI's Metal device on dual-GPU Intel Macs (see Risks).
- `MPV_RENDER_PARAM_ADVANCED_CONTROL`, frame-timing integration with GPUI's display link, and `report_swap` from actual presentation.

## Decisions

### NV12 through a GPU conversion pass

| Option | Verdict |
|---|---|
| **mpv → RGBA GL texture → GLSL pass → NV12 IOSurface → stock `surface()`** | **Chosen by the user.** Zero-copy, works with GPUI as published, and keeps GPUI out of `[patch]`. The cost is chroma at half resolution of the *screen-sized* picture, and one extra full-screen pass (two small draws), which is negligible next to mpv's own scaling. |
| BGRA support upstream in GPUI (Zed) and gpui-fast (Longbridge) | Full quality, but blocked on two third-party reviews and releases. Until then every consumer would need a `[patch]`. Left as a slice 6+ upgrade: `SurfaceFormat::Bgra` already exists, so the GPUI crate would only switch formats. |
| Native `CALayer` showing the IOSurface over the GPUI view | Full quality and the cheapest compositing, but it sits above GPUI's single Metal layer, so slice 5's control bar could not be drawn over the video. |
| mpv's `sw` render API into a CPU buffer, then upload | A CPU copy. This violates the project's zero-copy rule. |

The encode is the exact inverse of GPUI's decoder, so colours round-trip apart from quantisation and chroma subsampling. It is done in gamma-encoded RGB, as video YCbCr is:
- `Y = 0.299R + 0.587G + 0.114B`
- `Cb = (B − Y)/1.772 + ½`
- `Cr = (R − Y)/1.402 + ½`

The buffers carry `kCVImageBufferYCbCrMatrix_ITU_R_601_4` and the full-range pixel format, so any CoreVideo or AVFoundation consumer decodes them the same way.

### Public API (`player::macos`, `cfg(target_os = "macos")`)

```rust
#[non_exhaustive] pub enum SurfaceFormat { Bgra, Nv12 }
#[non_exhaustive] pub enum GlRenderer { Auto, Hardware, Software }   // Auto: hardware, else software

pub struct SurfaceRenderer { /* Send + Sync */ }
impl SurfaceRenderer {
    pub fn builder(player: &Player) -> SurfaceRendererBuilder;
    pub fn set_size(&self, width: u32, height: u32);   // device pixels; NV12 rounds odd sizes up
    pub fn latest(&self) -> Option<Frame>;             // never waits on mpv or GL
}
impl SurfaceRendererBuilder {
    pub fn format(self, SurfaceFormat) -> Self;        // default Bgra
    pub fn gl_renderer(self, GlRenderer) -> Self;      // default Auto
    pub fn on_frame(self, impl Fn() + Send + Sync + 'static) -> Self;   // called on the render thread
    pub fn build(self) -> Result<SurfaceRenderer>;     // Error::Render on failure
}

#[derive(Clone)] pub struct Frame { .. }
impl Frame {
    pub fn pixel_buffer(&self) -> &CFRetained<CVPixelBuffer>;   // objc2-core-video 0.3, IOSurface-backed
    pub fn width(&self) -> u32; pub fn height(&self) -> u32;
    pub fn format(&self) -> SurfaceFormat; pub fn sequence(&self) -> u64;
}

impl Player { pub fn ptr_eq(a: &Player, b: &Player) -> bool }   // all platforms
```

- **Safe because it owns the GL context.** The render thread creates the CGL context, makes it current, creates the `RenderContext` on it, uses it and drops it there. It is the only thread that ever touches GL, so slice 2's `unsafe` contract is upheld inside the module rather than passed on. `build()` waits for the thread's initialisation result over a one-shot channel, so errors (no `VideoOutput::Render`, a second renderer through the existing `ActiveFlag`, no CGL pixel format, mpv refusing the context) come back synchronously as `Error::Render`.
- **`on_frame` runs on the render thread, after publication, not on mpv's threads.** The host can therefore do more than signal (GPUI's sender is a `try_send` anyway), and "a frame is ready to read" holds when it is called.
- **`Frame` hands out `CFRetained<CVPixelBuffer>`.** This is the type GPUI's `paint_surface` takes, and AVFoundation and wgpu-hal importers start from it too. Using the same `objc2-core-video` 0.3 as GPUI makes it the identical type, with no conversion. The alternative, a raw `IOSurfaceRef` that GUI crates wrap, pushes `unsafe` into every frontend.
- **`GlRenderer::Auto`** lets the GPUI element work both on real Macs and on GPU-less CI VMs without configuration.

### The render thread

```
spawn: CGL context (current) → glow + RenderContext::new(loader, on_mpv_frame) → init result → loop
loop:  wait on Condvar<Signals { mpv_frame, size, shutdown }>
       shutdown → break
       render.update()                          // clears mpv's flag; a redraw also re-renders
       no size or zero size → continue
       size changed → rebuild the ring for the new size
       slot = ring[next]                         // 3 slots, round-robin
       Bgra: render.render({fbo: slot.fbo, w, h}, flip_y = false)
       Nv12: render.render({fbo: scratch.fbo, w, h}, false); convert(scratch.tex → slot.y_fbo, slot.cbcr_fbo)
       fence = glFenceSync; glClientWaitSync(fence, FLUSH_COMMANDS, 1 s); glDeleteSync
       latest.lock() = Some(Frame { slot.buffer.clone(), .., seq += 1 })   // lock held only for the swap
       render.report_swap(); on_frame()
exit:  drop RenderContext (GL current) → delete GL objects → CGLSetCurrentContext(null) → CGLDestroyContext
```

- **Signals** are a `Mutex<Signals>` plus a `Condvar`. mpv's update callback only sets `mpv_frame` and notifies, which obeys mpv's "signal only" rule. `set_size` sets the size, and `Drop` sets `shutdown`, notifies and joins the thread. The join is bounded: the only waits inside the loop are the 1 s fence wait and mpv's render call. Dropping a renderer while video plays therefore returns within a second (spec).
- **Orientation.** IOSurface row 0 is the top row for Metal and CoreVideo, and GL writes row 0 at the texture's bottom origin. So `flip_y = false` gives an upright picture, as slice 2's docs already say for Metal consumers. The NV12 pass maps texel rows 1:1, so it keeps that orientation.
- **The fence before publication** is what makes it safe for another API to read the surface. IOSurface does not synchronise GL with Metal. A CPU-side `glClientWaitSync` on the render thread costs no GUI-thread time.
- **`report_swap` after publication** approximates presentation. GPUI does not tell the element when its frame reached the screen, and mpv only uses this for timing statistics without `ADVANCED_CONTROL`.

### GL objects and IOSurface binding

- **CGL context**: `kCGLPFAOpenGLProfile = 3.2 Core`.
  - Hardware: `kCGLPFAAccelerated`, `kCGLPFAAllowOfflineRenderers` and `kCGLPFASupportsAutomaticGraphicsSwitching`, so a dual-GPU Intel Mac is not forced onto its discrete GPU.
  - Software: `kCGLPFARendererID = kCGLRendererGenericFloatID`.
  - `Auto` tries hardware, then software.
  - mpv's loader and glow's both resolve names with `dlsym` on the `OpenGL.framework` image (`dlopen` once per renderer), declared in a small `extern "C"` block.
- **Surfaces**: `CVPixelBufferCreate` with `kCVPixelBufferIOSurfacePropertiesKey = {}` and `kCVPixelBufferOpenGLCompatibilityKey`, in `32BGRA` or `420YpCbCr8BiPlanarFullRange`. Each buffer's IOSurface (`CVPixelBufferGetIOSurface`) is bound to `GL_TEXTURE_RECTANGLE` textures with `CGLTexImageIOSurface2D`:
  - BGRA: `GL_RGBA8` / `GL_BGRA` / `GL_UNSIGNED_INT_8_8_8_8_REV`, plane 0
  - NV12 Y: `GL_R8` / `GL_RED`, plane 0
  - NV12 CbCr: `GL_RG8` / `GL_RG`, plane 1, at half size

  Each texture is attached to its own FBO. mpv draws into a rectangle-texture FBO like any other FBO.
- **NV12 scratch target**: one `GL_RGBA8` 2D texture and FBO at the frame size. mpv dithers to the FBO's 8 bits as it does for a window.
- **Conversion program** (GLSL 150): a full-screen triangle with no vertex buffer. The Y pass writes `Y` per pixel. The CbCr pass runs at half size and samples the scratch texture with `GL_LINEAR` at the centre of each 2×2 block, which averages exactly those four texels (a box filter, as for MPEG-2/H.264 centre-sited chroma in an encoder).
- **The ring** holds 3 slots, each a `CVPixelBuffer` plus its textures and FBOs. It is rebuilt on a size change. Frames the host still holds keep their `CVPixelBuffer` (and IOSurface) alive through `CFRetained`, so they stay readable after a resize or after the renderer is dropped. A slot written for frame *n* is written again at *n + 3*, which gives the spec's "unchanged until two newer frames are published".

### Video when created after loading

After a successful `build()`, the builder selects the media's first video track again (`vid = <id>`), but only when all three hold: the media has a video track, `vid` reads `no`, and the `vid` option (`options/vid`) is not `no`. mpv re-initialises the video decoder and VO, now with a render context, while audio and the position continue.

This replaces the first plan of setting `vid` to `no` and back to its id. Task 1.5 found that the first plan never fires:
- When the VO fails to start because no render context exists, mpv deselects the video track itself, so `vid` already reads `no`, while `options/vid` keeps the user's `auto`.
- Setting `vid=auto` then has no effect.
- Setting the track id restarts the video with the position continuous.

Checking `options/vid` leaves video off when the application turned it off itself. This is done only in `SurfaceRenderer`, which makes the GPUI "load in the view constructor" pattern work. `RenderContext`'s own documented behaviour (slice 2's spec) is unchanged. Alternatives: a `loadfile ... replace` with `start=<pos>` (rejected: it interrupts audio and re-emits `Loaded`, which resets slice 3's started state), and documenting "create the element first" (rejected: GPUI views naturally load in `new`, before the first paint).

### hwdec default

In `PlayerBuilder::build`, on `cfg(target_os = "macos")` with `VideoOutput::Render`, `("hwdec", "videotoolbox")` joins the embedding defaults. These are applied before raw options, so `mpv_option("hwdec", ..)` overrides it.
- mpv's VideoToolbox GL interop maps each decoded `CVPixelBuffer`'s IOSurface into GL with `CGLTexImageIOSurface2D` on the current CGL context, so with the hardware renderer, decode → render → NV12 → Metal never touches the CPU.
- Where interop or decoder initialisation fails (a codec VideoToolbox lacks, or no VideoToolbox, as under Rosetta), mpv falls back to software decoding by itself. Apple's software renderer does support the interop on Apple Silicon (see the spike's outcome), so it is not a reliable way to force the fallback.
- `videotoolbox` is preferred over `auto-safe` because it names the one zero-copy method intended here, and it cannot pick a `-copy` variant.
- The same default applies to slice 2's host-FBO `RenderContext` on macOS. Its tests use the software renderer and pass through the fallback.

### `Player::ptr_eq`

`Arc::ptr_eq` on the inner handle. The GPUI element uses it to notice a different player passed under the same id. It is a plain public associated function, like `Arc::ptr_eq`, rather than `PartialEq`, because equality of players reads as comparing state.

### The GPUI element (raw GPUI)

GPUI-Kit has no video or surface component, and `surface()`, `canvas` and `paint_surface` are core GPUI. So the element uses `gpui_kit::*` (which is GPUI) directly. GPUI-Kit components come in slice 5's control bar.

```rust
#[derive(IntoElement)]
pub struct VideoPlayer { id: ElementId, player: Player, style: StyleRefinement }
impl VideoPlayer { pub fn new(id: impl Into<ElementId>, player: &Player) -> Self }
impl Styled for VideoPlayer {}         // size, position, etc. like any div
impl RenderOnce for VideoPlayer { .. }
```

```
render(self, window, cx):
  slot  = window.use_keyed_state(id, cx, |_, cx| Slot { view: cx.new(|cx| VideoView::new(&player, cx)) })
  if !Player::ptr_eq(slot.view.player, &self.player) → replace slot.view with a new VideoView (old one drops)
  div().id(id).bg(black).refine(style).child(AnyView::from(slot.view).cached(size_full))

VideoView (Render):
  renderer: SurfaceRenderer (Nv12, GlRenderer::Auto), on_frame = tx.try_send(())  // flume::bounded(1)
  _frames: Task — cx.spawn: while rx.recv_async().await.is_ok() { this.update(cx, |_, cx| cx.notify()) }
  render → canvas(
     prepaint: |bounds, window| renderer.set_size(bounds.size * window.scale_factor(), rounded),
     paint:    |bounds, window| if let Some(f) = renderer.latest() {
                   window.paint_surface(ObjectFit::Contain.get_bounds(bounds, f.size), f.pixel_buffer().clone()) })
```

- **Why a `Slot` holding a separate `VideoView` entity.** Replacing the view entity is how a different player under the same id gets a fresh renderer, and `AnyView::cached` keeps the view from re-rendering when the parent re-renders for its own reasons.
- **Per-frame cost (corrected in task 2.2).** The first plan was that per frame only `VideoView` re-renders, with the parent's cached output reused. That does not hold in GPUI 0.3.10. `cx.notify()` marks every ancestor view dirty (`Window::mark_view_dirty`), and GPUI has no paint-only invalidation. So each frame re-renders every view from the window root down to the element, even a parent shown with `.cached()`. In the harness, a cached parent rendered 39 times for 38 frames.
  - This is GPUI's cost for any animated view.
  - Notifying through the keyed entity instead would cost the same.
  - **Decided by the user:** accept it. The harness test checks that the parent re-renders at most once per published frame, that its state survives, and that the video view repaints per frame. The crate docs tell apps to keep the views around a video element cheap, and to show expensive siblings as cached views, which a frame does not re-render.
- **Sizing at prepaint.** Bounds are known there, and `set_size` only stores a value and signals the render thread. Until a frame of the new size arrives, the previous frame is painted with `ObjectFit::Contain`, so a resize never stretches the picture. An NV12 frame one device pixel larger than the bounds (odd sizes) is fitted the same way.
- **No repaint when idle.** Only `on_frame` notifies, and it fires only when mpv renders or the size changes. A paused video therefore does not keep the window busy (spec).
- **Errors.** If `build()` fails (for example the player was not built with `VideoOutput::Render`), `VideoView` keeps no renderer, paints black, and logs the error once with `log::warn!`, as GPUI itself logs.
- **Windows and Linux.** `VideoPlayer` exists with the same API and renders the black `div` only, without `VideoView` or a renderer, so app code is portable and audio plays.
- **Dropping.** When the id is not rendered in a frame, GPUI releases the keyed `Slot`, then `VideoView`, which drops the renderer. That stops the thread and leaves the player playing without video (spec).

### Dependencies

| Crate | Where | Why, and alternatives |
|---|---|---|
| `glow` 0.16 | `player`, macOS runtime (already a dev-dependency) | Typed GL calls over a loader, used by egui. The conversion pass, FBOs and fences need about 30 GL functions. Alternatives: hand-written `extern "C"` declarations (more unsafe surface to maintain) and the `gl` crate (process-global function pointers). |
| `objc2-core-foundation`, `objc2-core-video`, `objc2-io-surface` 0.3 | `player`, macOS | The exact crates and versions GPUI uses, so `CFRetained<CVPixelBuffer>` is the same type. Alternatives: `core-video-sys` and `io-surface` (older servo crates whose types do not match GPUI's). |
| CGL | `player`, macOS | About 8 functions in a local `extern "C"` block, as the test helper already has. The `cgl` crate (0.3) is unmaintained and pulls in `core-foundation` 0.9, so it was rejected. |
| `gpui-kit` 0.8, `default-features = false` | `gpui` | The project's GUI toolkit. Without default features it is GPUI and the platform only. The app's own `gpui-kit` features (components, assets, `gpui-fast`) unify with it, so the element follows whichever backend the app picked. The app must use the same `gpui-kit` minor version. |
| `flume`, `log` | `gpui` | `flume` is already the core's event channel and has `recv_async` for GPUI's executor. `log` is GPUI's logging facade. |

`rust-sak`: none of its modules (`crypto`, `fetch`, `fs`, `github`, `image`, `memo`, `o11y`, `sysinfo`, `tui`) covers graphics, GL, IOSurface or GPUI, and a mpv-to-IOSurface bridge is not general-purpose infrastructure. No `rust-sak` feature is used and nothing is added to it.

### Backend / UI separation

Everything platform- and GL-specific is in `crates/player`, behind `cfg(target_os = "macos")`, and it has no GUI types: `SurfaceRenderer` deals in sizes, formats and `CVPixelBuffer`s. `crates/gpui` contains no GL, no CGL, no libmpv and no `unsafe`. It sizes, subscribes and paints. A future egui-wgpu frontend would build a `SurfaceRenderer` with `SurfaceFormat::Bgra`, import `frame.pixel_buffer()`'s IOSurface as a Metal texture through wgpu-hal, and reuse the core unchanged.

### FFI, threading and lifetimes

- **`unsafe` lives in `crates/player/src/macos/`.** It covers the CGL calls, `dlopen`/`dlsym`, `CGLTexImageIOSurface2D`, the CoreVideo buffer creation and attachments, glow's `unsafe` GL calls, and the one `RenderContext::new` call. Each block has a `// SAFETY:` note, and all of it runs on the render thread except buffer creation, which is thread-safe in CoreVideo.
- **Threads.**
  - mpv's threads only set a flag and notify a condvar.
  - The render thread owns every GL object and the `RenderContext`.
  - Any thread may call `set_size` and `latest`, which take a mutex held only for a field swap.
  - In GPUI, the frame task runs on the foreground executor, and `canvas` closures run during GPUI's frame on the main thread.
- **Lifetimes.**
  - `SurfaceRenderer` → the render thread → the `RenderContext` → `Arc<Handle>`, so mpv outlives the renderer whatever the drop order (slice 2).
  - `Frame`s are independent `CFRetained` references.
  - GPUI's scene holds its own `CFRetained` clone until the next frame replaces it.

### Zero-copy

- **Hardware path**: VideoToolbox decodes to IOSurfaces, which mpv's interop maps into GL. mpv scales and converts into the scratch texture (or straight into the BGRA IOSurface), and the GLSL pass writes the NV12 IOSurface. GPUI then samples it through `CVMetalTextureCache`, which is IOSurface-backed and does not copy. All of this happens on the GPU.
- **Software decoding** (the fallback) uploads decoded frames inside mpv, as slice 2 noted. That is mpv's normal path, not a copy this crate adds.
- **CPU readback** (`CVPixelBufferLockBaseAddress`) appears only in tests, to check pixels.

### Testing

- **`crates/player/tests/surface.rs`** (`#![cfg(target_os = "macos")]`) uses `GlRenderer::Software` by default. `PLAYER_TEST_GL=hardware` switches it to `Hardware`, following the existing convention. It covers:
  - creation errors
  - no frames without a size
  - frames at the set size, and the odd-size NV12 rounding
  - a resize while paused
  - a black frame with nothing loaded
  - the BGRA test pattern, upright (top/bottom asymmetry as in slice 2)
  - NV12 colours: a `lavfi` `hstack` of solid red, green, blue, white and grey fields, decoded on the CPU with GPUI's matrix and compared with the BGRA frame within 3%
  - NV12 orientation
  - letterboxing
  - a renderer created after load
  - dropping in both orders
  - a frame outliving its renderer
  - `on_frame` not called after drop
- **hwdec.** A committed fixture, `tests/fixtures/testsrc-h264.mp4` (1 s, 160×120 H.264, about 20 KB, with the `ffmpeg` command recorded beside it), backs two tests:
  - the software fallback test, which runs everywhere
  - the VideoToolbox test, which asserts `hwdec-current == "videotoolbox"` and runs only with `PLAYER_TEST_GL=hardware` (locally on Apple Silicon)
- **Builder tests** (all platforms): the hwdec default, an explicit override, and `Null` keeping `no`. Plus `Player::ptr_eq`.
- **`crates/gpui`** uses GPUI's test harness (`gpui-kit` `test-support`, dev-only) with a headless window and a software-GL `Player`. Test-only accessors in `#[cfg(test)]` let it check that:
  - the state survives parent re-renders (the same renderer sequence keeps advancing)
  - a different player under the same id switches
  - removing the element releases the renderer (a new `SurfaceRenderer` for that player then builds)
  - prepaint forwards bounds × scale factor
  - on Windows and Linux the element builds and paints black

  Visual checks (upright, colours, letterboxing on screen, resize, idle repaint, low CPU with a 4K HEVC file) are manual, with `examples/play.rs`, because a test window has no Metal drawable.

### CI

- **Linux**: GPUI needs extra `-dev` packages to build (fontconfig, freetype, xkbcommon-x11, xcb). These are added to `LINUX_DEV_PACKAGES`, which the lint job also uses, and the exact list is settled by the first CI run.
- **macOS**: the legs run `tests/surface.rs` on the software renderer, arm64 natively and x64 under Rosetta.
- **Windows**: both legs build `crates/gpui` and run its non-macOS tests.

### Outcome of the spike (task 1.1)

A throwaway test (since deleted) ran on a local M2 Max and on the CI macOS legs, on the `spike/add-player-4-iosurface` branch (also deleted). Each leg created a 3.2 core CGL context, bound IOSurface-backed `CVPixelBuffer`s with `CGLTexImageIOSurface2D` (32BGRA plane 0 as `GL_RGBA8`/`GL_BGRA`; NV12 plane 0 as `GL_R8` and plane 1 as `GL_RG8` at half size) to rectangle-texture FBOs, cleared them and read them back with `CVPixelBufferLockBaseAddress`. It then played a 160×120 H.264 file through mpv with `hwdec=videotoolbox` and read `hwdec-current`.

| Leg | Software CGL: FBOs and readback | Software CGL: `hwdec-current` | Hardware CGL |
|---|---|---|---|
| Local, Apple Silicon (M2 Max) | complete, exact | `videotoolbox` | FBOs complete, readback exact, `hwdec-current = videotoolbox` |
| CI macOS arm64 | complete, exact | `videotoolbox` | no pixel format (`kCGLBadPixelFormat`, 10002) |
| CI macOS x64 (Rosetta) | complete, exact | `no` (software decoding) | no pixel format (10002) |

Decisions that follow:
- The Risks entry's fallback is not needed. The surface tests run on both CI legs with the software renderer, without a skip.
- The CI VMs expose no accelerated CGL renderer, so `GlRenderer::Hardware` fails there and `GlRenderer::Auto` falls back to software, as designed.
- Apple's software renderer supports mpv's VideoToolbox interop on Apple Silicon. So the "software renderer fallback" test checks only that frames are published, whichever decoder is active (as the spec says). It sees VideoToolbox on arm64 and software decoding on x64. Only the `PLAYER_TEST_GL=hardware` test asserts `videotoolbox`.

### Apple's software renderer (found in task 1.6)

Two quirks surfaced once the surface tests ran on Apple's software renderer. Neither appears on the GPU.
- **Fences.** The software renderer signals a fence before its writes have reached the IOSurface's memory. A CPU or Metal reader can then see a stale or empty surface. When the context is the software renderer, the render thread calls `glFinish` instead of waiting on a fence. On the GPU it keeps the fenced wait. The CGL context records which renderer `Auto` ended up on.
- **Scaling.** mpv's default scalers sample lookup-table textures, and the software renderer draws scaled video with them as solid black (sometimes, depending on what was drawn before). Slice 2's tests never scaled, so they never saw this. With `scale=bilinear` it is reliable, and bilinear is also the only scaling a CPU renderer can afford. **Decided by the user:** when a surface renderer ends up on the software renderer, `build()` sets the player's `scale`, `dscale` and `cscale` to `bilinear`. This is documented on `GlRenderer::Software` and on the builder. It overrides a scaler the application chose on that player, but only on the software path.

### Change notes: manual verification (task 2.3)

Run with `examples/play.rs` (debug build) on a local M2 Max, on a Retina display.
- **Upright, colours.** A 640x480 `testsrc` clip, compared with FFmpeg's decoded reference frame of the same file (mpv is not installed on this machine): same bar order and colours, gradient band at the bottom.
- **Letterboxing.** The 4:3 clip is pillarboxed in the 16:9 window, and letterboxed after resizing the window to 500x700 points.
- **Sharp on Retina.** The window is 1000x1400 device pixels at scale 2, and frames are requested at bounds x scale factor (checked by the harness test).
- **4K HEVC.** A 3840x2160 HEVC clip at 30 fps reported `hwdec-current=videotoolbox`. The whole process used 22-29% of one core, in a debug build.
- **Idle.** While paused, the window re-rendered 0 times per second. While playing, it re-rendered once per frame (about 31 per second for 30 fps).
- **Loaded before the first frame.** Media with audio showed video. Media **without audio** first stayed black: its load ended at once with `MPV_ERROR_NOTHING_TO_PLAY`, because no render context existed yet. **Decided by the user:** the player now records whether a load was made without a render context, and a surface renderer loads such media again once its video has failed. Either the event thread or `build()` does it, whichever learns last of the other's side; the event thread uses only asynchronous mpv calls. Retested: the video-only clip now shows video.
- **Closing.** Closing the window while paused exited the process in 0.2 s.

### Change notes: CI (task 3.3)

The manual `test` workflow ran on branch `add-player-4-gpui-macos` (run 40768239195), and the lint job and all six legs passed.
- **macOS arm64 and x64:** `tests/surface.rs` ran all 24 tests on Apple's software renderer, and `player-gpui` ran its 7 harness tests. On x64 under Rosetta, GPUI's harness also passed.
- **Linux x64 and arm64, Windows x64 and arm64:** `crates/gpui` built, with the added Linux packages, and ran its 2 tests: the link test, and the black element on platforms without surfaces. `tests/surface.rs` compiles to no tests there, as designed.
- No leg-specific skip was needed. Windows arm64 keeps the existing skip of OpenGL tests from slice 2, which this change's tests do not depend on.

## Risks / Trade-offs

- [Apple's software renderer may not support `CGLTexImageIOSurface2D`, or IOSurface FBOs, on the CI VMs] → Task 1.1 is a spike on a CI macOS leg before anything depends on it. If the software renderer cannot bind IOSurfaces, the macOS legs try `GlRenderer::Hardware` (the arm64 VMs expose a paravirtual GPU). If neither works there, the surface tests skip with a message on CI, like Windows arm64 in slice 2, and the gap is recorded here and in the README, with local runs covering them.
- [The GPU may still sample a ring slot when it is overwritten three frames later (tearing)] → Publication is fenced, and GPUI paints only the newest frame. At video rates up to the display rate, a slot is reused only after at least two display frames, by which time GPUI's command buffer for it has completed. The ring size is one constant, so it can grow if manual testing shows tearing at high frame rates.
- [On dual-GPU Intel Macs, the CGL context may run on the integrated GPU while GPUI's Metal device is the discrete one, and the driver then migrates the IOSurface between GPUs] → Recorded as a known gap, untested (no such hardware). The fix belongs to a later slice: pick the CGL virtual screen whose renderer matches GPUI's `MTLDevice` registry id. Apple Silicon has one GPU, so it is unaffected.
- [NV12 softens fine coloured detail and caps output at SDR 8-bit] → This was the user's choice over waiting on upstream. `SurfaceFormat::Bgra` is ready for when GPUI accepts it (slice 6+), and the trade-off is in the proposal and the crate docs.
- [The `vid` re-select could glitch on some media (a brief black frame, or a seek to a keyframe)] → It happens only when a renderer is created after load. The spec allows up to a 1 s position change, and the test checks the video appears and playback continues.
- [`AnyView::cached` may not isolate per-frame re-renders the way the design expects in this GPUI version] → Task 2.2 checks it with a render counter on the parent view in the harness test. The fallback is to notify through the keyed entity: correct but costlier, and documented. **Outcome:** it does not isolate them (see "Per-frame cost" above). The ancestors re-render once per frame, and this is documented.
- [`gpui-kit` minor-version coupling: an app on 0.7 or 0.9 gets two GPUI copies, and the types do not match] → The README states the required `gpui-kit` version, and `player-gpui` releases track it.
- [GPUI's `assert_eq!` on the pixel format changes in a future version] → Only `SurfaceFormat::Nv12` is ever passed. A GPUI upgrade that adds BGRA would be an opportunity, not a break.
- [Building GPUI on Windows arm64 or Linux arm64 CI fails] → The first CI run (task 3.3) shows it. If a leg cannot build GPUI, `crates/gpui` is excluded on that leg, and the gap is recorded.

## Migration Plan

Additive, except for one behaviour change: macOS players built with `VideoOutput::Render` now hardware-decode by default. Apps that need software decoding pass `mpv_option("hwdec", "no")`. Nothing else in the public API changes.
