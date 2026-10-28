//! Prints the client API version of the linked libmpv.
//!
//! ```sh
//! cargo run -p player-rs --example version
//! ```

fn main() {
    let (major, minor) = player::client_api_version();
    let (bindings_major, bindings_minor) = player::BINDINGS_CLIENT_API_VERSION;
    println!("libmpv client API {major}.{minor} (bindings {bindings_major}.{bindings_minor})");
}
