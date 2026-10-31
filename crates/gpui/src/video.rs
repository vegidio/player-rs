//! The view that draws a player's video surfaces with `paint_surface`: on macOS, Windows and Linux.
//! Only building the renderer and the paint call differ per platform, in `surface`.

use gpui_kit::*;
use player::Player;
#[cfg(test)]
use std::cell::Cell;
#[cfg(test)]
use std::rc::Rc;
use std::sync::Arc;
use surface::{Frame, SurfaceRenderer};

#[cfg_attr(target_os = "macos", path = "video/macos.rs")]
#[cfg_attr(windows, path = "video/windows.rs")]
#[cfg_attr(target_os = "linux", path = "video/linux.rs")]
mod surface;

#[cfg(all(test, any(windows, target_os = "linux")))]
mod pixels;

/// Owns the player's surface renderer and repaints when it publishes a frame. Kept in the element's keyed state (see
/// [`VideoPlayer`](crate::VideoPlayer)) and rendered cached, so the parent re-rendering does not re-render it.
pub(crate) struct VideoView {
    #[cfg_attr(not(test), expect(dead_code, reason = "kept with its renderer, read by the tests"))]
    player: Player,
    /// `None` if the renderer could not be created: the view then stays black.
    renderer: Option<Arc<SurfaceRenderer>>,
    /// The size last asked of the renderer, in device pixels.
    #[cfg(test)]
    requested: Rc<Cell<Option<(u32, u32)>>>,
    /// Notifies this view for each published frame; dropped with it.
    _frames: Option<Task<()>>,
    #[cfg(test)]
    renders: usize,
}

impl VideoView {
    pub(crate) fn new(player: &Player, cx: &mut Context<Self>) -> Self {
        // The renderer only signals; a frame already waiting covers any that arrive before it is handled.
        let (signal, frames) = flume::bounded(1);
        let renderer = surface::build(player, move || {
            let _ = signal.try_send(());
        });

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
            #[cfg(test)]
            requested: Rc::default(),
            _frames: task,
            #[cfg(test)]
            renders: 0,
        }
    }
}

/// Where a `width` x `height` frame goes in `bounds`: centred at its aspect ratio, as Video.js letterboxes. A frame of
/// a previous size, or rounded up to even for NV12, is fitted rather than stretched.
fn fit(bounds: Bounds<Pixels>, width: u32, height: u32) -> Bounds<Pixels> {
    ObjectFit::Contain.get_bounds(bounds, size(DevicePixels(width as i32), DevicePixels(height as i32)))
}

impl Render for VideoView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        {
            self.renders += 1;
        }
        let renderer = self.renderer.clone();
        #[cfg(test)]
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
                #[cfg(test)]
                requested.set(Some(size));
                renderer.latest()
            },
            |bounds, frame: Option<Frame>, window, _| {
                if let Some(frame) = frame {
                    surface::paint(window, fit(bounds, frame.width(), frame.height()), &frame);
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

    /// The error building a renderer fails with where this machine cannot render: on Windows, an OpenGL driver
    /// without `WGL_NV_DX_interop2`, as Mesa's on CI; on Linux, no usable DRM render node, as WSL2 without `vgem`.
    #[cfg(windows)]
    const UNAVAILABLE: &str = "cannot share textures with Direct3D 11";
    #[cfg(target_os = "linux")]
    const UNAVAILABLE: &str = "DRM render node is available";

    /// Whether the GL tests are skipped (`PLAYER_TEST_SKIP_GL`), as on CI's Windows arm64.
    fn gl_skipped() -> bool {
        std::env::var("PLAYER_TEST_SKIP_GL").is_ok_and(|value| !value.is_empty())
    }

    /// Whether no renderer can be built here, for the reason [`UNAVAILABLE`] names.
    #[cfg(any(windows, target_os = "linux"))]
    fn render_unavailable() -> bool {
        static UNAVAILABLE_HERE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *UNAVAILABLE_HERE.get_or_init(|| match surface::build(&render_player(), || {}) {
            Ok(_) => false,
            Err(Error::Render(message)) if message.contains(UNAVAILABLE) => true,
            Err(error) => panic!("the surface renderer cannot be created: {error}"),
        })
    }

    /// Whether this machine cannot show video, so the tests that need frames return early: GL tests are skipped, on
    /// Windows the OpenGL driver cannot share textures with Direct3D 11, or on Linux no render node is usable.
    pub(crate) fn cannot_show_video() -> bool {
        let reason = if gl_skipped() {
            Some("PLAYER_TEST_SKIP_GL is set, so no OpenGL context is created")
        } else {
            #[cfg(windows)]
            let reason = render_unavailable().then_some("the OpenGL driver cannot share textures with Direct3D 11");
            #[cfg(target_os = "linux")]
            let reason = render_unavailable().then_some("no usable DRM render node is available (sudo modprobe vgem)");
            #[cfg(not(any(windows, target_os = "linux")))]
            let reason = None;
            reason
        };
        if let Some(reason) = reason {
            eprintln!("skipped: {reason}");
        }
        reason.is_some()
    }

    /// Generated 160x120 video with a tone, long enough not to end during a test.
    const TEST_SOURCE: &str =
        "av://lavfi:testsrc=duration=30:size=160x120:rate=25[out0];sine=frequency=440:duration=30[out1]";

    const TIMEOUT: Duration = Duration::from_secs(15);

    /// A render player with its audio discarded.
    #[cfg(any(windows, target_os = "linux"))]
    pub(crate) fn render_player() -> Player {
        Player::builder().video_output(VideoOutput::Render).mpv_option("ao", "null").build().unwrap()
    }

    /// A playing render player with `source` loaded, before any element exists for it.
    pub(crate) fn playing(source: &str) -> Player {
        let player = Player::builder()
            .video_output(VideoOutput::Render)
            .autoplay(Autoplay::On)
            .mpv_option("ao", "null")
            .build()
            .unwrap();
        player.load(Source::url(source).unwrap()).unwrap();
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
            // Bare video: the controls re-render with the played bar, which these tests do not measure.
            let bare = player::ui::Options::builder().controls(false).build().unwrap();
            let videos = self.players.iter().enumerate().map(|(id, player)| {
                VideoPlayer::new(ElementId::Integer(id as u64), player)
                    .options(bare.clone())
                    .w(px(320.))
                    .h(px(180.))
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
        if cannot_show_video() {
            return;
        }
        // Loaded before the element is first rendered, as a view constructor would.
        let player = playing(TEST_SOURCE);
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
        if cannot_show_video() {
            return;
        }
        let (first, second) = (playing(TEST_SOURCE), playing(TEST_SOURCE));
        let harness = open(cx, &[&first]);
        let old = harness.wait_for_video(cx, 1).downgrade();

        harness.update(cx, |parent| parent.players = vec![second.clone()]);
        harness.pump(cx, "the second player's video", |cx| {
            old.upgrade().is_none()
                && matches!(&live_views()[..], [view] if shows(cx, view, &second) && sequence(cx, view) > 0)
        });

        // The first player was released and keeps playing without video.
        assert!(!first.state().paused);
        let renderer = surface::build(&first, || {});
        assert!(renderer.is_ok(), "the first player's renderer was released: {:?}", renderer.err());
    }

    #[gpui_kit::test]
    fn removing_the_element_releases_its_state(cx: &mut TestAppContext) {
        if cannot_show_video() {
            return;
        }
        let player = playing(TEST_SOURCE);
        let harness = open(cx, &[&player]);
        harness.wait_for_video(cx, 1);

        harness.update(cx, |parent| parent.show = false);
        harness.pump(cx, "the view to be released", |_| live_views().is_empty());

        let renderer = surface::build(&player, || {});
        assert!(renderer.is_ok(), "the renderer was released: {:?}", renderer.err());
        player.pause().unwrap();
        assert!(player.get_property::<bool>("pause").unwrap(), "the player still responds");
    }

    #[gpui_kit::test]
    fn two_players_side_by_side(cx: &mut TestAppContext) {
        if cannot_show_video() {
            return;
        }
        let (first, second) = (playing(TEST_SOURCE), playing(TEST_SOURCE));
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
        if cannot_show_video() {
            return;
        }
        let player = playing(TEST_SOURCE);
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
        // Built with `VideoOutput::Null`: the renderer cannot be created, and the element stays black.
        let player = Player::builder().video_output(VideoOutput::Null).mpv_option("ao", "null").build().unwrap();
        let harness = open(cx, &[&player]);
        harness.pump(cx, "the view", |_| live_views().len() == 1);
        let view = live_views().remove(0);
        assert!(cx.read(|cx| view.read(cx).renderer().is_none()));
        player.set_volume(0.5).unwrap();
    }

    /// Warnings logged on this thread since the last call, for the tests that count them.
    #[cfg(any(windows, target_os = "linux"))]
    pub(crate) fn take_warnings() -> Vec<String> {
        struct Capture;

        thread_local! {
            static WARNINGS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
        }

        impl log::Log for Capture {
            fn enabled(&self, metadata: &log::Metadata) -> bool {
                metadata.level() <= log::Level::Warn
            }

            fn log(&self, record: &log::Record) {
                if self.enabled(record.metadata()) {
                    WARNINGS.with_borrow_mut(|warnings| warnings.push(record.args().to_string()));
                }
            }

            fn flush(&self) {}
        }

        static CAPTURE: Capture = Capture;
        static INSTALLED: std::sync::Once = std::sync::Once::new();
        INSTALLED.call_once(|| {
            log::set_logger(&CAPTURE).expect("no other logger in the tests");
            log::set_max_level(log::LevelFilter::Warn);
        });
        WARNINGS.with_borrow_mut(std::mem::take)
    }

    /// The scenario of CI's Mesa on Windows, and of WSL2 without `vgem` on Linux (CI runs it with `/dev/dri` hidden):
    /// the element stays black, warns once however often it is drawn, and the player keeps working.
    #[cfg(any(windows, target_os = "linux"))]
    #[gpui_kit::test]
    fn warns_once_when_it_cannot_render(cx: &mut TestAppContext) {
        if gl_skipped() || !render_unavailable() {
            eprintln!("skipped: needs a machine that cannot render, failing with \"{UNAVAILABLE}\"");
            return;
        }
        take_warnings();
        let player = playing(TEST_SOURCE);
        let harness = open(cx, &[&player]);
        harness.pump(cx, "the view", |_| live_views().len() == 1);
        for _ in 0..5 {
            harness.update(cx, |_| {});
            harness.draw(cx);
        }

        let view = live_views().remove(0);
        assert!(cx.read(|cx| view.read(cx).renderer().is_none()), "the element stays black");
        let warnings = take_warnings();
        let about_it = warnings.iter().filter(|warning| warning.contains(UNAVAILABLE)).count();
        assert_eq!(about_it, 1, "one warning naming the reason: {warnings:?}");
        #[cfg(target_os = "linux")]
        harness.pump(cx, "playback", |_| player.state().position.is_some_and(|position| position > Duration::ZERO));
        player.set_volume(0.5).unwrap();
        assert_eq!(player.get_property::<f64>("volume").unwrap(), 50.0);
    }
}
