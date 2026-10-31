//! The pill bar (`Skin2`, screen 1a) and its seek bar.

use super::{
    DISABLED_OPACITY, GLASS, INSET, Observable, PILL_GAP, PILL_HEIGHT, PILL_PADDING_LEFT, PILL_PADDING_RIGHT,
    PILL_RADIUS, white,
};
use crate::assets;
use crate::controls::skins::{Activates, activates, icon, measure};
use crate::controls::{ControlsView, Fullscreen};
use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use player::ui::{self, Action, PlayIcon};
use std::rc::Rc;
use std::sync::Arc;

/// `Skin2`: `.btn` at `width: 48px; height: 48px; border-radius: 9999px`, `.btn-ghost:hover { background:
/// rgba(255,255,255,.14) }`.
pub(super) const BUTTON: f32 = 24.;
const BUTTON_HOVER: u8 = 0x24;

/// `Skin2b`, `Skin2c`: the mute and Settings buttons while their popover is open, a `72px` circle at
/// `rgba(255,255,255,.18)` centred on the button.
const OPEN_HIGHLIGHT: f32 = 36.;
const OPEN_BACKGROUND: u8 = 0x2e;

/// `Skin2`: the icons' `width`/`height`: play 30, volume 36, settings 34, fullscreen 32.
const PLAY_ICON: f32 = 15.;
const VOLUME_ICON: f32 = 18.;
const SETTINGS_ICON: f32 = 17.;
const FULLSCREEN_ICON: f32 = 16.;

/// `Skin2`: the time labels, `font-size: 24px; font-weight: 500; height: 32px; color: rgba(255,255,255,.95);
/// font-variant-numeric: tabular-nums`; `Skin2f`: `rgba(255,255,255,.55)` while disabled.
pub(super) const TIME_FONT: f32 = 12.;
const TIME_LINE: f32 = 16.;
const TIME_COLOR: u8 = 0xf2;
const TIME_DISABLED: u8 = 0x8c;

/// `Skin2`: the seek bar, `height: 26px; --h: 6px; --t: 14px; --track: rgba(255,255,255,.2); --fill: #fff`. The
/// design has no hovered or buffered state: hovered, the track is 5 and the thumb 10, and the buffered part is
/// `rgba(255,255,255,.35)`, between the track and the fill.
const SEEK_HEIGHT: f32 = 13.;
const SEEK_TRACK: f32 = 3.;
const SEEK_TRACK_HOVER: f32 = 5.;
const SEEK_THUMB: f32 = 7.;
const SEEK_THUMB_HOVER: f32 = 10.;
const SEEK_BACKGROUND: u8 = 0x33;
const SEEK_BUFFERED: u8 = 0x59;

/// The seek bar's hover time, which has no screen: a glass pill of the time labels' text, 4 above the track.
const HOVER_LABEL_GAP: f32 = 4.;
const HOVER_LABEL_PADDING_X: f32 = 6.;
const HOVER_LABEL_PADDING_Y: f32 = 2.;

impl ControlsView {
    pub(super) fn pill(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = &self.view;
        let enabled = view.controls_enabled;
        let pill = div()
            .id("control-bar")
            .test_support()
            .absolute()
            .left(px(INSET))
            .right(px(INSET))
            .bottom(px(INSET))
            .h(px(PILL_HEIGHT))
            .pl(px(PILL_PADDING_LEFT))
            .pr(px(PILL_PADDING_RIGHT))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(PILL_GAP))
            .rounded(px(PILL_RADIUS))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| this.bar_hovered(*hovered, cx)))
            // Presses on the bar are not clicks on the video. The controls' own listeners run before this one, and a
            // focusable control has already taken focus; anywhere else on the bar focuses the player, as the root
            // would have.
            .on_any_mouse_down(cx.listener(|this, _: &MouseDownEvent, window, cx| {
                if !window.default_prevented() {
                    window.focus(&this.focus, cx);
                }
                cx.stop_propagation();
            }))
            .when(view.shown.play, |pill| {
                let (label, action) = match view.play_icon {
                    PlayIcon::Play => ("Play", Action::Play),
                    PlayIcon::Pause => ("Pause", Action::Pause),
                    PlayIcon::Replay => ("Replay", Action::Play),
                };
                let content = icon(assets::play(view.play_icon), PLAY_ICON, white(0xff));
                pill.child(button("play", content, label, enabled, false).when(enabled, |b| b.activates(action, cx)))
            })
            .when(view.shown.volume, |pill| pill.child(self.mute(window, cx)))
            .when(view.shown.current_time, |pill| {
                pill.child(time_label("current-time", &view.current_time, enabled))
            })
            // Without the seek bar, an empty space keeps the controls after it on the right.
            .map(|pill| if view.shown.seek_bar { pill.child(self.seek_bar(cx)) } else { pill.child(div().flex_1()) })
            .when(view.shown.time_display, |pill| {
                // Not a focusable control, as in Video.js: a click switches what it shows.
                pill.child(time_label("time-display", &view.time_label, enabled).when(enabled, |label| {
                    label.cursor_pointer().on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.perform(Action::ToggleTimeDisplay, window, cx)
                    }))
                }))
            })
            .when(view.settings.button, |pill| pill.child(self.settings_button(cx)))
            .when(view.shown.fullscreen, |pill| {
                // Fullscreen stays enabled during an error.
                let on = self.fullscreen != Fullscreen::Off;
                let label = if on { "Exit Fullscreen" } else { "Fullscreen" };
                let content = icon(assets::fullscreen(on), FULLSCREEN_ICON, white(0xff));
                pill.child(button("fullscreen", content, label, true, false).activates(Action::ToggleFullscreen, cx))
            });
        GLASS.apply(pill)
    }

    fn mute(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = &self.view;
        let enabled = view.controls_enabled;
        let label = if view.muted { "Unmute" } else { "Mute" };
        let content = icon(assets::volume(view.volume_level), VOLUME_ICON, white(0xff));
        let open = view.shown.volume && self.volume_open(window);
        button("mute", content, label, enabled, open)
            .child(measure(Rc::clone(&self.mute_bounds)))
            // The volume bar's focus is kept here, right after the mute button, so Tab reaches it in control-bar
            // order whether or not its popover, drawn above the pill, is open.
            .when(enabled, |button| {
                button.child(div().id("volume-focus").track_focus(&self.volume_focus).absolute().size_0())
            })
            .when(enabled, |button| {
                button
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| this.mute_hovered(*hovered, cx)))
                    .activates(Action::ToggleMute, cx)
            })
    }

    fn settings_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let enabled = self.view.controls_enabled;
        let content = icon(assets::lucide(IconName::Settings), SETTINGS_ICON, white(0xff));
        let open = self.view.settings.menu.is_some();
        button("settings", content, "Settings", enabled, open)
            .child(measure(Rc::clone(&self.settings_bounds)))
            .when(enabled, |button| {
                // Not `Activates`: from the keyboard, focus also moves into the menu it opens.
                button
                    .track_focus(&self.settings_focus)
                    .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                        if !event.is_keyboard() {
                            this.settings_activated(false, window, cx);
                        }
                    }))
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                        if activates(event) {
                            cx.stop_propagation();
                            this.settings_activated(true, window, cx);
                        }
                    }))
            })
    }

    fn seek_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let view = &self.view;
        let enabled = view.controls_enabled;
        let hovered = self.seek_hovered && view.seekable && enabled;
        let track = if hovered { SEEK_TRACK_HOVER } else { SEEK_TRACK };
        let thumb = if hovered { SEEK_THUMB_HOVER } else { SEEK_THUMB };
        div()
            .id("seek-bar")
            .test_support()
            .flex_1()
            .min_w(px(BUTTON))
            .h(px(SEEK_HEIGHT))
            .flex()
            .items_center()
            .when(!enabled, |bar| bar.opacity(DISABLED_OPACITY))
            .when(enabled, |bar| {
                bar.track_focus(&self.seek_focus)
                    .when(view.seekable, |bar| bar.cursor_pointer())
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _, cx| this.seek_pressed(event.position, cx)),
                    )
                    .on_mouse_move(
                        cx.listener(|this, event: &MouseMoveEvent, _, cx| this.seek_hover(Some(event.position), cx)),
                    )
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                        if !hovered {
                            this.seek_hover(None, cx);
                        }
                    }))
            })
            .child(
                div()
                    .w_full()
                    .h(px(track))
                    .relative()
                    .rounded_full()
                    .bg(white(SEEK_BACKGROUND))
                    .child(measure(Rc::clone(&self.seek_bounds)))
                    .child(fill(view.buffered, white(SEEK_BUFFERED)))
                    .child(fill(view.played, white(0xff)))
                    // `Skin2f`: a disabled seek bar has no thumb.
                    .when(enabled, |track| track.child(seek_thumb(view.played, thumb)))
                    .when_some(view.hover.clone().filter(|_| hovered), |holder, hover| {
                        holder.child(hover_label(hover, track))
                    }),
            )
    }
}

/// A pill button: a round, focusable icon with a tooltip, highlighted under the pointer and, while `open`, by the
/// larger circle of its open popover. Disabled, it is dimmed and takes no input but the pill's. The caller attaches
/// its action.
fn button(id: &'static str, content: Icon, tooltip: &'static str, enabled: bool, open: bool) -> Observable {
    let hover = white(BUTTON_HOVER);
    div()
        .id(id)
        .test_support()
        .relative()
        .flex_none()
        .size(px(BUTTON))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .when(open, |button| {
            let offset = (BUTTON - OPEN_HIGHLIGHT) / 2.;
            button.child(
                div()
                    .id("open-highlight")
                    .test_support()
                    .absolute()
                    .left(px(offset))
                    .top(px(offset))
                    .size(px(OPEN_HIGHLIGHT))
                    .rounded_full()
                    .bg(white(OPEN_BACKGROUND)),
            )
        })
        .map(|button| {
            if enabled {
                button
                    .cursor_pointer()
                    .tab_index(0)
                    .hover(move |style| style.bg(hover))
                    .tooltip(move |window, cx| Tooltip::new(tooltip).build(window, cx))
            } else {
                button.opacity(DISABLED_OPACITY)
            }
        })
        .child(content)
}

/// A time label in tabular figures, dimmed while the controls are disabled.
fn time_label(id: &'static str, text: &str, enabled: bool) -> Observable {
    let tabular = FontFeatures(Arc::new(vec![("tnum".into(), 1)]));
    div()
        .id(id)
        .test_support()
        .flex_none()
        .text_size(px(TIME_FONT))
        .line_height(px(TIME_LINE))
        .font_weight(FontWeight::MEDIUM)
        .font_features(tabular)
        .whitespace_nowrap()
        .text_color(white(if enabled { TIME_COLOR } else { TIME_DISABLED }))
        .child(text.to_owned())
}

/// A rounded bar fill from the left: the seek bar's buffered and played parts.
fn fill(fraction: f64, color: Rgba) -> impl IntoElement {
    div().absolute().left_0().top_0().h_full().w(relative(fraction as f32)).rounded_full().bg(color)
}

/// The seek bar's white thumb at the end of the played part, centred on the track.
fn seek_thumb(fraction: f64, size: f32) -> impl IntoElement {
    div()
        .absolute()
        .top_0()
        .left(relative(fraction as f32))
        .w(px(0.))
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .child(div().id("seek-knob").test_support().flex_none().size(px(size)).rounded_full().bg(white(0xff)))
}

/// The seek bar's hover time, in a glass pill centred above the pointer.
fn hover_label(hover: ui::SeekHover, track: f32) -> impl IntoElement {
    let label = div()
        .id("seek-label")
        .test_support()
        .absolute()
        .bottom(px(track + HOVER_LABEL_GAP))
        .flex_none()
        .px(px(HOVER_LABEL_PADDING_X))
        .py(px(HOVER_LABEL_PADDING_Y))
        .rounded_full()
        .text_size(px(TIME_FONT))
        .line_height(px(TIME_LINE))
        .font_weight(FontWeight::MEDIUM)
        .whitespace_nowrap()
        .child(hover.label);
    div()
        .absolute()
        .top_0()
        .left(relative(hover.fraction as f32))
        .w(px(0.))
        .h_full()
        .flex()
        .justify_center()
        .child(GLASS.apply(label))
}
