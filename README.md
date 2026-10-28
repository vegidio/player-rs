# player-rs

A fast, user-friendly video player for Rust, built on [libmpv](https://mpv.io). The goal is to play anything mpv can play, 4K/HDR included, smoothly on modest hardware, and eventually to expose every mpv feature through an idiomatic Rust API.

libmpv and everything it depends on (FFmpeg, libplacebo, libass, …) are linked **statically** from the prebuilt archives of [vegidio/binaries-mpv](https://github.com/vegidio/binaries-mpv). Programs built with `player-rs` need neither mpv nor FFmpeg installed.

> **Status:** early. `player-rs` plays media and draws its video into an OpenGL framebuffer the application provides, on every supported platform (rendering on Windows arm64 is not verified by CI, see [Building](#building)). On macOS it also renders into IOSurfaces that Metal toolkits can show without a copy. `player-gpui` shows the video in a GPUI window with Video.js's controls, from the toolkit-agnostic `player::ui` layer. Video in GPUI on Windows and Linux, and the Video.js menus, captions and poster, come in later changes.

## Workspace

| Path            | Package       | Library       | Purpose                                                                                                       |
|-----------------|---------------|---------------|---------------------------------------------------------------------------------------------------------------|
| `crates/player` | `player-rs`   | `player`      | Core crate: downloads and links libmpv, raw FFI bindings (`player::sys`), the safe API. No GUI dependency.    |
| `crates/gpui`   | `player-gpui` | `player_gpui` | The player UI for [GPUI](https://gpui.rs) apps: the `VideoPlayer` element, with Video.js's controls. Goes through `player-rs`, never libmpv. |
| `xtask`         | `xtask`       | -             | Dev-only tooling (not published): regenerates the FFI bindings.                                               |

## Usage

```rust,no_run
use player::prelude::*;

fn main() -> player::Result<()> {
    let player = Player::builder().autoplay(Autoplay::On).volume(0.8).build()?;
    let events = player.subscribe();
    player.load(Source::file("movie.mp4")?)?;

    for event in events {
        match event {
            Event::Loaded => println!("duration: {:?}", player.state().duration),
            Event::Ended(reason) => {
                println!("ended: {reason:?}");
                break;
            }
            _ => {}
        }
    }
    Ok(())
}
```

`Player` is a cheap, cloneable handle usable from any thread. It loads media (`Source::url`, `Source::file`), controls playback (`play`, `pause`, `toggle_pause`, `stop`, `seek`, `scrub` for fast keyframe seeks, `set_volume`, `set_muted`, `set_rate`), keeps a `state()` snapshot (including the buffered position, from mpv's demuxer cache), and sends `Event`s to any number of subscribers, which can block on them or await them in any async executor. Anything mpv offers that has no typed method yet is reachable through `get_property`, `set_property` and `command`.

Behaviour follows [Video.js](https://videojs.org) where the two differ:

- **End of media:** media that plays to its end stays loaded and paused there (mpv's `keep-open=yes`) and reports `Event::Ended(EndReason::Eof)`; `play()` then restarts it from the beginning. To get mpv's default instead (unload at the end), build with `.mpv_option("keep-open", "no")`.
- **Autoplay** (`Off`, `On`, `Muted`) applies to every `load`.
- **Volume** is `0.0..=1.0`. The builder rejects other values; the runtime setters clamp them, as does the playback rate (`0.01..=100.0`).

### Rendering video

Build the player with `VideoOutput::Render` and create a `RenderContext` from your toolkit's OpenGL context. mpv then draws each frame into a framebuffer you own, doing decoding, scaling, colour conversion and subtitles on the GPU, without copying frames through the CPU:

```rust,ignore
let player = Player::builder().video_output(VideoOutput::Render).build()?;
// SAFETY: the GL context is current on this thread, where `render` is used and dropped, and outlives it.
let mut render = unsafe { RenderContext::new(&player, get_proc_address, move || wake_render_thread())? };
player.load(Source::file("movie.mp4")?)?;

// On the render thread, after each wake-up:
if render.update() {
    render.render(RenderTarget { fbo, width, height }, flip_y)?;
}
```

- `RenderContext::new` is `unsafe` because the crate cannot check the GL context: it must be current whenever the render context is created, used or dropped, and must outlive it. OpenGL 2.1+ or OpenGL ES 2.0+.
- The frame callback runs on mpv's threads and must only wake the render thread.
- Create the render context before the first `load`; media loaded before it plays without video.
- `state().video_size` and `Event::VideoSizeChanged` give the video's display size. The video is letterboxed into the target with black bars.
- The render context keeps mpv alive, so it and the `Player` can be dropped in either order. Dropping the last `Player` stops playback.

On macOS, `VideoOutput::Render` players also use VideoToolbox hardware decoding by default (`mpv_option("hwdec", "no")` turns it off), and `player::macos::SurfaceRenderer` serves toolkits that draw with Metal. It owns a render thread and an offscreen OpenGL context, and publishes each frame as an IOSurface-backed `CVPixelBuffer` (BGRA, or NV12 for GPUI), with no `unsafe` code and no copy through the CPU:

```rust,ignore
let renderer = SurfaceRenderer::builder(&player).format(SurfaceFormat::Nv12).on_frame(|| request_repaint()).build()?;
renderer.set_size(width_px, height_px);
// When painting: renderer.latest() -> Option<Frame>, with frame.pixel_buffer()
```

Media loaded before the surface renderer exists still shows its video. NV12 halves chroma resolution at screen size and, like BGRA, is 8-bit SDR (mpv tone-maps HDR). A Mac without a GPU (a virtual machine) falls back to Apple's software OpenGL renderer, where the player is switched to bilinear scaling.

### GPUI

`player-gpui` adds the `VideoPlayer` element, a video player with [Video.js](https://videojs.org)'s controls, to apps built with [GPUI-Kit](https://gpui-kit.com) **0.8.1** or a later 0.8 (the app must use the same `gpui-kit` minor version). The app calls `gpui_kit::init(cx)`, which the controls' tooltips and spinner need, and registers `player_gpui::Assets`, which serves the controls' icons and falls back to GPUI-Kit's default icons. An app with an asset source of its own composes `player_gpui::Icons`, the player's extra icons alone, into it instead.

```rust,no_run
use gpui_kit::*;
use player::prelude::*;
use player::ui;
use player_gpui::VideoPlayer;

struct Movie {
    player: Player,
    options: ui::Options,
}

impl Render for Movie {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(
            VideoPlayer::new("movie", &self.player)
                .options(self.options.clone())
                .on_fullscreen_change(|on, _, _| println!("fullscreen: {on}"))
                .size_full(),
        )
    }
}

fn main() {
    gpui_kit::application().with_assets(player_gpui::Assets).run(|cx| {
        gpui_kit::init(cx);
        let player = Player::builder().video_output(VideoOutput::Render).build().expect("the player builds");
        player.load(Source::file("movie.mp4").expect("the file exists")).expect("the file loads");
        let options = ui::Options::builder().playback_rates([0.5, 1.0, 1.5, 2.0]).build().expect("valid options");
        gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| Movie { player, options }))
            .expect("the window opens");
    });
}
```

- **Video** is shown upright and letterboxed on black, at the element's size in device pixels. It repaints for each new frame and not while paused. Video is shown on **macOS only** for now; on Windows and Linux the element is black while audio plays and the controls work.
- **Controls** are Video.js 8's default skin, drawn from `player::ui`: the control bar (play, mute with a volume bar that expands on hover, seek bar with buffered part and hover time, remaining time, a playback-rate button when rates are set, fullscreen), the centred big play button, the loading spinner and the error display. The bar fades out after the inactivity timeout while playing. Clicking the video toggles play and double-clicking toggles fullscreen.
- **Keyboard**: clicking the player focuses it, and Tab moves through the controls. The `player::ui` hotkeys and seek/volume-bar keys apply as configured; keys the player does not handle, and every key held with Ctrl, Alt or Cmd, go on to the app. The `player_gpui::TogglePlay`, `ToggleMute` and `ToggleFullscreen` actions let app menus drive the focused player.
- **Controls are on by default.** `.options(...)` takes `player::ui::Options`; for bare video, pass `ui::Options::builder().controls(false).build()?`. As in Video.js, options are read once: when the element's state is created and when it is given a different player. With an aspect ratio set, the element's height follows its width (Video.js's fluid mode).
- **Fullscreen is the window's.** A GPUI element cannot fill the screen alone, so the fullscreen control, `f`, double-click and Escape (to leave) toggle the window's fullscreen. `.on_fullscreen_change(...)` tells the app, including when the window leaves fullscreen by other means, so it can show the player alone while fullscreen.
- **State** is kept per element id, so re-rendering the parent does not restart the video. Each video frame, and each change of the controls (the seek bar moves a pixel at a time), re-renders the views the element is in, as for any repainting GPUI view, so keep those views cheap.
- **Not yet:** the cursor stays visible while the user is inactive (GPUI has no way to hide it on demand), and the rate button cycles the rates instead of opening Video.js's menu. Menus (rates, tracks), captions and the poster come in later changes.

Try it with:

```sh
cargo run -p player-gpui --example play -- path/to/movie.mp4
```

### The UI layer

`player::ui` holds every Video.js interaction rule that is not drawing, so each GUI frontend only translates its input and draws the result, and all of them behave the same. It depends on no GUI toolkit.

```rust,ignore
use player::ui::{self, Focus, Key, Modifiers};

let options = ui::Options::builder().playback_rates([0.5, 1.0, 1.5, 2.0]).poster("poster.jpg").build()?;
let mut controls = ui::Controls::new(&player, options, Instant::now());

// Forward every event from the frontend's own subscription:
controls.handle_event(&event, Instant::now());
// Report input:
controls.pointer_moved(now);
let outcome = controls.key(Key::Char('k'), Modifiers::NONE, Focus::Player, now)?; // outcome.handled
controls.seek_press(0.1, now)?; controls.seek_drag(0.5, now)?; controls.seek_release(0.5, now)?;
// Draw everything it describes, and tick again at `view.next_deadline`:
let view = controls.view(now);
```

- **Options** follow Video.js (`controls`, `poster`, `playbackRates`, `aspectRatio`, `inactivityTimeout`, `userActions`), validated when built.
- **The view** says what to show (control bar, cursor, big play button, poster, delayed loading spinner, error), the play/pause/replay icon, time labels as Video.js writes them, seek-bar fractions with a hover preview, the volume icon level and the rate label.
- **Control-bar hiding**: hidden before playback starts and after the inactivity timeout while playing; hovering the control bar or holding a button keeps it shown. Fade timings are `ui::FADE_IN` and `ui::FADE_OUT`.
- **Seek-bar drags** pause, scrub with fast seeks, land exactly on release and resume, and never end the media by reaching its end.
- **Keys**: the Video.js seek-bar and volume-bar keys, and the global hotkeys `f` (fullscreen), `m` (mute) and `k`/Space (play). Unlike Video.js, the hotkeys are on by default (`ui::Hotkeys::disabled()` turns them off) and keys held with Ctrl, Alt or Cmd are left to the application.
- **Fullscreen** is returned as a `ui::Request`, since only the frontend controls its window.

Play a file or URL from the command line:

```sh
cargo run -p player-rs --example play -- path/to/movie.mp4
```

## Supported platforms

| OS             | x64 | arm64 |
|----------------|-----|-------|
| macOS          | ✓  | ✓    |
| Linux (glibc)  | ✓  | ✓    |
| Windows (MSVC) | ✓  | ✓    |

Windows GNU/MinGW targets and other architectures are not supported; the build fails with an error naming the unsupported OS, architecture or environment.

## Building

Requirements: a stable Rust toolchain (edition 2024) and network access on the first build. `bindgen`/`libclang` are **not** needed: the bindings are committed.

```sh
cargo build
cargo test --workspace --exclude xtask
cargo run -p player-rs --example version
cargo run -p player-rs --example play -- path/to/movie.mp4
```

The tests need no media files, display, GPU or sound device: they play media mpv generates itself (`av://lavfi:testsrc`) or WAV files they write to a temporary directory, with mpv's null audio output. The render tests (`tests/render.rs`) draw into an offscreen framebuffer of a headless, software-rendered OpenGL context:

- **macOS:** Apple's software renderer, built in. Set `PLAYER_TEST_GL=hardware` to run them on the GPU instead. The surface tests (`tests/surface.rs`) and the `player-gpui` tests follow the same setting, and the VideoToolbox test runs only with it.
- **Linux:** Mesa's EGL and llvmpipe (`sudo apt-get install libegl1 libegl-mesa0 libgl1-mesa-dri`; set `LIBGL_ALWAYS_SOFTWARE=1` to use llvmpipe on a machine with a GPU).
- **Windows:** the system's OpenGL is version 1.1 without a GPU driver, too old for mpv. CI copies Mesa's `opengl32.dll` and `libgallium_wgl.dll` from [mesa-dist-win](https://github.com/pal1000/mesa-dist-win) next to the test binaries (`target/<triple>/debug/deps`) and sets `GALLIUM_DRIVER=llvmpipe`. Only the tests use them; applications keep using the system's OpenGL.
- **Windows arm64:** no software OpenGL exists (mesa-dist-win builds x64 and x86 only), so CI skips the GL tests there by setting `PLAYER_TEST_SKIP_GL=1`. Rendering on Windows arm64 is therefore not verified by CI; it needs a machine with a GPU driver.

On the first build `crates/player/build.rs` downloads the `binaries-mpv` release pinned in [`crates/player/mpv-version.txt`](crates/player/mpv-version.txt) for the target (35-70 MB) and caches it in Cargo's `OUT_DIR`; later builds reuse it. The link line is derived from the archive's own `lib/pkgconfig/*.pc` files, so no `pkg-config` binary is needed.

### Linux

The Linux archive leaves the system display and audio stacks and the Vulkan loader dynamic. Install their development packages (Debian/Ubuntu names):

```sh
sudo apt-get install libx11-dev libxext-dev libxfixes-dev libxrandr-dev libxss-dev libxpresent-dev \
    libx11-xcb-dev libxcb-dri3-dev libxkbcommon-dev libwayland-dev wayland-protocols libegl-dev \
    libgbm-dev libdrm-dev \
    libdisplay-info-dev libvdpau-dev libpulse-dev libpipewire-0.3-dev libvulkan-dev
```

`build.rs` reads their `.pc` files from `PKG_CONFIG_PATH`, then `PKG_CONFIG_LIBDIR` or the standard system directories. A missing one fails the build with the name of the module to install.

### macOS

Nothing to install; binaries depend only on system frameworks and libraries.

For **x86_64** macOS, set the deployment target to at least 10.15, the minimum the x86_64 libmpv is built for. Rust's default for that target is 10.12, which would link mpv's Swift runtime through `@rpath` and produce binaries that fail to start, so `build.rs` refuses to build without it. Add this to your app's `.cargo/config.toml` (this repository already does):

```toml
[env]
MACOSX_DEPLOYMENT_TARGET = "10.15"
```

Apple Silicon builds are unaffected.

### Windows

Nothing to install. Binaries depend only on Windows system DLLs and the Visual C++ runtime; no DLL has to ship next to the executable.

### Offline or custom builds: `PLAYER_BINARIES_DIR`

Set `PLAYER_BINARIES_DIR` to a directory laid out like an extracted release archive (with `include/` and `lib/`) to build without downloading anything:

```sh
PLAYER_BINARIES_DIR=/path/to/static_osx_arm64 cargo build
```

## Updating libmpv

1. Change the version in `crates/player/mpv-version.txt`.
2. Regenerate the bindings (needs `libclang`; one run on any host is enough, the headers have no platform types):

   ```sh
   cargo run -p xtask -- bindings
   ```

3. Run the tests. `tests/link.rs` fails if the linked libmpv and the committed bindings disagree.

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
