## 1. Workspace

- [ ] 1.1 Create the root `Cargo.toml` workspace (`crates/player`, `crates/gpui`, `xtask`; shared `[workspace.package]` with edition 2024 and Apache-2.0), plus `rustfmt.toml`, `.editorconfig`, `.gitignore`, `.cargo/config.toml` coverage aliases and `LICENSE` copied/adapted from `media-rs`; verify `cargo metadata` lists the three members

## 2. crates/player - build script

- [ ] 2.1 Create the `player-rs` package (lib `player`, `links = "mpv"`, build-deps `ureq` + `zip`) and `mpv-version.txt` = `26.10.1`; verify `cargo package -p player-rs --list` includes `mpv-version.txt` and `build.rs`
- [ ] 2.2 Implement archive location in `build.rs`: target -> `static_<os>_<arch>.zip` mapping with clear errors for unsupported OS/arch/GNU env, `PLAYER_BINARIES_DIR` override with `include/`+`lib/` validation, download + `.partial` extract + rename into `OUT_DIR`, cache reuse, `DOCS_RS` early return and `rerun-if` directives; verify a second build downloads nothing and an invalid override fails with the directory named
- [ ] 2.3 Implement the `.pc` reader (variables with `${pcfiledir}`, `Requires`/`Requires.private` walk, `Libs`/`Libs.private` token mapping, de-duplication keeping last occurrence, loud failure on unknown flags) with unit tests against the macOS, Linux and Windows MSVC `mpv.pc` closures; verify `cargo test -p player-rs` runs the parser tests
- [ ] 2.4 Emit link directives from the `.pc` result (`static`/`dylib`/`framework`, link search paths, Linux `--start-group` link args for this package's targets, macOS swift search path only if required); verify `cargo build -p player-rs` links on the local macOS host

## 3. crates/player - bindings and smoke API

- [ ] 3.1 Create `xtask` (`publish = false`) with a `bindings` command that downloads the host archive, runs bindgen over `client.h`/`render.h`/`render_gl.h`/`stream_cb.h` with `mpv_.*`/`MPV_.*` allowlists, constified `c_int` enums and an `MPV_CLIENT_API_VERSION` constant; verify `cargo run -p xtask -- bindings` writes `crates/player/src/sys/bindings.rs` and a second run produces no diff
- [ ] 3.2 Commit the generated bindings and add `src/sys.rs` (`include!` plus FFI `allow` attributes); verify `cargo clippy -p player-rs -- -D warnings` is clean
- [ ] 3.3 Add `src/lib.rs` with crate docs, `#![warn(missing_docs)]`, `#![deny(unsafe_op_in_unsafe_fn)]`, `client_api_version() -> (u16, u16)` and `BINDINGS_CLIENT_API_VERSION`; verify `cargo doc -p player-rs --no-deps` builds with no warnings
- [ ] 3.4 Add `tests/link.rs` asserting the linked client API version equals the bindings' version, and `examples/version.rs` printing it; verify `cargo test -p player-rs` and `cargo run -p player-rs --example version` pass on macOS

## 4. crates/gpui

- [ ] 4.1 Create the `player-gpui` stub package (lib `player_gpui`, depends on `player-rs` by path, crate doc only, no GPUI deps) with a test that calls `player::client_api_version()`; verify `cargo test -p player-gpui` passes, proving a downstream crate links without its own linker flags

## 5. CI and docs

- [ ] 5.1 Add `.github/workflows/test.yml`: `lint` job (`cargo fmt --check`, `cargo clippy --workspace --exclude xtask --all-targets -- -D warnings`) and the 6-leg build+test matrix for `player-rs` and `player-gpui`, with Rosetta for macOS x64 and `rust-cache`; verify with `actionlint` (or a YAML lint) locally
- [ ] 5.2 Add the Linux `-dev` package install step, derived from the Linux `mpv.pc` closure; verify the step is scoped to the Linux legs and the Windows legs have no DLL/`PATH` workaround
- [ ] 5.3 Write `README.md` (what the project is, workspace layout, build requirements, `PLAYER_BINARIES_DIR`, supported platforms, regenerating bindings with `xtask`, Apache-2.0); verify every command it shows runs locally
- [ ] 5.4 Run `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace --exclude xtask` locally, then push to a branch only after the user approves, and confirm all 6 CI legs and lint pass
