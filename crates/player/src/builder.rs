//! Creating a [`Player`]: [`PlayerBuilder`] and its option types.

use crate::error::{Error, Result};
use crate::event::State;
use crate::mpv::Handle;
use crate::player::Player;

/// What a player does once media is loaded, as in Video.js `autoplay`. Applied to every [`Player::load`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Autoplay {
    /// Stay paused at the start until [`Player::play`] is called.
    #[default]
    Off,
    /// Start playing.
    On,
    /// Mute the player and start playing.
    Muted,
}

/// Where a player draws video. Chosen when the player is built, because mpv cannot change it afterwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum VideoOutput {
    /// No video is shown; audio still plays.
    #[default]
    Null,
    /// The application draws the video through a [`RenderContext`](crate::RenderContext), which must be created
    /// before the first [`Player::load`]. mpv opens no window of its own.
    Render,
}

impl VideoOutput {
    fn mpv_name(self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Render => "libmpv",
        }
    }
}

/// Builds a [`Player`]; created by [`Player::builder`].
///
/// Unset values default to [`Autoplay::Off`], volume `1.0`, unmuted and [`VideoOutput::Null`].
#[derive(Debug, Clone)]
pub struct PlayerBuilder {
    autoplay: Autoplay,
    volume: f64,
    muted: bool,
    video_output: VideoOutput,
    mpv_options: Vec<(String, String)>,
}

impl Default for PlayerBuilder {
    fn default() -> Self {
        Self {
            autoplay: Autoplay::Off,
            volume: 1.0,
            muted: false,
            video_output: VideoOutput::Null,
            mpv_options: vec![],
        }
    }
}

impl PlayerBuilder {
    /// What the player does once media is loaded.
    pub fn autoplay(mut self, autoplay: Autoplay) -> Self {
        self.autoplay = autoplay;
        self
    }

    /// The initial volume, from `0.0` to `1.0`. Other values make [`build`](Self::build) fail.
    pub fn volume(mut self, volume: f64) -> Self {
        self.volume = volume;
        self
    }

    /// Whether the player starts muted.
    pub fn muted(mut self, muted: bool) -> Self {
        self.muted = muted;
        self
    }

    /// Where the player draws video.
    pub fn video_output(mut self, video_output: VideoOutput) -> Self {
        self.video_output = video_output;
        self
    }

    /// Sets any mpv option (see <https://mpv.io/manual/stable/#options>), such as `("ao", "null")`. Raw options are
    /// applied last, so they override the player's own defaults and the typed values above.
    pub fn mpv_option(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.mpv_options.push((name.into(), value.into()));
        self
    }

    /// Creates the player.
    ///
    /// # Errors
    ///
    /// - [`Error::InvalidOption`] if the volume is not within `0.0..=1.0`, or mpv rejects a raw option's name or
    ///   value.
    /// - [`Error::Init`] if mpv cannot be created or initialised.
    pub fn build(self) -> Result<Player> {
        if !(0.0..=1.0).contains(&self.volume) {
            return Err(Error::InvalidOption {
                name: "volume".into(),
                message: format!("{} is outside the accepted range 0.0..=1.0", self.volume),
            });
        }

        // Dropping `mpv` on any early return destroys the half-built instance.
        let mpv = Handle::create().ok_or_else(|| Error::Init("mpv_create failed".into()))?;

        // Embedding defaults: the frontend owns all input and drawing, and media stays loaded at its end (Video.js).
        let defaults = [
            ("vo", self.video_output.mpv_name()),
            ("idle", "yes"),
            ("keep-open", "yes"),
            ("input-default-bindings", "no"),
            ("input-vo-keyboard", "no"),
            ("osc", "no"),
        ];
        for (name, value) in defaults {
            mpv.set_option(name, value)?;
        }

        let paused = self.autoplay == Autoplay::Off;
        let muted = self.muted || self.autoplay == Autoplay::Muted;
        mpv.set_option("volume", &(self.volume * 100.0).to_string())?;
        mpv.set_option("mute", if muted { "yes" } else { "no" })?;
        mpv.set_option("pause", if paused { "yes" } else { "no" })?;

        for (name, value) in &self.mpv_options {
            mpv.set_option(name, value)?;
        }

        mpv.initialize().map_err(Error::Init)?;

        let state = State {
            idle: true,
            position: None,
            duration: None,
            paused,
            buffering: false,
            volume: self.volume,
            muted,
            rate: 1.0,
            ended: false,
            video_size: None,
            buffered: None,
        };
        Player::start(mpv, state, self.autoplay, self.video_output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_player() {
        let player = Player::builder().mpv_option("ao", "null").build().unwrap();
        let state = player.state();
        assert!(state.idle);
        assert_eq!((state.position, state.duration), (None, None));
        assert_eq!((state.volume, state.muted, state.rate), (1.0, false, 1.0));
        assert_eq!(player.get_property::<f64>("volume").unwrap(), 100.0);
        assert!(player.get_property::<bool>("idle-active").unwrap());
    }

    #[test]
    fn embedding_defaults() {
        let player = Player::builder().mpv_option("ao", "null").build().unwrap();
        assert!(!player.get_property::<bool>("input-default-bindings").unwrap());
        assert!(!player.get_property::<bool>("input-vo-keyboard").unwrap());
        assert!(!player.get_property::<bool>("osc").unwrap());
    }

    #[test]
    fn render_output() {
        let player = Player::builder().video_output(VideoOutput::Render).mpv_option("ao", "null").build().unwrap();
        assert_eq!(player.get_property::<String>("vo").unwrap(), "libmpv");
    }

    #[test]
    fn rejects_volume_out_of_range() {
        for volume in [-0.1, 1.01, f64::NAN, f64::INFINITY] {
            let Err(Error::InvalidOption { name, message }) = Player::builder().volume(volume).build() else {
                panic!("volume {volume} was accepted")
            };
            assert_eq!(name, "volume");
            assert!(message.contains("0.0..=1.0"), "{message}");
        }
    }

    #[test]
    fn rejects_unknown_raw_option() {
        let Err(Error::InvalidOption { name, message }) = Player::builder().mpv_option("no-such-option", "1").build()
        else {
            panic!("an unknown option was accepted")
        };
        assert_eq!(name, "no-such-option");
        assert!(!message.is_empty());
    }

    #[test]
    fn rejects_bad_raw_option_value() {
        let result = Player::builder().mpv_option("keep-open", "sometimes").build();
        assert!(matches!(result, Err(Error::InvalidOption { name, .. }) if name == "keep-open"));
    }
}
