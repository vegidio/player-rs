## Why

`player::Player` can load and control media, but it can only play audio. Its only video output is `VideoOutput::Null`, so no frontend can show a picture. Every frontend needs the same toolkit-agnostic way to have mpv draw video frames into a GPU target it owns. That way the zero-copy path (mpv decodes, scales, converts and tone-maps on the GPU, then draws into our texture) is written once in the core crate, and each GUI crate only puts that texture on screen. This is slice 2 of the `add-player` series. It delivers the render API on top of a GL context the host provides. Slice 4 (GPUI on macOS) builds on it.

## What Changes

- Add `VideoOutput::Render`, which builds the player with mpv's `vo=libmpv`, so a render context created by the app draws the video instead of mpv opening a window.
- Add `player::RenderContext`, a safe wrapper over `mpv_render_context` with the OpenGL backend. Its single `unsafe` constructor takes the player, the host's GL function loader (`get_proc_address`) and a frame-ready callback. Its contract is that the host's GL context is current whenever the render context is used or dropped.
- Add rendering into any FBO the host owns (`RenderTarget { fbo, width, height }`, optional vertical flip), `update()` to ask mpv whether a new frame is ready, and `report_swap()` for mpv's frame timing.
- Add the frame-ready callback. mpv calls it from its own threads whenever a new frame or a redraw is due, and it must only signal the render thread.
- Add the video's display size to the playback state (`State.video_size`) and a matching `Event::VideoSizeChanged`, so frontends can size their render target and keep the aspect ratio.
- Add `Error::Render` for render context failures: a player built without `VideoOutput::Render`, a second render context for the same player, or mpv rejecting the GL context.
- Change what dropping the last `Player` handle does: playback now always stops, and if a `RenderContext` is still alive, the mpv instance is released when that context is dropped, because mpv requires the render context to be freed before the core is destroyed.
- Run the render tests on all six CI legs with software OpenGL (Mesa on Linux and Windows, Apple's software renderer on macOS).

## Capabilities

### New Capabilities

- `video-render`: drawing a player's video into a host-owned OpenGL framebuffer. Covers render context creation and its preconditions, frame-ready notification, rendering and frame timing, lifetimes relative to the player, and platform coverage.

### Modified Capabilities

- `playback-core`:
  - "Player creation with validated options" gains the render video output.
  - "Shareable player handle" changes what dropping the last handle does while a render context is alive.
  - "Playback state snapshot" and "Event subscription" gain the video display size and its change event.

## Impact

- **Crates**: only `crates/player`. Driving `mpv_render_context`, the frame callback and FBO rendering are the same for every frontend (GPUI, a future egui frontend over glow, or anything else with a GL context), so they belong in the core crate, which stays free of any GUI dependency. Creating an offscreen GL context and sharing its texture with a toolkit's renderer (IOSurface, `WGL_NV_DX_interop`, dma-buf) is not part of this slice. It is platform-specific and arrives with the per-platform frontend slices. `crates/gpui` is untouched.
- **libmpv APIs**: `mpv_render_context_create` (`MPV_RENDER_PARAM_API_TYPE` = `MPV_RENDER_API_TYPE_OPENGL`, `MPV_RENDER_PARAM_OPENGL_INIT_PARAMS`), `mpv_render_context_set_update_callback`, `mpv_render_context_update` (`MPV_RENDER_UPDATE_FRAME`), `mpv_render_context_render` (`MPV_RENDER_PARAM_OPENGL_FBO`, `MPV_RENDER_PARAM_FLIP_Y`), `mpv_render_context_report_swap`, `mpv_render_context_free`. Option `vo=libmpv`. Observed properties `dwidth`, `dheight`. Command `stop` on drop. All of these are in the committed bindings already. `MPV_RENDER_PARAM_ADVANCED_CONTROL` is deliberately not set (see design.md).
- **Public API**: new `VideoOutput::Render`, `RenderContext`, `RenderTarget`, `State.video_size`, `Event::VideoSizeChanged` and `Error::Render`. All of them are additions to `#[non_exhaustive]` types or new types, so nothing breaks.
- **Dependencies**: none at runtime. Dev-only: `glutin` (headless GL contexts in tests) and `glow` (FBO setup and pixel readback in tests), justified in design.md.
- **rust-sak**: none of its modules (`crypto`, `fetch`, `fs`, `memo`, `o11y`, `sysinfo`) touches graphics, GL or mpv, so nothing in it covers this change, and nothing belongs there: a GL render wrapper is specific to this project. Nothing is added to `rust-sak`.
- **CI**: `.github/workflows/test.yml` installs software OpenGL on each leg: Mesa EGL packages on Linux, Mesa's `opengl32.dll` from `pal1000/mesa-dist-win` next to the test binaries on Windows, and nothing extra on macOS, which uses its built-in software renderer.
- **Platforms**: the API and its tests work on macOS, Windows and Linux with any GL context the host provides. Shown on screen in a real app, it still depends on the per-platform frontend slices: macOS first (slice 4), Windows and Linux later. Hardware decoding is not turned on by this slice. Which zero-copy hwdec interop works depends on the GL context the frontend creates, so it is enabled and checked per platform in those slices. Until then, `mpv_option("hwdec", ...)` already works.
- **Size**: 12 tasks, slice 2 of the `add-player` series (see design.md).
