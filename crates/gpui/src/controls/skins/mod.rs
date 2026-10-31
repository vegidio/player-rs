//! The skins that draw a [`ControlsView`]'s [`ui::View`](player::ui::View). Only drawing lives here: the view's root
//! keeps every input listener, so clicks, keys and fullscreen work the same under every skin.

mod interim;
mod none;

use super::ControlsView;
use crate::Skin;
use gpui_kit::*;
use player::ui::BarPolicy;

/// Draws the controls for one [`Skin`].
pub(crate) trait SkinRenderer {
    /// When the skin's design shows the control bar besides Video.js's rules.
    fn bar_policy(&self) -> BarPolicy;

    /// Whether anything the skin draws depends on the view, so the controls re-render when it changes.
    fn draws_view(&self) -> bool {
        true
    }

    /// The skin's elements, laid over the video inside the view's root.
    fn render(&self, view: &ControlsView, window: &mut Window, cx: &mut Context<ControlsView>) -> AnyElement;
}

/// The renderer of `skin`.
pub(crate) fn renderer(skin: Skin) -> &'static dyn SkinRenderer {
    match skin {
        Skin::Default => &interim::Interim,
        Skin::None => &none::None,
    }
}
