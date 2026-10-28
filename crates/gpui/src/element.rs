//! [`VideoPlayer`]: a player's video as a GPUI element.

use gpui_kit::*;
use player::Player;

/// Shows a [`Player`]'s video, letterboxed on black, and repaints as frames arrive. Size and position it like any
/// `div`.
///
/// Its state, including the player's video surfaces, is kept per element id for as long as an element with that id is
/// rendered in consecutive frames, so the parent view can re-render freely. Rendering it with a different player under
/// the same id switches to that player's video. Once no element with the id is rendered, the state is released and the
/// player keeps playing without video.
///
/// The player must be built with [`VideoOutput::Render`](player::VideoOutput::Render). Video is shown on macOS only:
/// on Windows and Linux the element is black, while audio and control keep working.
#[derive(IntoElement)]
pub struct VideoPlayer {
    id: ElementId,
    #[cfg_attr(not(target_os = "macos"), expect(dead_code, reason = "no video surfaces on this platform yet"))]
    player: Player,
    style: StyleRefinement,
}

impl VideoPlayer {
    /// Creates the element for `player`, keeping its state under `id`.
    pub fn new(id: impl Into<ElementId>, player: &Player) -> Self {
        Self { id: id.into(), player: player.clone(), style: StyleRefinement::default() }
    }
}

impl Styled for VideoPlayer {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for VideoPlayer {
    #[cfg_attr(
        not(target_os = "macos"),
        expect(unused_variables, reason = "no video surfaces on this platform yet")
    )]
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let mut root = div().id(self.id.clone()).bg(black()).overflow_hidden();
        root.style().refine(&self.style);

        #[cfg(target_os = "macos")]
        let root = root.child(crate::video::view(self.id, &self.player, window, cx));
        root
    }
}

#[cfg(all(test, not(target_os = "macos")))]
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
