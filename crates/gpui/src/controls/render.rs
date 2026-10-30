//! Draws a [`ControlsView`]: the click target and input handling at its root, the overlays and the control bar.
//!
//! The look is Video.js 8's default skin (`video-js.css`, 8.10), whose sizes are in `em` of the player's 10 px font;
//! each constant notes the rule it comes from. Icons are Lucide's, through GPUI-Kit, in place of Video.js's icon font.

use super::{ControlsView, Fullscreen, ToggleFullscreen, ToggleMute, TogglePlay};
use crate::assets;
use gpui_kit::assets::IconName;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, Sizable, Size};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use player::ui::{self, Action, PlayIcon};
use std::cell::Cell;
use std::rc::Rc;

/// `.video-js .vjs-control-bar { height: 3em; background-color: rgba(43, 51, 63, 0.7) }`.
const BAR_HEIGHT: f32 = 30.;
const BAR_BACKGROUND: u32 = 0x2b333fb3;

/// `.video-js .vjs-control { width: 4em }`, with `.vjs-button > .vjs-icon-placeholder:before { font-size: 1.8em }`.
const BUTTON_WIDTH: f32 = 40.;
const ICON_SIZE: f32 = 18.;

/// `.video-js .vjs-slider { background-color: rgba(115, 133, 159, 0.5) }`, `.vjs-load-progress div` at 0.75, and a
/// white `.vjs-play-progress` and `.vjs-volume-level`.
const SLIDER_BACKGROUND: u32 = 0x73859f80;
const BUFFERED: u32 = 0x73859fbf;

/// `.vjs-progress-holder { height: 0.3em; margin: 0 10px }`, `font-size: 1.6666666667em` on hover: 5 px.
const SEEK_HEIGHT: f32 = 3.;
const SEEK_HOVER_HEIGHT: f32 = 5.;
const SEEK_MARGIN: f32 = 10.;

/// `.vjs-play-progress:before`: Video.js's circle icon at `font-size: 0.9em` of the holder, centred on the played
/// part's end (`right: -0.5em`), so 9 px, and 15 px while the holder is hovered.
const KNOB: f32 = 9.;
const KNOB_HOVER: f32 = 15.;

/// `.vjs-mouse-display .vjs-time-tooltip`: white on `rgba(0, 0, 0, 0.8)`, `font-size: 0.6em` of the hovered holder
/// (10 px), `padding: 6px 8px 8px 8px`, `border-radius: 0.3em`, `top: -3.4em`.
const HOVER_LABEL_BACKGROUND: u32 = 0x000000cc;
const HOVER_LABEL_TOP: f32 = -34.;

/// `.vjs-volume-control.vjs-volume-horizontal { width: 5em; margin-right: 1em }`, `.vjs-volume-bar { margin: 1.35em
/// 0.45em }`, `.vjs-slider-horizontal .vjs-volume-level { height: 0.3em }`.
const VOLUME_WIDTH: f32 = 50.;
const VOLUME_MARGIN_RIGHT: f32 = 10.;
const VOLUME_BAR_MARGIN: f32 = 4.5;
const VOLUME_HEIGHT: f32 = 3.;

/// `.video-js .vjs-time-control { font-size: 1em; line-height: 3em; padding: 0 1em }`.
const TIME_FONT: f32 = 10.;
const TIME_PADDING: f32 = 10.;

/// `.vjs-playback-rate .vjs-playback-rate-value { font-size: 1.5em }`.
const RATE_FONT: f32 = 15.;

/// `.video-js .vjs-big-play-button`: `font-size: 3em`, `width: 3em; height: 1.63332em`, `border: 0.06666em solid
/// #fff`, `border-radius: 0.3em`, the control bar's background, and `rgba(115, 133, 159, 0.5)` while the player is
/// hovered. Centred (`top: 50%; left: 50%`).
const BIG_PLAY_WIDTH: f32 = 90.;
const BIG_PLAY_HEIGHT: f32 = 49.;
const BIG_PLAY_BORDER: f32 = 2.;
const BIG_PLAY_RADIUS: f32 = 9.;
const BIG_PLAY_ICON: f32 = 30.;
const BIG_PLAY_HOVER: u32 = 0x73859f80;

/// `.vjs-loading-spinner { width: 5em; height: 5em; opacity: 0.85 }`.
const SPINNER_SIZE: f32 = 50.;
const SPINNER_OPACITY: f32 = 0.85;

/// `.video-js .vjs-modal-dialog { background: linear-gradient(180deg, rgba(0, 0, 0, 0.8), rgba(255, 255, 255, 0)) }`,
/// with `.vjs-error-display .vjs-modal-dialog-content { font-size: 1.4em; text-align: center }`.
const ERROR_TOP: u32 = 0x000000cc;
const ERROR_BOTTOM: u32 = 0xffffff00;
const ERROR_FONT: f32 = 14.;

impl Render for ControlsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.rearm(cx);
        if !self.bar_drawn() && !self.view.big_play_button && self.focus.contains_focused(window, cx) {
            // A control that faded out away, or the big play button, had focus: keep the keys in the player.
            if !self.focus.is_focused(window) {
                window.focus(&self.focus, cx);
            }
        }

        let root = div()
            .id("controls")
            .test_support()
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .on_key_down(cx.listener(Self::key_down))
            .on_action(cx.listener(|this, _: &TogglePlay, window, cx| this.action(Action::TogglePlay, window, cx)))
            .on_action(cx.listener(|this, _: &ToggleMute, window, cx| this.action(Action::ToggleMute, window, cx)))
            .on_action(
                cx.listener(|this, _: &ToggleFullscreen, window, cx| this.action(Action::ToggleFullscreen, window, cx)),
            )
            .on_mouse_move(cx.listener(|this, _: &MouseMoveEvent, _, cx| this.pointer_moved(cx)))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| this.pointer_hovered(*hovered, cx)))
            .capture_any_mouse_down(cx.listener(|this, _: &MouseDownEvent, _, cx| this.pointer_pressed(cx)))
            .on_click(cx.listener(Self::video_clicked))
            .child(self.window_listeners(cx));
        if !self.view.controls {
            return root;
        }
        root.when(self.view.spinner, |root| root.child(spinner()))
            .when_some(self.view.error.clone(), |root, message| root.child(error(message)))
            .when(self.view.big_play_button, |root| root.child(self.big_play_button(cx)))
            .when(self.bar_drawn(), |root| root.child(self.control_bar(window, cx)))
    }
}

impl ControlsView {
    /// Window-level pointer listeners, so a drag continues, and a press ends, wherever the pointer goes. They run in
    /// the capture phase, before anything in the window can stop the event.
    fn window_listeners(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity().downgrade();
        canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                let moved = this.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture {
                        let _ = moved.update(cx, |this, cx| this.window_mouse_moved(event, cx));
                    }
                });
                window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture {
                        let _ = this.update(cx, |this, cx| this.window_mouse_up(event, cx));
                    }
                });
            },
        )
        .absolute()
        .size_0()
    }

    fn control_bar(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let view = &self.view;
        let bar = div()
            .id("control-bar")
            .test_support()
            .absolute()
            .left_0()
            .bottom_0()
            .w_full()
            .h(px(BAR_HEIGHT))
            .flex()
            .flex_row()
            .bg(rgba(BAR_BACKGROUND))
            .text_color(white())
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
            .child({
                let (label, action) = match view.play_icon {
                    PlayIcon::Play => ("Play", Action::Play),
                    PlayIcon::Pause => ("Pause", Action::Pause),
                    PlayIcon::Replay => ("Replay", Action::Play),
                };
                button("play", control_icon(assets::play(view.play_icon)), label, action, cx)
            })
            .child(self.volume_panel(window, cx))
            .child(self.seek_bar(cx))
            .child(
                div()
                    .id("remaining-time")
                    .test_support()
                    .flex_none()
                    .px(px(TIME_PADDING))
                    .text_size(px(TIME_FONT))
                    .line_height(px(BAR_HEIGHT))
                    .child(view.remaining.clone()),
            )
            .when_some(view.rate.clone(), |bar, rate| {
                let label = div().text_size(px(RATE_FONT)).child(rate);
                bar.child(button("rate", label, "Playback Rate", Action::CycleRate, cx))
            })
            .child({
                let on = self.fullscreen != Fullscreen::Off;
                let label = if on { "Exit Fullscreen" } else { "Fullscreen" };
                button("fullscreen", control_icon(assets::fullscreen(on)), label, Action::ToggleFullscreen, cx)
            });

        // Video.js fades with CSS transitions: in over 0.1 s, out over 1 s. The first appearance is not animated.
        if self.fade.generation == 0 {
            return bar.into_any_element();
        }
        let visible = view.control_bar_visible;
        let duration = if visible { ui::FADE_IN } else { ui::FADE_OUT };
        bar.with_animation(("fade", self.fade.generation), Animation::new(duration), move |bar, delta| {
            bar.opacity(if visible { delta } else { 1. - delta })
        })
        .into_any_element()
    }

    fn volume_panel(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = &self.view;
        let label = if view.muted { "Unmute" } else { "Mute" };
        let expanded =
            self.volume_hovered || self.drag == Some(super::Drag::Volume) || self.volume_focus.is_focused(window);
        let bounds = Rc::clone(&self.volume_bounds);
        div()
            .id("volume-panel")
            .test_support()
            .flex()
            .flex_row()
            .flex_none()
            .h_full()
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| this.volume_panel_hovered(*hovered, cx)))
            .child(button("mute", control_icon(assets::volume(view.volume_level)), label, Action::ToggleMute, cx))
            .child(
                // Collapsed to no width until the panel is hovered or the bar focused, but always reachable by Tab.
                div()
                    .id("volume-bar")
                    .test_support()
                    .track_focus(&self.volume_focus)
                    .h_full()
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .cursor_pointer()
                    .map(|bar| {
                        if expanded {
                            bar.w(px(VOLUME_WIDTH)).mr(px(VOLUME_MARGIN_RIGHT))
                        } else {
                            bar.w(px(0.)).opacity(0.)
                        }
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _, cx| this.volume_pressed(event.position, cx)),
                    )
                    .child(track(VOLUME_BAR_MARGIN, VOLUME_HEIGHT, bounds).child(fill(view.volume_fraction, white()))),
            )
    }

    fn seek_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let view = &self.view;
        let hovered = self.seek_hovered && view.seekable;
        let bounds = Rc::clone(&self.seek_bounds);
        div()
            .id("seek-bar")
            .test_support()
            .track_focus(&self.seek_focus)
            .flex_1()
            .min_w(px(BUTTON_WIDTH))
            .h_full()
            .flex()
            .items_center()
            .when(view.seekable, |bar| bar.cursor_pointer())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| this.seek_pressed(event.position, cx)),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| this.seek_hover(Some(event.position), cx)))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if !hovered {
                    this.seek_hover(None, cx);
                }
            }))
            .child(
                track(SEEK_MARGIN, if hovered { SEEK_HOVER_HEIGHT } else { SEEK_HEIGHT }, bounds)
                    .child(fill(view.buffered, rgba(BUFFERED)))
                    .child(fill(view.played, white()))
                    .child(knob(view.played, if hovered { KNOB_HOVER } else { KNOB }))
                    .when_some(view.hover.clone().filter(|_| hovered), |holder, hover| {
                        holder.child(hover_label(hover))
                    }),
            )
    }

    fn big_play_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let hover = rgba(BIG_PLAY_HOVER);
        div().absolute().top_0().left_0().size_full().flex().items_center().justify_center().child(
            div()
                .id("big-play")
                .test_support()
                .w(px(BIG_PLAY_WIDTH))
                .h(px(BIG_PLAY_HEIGHT))
                .flex()
                .items_center()
                .justify_center()
                .border(px(BIG_PLAY_BORDER))
                .border_color(white())
                .rounded(px(BIG_PLAY_RADIUS))
                .bg(rgba(BAR_BACKGROUND))
                .hover(move |style| style.bg(hover))
                .cursor_pointer()
                .tab_index(0)
                .tooltip(|window, cx| Tooltip::new("Play Video").build(window, cx))
                .activates(Action::Play, cx)
                // Not a click on the video. The player takes focus in place of the button, which hides at once, so
                // the hotkeys work after it, as Video.js focuses the video.
                .on_any_mouse_down(cx.listener(|this, _: &MouseDownEvent, window, cx| {
                    window.focus(&this.focus, cx);
                    cx.stop_propagation();
                }))
                .child(icon(assets::play(PlayIcon::Play), BIG_PLAY_ICON)),
        )
    }
}

/// A button: Video.js's `.vjs-control`, a 40-pixel-wide, focusable icon or label with a tooltip, which acts on click
/// and on Space or Enter while focused.
fn button(
    id: &'static str,
    content: impl IntoElement,
    tooltip: &'static str,
    action: Action,
    cx: &mut Context<ControlsView>,
) -> impl IntoElement {
    div()
        .id(id)
        .test_support()
        .flex_none()
        .w(px(BUTTON_WIDTH))
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .tab_index(0)
        .tooltip(move |window, cx| Tooltip::new(tooltip).build(window, cx))
        .activates(action, cx)
        .child(content)
}

trait Activates {
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
fn activates(event: &KeyDownEvent) -> bool {
    let keystroke = &event.keystroke;
    matches!(keystroke.key.as_str(), "space" | "enter") && !keystroke.modifiers.modified()
}

fn icon(name: IconName, size: f32) -> Icon {
    Icon::new(name).with_size(Size::Size(px(size))).text_color(white())
}

fn control_icon(name: IconName) -> Icon {
    icon(name, ICON_SIZE)
}

/// The track shared by the seek and volume bars: `.vjs-slider`'s background, inset by `margin` on each side, with its
/// bounds recorded for measuring pointer positions against. The fills go in it.
fn track(margin: f32, height: f32, bounds: Rc<Cell<Bounds<Pixels>>>) -> Div {
    div()
        .flex_1()
        .mx(px(margin))
        .h(px(height))
        .relative()
        .bg(rgba(SLIDER_BACKGROUND))
        .child(measure(bounds))
}

/// A bar fill from the left: the seek bar's buffered and played parts, the volume level.
fn fill(fraction: f64, color: impl Into<Fill>) -> impl IntoElement {
    div().absolute().left_0().top_0().h_full().w(relative(fraction as f32)).bg(color)
}

/// The seek bar's circle at the end of the played part, centred on the track.
fn knob(fraction: f64, size: f32) -> impl IntoElement {
    div()
        .absolute()
        .top_0()
        .left(relative(fraction as f32))
        .w(px(0.))
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .child(div().id("seek-knob").test_support().flex_none().size(px(size)).rounded_full().bg(white()))
}

/// Records its parent's bounds at prepaint, for measuring pointer positions against.
fn measure(bounds: Rc<Cell<Bounds<Pixels>>>) -> impl IntoElement {
    canvas(move |measured, _, _| bounds.set(measured), |_, _, _, _| {})
        .absolute()
        .top_0()
        .left_0()
        .size_full()
}

/// The seek bar's hover time, centred above the pointer, with Video.js's mouse-display line under it.
fn hover_label(hover: ui::SeekHover) -> impl IntoElement {
    div()
        .absolute()
        .top_0()
        .left(relative(hover.fraction as f32))
        .w(px(1.))
        .h_full()
        .bg(black())
        .flex()
        .justify_center()
        .child(
            div()
                .id("seek-label")
                .test_support()
                .absolute()
                .top(px(HOVER_LABEL_TOP))
                .flex_none()
                .pt(px(6.))
                .px(px(8.))
                .pb(px(8.))
                .rounded(px(3.))
                .bg(rgba(HOVER_LABEL_BACKGROUND))
                .text_size(px(TIME_FONT))
                .line_height(px(TIME_FONT))
                .whitespace_nowrap()
                .child(hover.label),
        )
}

fn spinner() -> impl IntoElement {
    div().absolute().top_0().left_0().size_full().flex().items_center().justify_center().child(
        div()
            .id("spinner")
            .test_support()
            .opacity(SPINNER_OPACITY)
            .child(Spinner::new().icon(assets::SPINNER).with_size(Size::Size(px(SPINNER_SIZE))).color(white())),
    )
}

fn error(message: String) -> impl IntoElement {
    div()
        .id("error")
        .test_support()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .p(px(20.))
        .bg(linear_gradient(
            180.,
            linear_color_stop(rgba(ERROR_TOP), 0.),
            linear_color_stop(rgba(ERROR_BOTTOM), 1.),
        ))
        .text_color(white())
        .text_size(px(ERROR_FONT))
        .text_center()
        .child(message)
}
