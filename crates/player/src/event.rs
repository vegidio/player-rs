//! Playback state, events, and the thread that turns mpv's events into both.

use crate::builder::Autoplay;
use crate::mpv::{Handle, MpvEvent, PropertyData};
use crate::source::Source;
use crate::sys;
use std::ffi::c_int;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError, RwLock};
use std::time::Duration;

/// A snapshot of a player's state, from [`Player::state`](crate::Player::state).
///
/// It is updated before the [`Event`] announcing a change is sent, so a snapshot taken after receiving an event is at
/// least as new as that event.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct State {
    /// No media is loaded.
    pub idle: bool,
    /// The playback position, if known.
    pub position: Option<Duration>,
    /// The media's duration, if known.
    pub duration: Option<Duration>,
    /// Playback is paused.
    pub paused: bool,
    /// Playback is stalled waiting for the network cache to fill.
    pub buffering: bool,
    /// The volume, from `0.0` to `1.0`.
    pub volume: f64,
    /// The audio is muted. Independent of [`volume`](Self::volume).
    pub muted: bool,
    /// The playback rate, `1.0` being normal speed.
    pub rate: f64,
    /// Playback reached the end of the media, which stays loaded and paused at its end.
    pub ended: bool,
    /// The size at which the video is meant to be shown, as `(width, height)` in pixels, with its pixel aspect ratio
    /// and rotation applied. `None` while unknown or when no video is shown: with [`VideoOutput::Null`], for media
    /// without video, or with no [`RenderContext`](crate::RenderContext).
    ///
    /// [`VideoOutput::Null`]: crate::VideoOutput::Null
    pub video_size: Option<(u32, u32)>,
    /// The media time up to which data is cached ahead of playback, from mpv's `demuxer-cache-time`. `None` while
    /// unknown or when mpv keeps no demuxer cache for the media.
    ///
    /// With mpv's default `cache=auto`, a local file reports only the demuxer's readahead, about a second past the
    /// position, rather than the whole file; set the mpv option `cache=yes` to cache local files fully.
    pub buffered: Option<Duration>,
}

/// Something that happened in a player, received through [`Events`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Event {
    /// Media finished loading and is ready to play.
    Loaded,
    /// Media ended or could not be loaded.
    Ended(EndReason),
    /// The playback position changed.
    PositionChanged(Option<Duration>),
    /// The media's duration became known or changed.
    DurationChanged(Option<Duration>),
    /// Playback was paused (`true`) or resumed (`false`).
    PausedChanged(bool),
    /// Playback started (`true`) or stopped (`false`) waiting for the network cache.
    BufferingChanged(bool),
    /// The volume changed, from `0.0` to `1.0`.
    VolumeChanged(f64),
    /// The audio was muted or unmuted.
    MutedChanged(bool),
    /// The playback rate changed.
    RateChanged(f64),
    /// The video's display size changed; see [`State::video_size`].
    VideoSizeChanged(Option<(u32, u32)>),
    /// The buffered position changed; see [`State::buffered`].
    BufferedChanged(Option<Duration>),
    /// A seek started.
    Seeking,
    /// A seek completed and playback continues from the new position.
    Seeked,
    /// No media is loaded any more.
    Idle,
}

/// Why media ended.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum EndReason {
    /// Playback reached the end of the media. The media stays loaded and paused at its end, as in Video.js.
    Eof,
    /// Playback was stopped.
    Stopped,
    /// Other media was loaded in its place.
    Replaced,
    /// The media could not be opened or played. Carries mpv's message.
    Error(String),
}

/// A subscription to a player's events, from [`Player::subscribe`](crate::Player::subscribe).
///
/// Events are queued from the moment of subscribing, without limit, so drop an `Events` that is no longer read.
/// Every receiving method returns `None` once the player has been dropped and the queue is empty.
#[derive(Debug)]
pub struct Events {
    receiver: flume::Receiver<Event>,
}

impl Events {
    /// Blocks until the next event.
    pub fn recv(&self) -> Option<Event> {
        self.receiver.recv().ok()
    }

    /// Returns the next event if one is queued, without blocking.
    pub fn try_recv(&self) -> Option<Event> {
        self.receiver.try_recv().ok()
    }

    /// Blocks until the next event, for at most `timeout`. Returns `None` on timeout.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<Event> {
        self.receiver.recv_timeout(timeout).ok()
    }

    /// Waits for the next event in any async executor, without blocking the thread.
    pub async fn recv_async(&self) -> Option<Event> {
        self.receiver.recv_async().await.ok()
    }
}

impl Iterator for Events {
    type Item = Event;

    /// Blocks until the next event; ends once the player has been dropped.
    fn next(&mut self) -> Option<Event> {
        self.recv()
    }
}

/// What the player handles and the event thread share.
pub(crate) struct Shared {
    state: RwLock<State>,
    subscribers: Mutex<Vec<flume::Sender<Event>>>,
    /// Set when the player is dropped, so the event thread exits.
    pub(crate) stop: AtomicBool,
    recovery: Mutex<VideoRecovery>,
}

/// Media without audio, loaded while no render context exists, ends at once: its video output cannot start. A surface
/// renderer loads it again, from whichever of it or the event thread learns last of the other's side.
#[derive(Debug, Default)]
struct VideoRecovery {
    /// What [`Player::load`](crate::Player::load) last loaded.
    last: Option<Source>,
    /// It was loaded while the player had no render context, so its video output could not start.
    without_render: bool,
    /// It ended for that reason.
    failed: bool,
    /// A surface renderer exists.
    surface: bool,
}

impl VideoRecovery {
    /// Records an END_FILE error. mpv reports a video output that cannot start as `MPV_ERROR_NOTHING_TO_PLAY` when the
    /// media has no audio, which media with nothing playable reports too; only a load made without a render context
    /// counts.
    fn ended(&mut self, error: c_int) {
        let video = matches!(error, sys::MPV_ERROR_VO_INIT_FAILED | sys::MPV_ERROR_NOTHING_TO_PLAY);
        self.failed = video && self.without_render;
    }

    /// The source to load again, if its media failed while a surface renderer exists. A source is loaded again once,
    /// and with a render context, so loading it again can never ask for another.
    fn take(&mut self) -> Option<Source> {
        if self.failed && self.surface {
            self.failed = false;
            self.without_render = false;
            self.last.clone()
        } else {
            None
        }
    }
}

impl Shared {
    pub(crate) fn new(state: State) -> Self {
        Self {
            state: RwLock::new(state),
            subscribers: Mutex::new(Vec::new()),
            stop: AtomicBool::new(false),
            recovery: Mutex::default(),
        }
    }

    fn recovery(&self) -> std::sync::MutexGuard<'_, VideoRecovery> {
        self.recovery.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Records the source [`Player::load`](crate::Player::load) loads, and whether the player has a render context.
    pub(crate) fn loading(&self, source: &Source, rendering: bool) {
        let mut recovery = self.recovery();
        recovery.last = Some(source.clone());
        recovery.without_render = !rendering;
        recovery.failed = false;
    }

    /// Records whether a surface renderer exists, and returns the source to load again if its media already failed
    /// for want of one.
    #[cfg(any(target_os = "macos", windows))]
    pub(crate) fn surface_attached(&self, attached: bool) -> Option<Source> {
        let mut recovery = self.recovery();
        recovery.surface = attached;
        recovery.take()
    }

    pub(crate) fn state(&self) -> State {
        self.state.read().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub(crate) fn subscribe(&self) -> Events {
        let (sender, receiver) = flume::unbounded();
        self.subscribers.lock().unwrap_or_else(PoisonError::into_inner).push(sender);
        Events { receiver }
    }

    /// Applies `change` to the state and sends the event it returns, in that order.
    fn update(&self, change: impl FnOnce(&mut State) -> Option<Event>) {
        let event = change(&mut self.state.write().unwrap_or_else(PoisonError::into_inner));
        if let Some(event) = event {
            self.publish(event);
        }
    }

    /// Sends `event` to every subscriber, forgetting those whose [`Events`] was dropped.
    fn publish(&self, event: Event) {
        let mut subscribers = self.subscribers.lock().unwrap_or_else(PoisonError::into_inner);
        subscribers.retain(|sender| sender.send(event.clone()).is_ok());
    }
}

/// The `reply_userdata` of the asynchronous `loadfile` sent by [`Player::load`](crate::Player::load).
pub(crate) const LOAD_REPLY: u64 = 1;

/// Loads `source` as [`Player::load`](crate::Player::load) does, applying the [`Autoplay`] mode.
pub(crate) fn load(mpv: &Handle, autoplay: Autoplay, source: &Source) -> crate::Result<()> {
    mpv.set("pause", autoplay == Autoplay::Off)?;
    if autoplay == Autoplay::Muted {
        mpv.set("mute", true)?;
    }
    mpv.command_async(LOAD_REPLY, &["loadfile", source.as_str(), "replace"])
}

/// [`load`] without waiting on mpv, for the event thread: a synchronous call there could wait on mpv while mpv waits
/// for its events to be read. mpv runs asynchronous commands in order.
fn reload_async(mpv: &Handle, autoplay: Autoplay, source: &Source) {
    let pause = if autoplay == Autoplay::Off { "yes" } else { "no" };
    let _ = mpv.command_async(0, &["set", "pause", pause]);
    if autoplay == Autoplay::Muted {
        let _ = mpv.command_async(0, &["set", "mute", "yes"]);
    }
    let _ = mpv.command_async(LOAD_REPLY, &["loadfile", source.as_str(), "replace"]);
}

/// The observed properties, identified by their `reply_userdata`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Property {
    TimePos = 1,
    Duration,
    Pause,
    PausedForCache,
    Volume,
    Mute,
    Speed,
    IdleActive,
    EofReached,
    DWidth,
    DHeight,
    DemuxerCacheTime,
}

impl Property {
    const ALL: [Self; 12] = [
        Self::TimePos,
        Self::Duration,
        Self::Pause,
        Self::PausedForCache,
        Self::Volume,
        Self::Mute,
        Self::Speed,
        Self::IdleActive,
        Self::EofReached,
        Self::DWidth,
        Self::DHeight,
        Self::DemuxerCacheTime,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::TimePos => "time-pos",
            Self::Duration => "duration",
            Self::Pause => "pause",
            Self::PausedForCache => "paused-for-cache",
            Self::Volume => "volume",
            Self::Mute => "mute",
            Self::Speed => "speed",
            Self::IdleActive => "idle-active",
            Self::EofReached => "eof-reached",
            Self::DWidth => "dwidth",
            Self::DHeight => "dheight",
            Self::DemuxerCacheTime => "demuxer-cache-time",
        }
    }

    fn is_flag(self) -> bool {
        !matches!(
            self,
            Self::TimePos
                | Self::Duration
                | Self::Volume
                | Self::Speed
                | Self::DWidth
                | Self::DHeight
                | Self::DemuxerCacheTime
        )
    }

    fn from_id(id: u64) -> Option<Self> {
        Self::ALL.into_iter().find(|property| *property as u64 == id)
    }

    /// Reads the property's current value directly, for when the state must be current before an event is sent.
    fn read(self, mpv: &Handle) -> PropertyData {
        if self.is_flag() {
            mpv.get::<bool>(self.name()).map_or(PropertyData::None, PropertyData::Flag)
        } else {
            mpv.get::<f64>(self.name()).map_or(PropertyData::None, PropertyData::Double)
        }
    }

    /// Writes the property's new value into `state` and returns the event announcing it, if the value changed.
    fn apply(self, state: &mut State, value: PropertyData) -> Option<Event> {
        let flag = matches!(value, PropertyData::Flag(true));
        let seconds = match value {
            PropertyData::Double(seconds) if seconds.is_finite() => Some(Duration::from_secs_f64(seconds.max(0.0))),
            _ => None,
        };
        let number = match value {
            PropertyData::Double(number) => Some(number),
            _ => None,
        };

        fn set<T: PartialEq + Copy>(field: &mut T, value: T, event: impl FnOnce(T) -> Option<Event>) -> Option<Event> {
            if *field == value {
                return None;
            }
            *field = value;
            event(value)
        }

        match self {
            Self::TimePos => set(&mut state.position, seconds, |v| Some(Event::PositionChanged(v))),
            Self::Duration => set(&mut state.duration, seconds, |v| Some(Event::DurationChanged(v))),
            Self::DemuxerCacheTime => set(&mut state.buffered, seconds, |v| Some(Event::BufferedChanged(v))),
            Self::Pause => set(&mut state.paused, flag, |v| Some(Event::PausedChanged(v))),
            Self::PausedForCache => set(&mut state.buffering, flag, |v| Some(Event::BufferingChanged(v))),
            Self::Volume => set(&mut state.volume, number? / 100.0, |v| Some(Event::VolumeChanged(v))),
            Self::Mute => set(&mut state.muted, flag, |v| Some(Event::MutedChanged(v))),
            Self::Speed => set(&mut state.rate, number?, |v| Some(Event::RateChanged(v))),
            Self::IdleActive => set(&mut state.idle, flag, |v| v.then_some(Event::Idle)),
            Self::EofReached => set(&mut state.ended, flag, |v| v.then_some(Event::Ended(EndReason::Eof))),
            // Both dimensions make one value; see `apply_video_size`.
            Self::DWidth | Self::DHeight => None,
        }
    }
}

/// Writes the video's display size into `state` and returns the event announcing it, if it changed. The size is
/// known only when both dimensions are known and positive.
fn apply_video_size(state: &mut State, width: PropertyData, height: PropertyData) -> Option<Event> {
    let pixels = |value| match value {
        PropertyData::Double(pixels) if pixels.is_finite() && pixels >= 1.0 => Some(pixels.round() as u32),
        _ => None,
    };
    let size = pixels(width).zip(pixels(height));
    if state.video_size == size {
        return None;
    }
    state.video_size = size;
    Some(Event::VideoSizeChanged(size))
}

/// Subscribes to every property in the state table. The video's dimensions are observed only when `video` is set:
/// mpv reports them even with `vo=null`, which shows no video.
pub(crate) fn observe(mpv: &Handle, video: bool) -> Result<(), String> {
    for property in Property::ALL {
        if !video && matches!(property, Property::DWidth | Property::DHeight) {
            continue;
        }
        let format = if property.is_flag() { sys::MPV_FORMAT_FLAG } else { sys::MPV_FORMAT_DOUBLE };
        mpv.observe(property as u64, property.name(), format)?;
    }
    Ok(())
}

/// Tells media that was replaced apart from media that was stopped: mpv reports both as `MPV_END_FILE_REASON_STOP`.
///
/// It counts the `loadfile` commands mpv has accepted since the current media started. mpv accepts a `loadfile`
/// before it ends the media being replaced, and starts the new media after, so a stop with a count above zero is a
/// replacement. Counting accepted commands rather than requests keeps the count right when several loads arrive
/// before any of them starts, since mpv then starts only the last one.
#[derive(Debug, Default)]
struct LoadTracker {
    loads_since_start: u32,
}

impl LoadTracker {
    fn on_load_accepted(&mut self) {
        self.loads_since_start = self.loads_since_start.saturating_add(1);
    }

    fn on_start_file(&mut self) {
        self.loads_since_start = 0;
    }

    /// Maps an END_FILE to the reason to report, or `None` for one that is not reported.
    fn on_end_file(&self, reason: c_int, error: c_int) -> Option<EndReason> {
        match reason {
            sys::MPV_END_FILE_REASON_EOF => Some(EndReason::Eof),
            sys::MPV_END_FILE_REASON_STOP if self.loads_since_start > 0 => Some(EndReason::Replaced),
            sys::MPV_END_FILE_REASON_STOP | sys::MPV_END_FILE_REASON_QUIT => Some(EndReason::Stopped),
            sys::MPV_END_FILE_REASON_ERROR => Some(EndReason::Error(crate::error::mpv_message(error))),
            // REDIRECT: a playlist file was expanded and its entries play next.
            _ => None,
        }
    }
}

/// The event loop: the only caller of `mpv_wait_event` on `mpv`. Runs until `shared.stop` is set or mpv shuts down.
pub(crate) fn run(mpv: &Handle, shared: &Shared, autoplay: Autoplay) {
    let mut loads = LoadTracker::default();
    let mut seeking = false;
    let refresh = |properties: &[Property]| {
        for &property in properties {
            let value = property.read(mpv);
            shared.update(|state| property.apply(state, value));
        }
    };

    while !shared.stop.load(Ordering::Acquire) {
        // SAFETY: this loop runs on one thread and is the only caller of `wait_event` on this handle.
        match unsafe { mpv.wait_event(-1.0) } {
            MpvEvent::Shutdown => break,
            MpvEvent::PropertyChange { id, value } => match Property::from_id(id) {
                // A new video size usually changes both dimensions, notified one at a time. Reading both current
                // values on either notification announces the new size once, instead of passing through a mix of
                // old and new dimensions.
                Some(Property::DWidth | Property::DHeight) => {
                    let (width, height) = (Property::DWidth.read(mpv), Property::DHeight.read(mpv));
                    shared.update(|state| apply_video_size(state, width, height));
                }
                Some(property) => shared.update(|state| property.apply(state, value)),
                None => {}
            },
            MpvEvent::CommandReply { id: LOAD_REPLY, error } if error >= 0 => loads.on_load_accepted(),
            MpvEvent::StartFile => {
                loads.on_start_file();
                shared.recovery().failed = false;
                // With `keep-open=no` the END_FILE that set `ended` unloads the media, so no `eof-reached` change
                // clears it; new media has not ended.
                shared.update(|state| {
                    state.ended = false;
                    None
                });
            }
            MpvEvent::FileLoaded => {
                // The observed values may arrive after FILE_LOADED; read them so `Loaded` comes with a current state.
                refresh(&[Property::Duration, Property::TimePos, Property::Pause]);
                shared.publish(Event::Loaded);
            }
            MpvEvent::EndFile { reason, error } => {
                if reason == sys::MPV_END_FILE_REASON_ERROR {
                    let reload = {
                        let mut recovery = shared.recovery();
                        recovery.ended(error);
                        recovery.take()
                    };
                    if let Some(source) = reload {
                        reload_async(mpv, autoplay, &source);
                    }
                }
                if let Some(reason) = loads.on_end_file(reason, error) {
                    shared.update(|state| match reason {
                        // With `keep-open=no`, an `eof-reached` change may have announced this end already.
                        EndReason::Eof if state.ended => None,
                        EndReason::Eof => {
                            state.ended = true;
                            Some(Event::Ended(reason))
                        }
                        _ => Some(Event::Ended(reason)),
                    });
                }
            }
            MpvEvent::Seek => {
                seeking = true;
                shared.publish(Event::Seeking);
            }
            // PLAYBACK_RESTART also follows loading; only one that ends a seek is reported.
            MpvEvent::PlaybackRestart if seeking => {
                seeking = false;
                refresh(&[Property::TimePos]);
                shared.publish(Event::Seeked);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> State {
        State {
            idle: true,
            position: None,
            duration: None,
            paused: false,
            buffering: false,
            volume: 1.0,
            muted: false,
            rate: 1.0,
            ended: false,
            video_size: None,
            buffered: None,
        }
    }

    #[test]
    fn end_file_reasons() {
        let mut loads = LoadTracker::default();
        assert_eq!(loads.on_end_file(sys::MPV_END_FILE_REASON_EOF, 0), Some(EndReason::Eof));
        assert_eq!(loads.on_end_file(sys::MPV_END_FILE_REASON_STOP, 0), Some(EndReason::Stopped));
        assert_eq!(loads.on_end_file(sys::MPV_END_FILE_REASON_QUIT, 0), Some(EndReason::Stopped));
        assert_eq!(loads.on_end_file(sys::MPV_END_FILE_REASON_REDIRECT, 0), None);
        assert_eq!(
            loads.on_end_file(sys::MPV_END_FILE_REASON_ERROR, sys::MPV_ERROR_LOADING_FAILED),
            Some(EndReason::Error("loading failed".into()))
        );

        // A load accepted while media plays: the media is replaced, then the new media starts.
        loads.on_load_accepted();
        assert_eq!(loads.on_end_file(sys::MPV_END_FILE_REASON_STOP, 0), Some(EndReason::Replaced));
        loads.on_start_file();
        assert_eq!(loads.on_end_file(sys::MPV_END_FILE_REASON_STOP, 0), Some(EndReason::Stopped));

        // Two loads accepted before either starts: mpv only starts the last, and a later stop is still a stop.
        loads.on_load_accepted();
        loads.on_load_accepted();
        loads.on_start_file();
        assert_eq!(loads.on_end_file(sys::MPV_END_FILE_REASON_STOP, 0), Some(EndReason::Stopped));
    }

    #[test]
    fn property_changes_update_state_once() {
        let mut state = state();
        let half = PropertyData::Double(50.0);
        assert_eq!(Property::Volume.apply(&mut state, half), Some(Event::VolumeChanged(0.5)));
        assert_eq!(Property::Volume.apply(&mut state, half), None);
        assert_eq!(state.volume, 0.5);

        let position = PropertyData::Double(1.5);
        let expected = Some(Duration::from_millis(1500));
        assert_eq!(Property::TimePos.apply(&mut state, position), Some(Event::PositionChanged(expected)));
        assert_eq!(Property::TimePos.apply(&mut state, PropertyData::None), Some(Event::PositionChanged(None)));
        assert_eq!(state.position, None);
    }

    #[test]
    fn video_size_changes_once_per_size() {
        let mut state = state();
        let px = PropertyData::Double;
        // Width first known, height not yet: still unknown.
        assert_eq!(apply_video_size(&mut state, px(160.0), PropertyData::None), None);
        // The width's and then the height's notification each read both dimensions: one event.
        assert_eq!(
            apply_video_size(&mut state, px(160.0), px(120.0)),
            Some(Event::VideoSizeChanged(Some((160, 120))))
        );
        assert_eq!(apply_video_size(&mut state, px(160.0), px(120.0)), None);
        assert_eq!(state.video_size, Some((160, 120)));

        // Both dimensions change: one event.
        assert_eq!(
            apply_video_size(&mut state, px(320.0), px(240.0)),
            Some(Event::VideoSizeChanged(Some((320, 240))))
        );
        assert_eq!(apply_video_size(&mut state, px(320.0), px(240.0)), None);

        // A zero dimension or an unavailable one makes the size unknown.
        assert_eq!(apply_video_size(&mut state, px(0.0), px(240.0)), Some(Event::VideoSizeChanged(None)));
        assert_eq!(apply_video_size(&mut state, PropertyData::None, PropertyData::None), None);
        assert_eq!(state.video_size, None);
    }

    #[test]
    fn idle_and_eof_only_announce_becoming_true() {
        let mut state = state();
        assert_eq!(Property::IdleActive.apply(&mut state, PropertyData::Flag(false)), None);
        assert!(!state.idle);
        assert_eq!(Property::IdleActive.apply(&mut state, PropertyData::Flag(true)), Some(Event::Idle));

        let eof = Property::EofReached.apply(&mut state, PropertyData::Flag(true));
        assert_eq!(eof, Some(Event::Ended(EndReason::Eof)));
        assert_eq!(Property::EofReached.apply(&mut state, PropertyData::None), None);
        assert!(!state.ended);
    }
}
