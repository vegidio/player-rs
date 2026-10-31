//! Plays a local file or a URL without a window (audio only), printing the player's events until the media ends.
//!
//! The player is built with `VideoOutput::Null`: with the default `VideoOutput::Render` and no render context, mpv
//! drops the video track, so media without audio would end at once instead of playing through.
//!
//! ```sh
//! cargo run -p player-rs --example listen -- path/to/movie.mp4
//! cargo run -p player-rs --example listen -- https://example.com/movie.mp4
//! ```

use player::prelude::*;
use std::path::Path;
use std::process::ExitCode;

fn main() -> Result<ExitCode> {
    let Some(arg) = std::env::args().nth(1) else {
        eprintln!("usage: listen <path or URL>");
        return Ok(ExitCode::from(2));
    };
    // An existing path is a file even if it looks like a URL, such as `http://clip.mp4` (a directory named `http:`).
    let source = if Path::new(&arg).exists() { Source::file(&arg)? } else { Source::url(&arg)? };

    let player = Player::builder().video_output(VideoOutput::Null).autoplay(Autoplay::On).build()?;
    let events = player.subscribe();
    player.load(source)?;

    // Position changes arrive several times per second; print each whole second once.
    let mut last_second = None;
    for event in events {
        match event {
            Event::PositionChanged(position) => {
                let second = position.map(|position| position.as_secs());
                if second != last_second {
                    last_second = second;
                    println!("position: {}s", second.unwrap_or_default());
                }
            }
            Event::Ended(EndReason::Error(message)) => {
                eprintln!("error: {message}");
                return Ok(ExitCode::FAILURE);
            }
            Event::Ended(reason) => {
                println!("ended: {reason:?}");
                break;
            }
            event => println!("{event:?}"),
        }
    }
    Ok(ExitCode::SUCCESS)
}
