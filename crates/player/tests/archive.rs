//! Tests for the build script's target -> release archive mapping.

#[path = "../build/archive.rs"]
mod archive;

use archive::archive_name;

#[test]
fn supported_targets() {
    for (os, arch, env, expected) in [
        ("macos", "aarch64", "", "static_osx_arm64.zip"),
        ("macos", "x86_64", "", "static_osx_x64.zip"),
        ("linux", "aarch64", "gnu", "static_linux_arm64.zip"),
        ("linux", "x86_64", "gnu", "static_linux_x64.zip"),
        ("windows", "aarch64", "msvc", "static_windows_arm64.zip"),
        ("windows", "x86_64", "msvc", "static_windows_x64.zip"),
    ] {
        assert_eq!(archive_name(os, arch, env).as_deref(), Ok(expected), "{os}/{arch}/{env}");
    }
}

#[test]
fn unsupported_targets_name_the_cause() {
    for (os, arch, env, named) in [
        ("freebsd", "x86_64", "", "OS `freebsd`"),
        ("linux", "riscv64", "gnu", "architecture `riscv64`"),
        ("windows", "x86_64", "gnu", "environment `gnu`"),
    ] {
        let err = archive_name(os, arch, env).unwrap_err();
        assert!(err.contains(named), "{os}/{arch}/{env}: {err}");
    }
}
