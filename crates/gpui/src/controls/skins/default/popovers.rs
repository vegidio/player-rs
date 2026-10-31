//! The popovers above the pill: the vertical volume popover (`Skin2b`, screen 1b) and the Settings menu, on its root
//! page (`Skin2c`, screen 1c) and an item's page (`Skin2d`, screen 1d).

use super::bar::BUTTON;
use super::{GLASS, INSET, Observable, PILL_HEIGHT, POPOVER_GAP, focus_ring, white};
use crate::assets;
use crate::controls::ControlsView;
use crate::controls::skins::{icon, measure};
use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use player::ui::{Action, MenuItem, Page, SettingsMenu};
use std::rc::Rc;

/// `Skin2b`: the popover, `width: 64px; height: 192px; border-radius: 32px`, and its slider, `width: 154px` rotated,
/// `--h: 8px; --t: 24px; --track: rgba(255,255,255,.3)`.
const VOLUME_WIDTH: f32 = 32.;
const VOLUME_HEIGHT: f32 = 96.;
const VOLUME_RADIUS: f32 = 16.;
const VOLUME_LENGTH: f32 = 77.;
const VOLUME_TRACK: f32 = 4.;
const VOLUME_THUMB: f32 = 12.;
const VOLUME_BACKGROUND: u8 = 0x4d;

/// `Skin2c`: the menu, `right: 41px` (the pill's 24 plus 17), `width: 350px; padding: 6px; border-radius: 20px`; its
/// row, `height: 66px; padding: 0 17px; gap: 14px; border-radius: 14px; background: rgba(255,255,255,.12);
/// font-size: 24px`, the value at `rgba(255,255,255,.62)`, the gauge 34 and the chevron 26.
const MENU_RIGHT: f32 = 20.5;
const MENU_WIDTH: f32 = 175.;
const ROOT_PADDING: f32 = 3.;
const ROOT_RADIUS: f32 = 10.;
const ROOT_ROW: f32 = 33.;
const ROW_PADDING: f32 = 8.5;
const ROW_GAP: f32 = 7.;
const ROW_RADIUS: f32 = 7.;
const ROW_HOVER: u8 = 0x1f;
const VALUE_COLOR: u8 = 0x9e;
const MENU_FONT: f32 = 12.;
const ITEM_ICON: f32 = 17.;
const CHEVRON: f32 = 13.;

/// `Skin2d`: the page, `height: 446px; border-radius: 22px`; the header, `height: 79px; margin: 0 8px; padding-left:
/// 14px; gap: 14px; border-bottom: 1px solid rgba(255,255,255,.14)`, with a `36px` Back button at
/// `rgba(255,255,255,.8)`; the list, `padding: 8px 7px 0 8px; margin-right: 12px`, of `.opt` rows `height: 67px`,
/// hovered at `rgba(255,255,255,.1)`, checked at `.18` with a 30 check; the scrollbar, `right: 8px; top: 4px; width:
/// 6px` at `rgba(255,255,255,.35)`.
const PAGE_HEIGHT: f32 = 223.;
const PAGE_RADIUS: f32 = 11.;
const HEADER_HEIGHT: f32 = 39.5;
const HEADER_MARGIN: f32 = 4.;
const HEADER_PADDING: f32 = 7.;
const DIVIDER: u8 = 0x24;
const BACK: f32 = 18.;
const BACK_HOVER: u8 = 0x24;
const BACK_COLOR: u8 = 0xcc;
const LIST_PADDING_TOP: f32 = 4.;
const LIST_PADDING_LEFT: f32 = 4.;
const LIST_PADDING_RIGHT: f32 = 3.5;
const LIST_MARGIN_RIGHT: f32 = 6.;
const PAGE_ROW: f32 = 33.5;
const PAGE_ROW_HOVER: u8 = 0x1a;
const CHECKED: u8 = 0x2e;
const CHECK: f32 = 15.;
const SCROLLBAR_RIGHT: f32 = 4.;
const SCROLLBAR_INSET: f32 = 2.;
const SCROLLBAR_WIDTH: f32 = 3.;
const SCROLLBAR_COLOR: u8 = 0x59;

/// The space kept between a page and the player's top edge.
const PAGE_MARGIN: f32 = 12.;

/// The least a page shrinks to on a short player, its header and one entry, below which it is clipped instead.
const PAGE_MIN_HEIGHT: f32 = HEADER_HEIGHT + LIST_PADDING_TOP + PAGE_ROW;

/// How far above the player's bottom edge popovers start: the pill and the gap above it.
const POPOVER_BOTTOM: f32 = INSET + PILL_HEIGHT + POPOVER_GAP;

impl ControlsView {
    /// The vertical volume popover, centred above the mute button. Its hover region reaches down to the button, so
    /// the pointer can cross the gap without closing it.
    pub(super) fn volume_popover(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let view = &self.view;
        let mute = self.mute_bounds.get();
        let left = mute.center().x - self.root_bounds.get().left() - px(VOLUME_WIDTH / 2.);
        // From the top of the mute button, which is centred in the pill.
        let bottom = INSET + (PILL_HEIGHT + BUTTON) / 2.;
        let fraction = view.volume_fraction as f32;
        let glass = div().relative().w(px(VOLUME_WIDTH)).h(px(VOLUME_HEIGHT)).rounded(px(VOLUME_RADIUS)).child(
            div()
                .id("volume-bar")
                .test_support()
                .absolute()
                .left_0()
                .top(px((VOLUME_HEIGHT - VOLUME_LENGTH) / 2.))
                .w_full()
                .h(px(VOLUME_LENGTH))
                .flex()
                .justify_center()
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseDownEvent, window, cx| {
                        // Its focus is held by the mute button's (see `bar`).
                        window.focus(&this.volume_focus, cx);
                        this.volume_pressed(event.position, cx)
                    }),
                )
                .child(
                    div()
                        .relative()
                        .w(px(VOLUME_TRACK))
                        .h_full()
                        .rounded_full()
                        .bg(white(VOLUME_BACKGROUND))
                        .child(measure(Rc::clone(&self.volume_bounds)))
                        .child(
                            div()
                                .absolute()
                                .left_0()
                                .bottom_0()
                                .w_full()
                                .h(relative(fraction))
                                .rounded_full()
                                .bg(white(0xff)),
                        ),
                )
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .bottom(relative(fraction))
                        .w_full()
                        .h(px(0.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(div().flex_none().size(px(VOLUME_THUMB)).rounded_full().bg(white(0xff))),
                ),
        );
        div()
            .id("volume-popover")
            .test_support()
            .absolute()
            .left(left)
            .bottom(px(bottom))
            .pb(px(POPOVER_BOTTOM - bottom))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| this.volume_popover_hovered(*hovered, cx)))
            // Not a click on the video.
            .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
            .child(GLASS.apply(glass))
    }

    /// The Settings menu, at the player's right end above the pill, on whichever page is open.
    pub(super) fn settings_menu(
        &self,
        menu: SettingsMenu,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let in_menu = self.menu_focus.is_focused(window);
        let popover = div()
            .id("settings-menu")
            .test_support()
            .track_focus(&self.menu_focus)
            .absolute()
            .right(px(MENU_RIGHT))
            .bottom(px(POPOVER_BOTTOM))
            .w(px(MENU_WIDTH))
            .child(measure(Rc::clone(&self.menu_bounds)))
            // Not a click on the video, and the menu keeps focus where it is.
            .on_any_mouse_down(|_, _, cx| cx.stop_propagation());
        let popover = match menu.page {
            Page::Root => self.root_page(popover, &menu, in_menu, cx),
            page => self.item_page(popover, page, &menu, in_menu, cx),
        };
        GLASS.apply(popover)
    }

    fn root_page(&self, popover: Observable, menu: &SettingsMenu, in_menu: bool, cx: &mut Context<Self>) -> Observable {
        popover.p(px(ROOT_PADDING)).rounded(px(ROOT_RADIUS)).children(menu.items.iter().enumerate().map(
            |(index, item)| {
                row(index, item, in_menu, cx)
                    .h(px(ROOT_ROW))
                    .gap(px(ROW_GAP))
                    .hover(|style| style.bg(white(ROW_HOVER)))
                    .when_some(item.opens.and_then(item_icon), |row, name| {
                        row.child(icon(assets::lucide(name), ITEM_ICON, white(0xff)))
                    })
                    .child(div().flex_1().child(item.label.clone()))
                    .when_some(item.value.clone(), |row, value| {
                        row.child(div().text_color(white(VALUE_COLOR)).child(value))
                    })
                    .child(icon(assets::lucide(IconName::ChevronRight), CHEVRON, white(0xff)))
            },
        ))
    }

    fn item_page(
        &self,
        popover: Observable,
        page: Page,
        menu: &SettingsMenu,
        in_menu: bool,
        cx: &mut Context<Self>,
    ) -> Observable {
        // Capped to the space above the pill, so a short player does not clip it; the list then scrolls sooner.
        let available = f32::from(self.root_bounds.get().size.height) - POPOVER_BOTTOM - PAGE_MARGIN;
        let height = available.clamp(PAGE_MIN_HEIGHT, PAGE_HEIGHT);
        let header = div()
            .flex_none()
            .h(px(HEADER_HEIGHT))
            .mx(px(HEADER_MARGIN))
            .pl(px(HEADER_PADDING))
            .flex()
            .items_center()
            .gap(px(ROW_GAP))
            .border_b_1()
            .border_color(white(DIVIDER))
            .child(
                div()
                    .id("menu-back")
                    .test_support()
                    .flex_none()
                    .size(px(BACK))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .cursor_pointer()
                    .hover(|style| style.bg(white(BACK_HOVER)))
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.perform(Action::Back, window, cx)))
                    .child(icon(assets::lucide(IconName::ChevronLeft), CHEVRON, white(BACK_COLOR))),
            )
            .child(page_title(page));
        let list = div()
            .id("menu-list")
            .test_support()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .track_scroll(&self.menu_scroll)
            .pt(px(LIST_PADDING_TOP))
            .pl(px(LIST_PADDING_LEFT))
            .pr(px(LIST_PADDING_RIGHT))
            .mr(px(LIST_MARGIN_RIGHT))
            // The scrollbar is drawn from the scroll offset, which the wheel changes without the view.
            .on_scroll(cx.listener(|_, _: &ScrollEvent, _, cx| cx.notify()))
            .children(menu.items.iter().enumerate().map(|(index, item)| {
                row(index, item, in_menu, cx)
                    .flex_none()
                    .h(px(PAGE_ROW))
                    .justify_between()
                    .map(|row| {
                        if item.checked {
                            row.bg(white(CHECKED))
                        } else {
                            row.hover(|style| style.bg(white(PAGE_ROW_HOVER)))
                        }
                    })
                    .child(item.label.clone())
                    .when(item.checked, |row| row.child(icon(assets::lucide(IconName::Check), CHECK, white(0xff))))
            }));
        popover
            .h(px(height))
            .rounded(px(PAGE_RADIUS))
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(header)
            .child(list)
            .when_some(self.menu_scrollbar(height), |popover, thumb| popover.child(thumb))
    }

    /// The thin scrollbar thumb over the whole page's height, while the list overflows, from the scroll offset at
    /// the last layout.
    fn menu_scrollbar(&self, height: f32) -> Option<impl IntoElement> {
        let viewport = f32::from(self.menu_scroll.bounds().size.height);
        let max = f32::from(self.menu_scroll.max_offset().y);
        if viewport <= 0. || max <= 0. {
            return None;
        }
        let offset = (-f32::from(self.menu_scroll.offset().y)).clamp(0., max);
        let track = height - 2. * SCROLLBAR_INSET;
        let thumb = track * viewport / (viewport + max);
        let top = SCROLLBAR_INSET + (track - thumb) * offset / max;
        Some(
            div()
                .id("menu-scrollbar")
                .test_support()
                .absolute()
                .right(px(SCROLLBAR_RIGHT))
                .top(px(top))
                .w(px(SCROLLBAR_WIDTH))
                .h(px(thumb))
                .rounded_full()
                .bg(white(SCROLLBAR_COLOR)),
        )
    }
}

/// A menu row: activating it selects its entry. The entry with keyboard focus shows the focus ring while focus is in
/// the menu.
fn row(index: usize, item: &MenuItem, in_menu: bool, cx: &mut Context<ControlsView>) -> Observable {
    div()
        .id(("menu-item", index))
        .test_support()
        .flex()
        .items_center()
        .px(px(ROW_PADDING))
        .rounded(px(ROW_RADIUS))
        .text_size(px(MENU_FONT))
        .cursor_pointer()
        .when(in_menu && item.focused, |row| row.shadow(focus_ring()))
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.perform(Action::Select(index), window, cx)))
}

/// The icon of the root page's item that opens `page`.
fn item_icon(page: Page) -> Option<IconName> {
    match page {
        Page::Speed => Some(IconName::Gauge),
        _ => None,
    }
}

fn page_title(page: Page) -> impl IntoElement {
    let title = match page {
        Page::Speed => "Speed",
        _ => "",
    };
    div().text_size(px(MENU_FONT)).child(title)
}
