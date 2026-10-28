//! # player_gpui
//!
//! The [GPUI](https://gpui.rs) front end of `player-rs`: a video player component with the look and behaviour of
//! [Video.js](https://videojs.org) for apps built with GPUI-Kit.
//!
//! It contains no UI yet; it only depends on `player-rs`, the crate that owns all libmpv access.

#[cfg(test)]
mod tests {
    /// Links libmpv through `player-rs` alone, without linker flags of this crate's own.
    #[test]
    fn links_libmpv_through_player() {
        assert_eq!(player::client_api_version(), player::BINDINGS_CLIENT_API_VERSION);
    }
}
