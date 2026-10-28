## Purpose

Defines how the core player crate obtains, links and binds the prebuilt static libmpv for the target platform, so that any program depending on it plays media without installing mpv or FFmpeg separately.

## ADDED Requirements

### Requirement: Pinned prebuilt libmpv is obtained automatically

The core crate SHALL, when built, obtain the static libmpv archive for the build target from the `vegidio/binaries-mpv` release whose version is pinned in a single version file inside the crate. Changing that one file SHALL be the only edit needed to move to another release.

#### Scenario: First build downloads the matching archive
- **WHEN** the core crate is built for a supported target with no override and no cached archive
- **THEN** the build downloads the `static_<os>_<arch>` archive of the pinned release for that target and links against it

#### Scenario: Bumping the pinned version
- **WHEN** the version in the pinned version file is changed and the crate is rebuilt
- **THEN** the build obtains the archive of the new release instead of reusing the previous one

#### Scenario: Unsupported target
- **WHEN** the core crate is built for a target outside the supported set (an OS other than macOS, Linux or Windows; an architecture other than x64 or arm64; or a Windows GNU target)
- **THEN** the build fails with an error naming the unsupported OS, architecture or environment

### Requirement: Downloaded archive is cached safely

The build SHALL reuse a previously extracted archive of the same version instead of downloading it again, and SHALL never treat an incompletely extracted archive as complete.

#### Scenario: Rebuild reuses the cache
- **WHEN** the core crate is rebuilt for the same target after a successful build
- **THEN** no download takes place

#### Scenario: Interrupted extraction
- **WHEN** a previous build was interrupted while extracting the archive
- **THEN** the next build discards the partial result and extracts the archive again

#### Scenario: Download failure
- **WHEN** the archive cannot be downloaded
- **THEN** the build fails with an error that includes the URL it tried

### Requirement: Offline override

The build SHALL accept the `PLAYER_BINARIES_DIR` environment variable pointing at a directory laid out like an extracted release archive (`include/` and `lib/`), and SHALL then use it without any network access.

#### Scenario: Valid override
- **WHEN** `PLAYER_BINARIES_DIR` points at a directory containing `include/` and `lib/`
- **THEN** the build links against the libraries in that directory and downloads nothing

#### Scenario: Invalid override
- **WHEN** `PLAYER_BINARIES_DIR` points at a directory missing `include/` or `lib/`
- **THEN** the build fails with an error naming the directory and the missing subdirectory

#### Scenario: Override changes
- **WHEN** the value of `PLAYER_BINARIES_DIR` changes between builds
- **THEN** the build script runs again and uses the new location

### Requirement: Self-contained static linking

The core crate SHALL link libmpv and every library it depends on that the archive provides statically, deriving the set of libraries and platform system libraries from the archive's own package metadata rather than from a hand-maintained list. Programs that depend on the core crate, directly or through another crate, SHALL link without any extra linker configuration of their own.

#### Scenario: Downstream binary links
- **WHEN** a binary in another crate depends on the core crate (directly, or through the GUI crate) and is built
- **THEN** it links successfully without the developer adding linker flags, and it runs without mpv or FFmpeg installed on the system

#### Scenario: macOS runtime dependencies
- **WHEN** a binary linked on macOS is inspected for dynamic dependencies
- **THEN** it depends only on Apple system frameworks and libraries shipped with the OS

#### Scenario: Linux runtime dependencies
- **WHEN** a binary linked on Linux is inspected for dynamic dependencies
- **THEN** it depends only on the C/C++ runtime, the Vulkan loader, and the system display and audio stacks (X11, Wayland, EGL/GL, GBM, libdisplay-info, VDPAU, PulseAudio, PipeWire) that the archive leaves dynamic

#### Scenario: Windows runtime dependencies
- **WHEN** a binary linked for a Windows MSVC target is inspected for DLL imports
- **THEN** it imports only Windows system DLLs and the Visual C++ runtime, and needs no DLL shipped next to the executable

### Requirement: Bindings require no libclang

The raw FFI bindings for the libmpv client headers (`mpv/client.h`, `mpv/render.h`, `mpv/render_gl.h`, `mpv/stream_cb.h`) SHALL ship with the crate source, so building the crate or any crate depending on it does not require bindgen or libclang. The bindings SHALL be identical on every supported target.

#### Scenario: Build without libclang
- **WHEN** the core crate is built on a machine with no libclang installed
- **THEN** the build succeeds

### Requirement: Documentation builds without the native library

When built for documentation on docs.rs, the crate SHALL skip downloading and linking libmpv, so documentation can be generated without network access.

#### Scenario: docs.rs build
- **WHEN** the crate's documentation is built in the docs.rs environment
- **THEN** no download is attempted and the documentation build succeeds

### Requirement: Linked libmpv version is observable

The core crate SHALL expose the client API version of the libmpv it is linked against, and the version the bindings were generated from, so a mismatch between the committed bindings and the linked library can be detected.

#### Scenario: Bindings match the linked library
- **WHEN** the client API version reported by the linked libmpv is compared with the version from the committed bindings
- **THEN** both are equal

#### Scenario: Version is readable as major/minor
- **WHEN** a caller asks for the linked client API version
- **THEN** it receives the major and minor version numbers as encoded by libmpv
