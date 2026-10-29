//! Sets the `video` cfg where the element shows video: on macOS and Windows.

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rustc-check-cfg=cfg(video)");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if os == "macos" || os == "windows" {
        println!("cargo::rustc-cfg=video");
    }
}
