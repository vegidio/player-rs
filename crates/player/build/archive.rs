//! Maps the build target to the `binaries-mpv` release archive. Shared by `build.rs` and by the crate's
//! `tests/archive.rs`, which checks every supported and unsupported target.

/// Returns the release archive file name for a target, e.g. `static_osx_arm64.zip`, from the values of
/// `CARGO_CFG_TARGET_OS`, `CARGO_CFG_TARGET_ARCH` and `CARGO_CFG_TARGET_ENV`.
pub fn archive_name(target_os: &str, target_arch: &str, target_env: &str) -> Result<String, String> {
    let os = match target_os {
        "linux" => "linux",
        "macos" => "osx",
        "windows" if target_env == "msvc" => "windows",
        "windows" => {
            return Err(format!("unsupported Windows target environment `{target_env}` (only MSVC is supported)"));
        }
        other => return Err(format!("unsupported target OS `{other}`")),
    };

    let arch = match target_arch {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => return Err(format!("unsupported target architecture `{other}`")),
    };

    Ok(format!("static_{os}_{arch}.zip"))
}
