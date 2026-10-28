//! Dev-only build automation for `player-rs`.
//!
//! Subcommands:
//!   bindings
//!       Download the host's `binaries-mpv` archive (or use `PLAYER_BINARIES_DIR`) and regenerate
//!       `crates/player/src/sys/bindings.rs` from the libmpv client headers. The headers have no platform types, so
//!       one run on any host produces bindings that are valid on every target.
//!
//! Example:
//!   cargo run -p xtask -- bindings

mod binaries;
mod bindings;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("bindings") if args.len() == 1 => bindings::run(),
        other => {
            eprintln!("xtask: unknown command {other:?}");
            eprintln!("usage: xtask bindings");
            std::process::exit(2);
        }
    }
}
