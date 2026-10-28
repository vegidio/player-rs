//! Proves libmpv is linked and that the committed bindings match it. After a bump of `mpv-version.txt`, this fails
//! until the bindings are regenerated with `cargo run -p xtask -- bindings`.

#[test]
fn linked_version_matches_bindings() {
    assert_eq!(player::client_api_version(), player::BINDINGS_CLIENT_API_VERSION);
}
