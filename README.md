# player-rs

A fast, user-friendly video player for Rust, built on [libmpv](https://mpv.io). The goal is to play anything mpv can play, 4K/HDR included, smoothly on modest hardware, and eventually to expose every mpv feature through an idiomatic Rust API.

libmpv and everything it depends on (FFmpeg, libplacebo, libass, …) are linked **statically** from the prebuilt archives of [vegidio/binaries-mpv](https://github.com/vegidio/binaries-mpv). Programs built with `player-rs` need neither mpv nor FFmpeg installed.

> **Status:** early. `player-rs` plays media and draws its video into an OpenGL framebuffer the application provides, on every supported platform (rendering on Windows arm64 is not verified by CI, see [Building](#building)). Its toolkit-agnostic UI layer (`player::ui`) implements the Video.js control behaviour, ready for a frontend to draw. Showing video in a GPUI window, and the GPUI control bar itself, come in later changes.

## Workspace

| Path            | Package       | Library       | Purpose                                                                                                       |
|-----------------|---------------|---------------|---------------------------------------------------------------------------------------------------------------|
| `crates/player` | `player-rs`   | `player`      | Core crate: downloads and links libmpv, raw FFI bindings (`player::sys`), the safe API. No GUI dependency.    |
| `crates/gpui`   | `player-gpui` | `player_gpui` | The player UI for [GPUI](https://gpui.rs) apps (placeholder for now). Goes through `player-rs`, never libmpv. |
| `xtask`         | `xtask`       | -             | Dev-only tooling (not published): regenerates the FFI bindings.                                               |

## Usage

```rust
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

```rust
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

### The UI layer

`player::ui` holds every Video.js interaction rule that is not drawing, so each GUI frontend only translates its input and draws the result, and all of them behave the same. It depends on no GUI toolkit.

```rust
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

- **macOS:** Apple's software renderer, built in. Set `PLAYER_TEST_GL=hardware` to run them on the GPU instead.
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
