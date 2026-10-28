//! Rendering through [`RenderContext`] into an offscreen framebuffer of a headless GL context (see `common/gl.rs`),
//! against media mpv generates itself (`av://lavfi:testsrc`), with null audio. Drawn pixels are checked for properties
//! of the test pattern (many colours, black bars, top/bottom asymmetry) rather than exact images, so different
//! software rasterisers cannot make them flaky. Waiting is always done on channels with a deadline, never with sleeps.

mod common {
    pub mod gl;
}

use common::gl::{self, Framebuffer, Gl, Image};
use player::prelude::*;
use std::collections::HashSet;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

/// Generated 160x120 video, long enough not to end during a test.
const TEST_SOURCE: &str = "av://lavfi:testsrc=duration=30:size=160x120:rate=25";

/// The same video with a sine tone, for the scenarios where playback must go on without video: without audio, mpv
/// ends the media once video is gone.
const TEST_SOURCE_WITH_AUDIO: &str =
    "av://lavfi:testsrc=duration=30:size=160x120:rate=25[out0];sine=frequency=440:duration=30[out1]";

/// How long to wait for anything expected before failing.
const TIMEOUT: Duration = Duration::from_secs(15);

/// A render player, its render context and the channel its `on_frame` notifications arrive on, with the GL context
/// they draw with. Declared so the render context is dropped before the GL context.
struct Rig {
    render: RenderContext,
    frames: Receiver<()>,
    player: Player,
    gl: Gl,
}

fn render_player(autoplay: Autoplay) -> Player {
    Player::builder()
        .video_output(VideoOutput::Render)
        .autoplay(autoplay)
        .mpv_option("ao", "null")
        .build()
        .expect("the player builds")
}

/// Creates a render context for `player` on `gl`, notifying on the returned channel.
fn render_context(gl: &Gl, player: &Player) -> (RenderContext, Receiver<()>) {
    let (sender, frames) = mpsc::channel();
    // SAFETY: `gl` is current on this thread, where the render context is used and dropped, and outlives it.
    let render = unsafe {
        RenderContext::new(player, gl.loader(), move || {
            let _ = sender.send(());
        })
    }
    .expect("the render context is created");
    (render, frames)
}

fn rig(autoplay: Autoplay) -> Rig {
    let gl = Gl::new();
    let player = render_player(autoplay);
    let (render, frames) = render_context(&gl, &player);
    Rig { render, frames, player, gl }
}

fn source() -> Source {
    Source::url(TEST_SOURCE).unwrap()
}

fn source_with_audio() -> Source {
    Source::url(TEST_SOURCE_WITH_AUDIO).unwrap()
}

fn target(framebuffer: &Framebuffer) -> RenderTarget {
    RenderTarget { fbo: framebuffer.name(), width: framebuffer.width, height: framebuffer.height }
}

impl Rig {
    /// Draws every frame mpv announces into `framebuffer` until `done` returns true. Returns the number of new frames
    /// drawn. Fails after [`TIMEOUT`].
    fn pump(&mut self, framebuffer: &Framebuffer, what: &str, mut done: impl FnMut(&Player, u32) -> bool) -> u32 {
        let deadline = Instant::now() + TIMEOUT;
        let mut drawn = 0;
        while !done(&self.player, drawn) {
            assert!(Instant::now() < deadline, "timed out waiting for {what}; state: {:?}", self.player.state());
            // Notifications only wake the loop; `update` decides whether there is a new frame.
            let _ = self.frames.recv_timeout(Duration::from_millis(50));
            if self.render.update() {
                self.render.render(target(framebuffer), false).expect("the frame is drawn");
                drawn += 1;
            }
        }
        drawn
    }

    /// Loads the test source and draws until its first frame has been drawn into `framebuffer`.
    fn load_first_frame(&mut self, framebuffer: &Framebuffer) {
        self.player.load(source()).unwrap();
        self.pump(framebuffer, "the first frame", |_, drawn| drawn > 0);
    }

    /// Draws the current frame into `framebuffer` and reads it back.
    fn draw(&mut self, framebuffer: &Framebuffer, flip_y: bool) -> Image {
        self.render.render(target(framebuffer), flip_y).expect("the frame is drawn");
        self.gl.read(framebuffer)
    }
}

fn is_black(pixel: [u8; 4]) -> bool {
    pixel[..3].iter().all(|&channel| channel < 16)
}

/// The number of distinct colours, ignoring the lowest bits so rasteriser rounding does not count.
fn colours(image: &Image) -> usize {
    let pixels = image.pixels.as_chunks::<4>().0;
    pixels.iter().map(|[r, g, b, _]| (r >> 4, g >> 4, b >> 4)).collect::<HashSet<_>>().len()
}

/// The share of pixels that differ by more than a small rounding tolerance in any channel.
fn difference(a: &Image, b: &Image) -> f64 {
    assert_eq!((a.width, a.height), (b.width, b.height));
    let (a, b) = (a.pixels.as_chunks::<4>().0, b.pixels.as_chunks::<4>().0);
    let differing = a.iter().zip(b).filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 8)).count();
    differing as f64 / a.len() as f64
}

#[test]
fn frame_ready_notification_while_playing() {
    if gl::skipped() {
        return;
    }
    let mut rig = rig(Autoplay::On);
    let framebuffer = rig.gl.framebuffer(160, 120, [0, 0, 0, 255]);
    rig.player.load(source()).unwrap();

    let deadline = Instant::now() + TIMEOUT;
    let (mut notifications, mut frames) = (0, 0);
    while notifications < 10 || frames == 0 {
        let left = deadline.saturating_duration_since(Instant::now());
        rig.frames.recv_timeout(left).expect("a frame-ready notification arrives");
        notifications += 1;
        if rig.render.update() {
            frames += 1;
            rig.render.render(target(&framebuffer), false).unwrap();
        }
    }
}

#[test]
fn pattern_is_drawn() {
    if gl::skipped() {
        return;
    }
    let mut rig = rig(Autoplay::Off);
    // A colour the test pattern is unlikely to contain, to see that the whole framebuffer is drawn over.
    let fill = [3, 141, 59, 255];
    let framebuffer = rig.gl.framebuffer(160, 120, fill);
    rig.load_first_frame(&framebuffer);
    rig.gl.fill(&framebuffer, fill);

    let image = rig.draw(&framebuffer, false);
    let colours = colours(&image);
    assert!(colours >= 8, "the test pattern has many colours, found {colours}");
    let untouched = image.pixels.as_chunks::<4>().0.iter().filter(|pixel| **pixel == fill).count();
    assert!(
        untouched < 160 * 120 / 100,
        "the whole framebuffer is drawn, but {untouched} pixels kept the fill"
    );
}

#[test]
fn aspect_ratio_is_kept_with_black_bars() {
    if gl::skipped() {
        return;
    }
    let mut rig = rig(Autoplay::Off);
    let framebuffer = rig.gl.framebuffer(320, 120, [255, 0, 255, 255]);
    rig.load_first_frame(&framebuffer);

    let image = rig.draw(&framebuffer, false);
    // The 160x120 video fills columns 80..240; a few columns at each edge are skipped for filtering.
    for y in 0..120 {
        for x in (0..76).chain(244..320) {
            assert!(is_black(image.pixel(x, y)), "pixel ({x}, {y}) is in a bar but is {:?}", image.pixel(x, y));
        }
    }
    let middle = (84..236).flat_map(|x| (0..120).map(move |y| (x, y)));
    let lit = middle.filter(|&(x, y)| !is_black(image.pixel(x, y))).count();
    assert!(lit > 152 * 120 / 2, "the video fills the middle, but only {lit} pixels are lit");
}

#[test]
fn vertical_flip() {
    if gl::skipped() {
        return;
    }
    let mut rig = rig(Autoplay::Off);
    let framebuffer = rig.gl.framebuffer(160, 120, [0, 0, 0, 255]);
    rig.load_first_frame(&framebuffer);

    let upright = rig.draw(&framebuffer, false);
    let flipped = rig.draw(&framebuffer, true);
    assert!(
        difference(&upright, &upright.flipped()) > 0.2,
        "the test pattern is not symmetric top to bottom"
    );
    let off = difference(&flipped, &upright.flipped());
    assert!(
        off < 0.01,
        "the flipped frame is the upright one mirrored, but {:.1}% of pixels differ",
        off * 100.0
    );
}

#[test]
fn black_with_nothing_loaded() {
    if gl::skipped() {
        return;
    }
    let mut rig = rig(Autoplay::Off);
    let framebuffer = rig.gl.framebuffer(160, 120, [255, 0, 255, 255]);

    let image = rig.draw(&framebuffer, false);
    let pixels = image.pixels.as_chunks::<4>().0;
    assert!(
        pixels.iter().all(|pixel| is_black(*pixel)),
        "the framebuffer is black, first pixel: {:?}",
        pixels[0]
    );
}

#[test]
fn playback_advances_without_report_swap() {
    if gl::skipped() {
        return;
    }
    let mut rig = rig(Autoplay::On);
    let framebuffer = rig.gl.framebuffer(160, 120, [0, 0, 0, 255]);
    rig.player.load(source()).unwrap();

    let past = Duration::from_millis(600);
    let drawn = rig.pump(&framebuffer, "playback to advance", |player, _| {
        player.state().position.is_some_and(|position| position > past)
    });
    assert!(drawn >= 5, "frames keep being drawn, but only {drawn} were");
}

#[test]
fn zero_sized_target_is_rejected() {
    if gl::skipped() {
        return;
    }
    let mut rig = rig(Autoplay::Off);
    let framebuffer = rig.gl.framebuffer(160, 120, [255, 0, 255, 255]);
    rig.load_first_frame(&framebuffer);
    rig.gl.fill(&framebuffer, [255, 0, 255, 255]);

    for (width, height) in [(0, 120), (160, 0)] {
        let empty = RenderTarget { fbo: framebuffer.name(), width, height };
        assert!(
            matches!(rig.render.render(empty, false), Err(Error::Render(_))),
            "{width}x{height} was accepted"
        );
    }
    let image = rig.gl.read(&framebuffer);
    assert!(
        image.pixels.as_chunks::<4>().0.iter().all(|pixel| *pixel == [255, 0, 255, 255]),
        "nothing was drawn"
    );
}

#[test]
fn video_size() {
    if gl::skipped() {
        return;
    }
    let mut rig = rig(Autoplay::On);
    let framebuffer = rig.gl.framebuffer(160, 120, [0, 0, 0, 255]);
    let events = rig.player.subscribe();
    assert_eq!(rig.player.state().video_size, None);
    rig.player.load(source()).unwrap();

    let mut announced = None;
    rig.pump(&framebuffer, "the video size", |_, _| {
        while let Some(event) = events.try_recv() {
            if let Event::VideoSizeChanged(size) = event {
                announced = size;
            }
        }
        announced.is_some()
    });
    assert_eq!(announced, Some((160, 120)));
    assert_eq!(rig.player.state().video_size, Some((160, 120)));
}

/// Waits until the player's position is past `after`, reading events; fails after [`TIMEOUT`].
fn wait_position_past(events: &Events, after: Duration) {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match events.recv_timeout(left) {
            Some(Event::PositionChanged(Some(position))) if position > after => return,
            Some(_) => {}
            None => panic!("timed out waiting for the position to pass {after:?}"),
        }
    }
}

/// Runs `drop` and checks that it returns within one second.
fn bounded_drop<T>(value: T, what: &str) {
    let start = Instant::now();
    drop(value);
    let elapsed = start.elapsed();
    assert!(elapsed < Duration::from_secs(1), "dropping {what} took {elapsed:?}");
}

#[test]
fn second_render_context_fails_and_the_first_keeps_working() {
    if gl::skipped() {
        return;
    }
    let mut rig = rig(Autoplay::Off);
    // SAFETY: as in `render_context`.
    let second = unsafe { RenderContext::new(&rig.player, rig.gl.loader(), || {}) };
    assert!(matches!(second, Err(Error::Render(_))), "a second render context was created: {second:?}");

    let framebuffer = rig.gl.framebuffer(160, 120, [0, 0, 0, 255]);
    rig.load_first_frame(&framebuffer);
    assert!(colours(&rig.draw(&framebuffer, false)) >= 8, "the first render context still draws");
}

#[test]
fn render_context_can_be_recreated_after_drop() {
    if gl::skipped() {
        return;
    }
    let gl = Gl::new();
    let player = render_player(Autoplay::Off);
    let (first, _frames) = render_context(&gl, &player);
    drop(first);
    let (mut second, _frames) = render_context(&gl, &player);

    let framebuffer = gl.framebuffer(160, 120, [255, 0, 255, 255]);
    second.render(target(&framebuffer), false).expect("the new render context draws");
}

#[test]
fn no_notification_after_drop() {
    if gl::skipped() {
        return;
    }
    let mut rig = rig(Autoplay::On);
    let framebuffer = rig.gl.framebuffer(160, 120, [0, 0, 0, 255]);
    let events = rig.player.subscribe();
    rig.player.load(source_with_audio()).unwrap();
    rig.pump(&framebuffer, "a few frames", |_, drawn| drawn >= 3);

    let Rig { render, frames, player, gl } = rig;
    bounded_drop(render, "the render context");
    // Forget what was sent before the drop, then let playback run on.
    while frames.try_recv().is_ok() {}
    let now = player.state().position.unwrap_or_default();
    wait_position_past(&events, now + Duration::from_millis(500));
    assert!(frames.try_recv().is_err(), "a notification arrived after the render context was dropped");
    drop(gl);
}

#[test]
fn drop_render_context_while_playing() {
    if gl::skipped() {
        return;
    }
    let mut rig = rig(Autoplay::On);
    let framebuffer = rig.gl.framebuffer(160, 120, [0, 0, 0, 255]);
    let events = rig.player.subscribe();
    rig.player.load(source_with_audio()).unwrap();
    rig.pump(&framebuffer, "a few frames", |_, drawn| drawn >= 3);

    let Rig { render, player, gl, .. } = rig;
    bounded_drop(render, "the render context");

    // Playback continues and the player still responds.
    let now = player.state().position.unwrap_or_default();
    wait_position_past(&events, now + Duration::from_millis(300));
    player.pause().unwrap();
    let deadline = Instant::now() + TIMEOUT;
    while !player.state().paused {
        let left = deadline.saturating_duration_since(Instant::now());
        assert!(events.recv_timeout(left).is_some(), "timed out waiting for the pause");
    }
    assert!(player.get_property::<bool>("pause").unwrap());
    drop(gl);
}

#[test]
fn drop_player_before_render_context() {
    if gl::skipped() {
        return;
    }
    let mut rig = rig(Autoplay::On);
    let framebuffer = rig.gl.framebuffer(160, 120, [0, 0, 0, 255]);
    rig.player.load(source()).unwrap();
    rig.pump(&framebuffer, "a few frames", |_, drawn| drawn >= 3);

    let Rig { mut render, frames, player, gl } = rig;
    bounded_drop(player, "the last player");

    // Drawing still works, and playback stops: the frame notifications die down. A stopped player may still ask for
    // a few redraws, so wait for a quiet half second rather than for none at all.
    let deadline = Instant::now() + TIMEOUT;
    loop {
        render
            .render(target(&framebuffer), false)
            .expect("drawing after the player is dropped succeeds");
        match frames.recv_timeout(Duration::from_millis(500)) {
            Err(_) => break,
            Ok(()) => assert!(Instant::now() < deadline, "frames are still announced; playback did not stop"),
        }
        render.update();
    }
    render.render(target(&framebuffer), false).expect("drawing after playback stopped succeeds");

    bounded_drop(render, "the render context after the player");
    drop(gl);
}

#[test]
fn load_before_render_context_plays_without_video() {
    if gl::skipped() {
        return;
    }
    let gl = Gl::new();
    let player = render_player(Autoplay::On);
    let events = player.subscribe();
    player.load(source_with_audio()).unwrap();
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match events.recv_timeout(left) {
            Some(Event::Loaded) => break,
            Some(Event::Ended(reason)) => panic!("the media ended instead of loading: {reason:?}"),
            Some(_) => {}
            None => panic!("timed out waiting for the media to load"),
        }
    }

    let (mut render, _frames) = render_context(&gl, &player);
    let framebuffer = gl.framebuffer(160, 120, [0, 0, 0, 255]);
    wait_position_past(&events, Duration::from_millis(500));
    assert!(!render.update(), "a frame is announced for media loaded before the render context");
    render.render(target(&framebuffer), false).expect("drawing succeeds");
    assert_eq!(player.state().video_size, None);
}
