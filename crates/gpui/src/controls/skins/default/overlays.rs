//! What the Default skin lays over the video besides the pill: the scrim under it (`Skin2`), the ring spinner
//! (`Skin2e`, screen 1e) and the error card (`Skin2f`, screen 1f).

use super::{GLASS, SCRIM_BOTTOM, SCRIM_START, SCRIM_TOP, white};
use crate::assets;
use crate::controls::ControlsView;
use crate::controls::skins::{Activates, icon};
use gpui_kit::assets::IconName;
use gpui_kit::*;
use player::ui::Action;
use std::time::Duration;

/// `Skin2e`: `width: 112px; height: 112px; border-radius: 9999px` of glass, holding a `60px` ring whose track is
/// `rgba(255,255,255,.22)`, turning once every `.9s`.
const SPINNER: f32 = 56.;
const RING: f32 = 30.;
const RING_TRACK: u8 = 0x38;
const TURN: Duration = Duration::from_millis(900);

/// `Skin2f`: the card, `width: 520px; max-width: calc(100% - 48px); padding: 40px 40px 36px; border-radius: 32px`;
/// the badge, `72px` at `rgba(255,255,255,.14)` holding a 38 icon; the title, `margin-top: 24px; font-size: 28px;
/// line-height: 34px; font-weight: 600`; the message, `margin-top: 12px; font-size: 20px; line-height: 28px;
/// color: rgba(255,255,255,.72)`; the button, `margin-top: 32px; height: 56px; padding: 0 32px; gap: 10px;
/// background: #fff; color: #18181b; font-size: 20px; font-weight: 600`, with a 24 icon.
const CARD_WIDTH: f32 = 260.;
const CARD_MARGIN: f32 = 12.;
const CARD_PADDING: f32 = 20.;
const CARD_PADDING_BOTTOM: f32 = 18.;
const CARD_RADIUS: f32 = 16.;
const BADGE: f32 = 36.;
const BADGE_BACKGROUND: u8 = 0x24;
const BADGE_ICON: f32 = 19.;
const TITLE_GAP: f32 = 12.;
const TITLE_FONT: f32 = 14.;
const TITLE_LINE: f32 = 17.;
const MESSAGE_GAP: f32 = 6.;
const MESSAGE_FONT: f32 = 10.;
const MESSAGE_LINE: f32 = 14.;
const MESSAGE_COLOR: u8 = 0xb8;
const BUTTON_GAP: f32 = 16.;
const BUTTON_HEIGHT: f32 = 28.;
const BUTTON_PADDING: f32 = 16.;
const BUTTON_ICON_GAP: f32 = 5.;
const BUTTON_TEXT: u32 = 0x18181bff;
const BUTTON_FONT: f32 = 10.;
const BUTTON_ICON: f32 = 12.;

/// The error card's title, the design's generic one; mpv's message is its description.
const ERROR_TITLE: &str = "This video can't be played";

/// The faint gradient over the video beneath the bar, which fades with it.
pub(super) fn scrim() -> impl IntoElement {
    div().absolute().top_0().left_0().size_full().bg(linear_gradient(
        180.,
        linear_color_stop(rgba(SCRIM_TOP), SCRIM_START),
        linear_color_stop(rgba(SCRIM_BOTTOM), 1.),
    ))
}

/// The ring spinner: a faint full circle with a white quarter arc turning over it, in a glass circle centred over
/// the video. GPUI tints an SVG with one colour, so the two are separate layers.
pub(super) fn spinner() -> impl IntoElement {
    let ring =
        div()
            .relative()
            .size(px(RING))
            .child(div().absolute().top_0().left_0().child(icon(assets::SPINNER_TRACK, RING, white(RING_TRACK))))
            .child(div().absolute().top_0().left_0().child(
                icon(assets::SPINNER_ARC, RING, white(0xff)).with_animation(
                    "turn",
                    Animation::new(TURN).repeat(),
                    |arc, delta| arc.transform(Transformation::rotate(percentage(delta))),
                ),
            ));
    let circle = div()
        .id("spinner")
        .test_support()
        .size(px(SPINNER))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .child(ring);
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .child(GLASS.apply(circle))
}

/// The error card centred over the video: an alert badge, the title, mpv's message, and Try again, which retries.
pub(super) fn error_card(message: String, cx: &mut Context<ControlsView>) -> impl IntoElement {
    let badge = div()
        .size(px(BADGE))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(white(BADGE_BACKGROUND))
        .child(icon(assets::lucide(IconName::CircleAlert), BADGE_ICON, white(0xff)));
    let retry = div()
        .id("try-again")
        .test_support()
        .mt(px(BUTTON_GAP))
        .h(px(BUTTON_HEIGHT))
        .px(px(BUTTON_PADDING))
        .flex()
        .items_center()
        .gap(px(BUTTON_ICON_GAP))
        .rounded_full()
        .bg(white(0xff))
        .text_color(rgba(BUTTON_TEXT))
        .text_size(px(BUTTON_FONT))
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .tab_index(0)
        .focus_visible(|style| style.shadow(super::focus_ring()))
        .activates(Action::Retry, cx)
        .child(icon(assets::lucide(IconName::RotateCcw), BUTTON_ICON, rgba(BUTTON_TEXT)))
        .child("Try again");
    let card = div()
        .id("error")
        .test_support()
        .w(px(CARD_WIDTH))
        .max_w_full()
        .pt(px(CARD_PADDING))
        .px(px(CARD_PADDING))
        .pb(px(CARD_PADDING_BOTTOM))
        .flex()
        .flex_col()
        .items_center()
        .text_center()
        .rounded(px(CARD_RADIUS))
        .role(Role::Alert)
        .aria_label(ERROR_TITLE)
        .aria_description(message.clone())
        // Not a click on the video.
        .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
        .child(badge)
        .child(
            div()
                .id("error-title")
                .test_support()
                .mt(px(TITLE_GAP))
                .text_size(px(TITLE_FONT))
                .line_height(px(TITLE_LINE))
                .font_weight(FontWeight::SEMIBOLD)
                .child(ERROR_TITLE),
        )
        .child(
            div()
                .id("error-message")
                .test_support()
                .mt(px(MESSAGE_GAP))
                .text_size(px(MESSAGE_FONT))
                .line_height(px(MESSAGE_LINE))
                .text_color(white(MESSAGE_COLOR))
                .child(message),
        )
        .child(retry);
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .p(px(CARD_MARGIN))
        .child(GLASS.apply(card))
}
