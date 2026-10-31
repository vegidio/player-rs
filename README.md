# player-rs

A fast, native video player for Rust apps. Drop a `VideoPlayer` element into your [GPUI](https://gpui.rs) window and get a full player with the look and behaviour of [Video.js](https://videojs.org): control bar, seek bar, volume, playback rates, fullscreen and keyboard shortcuts. It plays anything [mpv](https://mpv.io) can play, 4K/HDR included, with GPU decoding and no frame copies through the CPU.

## 💡 Motivation

Showing video in a native Rust GUI usually means one of two bad options: shelling out to an external player, or decoding frames on the CPU and uploading them to a texture every frame, which falls over at 4K.

`player-rs` takes a third route:

- **mpv does the hard part.** Decoding, scaling, colour conversion and subtitles all run on the GPU, through libmpv's render API, with hardware decoding on by default (VideoToolbox, NVDEC, VA-API).
- **Zero-copy into the UI.** Frames go straight from mpv to the toolkit as GPU surfaces: IOSurfaces on macOS, shared Direct3D 11 textures on Windows, dma-bufs on Linux.
- **Nothing to install.** libmpv and everything it depends on (FFmpeg, libplacebo, libass, …) are linked **statically** from the prebuilt archives of [vegidio/binaries-mpv](https://github.com/vegidio/binaries-mpv). Apps built with `player-rs` need neither mpv nor FFmpeg on the user's machine.

The project is split into two crates:

| Package       | Import as     | What it is                                                                                                          |
|---------------|---------------|---------------------------------------------------------------------------------------------------------------------|
| `player-gpui` | `player_gpui` | The `VideoPlayer` element for GPUI apps, with Video.js's controls. **Start here.**                                  |
| `player-rs`   | `player`      | The toolkit-agnostic core: the `Player` handle, video rendering, and the `player::ui` controls logic. No GUI dependency. |

## ⬇️ Installation

```bash
$ cargo add player-rs player-gpui --git https://github.com/vegidio/player-rs
$ cargo add gpui-kit@0.8
```

`player-gpui` is built on [GPUI-Kit](https://gpui-kit.com) **0.8.1** or a later 0.8. Your app must use the same `gpui-kit` minor version, otherwise it ends up with two copies of GPUI whose types don't match.

On **macOS** that's all. On **Windows** and **Linux**, GPUI must also be patched to draw video surfaces; see [Patched GPUI](#patched-gpui).

The first build downloads the prebuilt libmpv for your target (35-70 MB) and caches it; later builds reuse it. See [Building](#building) for the per-platform requirements.

## 🎬 Playing a video

```rust,no_run
use gpui_kit::*;
use player::prelude::*;
use player_gpui::VideoPlayer;

struct Movie {
    player: Player,
}

impl Render for Movie {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(VideoPlayer::new("movie", &self.player).size_full())
    }
}

fn main() {
    // Registers the controls' icons, with GPUI-Kit's default icons behind them.
    gpui_kit::application().with_assets(player_gpui::Assets).run(|cx| {
        // The controls' tooltips and spinner need GPUI-Kit's theme.
        gpui_kit::init(cx);

        let player = Player::builder().autoplay(Autoplay::On).build().expect("the player builds");
        player.load(Source::file("movie.mp4").expect("the file exists")).expect("the file loads");

        gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| Movie { player }))
            .expect("the window opens");
    });
}
```

That's a complete app. The three things it needs are:

1. **`player_gpui::Assets`** registered on the application, so the control icons show. An app with an asset source of its own composes `player_gpui::Icons` into it instead.
2. **`gpui_kit::init(cx)`** before a player is shown.
3. **A `Player`** that loads media from a file (`Source::file`) or a URL (`Source::url`). It may load before the element is first rendered.

The element's state is kept per id (`"movie"` above), so re-rendering the parent doesn't restart the video.

Try it on your own file:

```bash
$ cargo run -p player-gpui --example play -- path/to/movie.mp4
```

### Customising the controls

`VideoPlayer::skin` chooses how the controls look, and `VideoPlayer::options` takes a `player::ui::Options`, which mirrors Video.js's options (`poster`, `playbackRates`, `aspectRatio`, `inactivityTimeout`, `userActions`, and per-component options such as `controlBar: { volumePanel: false }`):

```rust,ignore
use player::ui::{self, Parts};
use player_gpui::{Skin, VideoPlayer};

let options = ui::Options::builder()
    .playback_rates([0.5, 1.0, 1.5, 2.0])
    .parts(Parts::default().volume(false))
    .build()?;

VideoPlayer::new("movie", &self.player)
    .skin(Skin::Default)
    .options(options)
    .on_fullscreen_change(|on, _, _| println!("fullscreen: {on}"))
    .size_full()
```

- **Skins.** `Skin::Default`, the default, draws the player's controls. `Skin::None` draws nothing over the video: no controls, overlays, spinner or error display.
- **Every part is visible by default.** `Parts` hides any of them: play, volume, seek bar, current time, time display, fullscreen, Speed, big play button, spinner, error display and seek hover time. The Settings button shows while at least one of its items (today only Speed) does.
- **Speed is offered by default**, with the rates 0.25x to 3x in steps of 0.25. Pass `playback_rates([])` or `Parts::default().speed(false)` to offer none.
- **The time display** shows the remaining time; clicking it switches to the duration and back.
- **Clicks and keys don't depend on the skin.** They follow the `click`, `double_click` and `hotkeys` options under every skin, `Skin::None` included.
- **Options and skin are read once**, as in Video.js: when the element's state is created and when it's given a different player.
- **Aspect ratio.** With one set, the element's height follows its width (Video.js's fluid mode).

#### Migrating from `controls(false)`

`ui::Options::builder().controls(false)` is gone. For bare video, use `VideoPlayer::skin(Skin::None)`. `controls(false)` also turned off clicks and keys; to keep that, add the options for it:

```rust,ignore
let options = ui::Options::builder()
    .click(false)
    .double_click(false)
    .hotkeys(ui::Hotkeys::disabled())
    .build()?;

VideoPlayer::new("movie", &self.player).skin(Skin::None).options(options)
```

### Mouse and keyboard

| Input                    | Action                                     |
|--------------------------|--------------------------------------------|
| Click the video          | Play / pause                               |
| Double-click the video   | Toggle fullscreen                          |
| `k` or `Space`           | Play / pause                               |
| `m`                      | Mute / unmute                              |
| `f`                      | Toggle fullscreen                          |
| `Escape`                 | Leave fullscreen                           |
| `Tab`                    | Move through the controls                  |
| Arrow keys on a bar      | Seek, or change the volume                 |

Clicking the player focuses it, and the hotkeys work from then on. Keys the player doesn't handle, and every key held with Ctrl, Alt or Cmd, go on to your app. The `player_gpui::TogglePlay`, `ToggleMute` and `ToggleFullscreen` actions let your app's menus drive the focused player.

**Fullscreen is the window's.** A GPUI element can't fill the screen on its own, so the player toggles its window's fullscreen. `.on_fullscreen_change(...)` tells your app, including when the window leaves fullscreen by other means, so it can show the player alone while fullscreen.

### Controlling playback from code

`Player` is a cheap, cloneable handle usable from any thread, so the rest of your app can drive the same player the element shows:

```rust,ignore
player.pause()?;
player.seek(Seek::To(Duration::from_secs(90)))?;
player.set_volume(0.5)?;
player.set_rate(1.5)?;

let state = player.state(); // position, duration, buffered, paused, video_size, …

for event in player.subscribe() {
    if let Event::Ended(reason) = event {
        println!("ended: {reason:?}");
    }
}
```

- **Methods:** `play`, `pause`, `toggle_pause`, `stop`, `seek`, `scrub` (fast keyframe seeks), `set_volume`, `set_muted`, `set_rate`.
- **Events** can be blocked on or awaited in any async executor, by any number of subscribers.
- **Anything else mpv offers** is reachable through `get_property`, `set_property` and `command`.

Behaviour follows Video.js where it differs from mpv's:

- **End of media:** media that plays to its end stays loaded and paused there, and `play()` restarts it from the beginning. For mpv's default (unload at the end), build with `.mpv_option("keep-open", "no")`.
- **Autoplay** (`Off`, `On`, `Muted`) applies to every `load`.
- **Volume** is `0.0..=1.0`, and the playback rate `0.01..=100.0`. The builder rejects other values; the runtime setters clamp them.

## 🪟 Platform notes

| OS      | Video in `VideoPlayer`                                                                      | Hardware decoding             |
|---------|---------------------------------------------------------------------------------------------|-------------------------------|
| macOS   | ✓                                                                                           | VideoToolbox                  |
| Windows | With the [patched GPUI](#patched-gpui)                                                      | `auto-safe` (NVDEC on NVIDIA) |
| Linux   | With the [patched GPUI](#patched-gpui), a DRM render node and a driver that imports dma-bufs | `vaapi,nvdec`                 |

Wherever video can't be shown, the element is black, one warning is logged, and audio and the controls keep working. Hardware decoding can be changed with `.mpv_option("hwdec", ...)`.

### Patched GPUI

The GPUI that GPUI-Kit 0.8 builds on can't draw a texture from another device on Windows, or a dma-buf on Linux. Until it can, video on those platforms needs a fork of three of its crates, [`vegidio/gpui-pre`](https://github.com/vegidio/gpui-pre), which adds that and nothing else.

Add this to your **workspace root** `Cargo.toml` (Cargo only applies the root's patches, so `player-gpui` can't do it for you):

```toml
[patch.crates-io]
gpui-pre = { git = "https://github.com/vegidio/gpui-pre", tag = "0.3.10-surfaces.2" }
gpui-pre-windows = { git = "https://github.com/vegidio/gpui-pre", tag = "0.3.10-surfaces.2" }
gpui-pre-wgpu = { git = "https://github.com/vegidio/gpui-pre", tag = "0.3.10-surfaces.2" }
```

- `gpui-pre` is needed on both platforms, `gpui-pre-windows` for Windows and `gpui-pre-wgpu` for Linux; an app that builds for only one may drop the other's line.
- Stay on GPUI-Kit's default backend (`gpui-pre`). With `gpui-fast`, `player-gpui` doesn't build on Windows or Linux.
- macOS needs no patch.

### Windows video

- The graphics driver must share OpenGL textures with Direct3D 11 (`WGL_NV_DX_interop2`), as NVIDIA's, AMD's and Intel's do, and OpenGL must run on the same GPU as GPUI.
- On **hybrid-GPU laptops** the two can land on different GPUs. Steer the app to one GPU by exporting `NvOptimusEnablement` and `AmdPowerXpressRequestHighPerformance` from the executable, or by choosing its GPU in Windows' graphics settings.
- **Verified** on NVIDIA (GeForce RTX 5090). AMD, Intel and Windows arm64 are unverified.

### Linux video

- Frames are dma-bufs that GPUI's renderer imports on its own device, through Vulkan or EGL, with no copy at all.
- A DRM render node is required. **WSL2** has none for its GPU, unless `vgem` is loaded for software rendering; with it, set `LIBGL_ALWAYS_SOFTWARE=1`, as Mesa's `d3d12` driver can't import dma-bufs.
- **Verified** with software drivers only (lavapipe and llvmpipe). Mesa on Intel and AMD GPUs and NVIDIA's driver are unverified.

## 🧪 Advanced usage

### Using another toolkit

`player-rs` itself has no GUI dependency. Everything `player-gpui` does is built on public pieces of the core that any frontend can use:

- **`RenderContext`** makes mpv draw each frame into an OpenGL framebuffer you own:

  ```rust,ignore
  // SAFETY: the GL context is current on this thread, where `render` is used and dropped, and outlives it.
  let mut render = unsafe { RenderContext::new(&player, get_proc_address, move || wake_render_thread())? };
  player.load(Source::file("movie.mp4")?)?;

  // On the render thread, after each wake-up:
  if render.update() {
      render.render(RenderTarget { fbo, width, height }, flip_y)?;
  }
  ```

  It needs OpenGL 2.1+ or OpenGL ES 2.0+, and must be created before the first `load`.

- **`SurfaceRenderer`** owns its own render thread and publishes each frame as a GPU surface for toolkits that don't use OpenGL, with no `unsafe` code:

  ```rust,ignore
  let renderer = SurfaceRenderer::builder(&player).on_frame(|| request_repaint()).build()?;
  renderer.set_size(width_px, height_px);
  // When painting: renderer.latest() -> Option<Frame>
  ```

  | OS      | Surface renderer                   | Frames                                                |
  |---------|------------------------------------|-------------------------------------------------------|
  | macOS   | `player::macos::SurfaceRenderer`   | IOSurface-backed `CVPixelBuffer`, BGRA or NV12        |
  | Windows | `player::windows::SurfaceRenderer` | Direct3D 11 texture shared through an NT handle, BGRA |
  | Linux   | `player::linux::SurfaceRenderer`   | Single-plane dma-buf, RGBA (`DRM_FORMAT_ABGR8888`)    |

- **`player::ui::Controls`** holds every Video.js interaction rule that isn't drawing: control-bar hiding, seek-bar scrubbing, hotkeys, time labels, which parts are shown, the Settings menu and Retry. A frontend forwards its input and events, and draws the `view()` it returns with its skins, so every frontend behaves the same.

### Audio-only or headless use

Players default to `VideoOutput::Render`. Without a renderer attached, such a player drops the video track and ends at once, so for audio-only or headless playback build with `.video_output(VideoOutput::Null)`:

```rust,no_run
use player::prelude::*;

fn main() -> player::Result<()> {
    let player = Player::builder().video_output(VideoOutput::Null).autoplay(Autoplay::On).build()?;
    let events = player.subscribe();
    player.load(Source::file("song.mp3")?)?;

    for event in events {
        if let Event::Ended(_) = event {
            break;
        }
    }
    Ok(())
}
```

## 💣 Troubleshooting

### ``cannot find value `WINDOWS_SURFACES_REQUIRE_PATCHED_GPUI` `` (or `LINUX_…`)

GPUI isn't patched. Add the [`[patch.crates-io]` lines](#patched-gpui) to your **workspace root** `Cargo.toml`. The same error appears when GPUI-Kit moves to a GPUI version the fork has no tag for, because Cargo then ignores the patch with only a warning.

### The video is black but audio plays

The video can't be shown on this machine; the log has one warning saying why. Check the [Windows video](#windows-video) or [Linux video](#linux-video) requirements. Also make sure the player wasn't built with `VideoOutput::Null`.

### The build refuses to run on x86_64 macOS

The x86_64 libmpv needs macOS 10.15 or later, and Rust's default target (10.12) would produce binaries that fail to start. Add this to your app's `.cargo/config.toml`:

```toml
[env]
MACOSX_DEPLOYMENT_TARGET = "10.15"
```

Apple Silicon builds are unaffected.

### Some control icons are blank

The app didn't register `player_gpui::Assets` (or compose `player_gpui::Icons` into its own asset source).

## 🛠️ Development

### Building

Requirements: a stable Rust toolchain (edition 2024) and network access on the first build. `bindgen`/`libclang` are **not** needed: the FFI bindings are committed.

```bash
cargo build
cargo test --workspace --exclude xtask
cargo run -p player-gpui --example play -- path/to/movie.mp4
```

Supported targets:

| OS             | x64 | arm64 |
|----------------|-----|-------|
| macOS          | ✓   | ✓     |
| Linux (glibc)  | ✓   | ✓     |
| Windows (MSVC) | ✓   | ✓     |

- **macOS** and **Windows:** nothing to install. Binaries depend only on system frameworks and DLLs (plus the Visual C++ runtime on Windows).
- **Linux:** the display and audio stacks and the Vulkan loader stay dynamic. Install their development packages (Debian/Ubuntu names):

  ```bash
  sudo apt-get install libx11-dev libxext-dev libxfixes-dev libxrandr-dev libxss-dev libxpresent-dev \
      libx11-xcb-dev libxcb-dri3-dev libxkbcommon-dev libwayland-dev wayland-protocols libegl-dev \
      libgbm-dev libdrm-dev \
      libdisplay-info-dev libvdpau-dev libpulse-dev libpipewire-0.3-dev libvulkan-dev
  ```

- **Offline builds:** set `PLAYER_BINARIES_DIR` to an extracted [binaries-mpv](https://github.com/vegidio/binaries-mpv) release archive (with `include/` and `lib/`) to build without downloading anything.

### Tests

The tests need no media files, display, GPU or sound device: they play media mpv generates itself, with mpv's null audio output, and render into headless, software-rendered OpenGL contexts.

- **macOS:** Apple's software renderer, built in. Set `PLAYER_TEST_GL=hardware` to run on the GPU instead.
- **Linux:** Mesa's EGL and llvmpipe (`sudo apt-get install libegl1 libegl-mesa0 libgl1-mesa-dri`). The surface and `player-gpui` video tests also need a DRM device:

  ```bash
  sudo modprobe vgem                  # on WSL2, from Windows: wsl -u root -- modprobe vgem
  sudo chmod 666 /dev/dri/*           # or join the video and render groups
  LIBGL_ALWAYS_SOFTWARE=1 cargo test -p player-rs --test surface
  LIBGL_ALWAYS_SOFTWARE=1 cargo test -p player-gpui                            # Vulkan: lavapipe
  VK_DRIVER_FILES=/dev/null LIBGL_ALWAYS_SOFTWARE=1 cargo test -p player-gpui  # OpenGL: llvmpipe
  ```

- **Windows:** the system's OpenGL is too old without a GPU driver. CI copies Mesa's `opengl32.dll` and `libgallium_wgl.dll` from [mesa-dist-win](https://github.com/pal1000/mesa-dist-win) next to the test binaries and sets `GALLIUM_DRIVER=llvmpipe`. Mesa lacks `WGL_NV_DX_interop2`, so the full surface tests need a machine with a GPU driver.
- **Windows arm64:** no software OpenGL exists, so CI skips the GL tests there (`PLAYER_TEST_SKIP_GL=1`).

### Updating libmpv

1. Change the version in [`crates/player/mpv-version.txt`](crates/player/mpv-version.txt).
2. Regenerate the bindings (needs `libclang`; one run on any host is enough):

   ```bash
   cargo run -p xtask -- bindings
   ```

3. Run the tests. `tests/link.rs` fails if the linked libmpv and the committed bindings disagree.

## 📝 License

**player-rs** is released under the Apache 2.0 License. See [LICENSE](LICENSE) for details.

## 👨🏾‍💻 Author

Vinicius Egidio ([vinicius.io](https://vinicius.io))
