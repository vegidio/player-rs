//! An app that registers no fonts gets Inter, the Default skin's typeface, from its first player.
//!
//! GPUI's headless test platform ignores added fonts, so this runs on the platform's own text system. macOS creates
//! that platform only on the main thread, so the test has no harness and runs from `main`.
//!
//! Where Inter is installed, the text system lists it either way, so the registration is also observed through the
//! crate's log: it reports that the embedded faces were added, and warns if they were not.

use gpui_kit::*;
use player::prelude::*;
use player_gpui::VideoPlayer;
use std::sync::Mutex;

struct Parent {
    player: Player,
}

impl Render for Parent {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(VideoPlayer::new("video", &self.player).size_full())
    }
}

/// The font module's log records, with their levels.
static RECORDS: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());

struct Capture;

impl log::Log for Capture {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.target() == "player_gpui::fonts"
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            RECORDS.lock().unwrap().push((record.level(), record.args().to_string()));
        }
    }

    fn flush(&self) {}
}

static CAPTURE: Capture = Capture;

fn main() {
    log::set_logger(&CAPTURE).expect("no other logger in the test");
    log::set_max_level(log::LevelFilter::Debug);

    let text_system = gpui_kit::platform::current_platform(true).text_system();
    let mut cx = TestAppContext::build_with_text_system(TestDispatcher::new(0), None, text_system);
    cx.update(gpui_kit::init);

    let player = Player::builder()
        .video_output(VideoOutput::Null)
        .mpv_option("ao", "null")
        .build()
        .expect("the player builds");
    let parent = cx.new(|_| Parent { player });
    let window = cx.add_window(move |window, cx| gpui_kit::base::Root::new(parent, window, cx));
    cx.run_until_parked();
    cx.update_window(window.into(), |_, window, _| window.refresh()).unwrap();
    cx.run_until_parked();

    let listed = cx.read(|cx| cx.text_system().all_font_names().iter().any(|name| name == "Inter"));
    assert!(listed, "Inter is listed after a player is created");
    let records = RECORDS.lock().unwrap().clone();
    assert_eq!(records, [(log::Level::Debug, "player_gpui: Inter is registered".to_owned())], "registered once");
    cx.quit();
    println!("fonts: ok");
}
