//! Raw, auto-generated FFI bindings to the libmpv client API.
//!
//! The contents of this module live in the **committed** `sys/bindings.rs`, produced by `cargo run -p xtask --
//! bindings` from the headers of the pinned `binaries-mpv` release: `mpv/client.h`, `mpv/render.h`,
//! `mpv/render_gl.h` and `mpv/stream_cb.h`. The headers contain no platform types and every C enum is emitted as
//! `c_int` constants, so the same file is valid on every supported target.
//!
//! Prefer the safe API of this crate; GUI crates must never call these functions directly.

#![allow(non_upper_case_globals, non_camel_case_types, non_snake_case, dead_code)]
// Generated FFI: hundreds of public items with no doc comments (see the C headers), so opt this module out of the
// crate-wide `missing_docs` warning (the module itself is documented above). The generated file cannot carry its own
// inner attributes through `include!`.
#![allow(missing_docs)]

include!("sys/bindings.rs");
