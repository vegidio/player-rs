//! Plays a local file in a resizable window, for checking the player element by hand. The window opens at 960×540 and
//! takes the video's aspect ratio, with its longer side at 960, once the video's size is known.
//!
//! ```sh
//! cargo run -p player-gpui --example play -- path/to/movie.mp4 [default|none]
//! ```
//!
//! The optional second argument chooses the skin: `default`, the default, or `none` for bare video, which still takes
//! clicks and hotkeys. The Default skin shows its pill bar from the start, with the volume popover over the mute
//! button and the default playback rates on the Settings menu's Speed page; its time display switches between the
//! remaining time and the duration when clicked. The controls' hotkeys work once the player is clicked: `k` or Space
//! toggles pause, `m` mutes and `f` toggles fullscreen. Once a second it prints how often the window re-rendered (once
//! per video frame while playing, plus the controls' own changes, and not at all while paused and idle) and the
//! hardware decoder in use. Fullscreen changes are printed as they happen.

use gpui_kit::*;
use player::prelude::*;
use player::ui;
use player_gpui::{Skin, VideoPlayer};
use std::process::ExitCode;
use std::time::Duration;

struct Play {
    player: Player,
    options: ui::Options,
    skin: Skin,
    renders: usize,
    _report: Task<()>,
    _fit: Task<()>,
}

impl Play {
    fn new(player: Player, skin: Skin, window: &Window, cx: &mut Context<Self>) -> Self {
        // The size is known only once the player element has made its render context, after the window opens.
        let events = player.subscribe();
        let fit = cx.spawn_in(window, async move |this, cx| {
            while let Some(event) = events.recv_async().await {
                if let Event::VideoSizeChanged(Some((width, height))) = event {
                    let _ = this.update_in(cx, |_, window, _| window.resize(fit(width, height)));
                    break;
                }
            }
        });
        let report = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let reported = this.update(cx, |play, _| {
                    let decoder = play.player.get_property::<String>("hwdec-current").unwrap_or_default();
                    eprintln!("renders in the last second: {:3}  hwdec-current: {decoder}", play.renders);
                    play.renders = 0;
                });
                if reported.is_err() {
                    break;
                }
            }
        });
        Self { player, options: ui::Options::default(), skin, renders: 0, _report: report, _fit: fit }
    }
}

/// The window size with the video's aspect ratio and its longer side at 960.
fn fit(width: u32, height: u32) -> Size<Pixels> {
    const LONGER: f32 = 960.;
    let (width, height) = (width as f32, height as f32);
    if width >= height {
        size(px(LONGER), px((LONGER * height / width).round()))
    } else {
        size(px((LONGER * width / height).round()), px(LONGER))
    }
}

impl Render for Play {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.renders += 1;
        div().size_full().child(
            VideoPlayer::new("video", &self.player)
                .options(self.options.clone())
                .skin(self.skin)
                .on_fullscreen_change(|on, _, _| eprintln!("fullscreen: {on}"))
                .size_full(),
        )
    }
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let usage = || {
        eprintln!("usage: play <path> [default|none]");
        ExitCode::from(2)
    };
    let Some(path) = args.next() else { return usage() };
    let skin = match args.next().as_deref() {
        None | Some("default") => Skin::Default,
        Some("none") => Skin::None,
        Some(_) => return usage(),
    };
    let source = match Source::file(&path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };

    // The player's icons, with GPUI-Kit's default bundle behind them.
    gpui_kit::application().with_assets(player_gpui::Assets).run(move |cx| {
        gpui_kit::init(cx);
        cx.on_window_closed(|cx, _| cx.quit()).detach();

        let player = Player::builder().autoplay(Autoplay::On).build().expect("the player builds");
        // Loaded before the window's first frame, as an app's view constructor would.
        player.load(source).expect("the file loads");

        let bounds = Bounds::centered(None, size(px(960.), px(540.)), cx);
        let options = WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() };
        gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| Play::new(player, skin, window, cx)))
            .expect("the window opens");
        cx.activate(true);
    });
    ExitCode::SUCCESS
}
