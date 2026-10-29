//! [`VideoPlayer`]: a player's video and its Video.js controls as a GPUI element.

use crate::controls::{ControlsView, FullscreenCallback};
#[cfg(video)]
use crate::video::VideoView;
use gpui_kit::*;
use player::{Player, ui};
use std::rc::Rc;

/// Shows a [`Player`]'s video, letterboxed on black, with Video.js's controls over it. Size and position it like any
/// `div`.
///
/// Its state, including the player's video surfaces and its controls, is kept per element id for as long as an element
/// with that id is rendered in consecutive frames, so the parent view can re-render freely. Rendering it with a
/// different player under the same id switches to that player. Once no element with the id is rendered, the state is
/// released and the player keeps playing without video.
///
/// The player must be built with [`VideoOutput::Render`](player::VideoOutput::Render). Video is shown on macOS, and on
/// Windows with a patched GPUI (see the [crate's setup](crate#setup)). On Linux, and wherever the video cannot be
/// shown, the element is black, while audio and the controls keep working.
#[derive(IntoElement)]
pub struct VideoPlayer {
    id: ElementId,
    player: Player,
    options: ui::Options,
    on_fullscreen_change: Option<FullscreenCallback>,
    style: StyleRefinement,
}

/// The element's keyed state. The views are separate entities, so they can be replaced when the element is given a
/// different player, and are rendered cached, so neither the parent re-rendering nor video frames re-render the
/// controls.
struct Slot {
    #[cfg(video)]
    video: Entity<VideoView>,
    controls: Entity<ControlsView>,
}

impl Slot {
    fn new(player: &Player, options: ui::Options, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            #[cfg(video)]
            video: cx.new(|cx| VideoView::new(player, cx)),
            controls: cx.new(|cx| ControlsView::new(player, options, window, cx)),
        }
    }
}

impl VideoPlayer {
    /// Creates the element for `player`, keeping its state under `id`.
    pub fn new(id: impl Into<ElementId>, player: &Player) -> Self {
        Self {
            id: id.into(),
            player: player.clone(),
            options: ui::Options::default(),
            on_fullscreen_change: None,
            style: StyleRefinement::default(),
        }
    }

    /// The player's UI options: controls, playback rates, aspect ratio, hotkeys and so on. Defaults to
    /// [`ui::Options::default`], which shows the controls.
    ///
    /// As Video.js reads its options at construction, they are read when the element's state is created and when it
    /// is given a different player; changing them on a later render under the same id and player has no effect.
    /// With an [aspect ratio](ui::OptionsBuilder::aspect_ratio), the element's height follows its width, as in
    /// Video.js's fluid mode, unless a height is set on the element.
    pub fn options(mut self, options: ui::Options) -> Self {
        self.options = options;
        self
    }

    /// Called with `true` when the player enters fullscreen and with `false` when it leaves, including when the window
    /// leaves fullscreen by other means. The player's fullscreen is its window's, so an app typically lays out the
    /// player alone while it is on.
    pub fn on_fullscreen_change(mut self, callback: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_fullscreen_change = Some(Rc::new(callback));
        self
    }
}

impl Styled for VideoPlayer {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for VideoPlayer {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Self { id, player, options, on_fullscreen_change, style } = self;
        let slot = window.use_keyed_state(id.clone(), cx, |window, cx| Slot::new(&player, options.clone(), window, cx));
        let controls = slot.update(cx, |slot, cx| {
            if !Player::ptr_eq(slot.controls.read(cx).player(), &player) {
                // The previous views, and with them the renderer, are released once nothing holds them.
                *slot = Slot::new(&player, options, window, cx);
            }
            slot.controls.clone()
        });
        // Not an option: the latest callback applies. Nothing drawn depends on it, so the view is not notified.
        let aspect_ratio = controls.update(cx, |controls, _| {
            controls.on_fullscreen_change = on_fullscreen_change;
            controls.options().aspect_ratio()
        });

        let mut root = div().id(id).relative().bg(black()).overflow_hidden();
        if let Some(ratio) = aspect_ratio {
            // Video.js's fluid mode. The app's style is refined over it, so a height set on the element still wins.
            root = root.aspect_ratio((ratio.width() / ratio.height()) as f32);
        }
        root.style().refine(&style);

        #[cfg(video)]
        let root = root.child(slot.read(cx).video.clone().cached(StyleRefinement::default().size_full()));
        root.child(controls.cached(StyleRefinement::default().absolute().top_0().left_0().size_full()))
    }
}

#[cfg(all(test, not(video)))]
mod tests {
    use super::*;
    use gpui_kit::TestAppContext;
    use player::VideoOutput;
    // The glob above brings in GPUI's `test` macro; `#[gpui_kit::test]` expands to Rust's `#[test]`, named here.
    use std::prelude::v1::test;

    struct Parent(Player);

    impl Render for Parent {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(VideoPlayer::new("video", &self.0).size_full())
        }
    }

    /// Without video surfaces the element is a black `div`, and the player keeps working.
    #[gpui_kit::test]
    fn builds_and_draws_black(cx: &mut TestAppContext) {
        // The controls hear the player's events from mpv's thread, which the test scheduler must accept.
        cx.executor().allow_parking();
        let player = Player::builder().video_output(VideoOutput::Render).mpv_option("ao", "null").build().unwrap();
        let window = cx.add_window({
            let player = player.clone();
            move |_, _| Parent(player)
        });
        cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx)).unwrap();
        player.set_volume(0.5).unwrap();
        assert_eq!(player.get_property::<f64>("volume").unwrap(), 50.0);
    }
}
