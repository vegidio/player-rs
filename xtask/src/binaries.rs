//! Locating / downloading the prebuilt libmpv static archives.
//!
//! This duplicates the small download+extract logic from `crates/player/build.rs` because a build script cannot be
//! imported as a library. Unlike `build.rs`, the target is the *host*, so the OS/arch come from `std::env::consts`
//! rather than the `CARGO_CFG_TARGET_*` variables.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The pinned `binaries-mpv` release, read from `crates/player/mpv-version.txt` (the single source of truth shared with
/// `build.rs`). `.trim()` tolerates a trailing newline.
pub fn version() -> &'static str {
    include_str!("../../crates/player/mpv-version.txt").trim()
}

/// Maps the host OS/arch to the release archive file name, e.g. `static_osx_arm64.zip`.
fn host_archive_name() -> String {
    let os = match env::consts::OS {
        "linux" => "linux",
        "macos" => "osx",
        "windows" => "windows",
        other => panic!("xtask: unsupported host OS `{other}`"),
    };
    let arch = match env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => panic!("xtask: unsupported host architecture `{other}`"),
    };
    format!("static_{os}_{arch}.zip")
}

/// Returns a directory containing `include/` and `lib/`, from `PLAYER_BINARIES_DIR` or by downloading + extracting the
/// pinned release for the host.
pub fn locate_binaries() -> PathBuf {
    if let Some(dir) = env::var_os("PLAYER_BINARIES_DIR") {
        let dir = PathBuf::from(dir);
        for sub in ["include", "lib"] {
            assert!(
                dir.join(sub).is_dir(),
                "xtask: PLAYER_BINARIES_DIR ({}) is missing the `{sub}/` subdirectory",
                dir.display()
            );
        }
        return dir;
    }

    let version = version();
    let archive = host_archive_name();
    let cache_root = env::var("XTASK_CACHE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| env::temp_dir().join("player-rs-xtask"));
    let cache_dir = cache_root.join(format!("binaries-mpv-{version}"));

    if cache_dir.join("lib").is_dir() && cache_dir.join("include").is_dir() {
        return cache_dir;
    }

    let url = format!("https://github.com/vegidio/binaries-mpv/releases/download/{version}/{archive}");
    eprintln!("xtask: downloading {url}");
    let bytes = download(&url);
    // Extract beside the cache and rename it into place, so an interrupted extraction can never leave a half-filled
    // cache that the check above would later mistake for a complete one.
    let partial = cache_root.join(format!("binaries-mpv-{version}.partial"));
    let _ = fs::remove_dir_all(&partial);
    extract_zip(&bytes, &partial);
    let _ = fs::remove_dir_all(&cache_dir);
    fs::rename(&partial, &cache_dir)
        .unwrap_or_else(|e| panic!("xtask: cannot move {} into place: {e}", partial.display()));
    cache_dir
}

fn download(url: &str) -> Vec<u8> {
    let mut reader = ureq::get(url)
        .call()
        .unwrap_or_else(|e| panic!("xtask: failed to download {url}: {e}"))
        .into_body()
        .into_reader();

    let mut bytes = Vec::new();
    io::copy(&mut reader, &mut bytes).unwrap_or_else(|e| panic!("xtask: failed to read response body from {url}: {e}"));
    bytes
}

fn extract_zip(bytes: &[u8], dest: &Path) {
    let reader = io::Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(reader).expect("xtask: invalid zip archive");

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).expect("xtask: corrupt zip entry");
        let Some(rel_path) = entry.enclosed_name() else {
            continue;
        };
        let out_path = dest.join(rel_path);

        if entry.is_dir() {
            fs::create_dir_all(&out_path).unwrap();
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut out_file =
            fs::File::create(&out_path).unwrap_or_else(|e| panic!("xtask: cannot create {}: {e}", out_path.display()));
        io::copy(&mut entry, &mut out_file).expect("xtask: failed to extract file");
    }
}
