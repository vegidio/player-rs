//! Plays a local file in a resizable window with Video.js controls, for checking the player element by hand.
//!
//! ```sh
//! cargo run -p player-gpui --example play -- path/to/movie.mp4
//! ```
//!
//! The controls' hotkeys work once the player is clicked: `k` or Space toggles pause, `m` mutes and `f` toggles
//! fullscreen. Once a second it prints how often the window re-rendered (once per video frame while playing, plus the
//! controls' own changes, and not at all while paused and idle) and the hardware decoder in use. Fullscreen changes are
//! printed as they happen.

use gpui_kit::*;
use player::prelude::*;
use player::ui;
use player_gpui::VideoPlayer;
use std::process::ExitCode;
use std::time::Duration;

struct Play {
    player: Player,
    options: ui::Options,
    renders: usize,
    _report: Task<()>,
}

impl Play {
    fn new(player: Player, cx: &mut Context<Self>) -> Self {
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
        let options = ui::Options::builder().playback_rates([0.5, 1.0, 1.5, 2.0]).build().expect("valid options");
        Self { player, options, renders: 0, _report: report }
    }
}

impl Render for Play {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.renders += 1;
        div().size_full().child(
            VideoPlayer::new("video", &self.player)
                .options(self.options.clone())
                .on_fullscreen_change(|on, _, _| eprintln!("fullscreen: {on}"))
                .size_full(),
        )
    }
}

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: play <path>");
        return ExitCode::from(2);
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
        gpui_kit::open_window(options, cx, |_, cx| cx.new(|cx| Play::new(player, cx))).expect("the window opens");
        cx.activate(true);
    });
    ExitCode::SUCCESS
}
