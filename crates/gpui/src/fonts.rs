//! Inter, the Default skin's typeface, embedded and registered with GPUI's text system the first time a player is
//! created, so apps need no setup. Static faces at the weights the design uses: 400, 500 and 600. Inter is licensed
//! under the SIL Open Font License 1.1, shipped beside the faces in `assets/fonts/OFL.txt`.

use gpui_kit::{App, Global};
use std::borrow::Cow;

/// The family the faces register as.
pub(crate) const FAMILY: &str = "Inter";

const FACES: [&[u8]; 3] = [
    include_bytes!("../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../assets/fonts/Inter-SemiBold.ttf"),
];

/// Marks an app whose text system has been given the faces.
struct Registered;

impl Global for Registered {}

/// Registers Inter with the app's text system, once per app. On failure, a warning is logged and GPUI draws the
/// skin's text in its default font.
pub(crate) fn register(cx: &mut App) {
    if cx.has_global::<Registered>() {
        return;
    }
    cx.set_global(Registered);
    let faces = FACES.iter().map(|face| Cow::Borrowed(*face)).collect();
    match cx.text_system().add_fonts(faces) {
        Ok(()) => log::debug!("player_gpui: Inter is registered"),
        Err(error) => log::warn!("player_gpui: Inter could not be registered, so the default font is used: {error}"),
    }
}
