//! Tests for the build script's `.pc` reader, run against the `mpv.pc` closures of the `binaries-mpv` archives.
//!
//! `tests/fixtures/pkgconfig/<os>/lib/pkgconfig` holds the `.pc` files reachable from `mpv.pc` in that OS's archive,
//! and `lib/files.txt` lists the archive's `lib/` directory. `linux/system` holds the Ubuntu 24.04 `.pc` files of the
//! system modules the Linux archive only names.

#[allow(dead_code)]
#[path = "../build/pkgconfig.rs"]
mod pkgconfig;

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use pkgconfig::{Lib, LinkPlan, Resolver, static_link_name};

fn fixture(os: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pkgconfig").join(os)
}

fn resolver(os: &str, msvc: bool, system_dirs: Vec<PathBuf>) -> Resolver {
    let lib_dir = fixture(os).join("lib");
    let files = fs::read_to_string(lib_dir.join("files.txt")).unwrap();
    let static_libs = files.lines().filter_map(|f| static_link_name(f, msvc)).collect();
    Resolver { target_os: os.to_string(), lib_dir, system_dirs, static_libs }
}

fn position(plan: &LinkPlan, lib: &Lib) -> usize {
    plan.libs
        .iter()
        .position(|l| l == lib)
        .unwrap_or_else(|| panic!("{lib:?} not in {:#?}", plan.libs))
}

fn st(name: &str) -> Lib {
    Lib::Static(name.to_string())
}

fn dy(name: &str) -> Lib {
    Lib::Dylib(name.to_string())
}

fn fw(name: &str) -> Lib {
    Lib::Framework(name.to_string())
}

/// Checks what holds on every OS: no duplicates, mpv first, dependents before dependencies.
fn assert_common(plan: &LinkPlan, os: &str) {
    let unique: HashSet<_> = plan.libs.iter().collect();
    assert_eq!(unique.len(), plan.libs.len(), "duplicate libraries");
    assert_eq!(plan.libs[0], st("mpv"));
    assert!(position(plan, &st("avformat")) < position(plan, &st("avcodec")));
    assert!(position(plan, &st("avcodec")) < position(plan, &st("avutil")));
    assert!(position(plan, &st("ass")) < position(plan, &st("harfbuzz")));
    assert_eq!(plan.search_paths, vec![fixture(os).join("lib")]);
}

#[test]
fn macos_closure() {
    let plan = resolver("macos", false, vec![]).resolve("mpv").unwrap();
    assert_common(&plan, "macos");

    for lib in [fw("Cocoa"), fw("VideoToolbox"), fw("OpenGL"), dy("c++"), dy("iconv"), st("placebo"), st("ssl")] {
        position(&plan, &lib);
    }
    assert!(!plan.libs.contains(&dy("stdc++")), "libstdc++ must be mapped to libc++ on macOS");
}

#[test]
fn linux_closure() {
    let plan = resolver("linux", false, vec![fixture("linux").join("system")]).resolve("mpv").unwrap();
    assert_common(&plan, "linux");

    // The display and audio stacks come from the host; the Vulkan loader is named by the archive but not shipped.
    for name in ["X11", "Xss", "Xrandr", "wayland-client", "EGL", "gbm", "display-info", "vdpau", "pulse"] {
        position(&plan, &dy(name));
    }
    for lib in [
        dy("pipewire-0.3"),
        dy("vulkan"),
        dy("pthread"),
        dy("stdc++"),
        st("asound"),
        st("va-drm"),
        st("va"),
    ] {
        position(&plan, &lib);
    }
    assert!(position(&plan, &st("va-drm")) < position(&plan, &st("va")));
    // The static libva-x11 calls into xcb directly; its `.pc` must say so for the link to succeed.
    for name in ["xcb", "X11-xcb", "xcb-dri3"] {
        assert!(position(&plan, &st("va-x11")) < position(&plan, &dy(name)));
    }
    // libdrm is named through a host `.pc` file, but the archive ships `libdrm.a`, so it stays static.
    position(&plan, &st("drm"));
    // Only `Libs` is read for system modules.
    assert!(!plan.libs.contains(&dy("pulsecommon-16.1")));
}

#[test]
fn linux_missing_system_module() {
    let err = resolver("linux", false, vec![]).resolve("mpv").unwrap_err();
    assert!(err.contains("is required by mpv"), "{err}");
    assert!(err.contains("install the development package"), "{err}");
}

#[test]
fn windows_closure() {
    let plan = resolver("windows", true, vec![]).resolve("mpv").unwrap();
    assert_common(&plan, "windows");

    for lib in [st("spirv-cross-glsl"), st("libcrypto"), st("libexpatMD"), st("zs"), dy("ole32"), st("opengl32")] {
        position(&plan, &lib);
    }
    assert!(plan.libs.iter().all(|l| !matches!(l, Lib::Framework(_))));
}

/// Writes `.pc` files into a fresh archive-like directory and returns a resolver for it.
fn synthetic(name: &str, files: &[(&str, &str)], static_libs: &[&str]) -> Resolver {
    let lib_dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name).join("lib");
    let _ = fs::remove_dir_all(&lib_dir);
    fs::create_dir_all(lib_dir.join("pkgconfig")).unwrap();
    for (file, text) in files {
        fs::write(lib_dir.join("pkgconfig").join(file), text).unwrap();
    }
    Resolver {
        target_os: "linux".to_string(),
        lib_dir,
        system_dirs: vec![],
        static_libs: static_libs.iter().map(|s| s.to_string()).collect(),
    }
}

#[test]
fn shared_dependency_is_linked_last() {
    let resolver = synthetic(
        "shared_dependency",
        &[
            ("a.pc", "Libs: -la\nRequires: b >= 1.0, c\n"),
            ("b.pc", "Libs: -lb -lz\nRequires.private: c\n"),
            ("c.pc", "Libs: -lc\nLibs.private: -lz -lm\n"),
        ],
        &["a", "b", "c", "z"],
    );
    let plan = resolver.resolve("a").unwrap();
    assert_eq!(plan.libs, vec![st("a"), st("b"), st("c"), st("z"), dy("m")]);
}

#[test]
fn variables_and_quotes() {
    let resolver = synthetic(
        "variables_and_quotes",
        &[(
            "a.pc",
            "prefix=${pcfiledir}/../..\nlibdir=${prefix}/my lib\n# comment\nName: a\nLibs: \"-L${libdir}\" -la -framework Cocoa\n",
        )],
        &[],
    );
    let plan = resolver.resolve("a").unwrap();
    assert_eq!(plan.search_paths, vec![resolver.lib_dir.parent().unwrap().join("my lib")]);
    assert_eq!(plan.libs, vec![dy("a"), fw("Cocoa")]);
}

#[test]
fn unknown_flag_fails() {
    let resolver = synthetic("unknown_flag", &[("a.pc", "Libs: -la -Wl,--as-needed\n")], &[]);
    let err = resolver.resolve("a").unwrap_err();
    assert!(err.contains("`-Wl,--as-needed`"), "{err}");
}

#[test]
fn static_link_names() {
    assert_eq!(static_link_name("libmpv.a", false).as_deref(), Some("mpv"));
    assert_eq!(static_link_name("mpv.a", false), None);
    assert_eq!(static_link_name("libcrypto.lib", true).as_deref(), Some("libcrypto"));
    assert_eq!(static_link_name("GlU32.Lib", true).as_deref(), Some("GlU32"));
    assert_eq!(static_link_name("pkgconfig", true), None);
}
