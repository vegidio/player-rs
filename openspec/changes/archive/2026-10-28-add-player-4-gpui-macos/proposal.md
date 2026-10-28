## Why

`player-rs` can draw video into an OpenGL framebuffer the host owns (slice 2), but no GUI can show it yet: GPUI renders with Metal and has no GL context to offer. This is slice 4 of the `add-player` series. It is the first time video reaches the screen. On macOS, mpv draws each frame on the GPU into an IOSurface, and a GPUI element composites that surface into the window without the frame ever passing through the CPU. VideoToolbox hardware decoding is on by default, so 4K plays smoothly on modest Macs.

A finding changed the plan in the series table. GPUI's `surface()` element accepts only NV12 `CVPixelBuffer`s: the Metal renderer in both backends GPUI-Kit 0.8 can use (`gpui-pre` 0.3.9 and `gpui-fast` 0.1.10) asserts `kCVPixelFormatType_420YpCbCr8BiPlanarFullRange` and converts with a fixed BT.601 full-range matrix, so it would panic on mpv's BGRA output. The user chose to convert on the GPU. mpv draws RGBA, and a small GL pass writes it into an NV12 IOSurface, which is still zero-copy and works with stock GPUI. The core also offers a BGRA surface, for toolkits that accept one.

## What Changes

- **`crates/player`, new macOS-only module `player::macos`** (toolkit-agnostic, no GPUI types):
  - `SurfaceRenderer`, a safe type that owns a dedicated render thread with:
    - an offscreen CGL context (hardware, or Apple's software renderer on request)
    - slice 2's `RenderContext` on that context
    - a small pool of IOSurface-backed `CVPixelBuffer`s
  - It renders each frame mpv announces at the size the host sets. It publishes the newest frame, which any thread can read, and calls a host callback after each one.
  - `SurfaceFormat::Bgra`: mpv draws straight into the IOSurface.
  - `SurfaceFormat::Nv12`: mpv draws into a GL texture, and a GPU pass converts it to BT.601 full-range NV12 with 4:2:0 chroma. That is what GPUI's `surface()` needs.
  - The frame is fenced on the GPU before it is published, so a consumer never samples a half-drawn frame.
  - A renderer created after media was loaded re-selects the video track, so video appears without reloading. This closes slice 2's "load before render context" trap for this path.
  - All `unsafe` stays inside the module: CGL, IOSurface binding and the `RenderContext` contract are upheld by the render thread itself, so `SurfaceRenderer` has no `unsafe` API.
- **`crates/player`, playback-core additions**:
  - On macOS, a player built with `VideoOutput::Render` defaults to `hwdec=videotoolbox`. That is zero-copy: mpv's VideoToolbox/CGL interop maps decoded IOSurfaces into GL. A raw `mpv_option("hwdec", ...)` overrides it, and mpv falls back to software decoding on its own when VideoToolbox cannot decode a file.
  - `Player::ptr_eq`, which tells whether two handles control the same player.
- **`crates/gpui`, new `player_gpui::VideoPlayer` element**, built with `VideoPlayer::new(id, &player)` and styled like any GPUI element:
  - Its state (the `SurfaceRenderer` and the frame subscription) is keyed by element id. It lives while the element is rendered in consecutive frames, and it is rebuilt if a different player is passed under the same id.
  - It draws the newest NV12 frame with GPUI's `paint_surface`, letterboxed on black, at the element's size in device pixels.
  - It repaints when a frame is published.
  - On Windows and Linux it paints black (audio still plays) until slice 6+.
  - This slice adds no controls, no input handling and no `init(cx)`; those come in slice 5.
- **`crates/gpui`, example `examples/play.rs`**: a GPUI-Kit window playing a file, used for manual verification on Apple Silicon.
- **CI**: Linux packages GPUI needs to build. The macOS legs run the surface tests on Apple's software renderer, where VideoToolbox is not available, so hwdec is checked locally on hardware.

## Capabilities

### New Capabilities

- `video-surface`: rendering a player's video into GPU surfaces another graphics API can sample (on macOS: IOSurface-backed `CVPixelBuffer`s in BGRA or NV12) on a render thread the core owns. Covers creation and preconditions, sizing, publishing the latest frame, frame notification, formats and colour encoding, orientation, hardware decoding, lifetimes and platform coverage. Windows and Linux are added to it by later slices.
- `gpui-player`: the GPUI video element: showing the player's video in a GPUI window, how it fits its bounds, repaint on new frames, state keyed by element id, and platform coverage.

### Modified Capabilities

- `playback-core`:
  - "Player creation with validated options" gains the macOS hardware-decoding default for the render output.
  - "Shareable player handle" gains the same-player identity check.

## Impact

- **Crates**: both.
  - `crates/player` gets everything that is not GPUI: the CGL context, the IOSurface pool, the NV12 pass, the render thread and the hwdec default. A future egui-wgpu or other Metal-based frontend imports the same surfaces (the BGRA format exists for those), and GUI crates never touch GL or libmpv.
  - `crates/gpui` gets only the element: keyed state, the bridge from frame notifications to repaint, sizing from layout bounds, and the `paint_surface` call.
- **libmpv APIs**: no new render API calls (slice 2's `RenderContext` is reused as is).
  - New option default: `hwdec=videotoolbox`.
  - Properties read in tests and docs: `hwdec-current`, `vid` (set once to re-select the video track when a renderer is created after load).
- **Apple APIs** (all macOS-only, behind `cfg(target_os = "macos")`): CGL (`CGLChoosePixelFormat`, `CGLCreateContext`, `CGLSetCurrentContext`, `CGLTexImageIOSurface2D`, `CGLDestroyContext`), CoreVideo (`CVPixelBufferCreate` with `kCVPixelBufferIOSurfacePropertiesKey`, `CVPixelBufferGetIOSurface`, YCbCr matrix attachments) and IOSurface. The GL functions are called through `glow`.
- **Public API**:
  - New `player::macos` module (`SurfaceRenderer`, its builder, `SurfaceFormat`, `Frame`) and `Player::ptr_eq`.
  - New `player_gpui::VideoPlayer`.
  - Behaviour change: macOS Render players now hardware-decode by default.
  - `Frame` exposes `CFRetained<CVPixelBuffer>` from `objc2-core-video` 0.3, which makes that crate part of the core's macOS public API, matching what GPUI uses.
- **Dependencies**:
  - `crates/player`, macOS only: `glow` (moves from dev to runtime there), `objc2-core-foundation`, `objc2-core-video`, `objc2-io-surface`.
  - `crates/gpui`: `gpui-kit` 0.8 (`default-features = false`) and `flume`.
  - Justified in design.md.
- **rust-sak**: nothing in it covers this. Its modules are `crypto`, `fetch`, `fs`, `github`, `image`, `memo`, `o11y`, `sysinfo` and `tui`. None touches GL, Metal, IOSurface or GPUI, and a GL/IOSurface bridge for mpv is specific to this project, so nothing belongs there. Nothing is added to `rust-sak`.
- **Platforms**: video on screen is **macOS only** in this slice.
  - On Windows and Linux, `player::macos` does not exist and `VideoPlayer` paints black while audio plays. Slice 6+ adds their surfaces.
  - `crates/gpui` must still build and pass its tests on all six CI legs.
  - macOS x64 is covered by CI under Rosetta with the software renderer. VideoToolbox is verified locally on Apple Silicon only.
- **Known quality trade-off**: NV12 halves chroma resolution at screen size. Fine coloured detail (thin red text, subtitle outlines) is slightly softer than mpv's RGBA output. The video is SDR 8-bit, because GPUI's Metal layer is `BGRA8Unorm`, so mpv tone-maps HDR to SDR. A later slice can lift both if GPUI gains BGRA or HDR surfaces.
- **Size**: 13 tasks. This is slice 4 of the `add-player` series (see design.md). It depends on slice 2. Slice 3 is done but not needed here.
