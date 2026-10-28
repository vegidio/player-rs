## Context

Slice 1 (`add-player-1-core`, archived) delivered `player::Player`: an `Arc<Inner>` handle holding `Arc<Handle>` (the `mpv_handle` owner, whose `Drop` calls `mpv_terminate_destroy`), the event thread, the `State`/`Event` model fed by observed properties, and a builder whose `VideoOutput` is `#[non_exhaustive]` with only `Null`. The committed bindings already contain `render.h` and `render_gl.h`. All three platforms' static libmpv include the OpenGL render backend: macOS links `-framework OpenGL`, Windows `opengl32`, Linux `egl`. See proposal.md for why this slice exists, and `specs/video-render/spec.md` and the `playback-core` delta for the required behaviour.

Rules from `render.h` that shape the design:
- Only one `mpv_render_*` call at a time per context.
- The context's GL context must be current on the calling thread for every call, including free.
- No `mpv_render_*` call from inside the update callback, and the update callback must not call any mpv API.
- The render thread must not wait on a libmpv call that is not render-safe. Without `MPV_RENDER_PARAM_ADVANCED_CONTROL`, breaking this only degrades playback (timeouts). With it, it deadlocks.
- `mpv_render_context_free` must run before `mpv_terminate_destroy`.
- `get_proc_address` has no documented call window, so the loader must stay valid for the context's whole life.

## Slices

The `add-player` series. **This change is slice 2.**

| # | Change | Delivers | Depends on | Status |
|---|---|---|---|---|
| 1 | `add-player-1-core` | `player::Player` handle: builder, sources, load, playback control, property/command access, state snapshot, event subscription; headless CI tests | `add-workspace-scaffold` | done |
| 2 | `add-player-2-render` | mpv OpenGL render API in core over a host-provided GL context: `VideoOutput::Render`, `RenderContext` (loader, frame-ready callback, render into host FBO, `report_swap`), `State.video_size`; software-GL render tests on all six CI legs | 1 | **current** |
| 3 | `add-player-3-ui-core` | `player::ui`, toolkit-agnostic UI layer: `ui::Options` (controls, poster, playback rates, aspect ratio, hotkeys, idle timeout), `ui::Controls`, `Action` enum, Video.js hotkey map, time formatting, idle-hide state machine, seek-bar maths | 1 | pending |
| 4 | `add-player-4-gpui-macos` | macOS video on screen: in core, an offscreen CGL context and an IOSurface-backed GL texture/FBO that slice 2's `RenderContext` draws into (toolkit-agnostic, reusable by e.g. egui-wgpu); in `crates/gpui`, the `player_gpui::VideoPlayer` element importing the IOSurface into GPUI, UI state keyed by element id; zero-copy `hwdec=videotoolbox` enabled and verified | 2 | pending |
| 5 | `add-player-5-gpui-controls` | Video.js control bar in GPUI and `player_gpui::init(cx)` (actions and key bindings), driven by slice 3 | 3, 4 | pending |
| 6+ | (to be planned) | Windows and Linux video surfaces (offscreen WGL/EGL context and texture sharing in core, import in GUI crates, after a spike on GPUI surface support), per-platform zero-copy hwdec, menus (rate, tracks), captions, fullscreen, poster, ... | 4, 5 | pending |

Slices 2 and 3 do not depend on each other. Compared with slice 1's table, slice 2 is narrowed to a host-provided GL context: creating offscreen contexts is platform code and moves to slice 4 (macOS) and 6+ (Windows, Linux), where each platform's texture sharing lives anyway.

## Goals / Non-Goals

**Goals:**
- A render API that any frontend with, or able to create, a GL context uses unchanged: GPUI through slice 4's offscreen context, egui-glow directly with its own context.
- Make the mpv render rules either impossible to break through the safe API or written down as one explicit `unsafe` contract.
- Render behaviour (frames notified, pixels drawn, lifetimes) verified on all six CI legs without GPUs.

**Non-Goals:**
- Creating GL contexts, windows or shareable textures (IOSurface, DX interop, dma-buf). Slices 4 and 6+.
- Hardware decoding defaults (`hwdec`). Which zero-copy interop works depends on the GL context the frontend creates, so it is decided per platform in slices 4 and 6+.
- `MPV_RENDER_PARAM_ADVANCED_CONTROL`, direct rendering (`vd-lavc-dr`), `BLOCK_FOR_TARGET_TIME`, `NEXT_FRAME_INFO`, `X11_DISPLAY`/`WL_DISPLAY` (VAAPI interop) and the software (`sw`) render API.
- A windowed example. It would need a windowing crate in the core crate's dev-dependencies, and the tests already exercise every path.

## Decisions

### Public API

```rust
#[non_exhaustive] pub enum VideoOutput { Null, Render }      // Render => vo=libmpv

pub struct RenderTarget { pub fbo: u32, pub width: u32, pub height: u32 }   // fbo 0 = default framebuffer

pub struct RenderContext { /* Send, !Sync */ }
impl RenderContext {
    /// # Safety
    /// The GL context that `get_proc_address` resolves functions for must be current on the calling thread
    /// for this call, for every later method call and for the drop, and must outlive the RenderContext.
    pub unsafe fn new(
        player: &Player,
        get_proc_address: impl FnMut(&CStr) -> *const c_void + Send + 'static,
        on_frame: impl Fn() + Send + Sync + 'static,
    ) -> Result<RenderContext>;
    pub fn update(&mut self) -> bool;                                  // MPV_RENDER_UPDATE_FRAME set
    pub fn render(&mut self, target: RenderTarget, flip_y: bool) -> Result<()>;
    pub fn report_swap(&mut self);
}

State { .., pub video_size: Option<(u32, u32)> }   Event::VideoSizeChanged(Option<(u32, u32)>)
Error::Render(String)
```

- **One `unsafe` constructor carries the GL rules.** The core crate cannot check whether a GL context is current, and calling mpv with the wrong one is undefined behaviour, so this has to be a stated `unsafe` contract. Putting it only on `new` (as `glow::Context::from_loader_function` does) keeps `render`, `update` and `report_swap` safe to call. The alternative, making every method `unsafe`, adds noise without adding safety, because the obligation is the same each time.
- **`&mut self` and `Send` but not `Sync`.** This gives "one `mpv_render_*` call at a time" at compile time, and still lets a frontend create the context on a dedicated render thread or move it there.
- **The callback is passed to the constructor, not set later.** That removes the question of whether a replaced callback can still be running on an mpv thread. The closure is boxed and passed as `cb_ctx`. It is freed only after `mpv_render_context_free` returns, after which mpv sends no further callbacks. A panic inside it is caught (`catch_unwind`) and dropped, so it cannot unwind into C.
- **The loader is boxed and kept for the context's lifetime**, because mpv does not document when it calls it. The C trampoline converts the name to `&CStr` and returns the pointer.
- **`update()` returns `bool`, not a flags type.** mpv defines a single flag, `MPV_RENDER_UPDATE_FRAME`. A `#[non_exhaustive]` struct can come later if mpv adds more.
- `render` passes `MPV_RENDER_PARAM_OPENGL_FBO` (`internal_format = 0`, meaning unknown, which mpv accepts) and `MPV_RENDER_PARAM_FLIP_Y`. A zero width or height is rejected with `Error::Render` before mpv is called. Black bars and aspect ratio come from mpv's defaults (`keepaspect=yes`, black background).

### Preconditions and the single-context rule

`new` checks that the player was built with `VideoOutput::Render`. `Inner` records the output choice, and otherwise `new` returns `Error::Render("player was not built with VideoOutput::Render")`. A `render_active: AtomicBool` in `Inner` (compare-exchange in `new`, cleared in `Drop`) rejects a second context with a clear message, so the caller does not get mpv's generic error. mpv itself also refuses a second context. Any `mpv_render_context_create` failure becomes `Error::Render` with `mpv_error_string`'s message, for example when the GL version is too old.

### Lifetimes and drop order

```
 Player ──> Arc<Inner> { mpv: Arc<Handle>, output, render_active: Arc<AtomicBool>, .. }
                                  │                 │
 RenderContext { ctx, mpv: Arc<Handle>, render_active: Arc<AtomicBool>, loader: Box<..>, callback: Box<..> }
```

- `RenderContext` holds its own `Arc<Handle>`, so the core can never be destroyed before the render context is freed, whatever order the app drops things in. Its `Drop` calls `mpv_render_context_free` (GL current, per the contract), then drops the callback and loader boxes, clears the single-context flag, and finally drops its `Arc<Handle>`. If that is the last reference, `mpv_terminate_destroy` runs.
- The flag the context clears lives in a small `Arc<AtomicBool>` shared with `Inner`, not in `Inner` itself, so the context does not keep the event thread and subscriber state alive.
- **Stopping playback on the last `Player` drop.** Before slice 2, `mpv_terminate_destroy` in `Inner`'s drop stopped playback. When a `RenderContext` outlives the player, that destroy is postponed. To keep the old behaviour, `Inner::drop` now sends `stop` with `mpv_command_async` before it stops the event thread. It is asynchronous so the drop can never wait on the core, which matters when the last `Player` is dropped on the render thread. This is the modified "Shareable player handle" requirement.
- A context dropped while video plays makes mpv disable video forcefully. Audio and control keep working.
- Media loaded with no context gets no video: `vo_libmpv` fails to initialise without a render context, and mpv plays the file without video. A later context does not bring video back until the media is loaded again. Frontends therefore create the context before the first `load`, as the docs say.

### Threading model

| Thread | What runs there |
|---|---|
| mpv's internal threads | `on_frame` (must only signal: wake a GUI loop, notify a condvar, send on a channel) |
| App render thread (GL current) | `RenderContext::new`, `update`, `render`, `report_swap`, drop |
| Any thread | `Player` methods, `state()`, `subscribe()` |

Without `ADVANCED_CONTROL`, a render thread that also calls blocking `Player` methods (egui-glow, where the UI thread renders) only risks degraded timing, not a deadlock. This is the reason not to set it in this slice. The docs recommend a dedicated render thread and, in render paths, `state()`, which never touches mpv. `ADVANCED_CONTROL` can come later as an opt-in once a frontend can guarantee the rules.

### Video display size

`dwidth` and `dheight` are observed like the existing numeric properties (requested as `MPV_FORMAT_DOUBLE`, which mpv converts from int64). They are the display size with pixel aspect ratio and rotation applied, which is what a frontend needs to size its target and lay out the player. `State.video_size` is `Some((w, h))` only when both are known and positive. `VideoSizeChanged` is emitted when the combined value changes, so a change of both dimensions emits one event, not two. When no video is shown (audio-only media, or no context), mpv leaves them unavailable, so the field is `None`. mpv does report them with `vo=null`, so a `Null` player does not observe them at all and its field stays `None`. This sits in `crates/player` because every frontend needs it and none of it depends on a toolkit.

### FFI layout

All new `unsafe` code goes in a private `mpv::render` submodule, next to the existing `mpv` module:
- `mpv_render_param` arrays built on the stack and terminated by `MPV_RENDER_PARAM_INVALID`.
- The two `extern "C"` trampolines (loader and update callback).
- Wrappers for create, update, render, report_swap and free.

Every block has a `// SAFETY:` comment. `RenderContext`'s raw `*mut mpv_render_context` gets a documented `unsafe impl Send` and no `Sync`.

### Zero-copy

mpv draws straight into the host's FBO on the GPU. The crate never maps or reads pixels. The only readback in this change is `glReadPixels` inside the tests, to check what was drawn. That is test-only and so does not count against the zero-copy rule. Software decoding (the default until hwdec is enabled per platform) uploads decoded frames to GL inside mpv. This is mpv's normal path, not a copy this crate adds.

### Test infrastructure: `glutin` + `glow` (dev-only)

| Option | Verdict |
|---|---|
| **`glutin` 0.32** | **Chosen.** Headless contexts through CGL (macOS, no window), EGL (Linux, device or surfaceless platform, which Mesa's llvmpipe supports) and WGL (Windows, via a hidden window created with `windows-sys`, which glutin already pulls in). It is maintained by the Rust windowing community and needs no winit, which matters because winit's macOS event loop must own the main thread and the tests do not. |
| `surfman` | Covers the same platforms, but is heavier and focused on Servo. |
| Raw CGL/EGL/WGL FFI in a test helper | No dependency, but three hand-written platform helpers to maintain. This is the fallback if the spike shows glutin cannot make a context headless on some leg. |

**Outcome of the spike (task 1.1):** glutin's CGL backend cannot select Apple's software renderer: its pixel format attributes offer only an optional "accelerated" flag, never `kCGLPFARendererID`. So macOS uses the raw FFI fallback: about 50 lines of CGL (`CGLChoosePixelFormat` with `kCGLRendererGenericFloatID`, or the GPU when `PLAYER_TEST_GL=hardware`) in `tests/common/gl.rs`. glutin stays a dev-dependency only for Linux and Windows.

`glow` 0.16 (also used by egui) sets up the offscreen FBO and texture and reads the pixels back. `rust-sak` has nothing for graphics, so there is nothing to extend there. Both crates are dev-dependencies only, so consumers never see them.

Pixel checks compare against properties of `lavfi testsrc` (several distinct colours, black letterbox bars, top/bottom asymmetry for the flip), not exact images, so different software rasterisers cannot make them flaky. All waits use timeouts on channels fed by `on_frame` and `Events`, never sleeps.

### CI: software OpenGL on every leg

- **Linux x64/arm64**: install Mesa's EGL and llvmpipe (`libegl1 libegl-mesa0 libgl1-mesa-dri`) and set `LIBGL_ALWAYS_SOFTWARE=1`.
- **Windows x64/arm64**: download Mesa's `opengl32.dll` and `libgallium_wgl.dll` from a pinned `pal1000/mesa-dist-win` release and copy them next to the test binaries (`target/<triple>/debug/deps`). Windows looks for DLLs in the executable's folder first, and `opengl32.dll` is not a KnownDLL. The system's GDI renderer only offers GL 1.1, which mpv cannot use. These DLLs replace the test binary's GL for CI only. Shipped apps keep importing only system DLLs, as `libmpv-build` requires.
- **macOS arm64/x64**: no setup. The test helper asks CGL for Apple's software renderer (`kCGLRendererGenericFloatID`), since GitHub's macOS VMs may expose no usable GPU. Locally the helper also runs once with the hardware renderer.

### Backend / UI separation

Everything is in `crates/player`, which still has no GUI dependency, and the API uses only GL concepts (a loader and an FBO name), no toolkit types. A future egui frontend over glow passes egui's loader and its framebuffer and needs nothing else. A GPUI or egui-wgpu frontend gets a GL context and texture from slice 4's core code and passes them in the same way. GPUI-Kit vs raw GPUI does not apply: no UI in this slice.

## Risks / Trade-offs

- [glutin cannot make a headless context on some CI leg (for example WGL without a window, or Rosetta CGL)] → Task 1.1 is a spike that proves contexts on every leg before the render code depends on them. The fallback is a small raw FFI helper for that platform. If a leg cannot get software GL at all, its render tests are skipped with a message and the gap is written down in the README and in this design.
- [`pal1000/mesa-dist-win` has no build for Windows arm64, or its DLLs do not load under the test runner] → The same spike checks this. The fallbacks are the x64 DLLs under Windows-on-Arm emulation, or skipping the GL tests on that one leg and recording it as a documented gap.
- [Apple's software renderer is slow, and mpv's default shaders on llvmpipe could make tests slow] → The tests use 160x120 media and only a few frames. The test player sets `profile=fast` through a raw option if timings need it.
- [A frontend's `on_frame` that does real work, or calls `Player` methods, deadlocks or stutters mpv] → It is documented on `new` and in the crate docs, with a channel or condvar example. The signature cannot forbid it.
- [The render thread calling blocking `Player` methods degrades frame timing] → It is documented, and it is the reason `ADVANCED_CONTROL` stays off. `state()` is the render-path API.
- [Load before the context gives no video until reload] → The scenario is in the spec and the docs say to create the context first. A later slice can re-trigger video automatically (for example `vid` cycling) if a frontend needs it.
- [`stop` on the last `Player` drop changes behaviour for apps that relied on an instance outliving its handles] → No such app can exist: before this change, the instance never outlived its handles.

## Migration Plan

Not applicable. Everything is added. The only behaviour change, playback stopping on the last `Player` drop even if the core lives on, matches what slice 1 already did.
