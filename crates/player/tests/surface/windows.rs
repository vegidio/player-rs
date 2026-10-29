//! Windows: [`SurfaceRenderer`] into Direct3D 11 textures shared through NT handles. Each frame is read back as an
//! application would use it: its handle is opened on a second device on the frame's adapter, and copied into a staging
//! texture there.
//!
//! Most scenarios need an OpenGL driver with `WGL_NV_DX_interop2`, as GPU drivers have. Where it is missing, as with
//! Mesa's software OpenGL on CI, they pass without running, and only the creation errors are tested. Set
//! `PLAYER_TEST_GL=hardware` to also test hardware decoding.

use super::*;
use crate::common::gl::{self, Gl};
use ::windows::Win32::Foundation::{HANDLE, HMODULE};
use ::windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
};
use ::windows::Win32::Graphics::Direct3D11::{
    D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
    D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING, D3D11CreateDevice, ID3D11Device, ID3D11Device1,
    ID3D11DeviceContext, ID3D11Texture2D,
};
use ::windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use ::windows::Win32::Graphics::Dxgi::{CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS, IDXGIAdapter1, IDXGIFactory1};
use ::windows::core::Interface;
use player::windows::{AdapterLuid, Frame, SurfaceRenderer};
use std::collections::HashSet;
use std::os::windows::io::AsRawHandle;
use std::sync::OnceLock;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};

/// The error a driver without the interop gives.
const NO_SHARING: &str = "cannot share textures with Direct3D 11";

/// Whether the scenarios needing the interop are skipped: GL tests are skipped, or the OpenGL driver lacks it.
fn skipped() -> bool {
    static NO_INTEROP: OnceLock<bool> = OnceLock::new();
    if gl::skipped() {
        return true;
    }
    let no_interop =
        *NO_INTEROP.get_or_init(|| match SurfaceRenderer::builder(&render_player(Autoplay::Off)).build() {
            Ok(_) => false,
            Err(Error::Render(message)) if message.contains(NO_SHARING) => true,
            Err(error) => panic!("the surface renderer cannot be created: {error}"),
        });
    if no_interop {
        eprintln!("skipped: the OpenGL driver cannot share textures with Direct3D 11");
    }
    no_interop
}

/// Creates a surface renderer for `player`, notifying on the returned channel.
fn renderer(player: &Player) -> (SurfaceRenderer, Receiver<()>) {
    let (sender, frames) = mpsc::channel();
    let renderer = SurfaceRenderer::builder(player)
        .on_frame(move || {
            let _ = sender.send(());
        })
        .build()
        .expect("the surface renderer is created");
    (renderer, frames)
}

/// A render player and its surface renderer, declared so the renderer is dropped first.
struct Rig {
    renderer: SurfaceRenderer,
    frames: Receiver<()>,
    player: Player,
}

fn rig(autoplay: Autoplay) -> Rig {
    let player = render_player(autoplay);
    let (renderer, frames) = renderer(&player);
    Rig { renderer, frames, player }
}

impl Rig {
    /// Waits for a published frame `accept` returns true for. Fails after [`TIMEOUT`].
    fn wait_frame(&self, what: &str, accept: impl Fn(&Frame) -> bool) -> Frame {
        wait_frame(&self.renderer, &self.frames, &self.player, what, accept)
    }
}

fn wait_frame(
    renderer: &SurfaceRenderer,
    frames: &Receiver<()>,
    player: &Player,
    what: &str,
    accept: impl Fn(&Frame) -> bool,
) -> Frame {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if let Some(frame) = renderer.latest().filter(&accept) {
            return frame;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}; state: {:?}", player.state());
        let _ = frames.recv_timeout(Duration::from_millis(50));
    }
}

/// The adapter with `luid`, and a new device on it.
fn device_on(luid: AdapterLuid) -> (ID3D11Device, ID3D11DeviceContext) {
    adapters()
        .into_iter()
        .find(|(_, found)| *found == luid)
        .and_then(|(adapter, _)| device(&adapter))
        .unwrap_or_else(|| panic!("no device on the adapter {luid}"))
}

/// Every DXGI adapter with its LUID, in DXGI's order.
fn adapters() -> Vec<(IDXGIAdapter1, AdapterLuid)> {
    // SAFETY: no flags; enumeration ends with an error past the last adapter.
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)).unwrap();
        (0..)
            .map_while(|index| factory.EnumAdapters1(index).ok())
            .map(|adapter| {
                let luid = adapter.GetDesc1().unwrap().AdapterLuid;
                (adapter, AdapterLuid { low: luid.LowPart, high: luid.HighPart })
            })
            .collect()
    }
}

fn device(adapter: &IDXGIAdapter1) -> Option<(ID3D11Device, ID3D11DeviceContext)> {
    let (mut device, mut context) = (None, None);
    // SAFETY: the out-pointers are valid; the adapter is used with the driver type it requires.
    unsafe {
        D3D11CreateDevice(
            adapter,
            D3D_DRIVER_TYPE_UNKNOWN,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_1]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )
        .ok()?;
    }
    device.zip(context)
}

/// Opens a frame's handle on a new device on its adapter, as an application would.
fn open(frame: &Frame) -> (ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_TEXTURE2D_DESC) {
    let (device, context) = device_on(frame.adapter());
    let device1: ID3D11Device1 = device.cast().unwrap();
    // SAFETY: the handle is a valid NT handle to a shared texture, borrowed for the call; the out-pointer is valid.
    unsafe {
        let texture: ID3D11Texture2D = device1
            .OpenSharedResource1(HANDLE(frame.shared_handle().as_raw_handle()))
            .expect("the frame's handle opens on another device");
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        texture.GetDesc(&mut desc);
        (device, context, texture, desc)
    }
}

/// A frame read back as RGB, top row first.
struct Picture {
    width: usize,
    height: usize,
    rgb: Vec<[u8; 3]>,
}

impl Picture {
    /// Reads a frame through a staging texture on a second device.
    fn of(frame: &Frame) -> Picture {
        let (device, context, texture, desc) = open(frame);
        assert_eq!((desc.Width, desc.Height), (frame.width(), frame.height()), "the texture has the frame's size");
        assert_eq!(desc.Format, DXGI_FORMAT_B8G8R8A8_UNORM);
        let staging = D3D11_TEXTURE2D_DESC {
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
            ..desc
        };
        let (width, height) = (desc.Width as usize, desc.Height as usize);
        // SAFETY: the textures belong to `device`; the mapped rows are read within their bounds while mapped.
        let rgb = unsafe {
            let mut copy = None;
            device.CreateTexture2D(&staging, None, Some(&mut copy)).unwrap();
            let copy = copy.unwrap();
            context.CopyResource(&copy, &texture);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            context.Map(&copy, 0, D3D11_MAP_READ, 0, Some(&mut mapped)).unwrap();
            let rgb = (0..height)
                .flat_map(|row| {
                    let start = mapped.pData.cast::<u8>().add(row * mapped.RowPitch as usize);
                    std::slice::from_raw_parts(start, width * 4).to_vec()
                })
                .collect::<Vec<u8>>()
                .as_chunks::<4>()
                .0
                .iter()
                .map(|&[b, g, r, _]| [r, g, b])
                .collect();
            context.Unmap(&copy, 0);
            rgb
        };
        Picture { width, height, rgb }
    }

    fn pixel(&self, x: usize, y: usize) -> [u8; 3] {
        self.rgb[y * self.width + x]
    }

    /// The number of distinct colours, ignoring the lowest bits so rounding does not count.
    fn colours(&self) -> usize {
        self.rgb.iter().map(|[r, g, b]| (r >> 4, g >> 4, b >> 4)).collect::<HashSet<_>>().len()
    }

    /// The mean number of distinct colours per row over `rows`.
    fn row_colours(&self, rows: std::ops::Range<usize>) -> f64 {
        let count = rows.len();
        let total: usize = rows
            .map(|y| {
                let row = &self.rgb[y * self.width..(y + 1) * self.width];
                row.iter().map(|[r, g, b]| (r >> 4, g >> 4, b >> 4)).collect::<HashSet<_>>().len()
            })
            .sum();
        total as f64 / count as f64
    }

    /// Whether `testsrc` is upright: its scrolling gradient, many colours along each row, is in the lower part of the
    /// picture (rows 88-108 of 120), and the mirrored rows hold only a few colour bars.
    fn is_upright_test_pattern(&self) -> bool {
        let scale = |row: usize| row * self.height / 120;
        let gradient = self.row_colours(scale(92)..scale(104));
        let mirrored = self.row_colours(scale(16)..scale(28));
        gradient > 2.0 * mirrored
    }
}

/// Whether a frame shows video rather than a blank picture.
fn shows_video(frame: &Frame) -> bool {
    Picture::of(frame).colours() >= 8
}

#[test]
fn rejects_a_player_without_render_output() {
    let player = Player::builder().mpv_option("ao", "null").build().unwrap();
    let result = SurfaceRenderer::builder(&player).build();
    let Err(Error::Render(message)) = result else {
        panic!("a surface renderer was created: {result:?}")
    };
    assert!(message.contains("VideoOutput::Render"), "{message}");

    player.set_volume(0.5).unwrap();
    assert_eq!(player.get_property::<f64>("volume").unwrap(), 50.0);
}

#[test]
fn driver_without_direct3d_sharing() {
    if gl::skipped() {
        return;
    }
    let player = render_player(Autoplay::Off);
    let result = SurfaceRenderer::builder(&player).build();
    let Err(Error::Render(message)) = result else {
        eprintln!("skipped: this OpenGL driver shares textures with Direct3D 11");
        return;
    };
    assert!(message.contains(NO_SHARING), "{message}");

    player.load(url(TEST_SOURCE)).unwrap();
    player.set_volume(0.5).unwrap();
    assert_eq!(player.get_property::<f64>("volume").unwrap(), 50.0, "the player keeps working");
}

#[test]
fn unknown_adapter() {
    if gl::skipped() {
        return;
    }
    let player = render_player(Autoplay::Off);
    let luid = AdapterLuid { low: 0xdead_beef, high: 0x7654_3210 };
    let result = SurfaceRenderer::builder(&player).adapter(luid).build();
    let Err(Error::Render(message)) = result else {
        panic!("a surface renderer was created on a missing adapter: {result:?}")
    };
    assert!(message.contains(&luid.to_string()), "{message}");

    player.set_volume(0.5).unwrap();
    assert_eq!(player.get_property::<f64>("volume").unwrap(), 50.0, "the player keeps working");
}

#[test]
fn default_adapter_is_the_first_with_direct3d_11() {
    if skipped() {
        return;
    }
    let expected = adapters().into_iter().find(|(adapter, _)| device(adapter).is_some()).unwrap().1;
    let rig = rig(Autoplay::Off);
    assert_eq!(rig.renderer.adapter(), expected);
    rig.renderer.set_size(16, 16);
    assert_eq!(rig.wait_frame("a frame", |_| true).adapter(), expected);
}

#[test]
fn chosen_adapter() {
    if skipped() {
        return;
    }
    let luid = adapters().into_iter().find(|(adapter, _)| device(adapter).is_some()).unwrap().1;
    let player = render_player(Autoplay::Off);
    let renderer = SurfaceRenderer::builder(&player).adapter(luid).build().expect("the renderer is created");
    assert_eq!(renderer.adapter(), luid);
}

#[test]
fn second_renderer_fails_and_the_first_keeps_working() {
    if skipped() {
        return;
    }
    let rig = rig(Autoplay::Off);
    let second = SurfaceRenderer::builder(&rig.player).build();
    let Err(Error::Render(message)) = second else {
        panic!("a second surface renderer was created: {second:?}")
    };
    assert!(message.contains("already has a render context"), "{message}");

    rig.renderer.set_size(160, 120);
    rig.player.load(url(TEST_SOURCE)).unwrap();
    rig.wait_frame("the first renderer to show video", shows_video);
}

#[test]
fn fails_while_a_render_context_exists() {
    if skipped() {
        return;
    }
    let gl = Gl::new();
    let player = render_player(Autoplay::Off);
    // SAFETY: `gl` is current on this thread, where the render context is used and dropped, and outlives it.
    let mut render = unsafe { RenderContext::new(&player, gl.loader(), || {}) }.expect("the render context is created");

    let result = SurfaceRenderer::builder(&player).build();
    let Err(Error::Render(message)) = result else {
        panic!("a surface renderer was created next to a render context: {result:?}")
    };
    assert!(message.contains("already has a render context"), "{message}");

    // The render context keeps working, and releases the player for a surface renderer once dropped.
    let framebuffer = gl.framebuffer(160, 120, [0, 0, 0, 255]);
    render.update();
    render
        .render(
            RenderTarget { fbo: framebuffer.name(), width: framebuffer.width, height: framebuffer.height },
            false,
        )
        .expect("the render context still renders");
    gl.delete(framebuffer);
    drop(render);
    let (renderer, frames) = renderer(&player);
    renderer.set_size(160, 120);
    player.load(url(TEST_SOURCE)).unwrap();
    wait_frame(&renderer, &frames, &player, "the surface renderer to show video", shows_video);
}

#[test]
fn recreated_after_drop_publishes_frames() {
    if skipped() {
        return;
    }
    let player = render_player(Autoplay::Off);
    let (first, _frames) = renderer(&player);
    drop(first);
    let (second, frames) = renderer(&player);
    second.set_size(160, 120);
    player.load(url(TEST_SOURCE)).unwrap();
    wait_frame(&second, &frames, &player, "the new renderer to show video", shows_video);
}

#[test]
fn no_frame_without_a_size() {
    if skipped() {
        return;
    }
    let rig = rig(Autoplay::On);
    let events = rig.player.subscribe();
    rig.player.load(url(TEST_SOURCE)).unwrap();
    wait_position_past(&events, Duration::from_millis(500));
    assert!(rig.renderer.latest().is_none(), "a frame was published");
    assert!(rig.frames.try_recv().is_err(), "a frame was announced");
}

#[test]
fn frames_while_playing_have_the_set_size() {
    if skipped() {
        return;
    }
    let rig = rig(Autoplay::On);
    rig.renderer.set_size(320, 180);
    rig.player.load(url(TEST_SOURCE)).unwrap();

    let first = rig.wait_frame("a video frame", shows_video);
    let mut sequence = first.sequence();
    for _ in 0..10 {
        rig.frames.recv_timeout(TIMEOUT).expect("frames keep being announced");
        let frame = rig.renderer.latest().expect("a frame is published before it is announced");
        assert_eq!((frame.width(), frame.height()), (320, 180));
        assert!(frame.sequence() >= sequence, "frames are published in order");
        sequence = frame.sequence();
    }
    assert!(sequence > first.sequence(), "the latest frame changes while playing");
    assert_eq!(Picture::of(&rig.renderer.latest().unwrap()).width, 320);
}

#[test]
fn latest_frame_from_another_thread() {
    if skipped() {
        return;
    }
    let rig = rig(Autoplay::On);
    rig.renderer.set_size(160, 120);
    rig.player.load(url(TEST_SOURCE)).unwrap();
    rig.wait_frame("a video frame", shows_video);

    std::thread::scope(|scope| {
        scope.spawn(|| {
            for _ in 0..20 {
                let frame = rig.renderer.latest().expect("a frame is published");
                assert_eq!((frame.width(), frame.height()), (160, 120));
                assert!(shows_video(&frame), "the frame is complete");
            }
        });
    });
}

#[test]
fn resize_while_paused() {
    if skipped() {
        return;
    }
    let rig = rig(Autoplay::Off);
    rig.renderer.set_size(160, 120);
    rig.player.load(url(TEST_SOURCE)).unwrap();
    rig.wait_frame("the first frame", shows_video);
    assert!(rig.player.state().paused);

    rig.renderer.set_size(240, 180);
    let frame = rig.wait_frame("a frame at the new size", |frame| (frame.width(), frame.height()) == (240, 180));
    assert!(shows_video(&frame), "the paused picture is shown at the new size");
}

#[test]
fn zero_size_stops_publication() {
    if skipped() {
        return;
    }
    let rig = rig(Autoplay::On);
    let events = rig.player.subscribe();
    rig.renderer.set_size(160, 120);
    rig.player.load(url(TEST_SOURCE)).unwrap();
    rig.wait_frame("a video frame", shows_video);

    rig.renderer.set_size(0, 120);
    // Let a frame drawn before the change land, then nothing more may be published. Notifications of earlier frames
    // are still queued, so wait until none has come for a while.
    while rig.frames.recv_timeout(Duration::from_millis(200)).is_ok() {}
    let sequence = rig.renderer.latest().unwrap().sequence();
    let now = rig.player.state().position.unwrap_or_default();
    wait_position_past(&events, now + Duration::from_millis(400));
    assert_eq!(rig.renderer.latest().unwrap().sequence(), sequence, "a frame was published at zero size");

    rig.renderer.set_size(160, 120);
    rig.wait_frame("frames again", |frame| frame.sequence() > sequence);
}

#[test]
fn black_with_nothing_loaded() {
    if skipped() {
        return;
    }
    let rig = rig(Autoplay::Off);
    rig.renderer.set_size(160, 120);
    let picture = Picture::of(&rig.wait_frame("a frame", |_| true));
    let lit = picture.rgb.iter().filter(|pixel| !is_black(**pixel)).count();
    assert_eq!(lit, 0, "the frame is black, first pixel: {:?}", picture.rgb[0]);
}

#[test]
fn test_pattern_is_upright() {
    if skipped() {
        return;
    }
    let rig = rig(Autoplay::Off);
    rig.renderer.set_size(160, 120);
    rig.player.load(url(TEST_SOURCE)).unwrap();
    let picture = Picture::of(&rig.wait_frame("the first frame", shows_video));
    assert!(picture.colours() >= 8, "the frame shows the test pattern, not a blank image");
    assert!(
        picture.is_upright_test_pattern(),
        "the gradient is at the bottom: {:.1} colours per row there, {:.1} in the mirrored rows",
        picture.row_colours(92..104),
        picture.row_colours(16..28)
    );
}

#[test]
fn solid_colours() {
    if skipped() {
        return;
    }
    let rig = rig(Autoplay::Off);
    rig.renderer.set_size(160, 120);
    rig.player.load(url(FIELDS_SOURCE)).unwrap();
    let picture = Picture::of(&rig.wait_frame("the first frame", |frame| Picture::of(frame).colours() >= 4));
    let fields: [[u8; 3]; 5] = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 255], [128, 128, 128]];
    for (field, expected) in fields.into_iter().enumerate() {
        let actual = picture.pixel(16 + 32 * field, 60);
        let close = expected.iter().zip(actual).all(|(e, a)| f64::from(e.abs_diff(a)) <= 0.03 * 255.0);
        assert!(close, "field {field}: {actual:?}, expected {expected:?}");
    }
}

#[test]
fn letterboxing() {
    if skipped() {
        return;
    }
    let rig = rig(Autoplay::Off);
    rig.renderer.set_size(320, 120);
    rig.player.load(url(TEST_SOURCE)).unwrap();
    let picture = Picture::of(&rig.wait_frame("the first frame", shows_video));
    // The 160x120 video fills columns 80..240; a few columns at each edge are skipped for filtering.
    for y in 0..120 {
        for x in (0..76).chain(244..320) {
            assert!(is_black(picture.pixel(x, y)), "pixel ({x}, {y}) is in a bar but is {:?}", picture.pixel(x, y));
        }
    }
    let lit = (84..236)
        .flat_map(|x| (0..120).map(move |y| (x, y)))
        .filter(|&(x, y)| !is_black(picture.pixel(x, y)));
    assert!(lit.count() > 152 * 120 / 2, "the video fills the middle");
}

#[test]
fn renderer_created_after_load() {
    if skipped() {
        return;
    }
    let player = render_player(Autoplay::On);
    let events = player.subscribe();
    // A seekable file: mpv re-selects the video track with a seek to the current position, which a generated source
    // cannot do, so its video would resume at the demuxer's read-ahead, more than a second later.
    player.load(fixture("testsrc-h264-aac.mp4")).unwrap();
    wait_loaded(&events);
    wait_position_past(&events, Duration::from_millis(500));
    let before = player.state().position.unwrap();
    let start = Instant::now();

    let (renderer, frames) = renderer(&player);
    renderer.set_size(160, 120);
    wait_frame(&renderer, &frames, &player, "the video to appear", shows_video);

    // Where playback would be had nothing happened, give or take the one second the spec allows.
    let position = player.state().position.unwrap();
    let expected = before + start.elapsed();
    assert!(
        position + Duration::from_secs(1) >= before && position <= expected + Duration::from_secs(1),
        "playback continues from about where it was: {before:?} before, {position:?} after, {expected:?} expected"
    );
    wait_position_past(&events, position + Duration::from_millis(300));
    assert!(!player.state().ended);
}

#[test]
fn media_without_audio_loaded_before_the_renderer() {
    if skipped() {
        return;
    }
    // The fixture has no audio, so its load ends at once when its video output cannot start.
    let player = render_player(Autoplay::On);
    let events = player.subscribe();
    player.load(h264_fixture()).unwrap();
    let deadline = Instant::now() + TIMEOUT;
    loop {
        match events.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Some(Event::Ended(EndReason::Error(_))) => break,
            Some(_) => {}
            None => panic!("timed out waiting for the load to fail without a renderer"),
        }
    }

    let (renderer, frames) = renderer(&player);
    renderer.set_size(160, 120);
    wait_loaded(&events);
    wait_frame(&renderer, &frames, &player, "the video to appear", shows_video);
}

#[test]
fn media_without_audio_loaded_just_before_the_renderer() {
    if skipped() {
        return;
    }
    // Whether the load fails before or after the renderer exists, the video appears.
    for _ in 0..5 {
        let player = render_player(Autoplay::On);
        player.load(h264_fixture()).unwrap();
        let (renderer, frames) = renderer(&player);
        renderer.set_size(160, 120);
        wait_frame(&renderer, &frames, &player, "the video to appear", shows_video);
    }
}

#[test]
fn drop_renderer_while_playing() {
    if skipped() {
        return;
    }
    let rig = rig(Autoplay::On);
    let events = rig.player.subscribe();
    rig.renderer.set_size(160, 120);
    rig.player.load(url(TEST_SOURCE_WITH_AUDIO)).unwrap();
    rig.wait_frame("a video frame", shows_video);

    let Rig { renderer, player, .. } = rig;
    bounded_drop(renderer, "the surface renderer");

    let now = player.state().position.unwrap_or_default();
    wait_position_past(&events, now + Duration::from_millis(300));
    player.pause().unwrap();
    let deadline = Instant::now() + TIMEOUT;
    while !player.state().paused {
        let left = deadline.saturating_duration_since(Instant::now());
        assert!(events.recv_timeout(left).is_some(), "timed out waiting for the pause");
    }
    assert!(player.get_property::<bool>("pause").unwrap());
}

#[test]
fn drop_player_before_renderer() {
    if skipped() {
        return;
    }
    let rig = rig(Autoplay::On);
    rig.renderer.set_size(160, 120);
    rig.player.load(url(TEST_SOURCE)).unwrap();
    rig.wait_frame("a video frame", shows_video);

    let Rig { renderer, player, .. } = rig;
    bounded_drop(player, "the last player");
    assert!(renderer.latest().is_some(), "the renderer still answers");
    bounded_drop(renderer, "the surface renderer after the player");
}

#[test]
fn frame_and_handle_outlive_the_renderer() {
    if skipped() {
        return;
    }
    let rig = rig(Autoplay::Off);
    rig.renderer.set_size(160, 120);
    rig.player.load(url(TEST_SOURCE)).unwrap();
    let frame = rig.wait_frame("the first frame", shows_video);

    let Rig { renderer, player, .. } = rig;
    drop(renderer);
    drop(player);
    assert_eq!((frame.width(), frame.height()), (160, 120));
    // Opened only now, after the renderer and its device are gone.
    assert!(Picture::of(&frame).is_upright_test_pattern(), "the texture opens and holds the frame's picture");
}

#[test]
fn no_notification_after_drop() {
    if skipped() {
        return;
    }
    let rig = rig(Autoplay::On);
    rig.renderer.set_size(160, 120);
    rig.player.load(url(TEST_SOURCE_WITH_AUDIO)).unwrap();
    rig.wait_frame("a video frame", shows_video);

    let Rig { renderer, frames, player } = rig;
    drop(renderer);
    // Notifications sent before the drop may still be queued. After them the channel must be closed: the callback,
    // which holds the only sender, is gone, so it cannot be called again.
    let deadline = Instant::now() + TIMEOUT;
    loop {
        match frames.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(()) => {}
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => panic!("the notification outlived the renderer"),
        }
    }
    drop(player);
}

#[test]
fn open_on_another_device() {
    if skipped() {
        return;
    }
    let rig = rig(Autoplay::Off);
    rig.renderer.set_size(200, 150);
    rig.player.load(url(TEST_SOURCE)).unwrap();
    let frame = rig.wait_frame("the first frame", shows_video);

    let (_device, _context, _texture, desc) = open(&frame);
    assert_eq!((desc.Width, desc.Height, desc.Format), (200, 150, DXGI_FORMAT_B8G8R8A8_UNORM));
    let picture = Picture::of(&frame);
    assert!(picture.is_upright_test_pattern(), "the opened texture holds the published picture");
}

/// Plays the H.264 fixture until it shows video, and returns the hardware decoder mpv reports.
fn play_h264() -> String {
    let rig = rig(Autoplay::On);
    rig.renderer.set_size(160, 120);
    rig.player.load(h264_fixture()).unwrap();
    let frame = rig.wait_frame("the H.264 video", shows_video);
    assert!(Picture::of(&frame).is_upright_test_pattern(), "the decoded video is drawn upright");
    rig.player.get_property::<String>("hwdec-current").unwrap()
}

#[test]
fn h264_frames_are_published() {
    if skipped() {
        return;
    }
    // Hardware or software decoding, depending on the GPU; frames are published either way.
    eprintln!("hwdec-current: {}", play_h264());
}

#[test]
fn hardware_decoding_on_the_gpu() {
    if !std::env::var("PLAYER_TEST_GL").is_ok_and(|value| value == "hardware") {
        eprintln!("skipped: needs a GPU with a hardware H.264 decoder; set PLAYER_TEST_GL=hardware");
        return;
    }
    assert!(
        !skipped(),
        "PLAYER_TEST_GL=hardware, but the OpenGL driver cannot share textures with Direct3D 11"
    );
    let decoder = play_h264();
    assert!(
        !decoder.is_empty() && decoder != "no" && !decoder.ends_with("-copy"),
        "a zero-copy hardware decoder is active: {decoder:?}"
    );
}
