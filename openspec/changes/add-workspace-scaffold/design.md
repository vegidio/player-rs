## Context

The repository has no code yet. See proposal.md for why. The approach copies `media-rs` (single static archive per target, downloaded in `build.rs`, committed bindings), with differences driven by what the `binaries-mpv` archives contain:

- A static archive holds ~160 static libraries (macOS arm64), but `pkg-config --static --libs mpv` resolves only ~57 of them; the rest (abseil, mbedtls, extra harfbuzz components, ...) are build leftovers.
- The archives ship relocatable `lib/pkgconfig/*.pc` files (`prefix=${pcfiledir}/../..`) that list both the bundled libraries and the platform system libraries/frameworks.
- The libmpv client headers are plain C with no platform SDK types.
- Since `26.10.1`, the Windows archives are built without Vulkan: the static MSVC zips contain no DLLs and no `.pc` file references `vulkan`. Earlier releases (`26.10.0`) imported `vulkan-1.dll` at load time, which is why the pin starts at `26.10.1`.

## Goals / Non-Goals

**Goals:**
- A workspace whose core crate links the static libmpv on all six supported targets, verified in CI.
- A build that works for downstream crates exactly as for this crate's own tests.
- Moving to a new `binaries-mpv` release is a one-line change.

**Non-Goals:**
- Any playback API (`mpv_create`, properties, events) or the render API. Later changes.
- Any GPUI/GPUI-Kit dependency or UI code.
- Publishing to crates.io (kept possible, not done).
- Windows GNU/MinGW targets and dynamic linking.

## Decisions

### Workspace layout

```
player-rs/
+-- Cargo.toml                 [workspace] members = crates/player, crates/gpui, xtask
+-- rustfmt.toml .editorconfig .gitignore .cargo/config.toml LICENSE README.md
+-- .github/workflows/test.yml
+-- crates/
|   +-- player/                package player-rs, lib player, links = "mpv"
|   |   +-- mpv-version.txt    pinned binaries-mpv release (26.10.1)
|   |   +-- build.rs
|   |   +-- src/lib.rs         client_api_version() smoke API
|   |   +-- src/sys.rs         include!("sys/bindings.rs") + allow attributes
|   |   +-- src/sys/bindings.rs   committed, generated
|   |   +-- examples/version.rs
|   |   +-- tests/link.rs
|   +-- gpui/                  package player-gpui, lib player_gpui, depends on player-rs
+-- xtask/                     publish = false; `cargo run -p xtask -- bindings`
```

Shared package metadata (`edition`, `license = "Apache-2.0"`, `repository`, `authors`, `version`) lives in `[workspace.package]`. Version scheme follows the sibling crates (CalVer `YY.M.patch`).

The version file sits inside `crates/player`, not at the repo root: `include_str!` of a path outside the package breaks `cargo package`, and the crates must stay publishable. `xtask` reads it through a relative path.

### Download and extraction: `ureq` + `zip`, not `rust-sak`

`rust-sak` `fetch` + `fs` cover downloading with retries and hardened extraction. They are not used because:
- `rust-sak` is a git-only dependency, and crates.io rejects packages with git dependencies of any kind, including build-dependencies. The crates must stay publishable.
- `fetch` is Tokio/reqwest-async; a build script would need a runtime for one blocking download.

`ureq` (blocking, small) and `zip` (deflate only), with the same logic as `media-rs`: map `CARGO_CFG_TARGET_{OS,ARCH,ENV}` to `static_<os>_<arch>.zip`, download to memory, extract into `<OUT_DIR>/binaries-mpv-<version>.partial`, then rename to `binaries-mpv-<version>`. Entries without an `enclosed_name()` are skipped. The gap is not filled in `rust-sak`: the constraint is crates.io's policy, not a missing feature.

`build.rs` emits `rerun-if-changed` for itself and `mpv-version.txt`, and `rerun-if-env-changed=PLAYER_BINARIES_DIR`. With `DOCS_RS` set, it returns before downloading or emitting link directives; the committed bindings make that safe.

### Link line from the archive's `.pc` files

Alternatives:
- **Link every library in `lib/`** (`media-rs`). Rejected: ~100 unrelated archives, a hand-maintained per-OS list of system libraries/frameworks, and a risk of duplicate-symbol surprises from leftovers (e.g. both mbedtls and OpenSSL ship).
- **`pkg-config` crate.** Rejected: needs a `pkg-config`/`pkgconf` binary on the host, which Windows usually lacks.
- **Small `.pc` reader in `build.rs`** (chosen). Parse variables (`prefix`, `libdir`, with `${pcfiledir}` substituted), `Requires`, `Requires.private`, `Libs`, `Libs.private`; walk `Requires*` depth-first from `mpv`, ignoring version constraints; collect tokens:
  - `-L<dir>` -> `rustc-link-search=native`
  - `-l<name>` -> `static=<name>` if the archive's `lib/` holds `lib<name>.a` / `<name>.lib`, else `dylib=<name>`
  - `-framework X` -> `framework=X`
  - absolute paths to `.a` files (the MinGW-style entries in `mpv.pc`) -> link search on the parent, `static=` on the stem
  - `-pthread` -> `dylib=pthread` on Linux; other flags are ignored.
  Duplicates are removed keeping the last occurrence, so dependencies stay after dependents.

Libraries are emitted as `cargo:rustc-link-lib`, not `rustc-link-arg`: link args apply only to this package's own targets, so downstream crates would not link (the `media-rs` lesson). The static archives are bundled into the rlib.

On Linux, the crate's own test binary links the archives directly and the single-pass GNU `bfd` linker can miss back-references; as in `media-rs`, the static set is additionally emitted inside `-Wl,--start-group ... --end-group` link args (affects only this package's targets).

### Committed bindings via `xtask`

Alternatives: bindgen in `build.rs` (`avif-rs`) forces libclang on every consumer and on every CI leg, including a manual LLVM download for Windows arm64. Rejected.

`xtask bindings` downloads the host archive (same logic as `build.rs`, duplicated because a build script is not a library), runs bindgen over a header that includes the four mpv headers, allowlists `mpv_.*` / `MPV_.*`, and writes `crates/player/src/sys/bindings.rs`. One run on any host is enough because the headers have no platform types.

C enums are emitted as constified enums with a fixed `c_int` repr (bindgen otherwise picks `u32` on GCC/Clang targets and `i32` on MSVC), so one file is correct everywhere. `src/sys.rs` carries the `allow` attributes generated FFI needs (naming lints, `missing_docs`, `clippy::missing_safety_doc`).

`MPV_CLIENT_API_VERSION` is a C macro built from `MPV_MAKE_VERSION`; if bindgen does not evaluate it, `xtask` computes it from `MPV_CLIENT_API_VERSION_MAJOR`/`MINOR` and appends it as a constant.

### Smoke API

`player::client_api_version() -> (u16, u16)` wraps `mpv_client_api_version()` (returns `(major << 16) | minor`), and `player::BINDINGS_CLIENT_API_VERSION` exposes the bindings' version. `tests/link.rs` asserts they match: it proves linking works and catches stale bindings after a version bump on all six CI legs.

### CI

`test.yml`, adapted from `media-rs`:
- `lint`: `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings` on Linux x64. Unlike `media-rs`, fmt is enforced from the start, since the repo begins clean.
- `test`: 6 legs (macOS arm64, macOS x64 via Rosetta, Linux x64, Linux arm64, Windows x64, Windows arm64), `cargo build` + `cargo test` for `player-rs` and `player-gpui`. `xtask` is excluded so CI never builds bindgen.
- Linux legs install the `-dev` packages providing the dynamic display/audio libraries named in the `.pc` files. The exact list is determined from the Linux `mpv.pc` closure during implementation.
- Windows legs need no extra steps: the `26.10.1` archives ship no DLLs.

### Backend / UI separation

Everything that touches libmpv lives in `crates/player`, which has no GUI dependency. `crates/gpui` depends on it and contains nothing. A future `crates/egui` would depend on `player-rs` the same way, unchanged.

GPUI-Kit vs raw GPUI: not applicable, no UI in this change.

### Unsafe / FFI, threading, rendering

The only unsafe call is `mpv_client_api_version()`, which takes no arguments, has no preconditions and is thread-safe. The crate sets `#![deny(unsafe_op_in_unsafe_fn)]` as `media-rs` does. No threads, callbacks or frames are involved, so the zero-copy rendering path is unaffected.

## Risks / Trade-offs

- [macOS: libmpv contains Swift code; rustc's link line may need `-L/usr/lib/swift` (or the toolchain's swift lib path) even though the archive ships the `libswiftCompatibility*.a` back-deployment libs] -> Verified first on the macOS legs; if needed, `build.rs` emits the search path on macOS only.
- [`.pc` files may contain constructs the small parser does not handle (quoted paths, `-Wl,` flags, `Libs.private` on Windows using `-l` names that map to system import libs)] -> Unknown flags fail loudly in `build.rs` rather than being silently dropped, so CI shows them; the parser is unit-tested against the `.pc` files of all three OSes.
- [Linux bfd single-pass linker ordering] -> `--start-group` for this package's own targets, as in `media-rs`; downstream crates get the bundled rlib.
- [Downloading ~35-80 MB per clean build and per CI leg] -> `OUT_DIR` cache plus `Swatinem/rust-cache`; `PLAYER_BINARIES_DIR` for offline work.
- [A future binaries-mpv release reintroduces a DLL dependency on Windows] -> The Windows test legs fail to start the test binary, and the spec's Windows scenario makes that a regression rather than something to work around.
- [Duplicated download code in `build.rs` and `xtask`] -> Accepted, as in `media-rs`; it is small and a build script cannot be shared as a library.
- [Committed bindings going stale after a bump] -> The client API version test fails on every leg.

## Migration Plan

Not applicable: first code in the repository.

## Open Questions

- Exact Linux `-dev` package list for the CI runners. Found during implementation from the `.pc` closure; does not change the approach.
