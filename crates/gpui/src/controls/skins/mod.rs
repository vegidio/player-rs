//! The skins that draw a [`ControlsView`]'s [`ui::View`](player::ui::View). Only drawing lives here: the view's root
//! keeps every input listener, so clicks, keys and fullscreen work the same under every skin. The helpers below are
//! skin-independent, for every skin to use.

mod default;
mod none;

use super::ControlsView;
use crate::Skin;
use gpui_kit::component::{Icon, Sizable, Size};
use gpui_kit::*;
use player::ui::{Action, BarPolicy};
use std::cell::Cell;
use std::rc::Rc;

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
        Skin::Default => &default::DefaultSkin,
        Skin::None => &none::None,
    }
}

pub(crate) trait Activates {
    /// Performs `action` on a click, and on Space or Enter while focused.
    fn activates(self, action: Action, cx: &mut Context<ControlsView>) -> Self;
}

impl<E: StatefulInteractiveElement> Activates for E {
    fn activates(self, action: Action, cx: &mut Context<ControlsView>) -> Self {
        // GPUI also clicks a focused element on Space or Enter; those are handled as keys instead, so the global
        // Space hotkey does not also see them.
        self.on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
            if !event.is_keyboard() {
                this.perform(action, window, cx);
            }
        }))
        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
            if activates(event) {
                cx.stop_propagation();
                this.perform(action, window, cx);
            }
        }))
    }
}

/// Space or Enter without modifiers, which activate a focused button.
pub(crate) fn activates(event: &KeyDownEvent) -> bool {
    let keystroke = &event.keystroke;
    matches!(keystroke.key.as_str(), "space" | "enter") && !keystroke.modifiers.modified()
}

/// An icon from the player's assets (see `assets`), `size` pixels square, in `color`.
pub(crate) fn icon(path: impl Into<SharedString>, size: f32, color: impl Into<Hsla>) -> Icon {
    Icon::empty().path(path).with_size(Size::Size(px(size))).text_color(color.into())
}

/// Records its parent's bounds at prepaint, for measuring pointer positions and placing popovers against.
pub(crate) fn measure(bounds: Rc<Cell<Bounds<Pixels>>>) -> impl IntoElement {
    canvas(move |measured, _, _| bounds.set(measured), |_, _, _, _| {})
        .absolute()
        .top_0()
        .left_0()
        .size_full()
}

/// A glass surface: a translucent background, a 1-pixel light ring and a soft shadow, as the designs draw their bars
/// and popovers over video. The designs also blur what is behind it, which GPUI cannot do.
pub(crate) struct Glass {
    pub background: u32,
    pub ring: u32,
    /// Each shadow's vertical offset, blur and colour.
    pub shadows: [(f32, f32, u32); 2],
}

impl Glass {
    pub(crate) fn apply<E: Styled>(&self, element: E) -> E {
        let ring = BoxShadow {
            color: rgba(self.ring).into(),
            offset: point(px(0.), px(0.)),
            blur_radius: px(0.),
            spread_radius: px(1.),
            inset: false,
        };
        let shadows = self.shadows.iter().map(|&(y, blur, color)| BoxShadow {
            color: rgba(color).into(),
            offset: point(px(0.), px(y)),
            blur_radius: px(blur),
            spread_radius: px(0.),
            inset: false,
        });
        element.bg(rgba(self.background)).shadow(std::iter::once(ring).chain(shadows).collect())
    }
}
