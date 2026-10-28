# player-rs

A fast, user-friendly video player for Rust, built on [libmpv](https://mpv.io). The goal is to play anything mpv can play, 4K/HDR included, smoothly on modest hardware, and eventually to expose every mpv feature through an idiomatic Rust API.

libmpv and everything it depends on (FFmpeg, libplacebo, libass, …) are linked **statically** from the prebuilt archives of [vegidio/binaries-mpv](https://github.com/vegidio/binaries-mpv). Programs built with `player-rs` need neither mpv nor FFmpeg installed.

> **Status:** early. The workspace builds and links libmpv on every supported platform; the playback API and the GPUI player come in later changes.

## Workspace

| Path            | Package       | Library       | Purpose                                                                                      |
|-----------------|---------------|---------------|----------------------------------------------------------------------------------------------|
| `crates/player` | `player-rs`   | `player`      | Core crate: downloads and links libmpv, raw FFI bindings (`player::sys`), the safe API. No GUI dependency. |
| `crates/gpui`   | `player-gpui` | `player_gpui` | The player UI for [GPUI](https://gpui.rs) apps (placeholder for now). Goes through `player-rs`, never libmpv. |
| `xtask`         | `xtask`       | -             | Dev-only tooling (not published): regenerates the FFI bindings.                              |

## Supported platforms

| OS                  | x64 | arm64 |
|---------------------|-----|-------|
| macOS               | ✓   | ✓     |
| Linux (glibc)       | ✓   | ✓     |
| Windows (MSVC)      | ✓   | ✓     |

Windows GNU/MinGW targets and other architectures are not supported; the build fails with an error naming the unsupported OS, architecture or environment.

## Building

Requirements: a stable Rust toolchain (edition 2024) and network access on the first build. `bindgen`/`libclang` are **not** needed: the bindings are committed.

```sh
cargo build
cargo test --workspace --exclude xtask
cargo run -p player-rs --example version
```

On the first build `crates/player/build.rs` downloads the `binaries-mpv` release pinned in [`crates/player/mpv-version.txt`](crates/player/mpv-version.txt) for the target (35-75 MB) and caches it in Cargo's `OUT_DIR`; later builds reuse it. The link line is derived from the archive's own `lib/pkgconfig/*.pc` files, so no `pkg-config` binary is needed.

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
