//! macOS: the view that draws a player's NV12 surfaces with `paint_surface`, and the keyed state that holds it.

use gpui_kit::*;
use player::Player;
use player::macos::{Frame, GlRenderer, SurfaceFormat, SurfaceRenderer};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

/// The element's keyed state. The view is a separate entity so that it can be replaced when the element is given a
/// different player, and is rendered cached, so the parent re-rendering does not re-render it.
struct Slot {
    view: Entity<VideoView>,
}

/// The video view for `player`, kept under `id` (see [`VideoPlayer`](crate::VideoPlayer)).
pub(crate) fn view(id: ElementId, player: &Player, window: &mut Window, cx: &mut App) -> AnyElement {
    let slot = window.use_keyed_state(id, cx, |_, cx| Slot { view: cx.new(|cx| VideoView::new(player, cx)) });
    let view = slot.update(cx, |slot, cx| {
        if !Player::ptr_eq(&slot.view.read(cx).player, player) {
            // The previous view, and with it its renderer, is released once nothing holds it.
            slot.view = cx.new(|cx| VideoView::new(player, cx));
        }
        slot.view.clone()
    });
    view.cached(StyleRefinement::default().size_full()).into_any_element()
}

/// Owns the player's surface renderer and repaints when it publishes a frame.
pub(crate) struct VideoView {
    player: Player,
    /// `None` if the renderer could not be created: the view then stays black.
    renderer: Option<Arc<SurfaceRenderer>>,
    /// The size last asked of the renderer, in device pixels.
    requested: Rc<Cell<Option<(u32, u32)>>>,
    /// Notifies this view for each published frame; dropped with it.
    _frames: Option<Task<()>>,
    #[cfg(test)]
    renders: usize,
}

impl VideoView {
    fn new(player: &Player, cx: &mut Context<Self>) -> Self {
        // The renderer only signals; a frame already waiting covers any that arrive before it is handled.
        let (signal, frames) = flume::bounded(1);
        let renderer = SurfaceRenderer::builder(player)
            .format(SurfaceFormat::Nv12)
            .gl_renderer(gl_renderer())
            .on_frame(move || {
                let _ = signal.try_send(());
            })
            .build();

        let (renderer, task) = match renderer {
            Ok(renderer) => {
                let task = cx.spawn(async move |this, cx| {
                    while frames.recv_async().await.is_ok() {
                        if this.update(cx, |_, cx| cx.notify()).is_err() {
                            break;
                        }
                    }
                });
                (Some(Arc::new(renderer)), Some(task))
            }
            Err(error) => {
                log::warn!("player_gpui: the video cannot be shown: {error}");
                (None, None)
            }
        };

        #[cfg(test)]
        tests::created(cx.entity().downgrade());
        Self {
            player: player.clone(),
            renderer,
            requested: Rc::default(),
            _frames: task,
            #[cfg(test)]
            renders: 0,
        }
    }
}

/// The OpenGL renderer: the GPU where there is one. The tests follow the crate's `PLAYER_TEST_GL` convention instead:
/// Apple's software renderer, as on CI, unless `PLAYER_TEST_GL=hardware`.
fn gl_renderer() -> GlRenderer {
    if !cfg!(test) {
        GlRenderer::Auto
    } else if std::env::var("PLAYER_TEST_GL").is_ok_and(|value| value == "hardware") {
        GlRenderer::Hardware
    } else {
        GlRenderer::Software
    }
}

impl Render for VideoView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        {
            self.renders += 1;
        }
        let renderer = self.renderer.clone();
        let requested = Rc::clone(&self.requested);
        canvas(
            move |bounds, window, _| {
                let renderer = renderer?;
                // Rendered at the element's size on screen, so the compositor never upscales a smaller picture.
                let scale = window.scale_factor();
                let device = |length: Pixels| (f32::from(length) * scale).round() as u32;
                let size = (device(bounds.size.width), device(bounds.size.height));
                // The renderer ignores a size it already has, so this does not cause a frame by itself.
                renderer.set_size(size.0, size.1);
                requested.set(Some(size));
                renderer.latest()
            },
            |bounds, frame: Option<Frame>, window, _| {
                if let Some(frame) = frame {
                    // A frame of a previous size, or rounded up to even for NV12, is fitted rather than stretched.
                    let frame_size = size(DevicePixels(frame.width() as i32), DevicePixels(frame.height() as i32));
                    let fitted = ObjectFit::Contain.get_bounds(bounds, frame_size);
                    window.paint_surface(fitted, frame.pixel_buffer().clone());
                }
            },
        )
        .size_full()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::VideoPlayer;
    use gpui_kit::TestAppContext;
    use player::prelude::*;
    use std::cell::RefCell;
    // The glob above brings in GPUI's `test` macro; `#[gpui_kit::test]` expands to Rust's `#[test]`, named here.
    use std::prelude::v1::test;
    use std::time::{Duration, Instant};

    thread_local! {
        /// Every video view created on this thread, so tests can find the views their windows created.
        static VIEWS: RefCell<Vec<WeakEntity<VideoView>>> = const { RefCell::new(Vec::new()) };
    }

    pub(super) fn created(view: WeakEntity<VideoView>) {
        VIEWS.with_borrow_mut(|views| views.push(view));
    }

    /// The video views created so far, oldest first, as weak handles.
    pub(crate) fn views() -> Vec<WeakEntity<VideoView>> {
        VIEWS.with_borrow(Clone::clone)
    }

    impl VideoView {
        pub(crate) fn player(&self) -> &Player {
            &self.player
        }

        pub(crate) fn renderer(&self) -> Option<&SurfaceRenderer> {
            self.renderer.as_deref()
        }

        pub(crate) fn requested(&self) -> Option<(u32, u32)> {
            self.requested.get()
        }

        pub(crate) fn renders(&self) -> usize {
            self.renders
        }
    }

    /// Generated 160x120 video with a tone, long enough not to end during a test.
    const TEST_SOURCE: &str =
        "av://lavfi:testsrc=duration=30:size=160x120:rate=25[out0];sine=frequency=440:duration=30[out1]";

    const TIMEOUT: Duration = Duration::from_secs(15);

    /// A playing render player with media loaded, before any element exists for it.
    fn playing() -> Player {
        let player = Player::builder()
            .video_output(VideoOutput::Render)
            .autoplay(Autoplay::On)
            .mpv_option("ao", "null")
            .build()
            .unwrap();
        player.load(Source::url(TEST_SOURCE).unwrap()).unwrap();
        player
    }

    /// The application's view: one 320x180-point video element per player, under ids 0, 1, ...
    struct Parent {
        players: Vec<Player>,
        show: bool,
        renders: usize,
    }

    impl Render for Parent {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.renders += 1;
            let videos =
                self.players.iter().enumerate().map(|(id, player)| {
                    VideoPlayer::new(ElementId::Integer(id as u64), player).w(px(320.)).h(px(180.))
                });
            div().size_full().children(self.show.then_some(videos).into_iter().flatten())
        }
    }

    /// The window's root, which GPUI re-renders on every draw, showing the parent as a cached view, so the parent
    /// renders only when it, or a view inside it, is marked dirty.
    struct Root(Entity<Parent>);

    impl Render for Root {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(self.0.clone().cached(StyleRefinement::default().size_full()))
        }
    }

    struct Harness {
        window: WindowHandle<Root>,
        parent: Entity<Parent>,
    }

    /// Opens a window showing `players`. Frames are announced from the render thread, so the test scheduler must
    /// accept wake-ups from other threads.
    fn open(cx: &mut TestAppContext, players: &[&Player]) -> Harness {
        cx.executor().allow_parking();
        let players = players.iter().map(|&player| player.clone()).collect();
        let parent = cx.new(|_| Parent { players, show: true, renders: 0 });
        let window = cx.add_window({
            let parent = parent.clone();
            move |_, _| Root(parent)
        });
        Harness { window, parent }
    }

    impl Harness {
        fn draw(&self, cx: &mut TestAppContext) {
            cx.update_window(self.window.into(), |_, window, cx| window.draw(cx).clear(cx)).unwrap();
        }

        /// Changes the parent and has it re-render.
        fn update(&self, cx: &mut TestAppContext, change: impl FnOnce(&mut Parent)) {
            cx.update(|cx| {
                self.parent.update(cx, |parent, cx| {
                    change(parent);
                    cx.notify();
                })
            });
        }

        fn parent_renders(&self, cx: &TestAppContext) -> usize {
            cx.read(|cx| self.parent.read(cx).renders)
        }

        /// Runs GPUI's tasks and draws frames until `done`. Polls with short sleeps: frames are announced from the
        /// render thread, which the test scheduler cannot wait on.
        fn pump(&self, cx: &mut TestAppContext, what: &str, mut done: impl FnMut(&mut TestAppContext) -> bool) {
            let deadline = Instant::now() + TIMEOUT;
            loop {
                cx.run_until_parked();
                self.draw(cx);
                if done(cx) {
                    return;
                }
                assert!(Instant::now() < deadline, "timed out waiting for {what}");
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        /// The single live view, once it has published at least `frames` frames.
        fn wait_for_video(&self, cx: &mut TestAppContext, frames: u64) -> Entity<VideoView> {
            self.pump(cx, "video frames", |cx| matches!(&live_views()[..], [view] if sequence(cx, view) >= frames));
            live_views().remove(0)
        }
    }

    fn live_views() -> Vec<Entity<VideoView>> {
        views().iter().filter_map(WeakEntity::upgrade).collect()
    }

    /// The sequence number of the view's latest frame, 0 before the first.
    fn sequence(cx: &TestAppContext, view: &Entity<VideoView>) -> u64 {
        cx.read(|cx| view.read(cx).renderer().and_then(SurfaceRenderer::latest).map_or(0, |frame| frame.sequence()))
    }

    fn shows(cx: &TestAppContext, view: &Entity<VideoView>, player: &Player) -> bool {
        cx.read(|cx| Player::ptr_eq(view.read(cx).player(), player))
    }

    #[gpui_kit::test]
    fn state_survives_parent_rerenders(cx: &mut TestAppContext) {
        // Loaded before the element is first rendered, as a view constructor would.
        let player = playing();
        let harness = open(cx, &[&player]);
        let view = harness.wait_for_video(cx, 3);
        assert!(shows(cx, &view, &player));

        for _ in 0..10 {
            harness.update(cx, |_| {});
            harness.draw(cx);
        }
        let before = sequence(cx, &view);
        let (parent_before, video_before) = (harness.parent_renders(cx), cx.read(|cx| view.read(cx).renders()));
        harness.pump(cx, "more frames", |cx| sequence(cx, &view) >= before + 5);
        assert_eq!(views().len(), 1, "re-rendering the parent created no new view");
        assert_eq!(live_views()[0].entity_id(), view.entity_id(), "the same view keeps playing");

        // Each frame repaints the video view and, as for any GPUI view that notifies, re-renders the views it is in,
        // once per frame and no more often.
        let frames = (sequence(cx, &view) - before) as usize;
        let parent = harness.parent_renders(cx) - parent_before;
        let video = cx.read(|cx| view.read(cx).renders()) - video_before;
        assert!(video >= 3, "the video view repaints as frames arrive");
        assert!(
            parent <= frames + 1,
            "the parent re-renders at most once per frame: {parent} times for {frames}"
        );
    }

    #[gpui_kit::test]
    fn switches_player_under_the_same_id(cx: &mut TestAppContext) {
        let (first, second) = (playing(), playing());
        let harness = open(cx, &[&first]);
        let old = harness.wait_for_video(cx, 1).downgrade();

        harness.update(cx, |parent| parent.players = vec![second.clone()]);
        harness.pump(cx, "the second player's video", |cx| {
            old.upgrade().is_none()
                && matches!(&live_views()[..], [view] if shows(cx, view, &second) && sequence(cx, view) > 0)
        });

        // The first player was released and keeps playing without video.
        assert!(!first.state().paused);
        let renderer = SurfaceRenderer::builder(&first).gl_renderer(gl_renderer()).build();
        assert!(renderer.is_ok(), "the first player's renderer was released: {renderer:?}");
    }

    #[gpui_kit::test]
    fn removing_the_element_releases_its_state(cx: &mut TestAppContext) {
        let player = playing();
        let harness = open(cx, &[&player]);
        harness.wait_for_video(cx, 1);

        harness.update(cx, |parent| parent.show = false);
        harness.pump(cx, "the view to be released", |_| live_views().is_empty());

        let renderer = SurfaceRenderer::builder(&player).gl_renderer(gl_renderer()).build();
        assert!(renderer.is_ok(), "the renderer was released: {renderer:?}");
        player.pause().unwrap();
        assert!(player.get_property::<bool>("pause").unwrap(), "the player still responds");
    }

    #[gpui_kit::test]
    fn two_players_side_by_side(cx: &mut TestAppContext) {
        let (first, second) = (playing(), playing());
        let harness = open(cx, &[&first, &second]);
        harness.pump(cx, "both videos", |cx| {
            let views = live_views();
            views.len() == 2 && views.iter().all(|view| sequence(cx, view) > 0)
        });
        let views = live_views();
        assert!(shows(cx, &views[0], &first) && shows(cx, &views[1], &second));
    }

    #[gpui_kit::test]
    fn renders_at_bounds_times_scale_factor(cx: &mut TestAppContext) {
        let player = playing();
        let harness = open(cx, &[&player]);
        let view = harness.wait_for_video(cx, 1);
        let scale = cx.update_window(harness.window.into(), |_, window, _| window.scale_factor()).unwrap();
        let expected = ((320.0 * scale).round() as u32, (180.0 * scale).round() as u32);
        assert_eq!(cx.read(|cx| view.read(cx).requested()), Some(expected));
        harness.pump(cx, "a frame at that size", |cx| {
            let latest = cx.read(|cx| view.read(cx).renderer().and_then(SurfaceRenderer::latest));
            latest.is_some_and(|frame| (frame.width(), frame.height()) == expected)
        });
    }

    #[gpui_kit::test]
    fn stays_black_when_the_player_cannot_render(cx: &mut TestAppContext) {
        // Built without `VideoOutput::Render`: the renderer cannot be created, and the element stays black.
        let player = Player::builder().mpv_option("ao", "null").build().unwrap();
        let harness = open(cx, &[&player]);
        harness.pump(cx, "the view", |_| live_views().len() == 1);
        let view = live_views().remove(0);
        assert!(cx.read(|cx| view.read(cx).renderer().is_none()));
        player.set_volume(0.5).unwrap();
    }
}
