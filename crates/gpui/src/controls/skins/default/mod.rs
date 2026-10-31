//! The Default skin, as designed on the "Video Player Skins" canvas, screens 1a-1f (artboards `Skin2` to
//! `Skin2f.dc.html`): a floating pill bar (`bar`), the volume popover and the Settings menu (`popovers`), and the
//! spinner, the error card and the scrim (`overlays`), each part only while the [`ui::View`](player::ui::View) shows
//! it.
//!
//! Sizes are fixed, whatever the player's size: the canvas is a 2x drawing, so each is half its CSS value, which the
//! constant's comment gives with its artboard. The design blurs the video behind its glass surfaces
//! (`backdrop-filter: blur(16px)`); GPUI cannot, so they keep the design's colours without the blur.

mod bar;
mod overlays;
mod popovers;

use super::{Glass, SkinRenderer, measure};
use crate::controls::ControlsView;
use crate::fonts;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use player::ui::{self, BarPolicy};
use std::rc::Rc;

/// The canvas's text colour, white, and the alpha-only whites it layers.
const WHITE: u32 = 0xffffffff;

/// `Skin2`: the pill, `left: 25px; right: 24px; height: 87px`, 24 above the bottom (`top: 679px` of 790), so inset by
/// 12 on each side; `border-radius: 44px`, `padding: 0 17px 0 18px`, `gap: 24px`.
const INSET: f32 = 12.;
const PILL_HEIGHT: f32 = 43.5;
const PILL_RADIUS: f32 = 22.;
const PILL_PADDING_LEFT: f32 = 9.;
const PILL_PADDING_RIGHT: f32 = 8.5;
const PILL_GAP: f32 = 12.;

/// `Skin2`: the glass, `background: rgba(0,0,0,.5)`, `box-shadow: 0 0 0 1px rgba(255,255,255,.1), 0 1px 2px
/// rgba(0,0,0,.2), 0 8px 24px rgba(0,0,0,.2)`, its shadows halved.
const GLASS: Glass = Glass {
    background: 0x00000080,
    ring: 0xffffff1a,
    shadows: [(0.5, 1., 0x00000033), (4., 12., 0x00000033)],
};

/// `Skin2`: the scrim, `linear-gradient(180deg, rgba(0,0,0,.12) 0%, rgba(0,0,0,.08) 55%, rgba(0,0,0,.3) 100%)`. GPUI's
/// gradients take two stops, so it runs from .08 at 55 % to .3 at the bottom.
const SCRIM_TOP: u32 = 0x00000014;
const SCRIM_BOTTOM: u32 = 0x0000004d;
const SCRIM_START: f32 = 0.55;

/// `Skin2f`: disabled controls, `.btn:disabled { opacity: .4 }`, and the time labels at `rgba(255,255,255,.55)`.
const DISABLED_OPACITY: f32 = 0.4;

/// `Skin2b`, `Skin2c`: popovers sit `17px` above the pill.
const POPOVER_GAP: f32 = 8.5;

/// `.btn:focus-visible { box-shadow: 0 0 0 3px rgba(161,161,170,.5) }`, halved.
const FOCUS_RING: u32 = 0xa1a1aa80;
const FOCUS_RING_WIDTH: f32 = 1.5;

/// The Default skin, which shows the bar before the first play and, disabled, during an error, in place of Video.js's
/// big play button.
pub(crate) struct DefaultSkin;

impl SkinRenderer for DefaultSkin {
    fn bar_policy(&self) -> BarPolicy {
        BarPolicy { before_start: true, on_error: true }
    }

    fn render(&self, view: &ControlsView, window: &mut Window, cx: &mut Context<ControlsView>) -> AnyElement {
        let shown = &view.view;
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .font_family(fonts::FAMILY)
            .text_color(rgba(WHITE))
            .child(measure(Rc::clone(&view.root_bounds)))
            .when(shown.spinner, |root| root.child(overlays::spinner()))
            .when_some(shown.error.clone().filter(|_| shown.shown.error_display), |root, message| {
                root.child(overlays::error_card(message, cx))
            })
            .when(view.bar_drawn(), |root| root.child(view.chrome(window, cx)))
            .into_any_element()
    }
}

impl ControlsView {
    /// The layer that fades with the bar: the scrim, the pill and its popovers.
    fn chrome(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let view = &self.view;
        let layer = div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .child(overlays::scrim())
            .child(self.pill(window, cx))
            .when(view.shown.volume && self.volume_open(window), |layer| layer.child(self.volume_popover(cx)))
            .when_some(view.settings.menu.clone().filter(|_| view.controls_enabled), |layer, menu| {
                layer.child(self.settings_menu(menu, window, cx))
            });

        // Video.js fades with CSS transitions: in over 0.1 s, out over 1 s. The first appearance is not animated.
        if self.fade.generation == 0 {
            return layer.into_any_element();
        }
        let visible = view.control_bar_visible;
        let duration = if visible { ui::FADE_IN } else { ui::FADE_OUT };
        layer
            .with_animation(("fade", self.fade.generation), Animation::new(duration), move |layer, delta| {
                layer.opacity(if visible { delta } else { 1. - delta })
            })
            .into_any_element()
    }
}

/// A `div` with an id, observed by the tests' element queries.
type Observable = gpui_kit::base::ObservedElement<Stateful<Div>>;

/// The design's white at `alpha` (0 to 255).
fn white(alpha: u8) -> Rgba {
    rgba(0xffffff00 | u32::from(alpha))
}

/// The design's focus ring.
fn focus_ring() -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: rgba(FOCUS_RING).into(),
        offset: point(px(0.), px(0.)),
        blur_radius: px(0.),
        spread_radius: px(FOCUS_RING_WIDTH),
        inset: false,
    }]
}
