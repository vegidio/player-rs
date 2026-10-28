## Why

The repository is empty: there is no Cargo workspace, no way to obtain or link libmpv, and no CI. Every later change (playback API, render API, GPUI UI) needs a workspace whose core crate builds and links the prebuilt static libmpv from `vegidio/binaries-mpv` on every supported platform, following the conventions already proven in `media-rs` and `avif-rs`.

## What Changes

- Create a Cargo workspace (edition 2024) with two members:
  - `crates/player` - package `player-rs`, lib `player`, `links = "mpv"`. Owns the build script, the raw FFI bindings and a minimal safe smoke-test API.
  - `crates/gpui` - package `player-gpui`, lib `player_gpui`. An empty stub that depends on `player-rs`; no GPUI/GPUI-Kit dependency yet.
- Add a `build.rs` in `crates/player` that, at build time:
  - downloads the pinned `static_<os>_<arch>.zip` release of `binaries-mpv` (version in `crates/player/mpv-version.txt`, initially `26.10.1`) into `OUT_DIR`, reusing a complete cache and never leaving a half-extracted one;
  - accepts a `PLAYER_BINARIES_DIR` override pointing at a pre-extracted archive for offline/custom builds;
  - skips downloading and linking on docs.rs;
  - derives the link line from the archive's relocatable `lib/pkgconfig/*.pc` files (following `Requires`/`Requires.private`), without needing a `pkg-config` binary on the host.
- Commit pre-generated raw FFI bindings for the libmpv client headers (`mpv/client.h`, `mpv/render.h`, `mpv/render_gl.h`, `mpv/stream_cb.h`), plus a small dev-only generator to regenerate them when the pinned version changes. Consumers never need bindgen or libclang.
- Expose a minimal safe API proving the link works: the libmpv client API version (`mpv_client_api_version`), with a test asserting it matches `MPV_CLIENT_API_VERSION` from the committed bindings.
- Add repo tooling copied from `media-rs`: `rustfmt.toml`, `.editorconfig`, `.gitignore`, `.cargo/config.toml` (coverage aliases), `LICENSE` (Apache-2.0), `README.md`.
- Add a GitHub Actions `test.yml`: clippy once on Linux, then build + test on a 6-leg matrix (macOS, Linux, Windows MSVC; x64 and arm64).
- Keep the crates publishable to crates.io (no git dependencies, free package names), but add no publish workflow yet.

## Capabilities

### New Capabilities

- `libmpv-build`: how `crates/player` obtains, caches, links and binds the prebuilt static libmpv for the target platform, including the offline override, docs.rs behaviour, and the version check that proves the bindings match the linked library.

### Modified Capabilities

None.

## Impact

- **Crates**: `crates/player` gets everything that touches libmpv (build script, bindings, smoke API), because GUI crates must never call libmpv directly. `crates/gpui` is only a placeholder so the workspace layout from the project context exists from day one; it has no behaviour.
- **libmpv APIs**: only the C client API headers are bound; the only function called is `mpv_client_api_version`. No `mpv_handle`, render context or properties yet.
- **Dependencies**: build-dependencies `ureq` (HTTP) and `zip` (extraction), as in `media-rs`/`avif-rs`; `thiserror` is not needed yet. The bindings generator (dev-only, unpublished) uses `bindgen`.
- **rust-sak**: `fetch` (download with retries) and `fs` (hardened extraction) cover this need, but neither can be used here. `rust-sak` is git-only, and crates.io rejects packages with git dependencies, including build-dependencies; `fetch` is also Tokio-async, which is heavy for a build script. The download path therefore uses `ureq` + `zip`, matching the sibling crates. Nothing is added to `rust-sak`.
- **Platforms**: macOS, Linux and Windows MSVC, x64 and arm64. Not supported: Windows GNU/MinGW (MSVC only, like `media-rs`) and any other architecture. Since binaries-mpv `26.10.1`, the Windows builds have no Vulkan, so `gpu-api=vulkan` and `hwdec=vulkan` are unavailable on Windows (as on macOS); rendering through the OpenGL render API and D3D11VA decoding are unaffected, and nothing beyond Windows system DLLs and the Visual C++ runtime has to ship with an app.
- **Size**: 14 tasks, within the 15-task limit, so this is a single change, not a series.
