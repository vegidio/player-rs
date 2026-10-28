//! [`Options`], the Video.js player options that concern the UI, and their parts.

use crate::error::{Error, Result};
use crate::ui::keys::Key;
use std::fmt;
use std::str::FromStr;
use std::time::Duration;

/// The UI options of a player, as in Video.js; created by [`Options::builder`].
///
/// `Clone`, so one value can configure many players.
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    controls: bool,
    poster: Option<String>,
    playback_rates: Vec<f64>,
    aspect_ratio: Option<AspectRatio>,
    inactivity_timeout: Duration,
    click: bool,
    double_click: bool,
    hotkeys: Hotkeys,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            controls: true,
            poster: None,
            playback_rates: vec![],
            aspect_ratio: None,
            inactivity_timeout: Duration::from_secs(2),
            click: true,
            double_click: true,
            hotkeys: Hotkeys::default(),
        }
    }
}

impl Options {
    /// Starts building UI options.
    pub fn builder() -> OptionsBuilder {
        OptionsBuilder::default()
    }

    /// Whether the controls are shown (Video.js `controls`).
    pub fn controls(&self) -> bool {
        self.controls
    }

    /// The poster image shown before playback starts (Video.js `poster`).
    pub fn poster(&self) -> Option<&str> {
        self.poster.as_deref()
    }

    /// The playback rates offered by the rate control (Video.js `playbackRates`). Empty means no rate control.
    pub fn playback_rates(&self) -> &[f64] {
        &self.playback_rates
    }

    /// The player's fixed aspect ratio (Video.js `aspectRatio`).
    pub fn aspect_ratio(&self) -> Option<AspectRatio> {
        self.aspect_ratio
    }

    /// How long without input before the user is inactive (Video.js `inactivityTimeout`). Zero means never.
    pub fn inactivity_timeout(&self) -> Duration {
        self.inactivity_timeout
    }

    /// Whether a click on the video toggles playback (Video.js `userActions.click`).
    pub fn click(&self) -> bool {
        self.click
    }

    /// Whether a double-click on the video toggles fullscreen (Video.js `userActions.doubleClick`).
    pub fn double_click(&self) -> bool {
        self.double_click
    }

    /// The global hotkeys (Video.js `userActions.hotkeys`).
    pub fn hotkeys(&self) -> &Hotkeys {
        &self.hotkeys
    }

    /// The height a player `width` wide should take, as in Video.js's fluid mode: from the configured aspect ratio,
    /// else from the video's display size (see [`State::video_size`](crate::State::video_size)), else 16:9.
    pub fn player_height(&self, width: f64, video_size: Option<(u32, u32)>) -> f64 {
        let ratio = self
            .aspect_ratio
            .or_else(|| video_size.and_then(|(w, h)| AspectRatio::checked(f64::from(w), f64::from(h))))
            .unwrap_or(AspectRatio { width: 16.0, height: 9.0 });
        width * ratio.height / ratio.width
    }
}

/// Builds [`Options`]; created by [`Options::builder`].
///
/// Unset values default as in Video.js: controls shown, no poster, no playback rates, no fixed aspect ratio, an
/// inactivity timeout of 2 seconds, and click and double-click enabled. Unlike Video.js, hotkeys are enabled.
#[derive(Debug, Clone, Default)]
pub struct OptionsBuilder {
    options: Options,
    aspect_ratio: Option<String>,
}

impl OptionsBuilder {
    /// Whether the controls are shown. Hidden controls also turn off clicks and double-clicks on the video.
    pub fn controls(mut self, controls: bool) -> Self {
        self.options.controls = controls;
        self
    }

    /// The poster image, as a path or URL, shown before playback starts.
    pub fn poster(mut self, poster: impl Into<String>) -> Self {
        self.options.poster = Some(poster.into());
        self
    }

    /// The playback rates the rate control cycles through, such as `[0.5, 1.0, 1.5, 2.0]`. Each must be finite and
    /// within `0.01..=100.0`, or [`build`](Self::build) fails.
    pub fn playback_rates(mut self, rates: impl IntoIterator<Item = f64>) -> Self {
        self.options.playback_rates = rates.into_iter().collect();
        self
    }

    /// A fixed aspect ratio for the player, as Video.js's `"W:H"` text (`"16:9"`, `"2.35:1"`) or an
    /// [`AspectRatio`]. A malformed ratio makes [`build`](Self::build) fail.
    pub fn aspect_ratio(mut self, ratio: impl Into<String>) -> Self {
        self.aspect_ratio = Some(ratio.into());
        self
    }

    /// How long without input before the user is inactive and the control bar hides during playback. Zero means
    /// never.
    pub fn inactivity_timeout(mut self, timeout: Duration) -> Self {
        self.options.inactivity_timeout = timeout;
        self
    }

    /// Whether a click on the video toggles playback.
    pub fn click(mut self, click: bool) -> Self {
        self.options.click = click;
        self
    }

    /// Whether a double-click on the video toggles fullscreen.
    pub fn double_click(mut self, double_click: bool) -> Self {
        self.options.double_click = double_click;
        self
    }

    /// The global hotkeys; [`Hotkeys::disabled`] turns them off.
    pub fn hotkeys(mut self, hotkeys: Hotkeys) -> Self {
        self.options.hotkeys = hotkeys;
        self
    }

    /// Validates the options.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidOption`], named after the Video.js option, if a playback rate is not finite or is outside
    /// `0.01..=100.0` (`playbackRates`), or the aspect ratio is not two positive numbers in `W:H` form
    /// (`aspectRatio`).
    pub fn build(self) -> Result<Options> {
        let mut options = self.options;
        if let Some(rate) = options.playback_rates.iter().find(|rate| !(0.01..=100.0).contains(*rate)) {
            return Err(Error::InvalidOption {
                name: "playbackRates".into(),
                message: format!("{rate} is not within the accepted range 0.01..=100.0"),
            });
        }
        if let Some(text) = self.aspect_ratio {
            let ratio = text.parse().map_err(|()| Error::InvalidOption {
                name: "aspectRatio".into(),
                message: format!("'{text}' is not two positive numbers in W:H form, such as 16:9"),
            })?;
            options.aspect_ratio = Some(ratio);
        }
        Ok(options)
    }
}

/// A player's aspect ratio: a positive width and height, such as 16:9.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AspectRatio {
    width: f64,
    height: f64,
}

impl AspectRatio {
    /// A ratio of `width` to `height`. [`OptionsBuilder::build`] rejects it unless both are positive and finite.
    pub fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }

    /// The ratio, if both sides are positive and finite.
    fn checked(width: f64, height: f64) -> Option<Self> {
        let positive = |n: f64| n.is_finite() && n > 0.0;
        (positive(width) && positive(height)).then_some(Self { width, height })
    }

    /// The width part of the ratio.
    pub fn width(self) -> f64 {
        self.width
    }

    /// The height part of the ratio.
    pub fn height(self) -> f64 {
        self.height
    }
}

/// Writes the ratio as `W:H`, the form [`OptionsBuilder::aspect_ratio`] accepts.
impl fmt::Display for AspectRatio {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.width, self.height)
    }
}

impl From<AspectRatio> for String {
    fn from(ratio: AspectRatio) -> Self {
        ratio.to_string()
    }
}

/// Parses Video.js's pattern, `^\d+(\.\d+)?:\d+(\.\d+)?$`, rejecting zero sides.
impl FromStr for AspectRatio {
    type Err = ();

    fn from_str(text: &str) -> std::result::Result<Self, ()> {
        fn number(text: &str) -> Option<f64> {
            let (whole, fraction) = text.split_once('.').unwrap_or((text, "0"));
            let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
            (digits(whole) && digits(fraction)).then(|| text.parse().ok()).flatten()
        }
        let (width, height) = text.split_once(':').ok_or(())?;
        Self::checked(number(width).ok_or(())?, number(height).ok_or(())?).ok_or(())
    }
}

/// The global hotkeys, as Video.js's `userActions.hotkeys`: `f` toggles fullscreen, `m` mute, and `k` or Space
/// play. Each action's keys can be replaced, and [`disabled`](Self::disabled) turns them all off.
///
/// Letters match in either case. Slider keys (arrows and the like on the seek and volume bars) are not hotkeys and
/// work even when these are disabled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hotkeys {
    enabled: bool,
    fullscreen: Vec<Key>,
    mute: Vec<Key>,
    play_pause: Vec<Key>,
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            enabled: true,
            fullscreen: vec![Key::Char('f')],
            mute: vec![Key::Char('m')],
            play_pause: vec![Key::Char('k'), Key::Space],
        }
    }
}

impl Hotkeys {
    /// No global hotkeys, as Video.js's default.
    pub fn disabled() -> Self {
        Self { enabled: false, ..Self::default() }
    }

    /// Replaces the keys that toggle fullscreen.
    pub fn fullscreen(mut self, keys: impl IntoIterator<Item = Key>) -> Self {
        self.fullscreen = keys.into_iter().collect();
        self
    }

    /// Replaces the keys that toggle mute.
    pub fn mute(mut self, keys: impl IntoIterator<Item = Key>) -> Self {
        self.mute = keys.into_iter().collect();
        self
    }

    /// Replaces the keys that toggle play.
    pub fn play_pause(mut self, keys: impl IntoIterator<Item = Key>) -> Self {
        self.play_pause = keys.into_iter().collect();
        self
    }

    /// Whether the hotkeys are on.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// The keys that toggle fullscreen.
    pub fn fullscreen_keys(&self) -> &[Key] {
        &self.fullscreen
    }

    /// The keys that toggle mute.
    pub fn mute_keys(&self) -> &[Key] {
        &self.mute
    }

    /// The keys that toggle play.
    pub fn play_pause_keys(&self) -> &[Key] {
        &self.play_pause
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invalid_option(result: Result<Options>) -> String {
        match result {
            Err(Error::InvalidOption { name, .. }) => name,
            other => panic!("expected an invalid option, got {other:?}"),
        }
    }

    #[test]
    fn defaults() {
        let options = Options::builder().build().unwrap();
        assert!(options.controls());
        assert_eq!(options.poster(), None);
        assert!(options.playback_rates().is_empty());
        assert_eq!(options.aspect_ratio(), None);
        assert_eq!(options.inactivity_timeout(), Duration::from_secs(2));
        assert!(options.click() && options.double_click());
        assert!(options.hotkeys().is_enabled());
        assert_eq!(options.hotkeys(), &Hotkeys::default());
    }

    #[test]
    fn set_values() {
        let options = Options::builder()
            .controls(false)
            .poster("poster.jpg")
            .playback_rates([0.5, 1.0, 2.0])
            .inactivity_timeout(Duration::ZERO)
            .click(false)
            .double_click(false)
            .hotkeys(Hotkeys::disabled())
            .build()
            .unwrap();
        assert!(!options.controls());
        assert_eq!(options.poster(), Some("poster.jpg"));
        assert_eq!(options.playback_rates(), [0.5, 1.0, 2.0]);
        assert_eq!(options.inactivity_timeout(), Duration::ZERO);
        assert!(!options.click() && !options.double_click());
        assert!(!options.hotkeys().is_enabled());
    }

    #[test]
    fn rejects_invalid_playback_rates() {
        for rates in [vec![0.5, f64::NAN], vec![0.0, 1.0], vec![f64::INFINITY], vec![100.5], vec![-1.0]] {
            let result = Options::builder().playback_rates(rates.clone()).build();
            assert_eq!(invalid_option(result), "playbackRates", "{rates:?}");
        }
        let edges = Options::builder().playback_rates([0.01, 100.0]).build();
        assert!(edges.is_ok());
    }

    #[test]
    fn aspect_ratio_text() {
        for (text, width, height) in [("16:9", 16.0, 9.0), ("4:3", 4.0, 3.0), ("2.35:1", 2.35, 1.0)] {
            let ratio = Options::builder().aspect_ratio(text).build().unwrap().aspect_ratio().unwrap();
            assert_eq!((ratio.width(), ratio.height()), (width, height), "{text}");
        }
        let ratio = Options::builder().aspect_ratio(AspectRatio::new(21.0, 9.0)).build().unwrap();
        assert_eq!(ratio.aspect_ratio(), Some(AspectRatio::new(21.0, 9.0)));
    }

    #[test]
    fn rejects_malformed_aspect_ratios() {
        for text in ["16x9", "16:0", "wide", "0:9", "", ":", "16:", ":9", "16:9:1", "-16:9", "1.:2", ".5:1", " 16:9"] {
            let result = Options::builder().aspect_ratio(text).build();
            assert_eq!(invalid_option(result), "aspectRatio", "{text:?}");
        }
        for (width, height) in [(0.0, 9.0), (f64::NAN, 9.0), (16.0, f64::INFINITY), (-4.0, 3.0)] {
            let result = Options::builder().aspect_ratio(AspectRatio::new(width, height)).build();
            assert_eq!(invalid_option(result), "aspectRatio", "{width}:{height}");
        }
    }

    #[test]
    fn player_height() {
        let configured = Options::builder().aspect_ratio("4:3").build().unwrap();
        assert_eq!(configured.player_height(800.0, None), 600.0);
        assert_eq!(configured.player_height(800.0, Some((1920, 1080))), 600.0);

        let unset = Options::default();
        assert_eq!(unset.player_height(800.0, Some((160, 120))), 600.0);
        assert_eq!(unset.player_height(1600.0, None), 900.0);
        assert_eq!(unset.player_height(1600.0, Some((0, 120))), 900.0);
    }

    #[test]
    fn hotkey_remapping() {
        let hotkeys = Hotkeys::default().mute([Key::Char('x')]).fullscreen([]);
        assert_eq!(hotkeys.mute_keys(), [Key::Char('x')]);
        assert!(hotkeys.fullscreen_keys().is_empty());
        assert_eq!(hotkeys.play_pause_keys(), [Key::Char('k'), Key::Space]);
    }
}
