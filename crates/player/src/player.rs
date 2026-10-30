//! The player handle.

use crate::builder::{Autoplay, PlayerBuilder, VideoOutput};
use crate::error::{Error, Result};
use crate::event::{self, Events, Shared, State};
use crate::mpv::{Handle, PropertyValue};
use crate::source::Source;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

/// Where [`Player::seek`] moves the playback position. mpv keeps it within the media's bounds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Seek {
    /// To this position from the start.
    To(Duration),
    /// By this many seconds from the current position: forwards if positive, backwards if negative.
    By(f64),
}

/// A handle to one mpv instance: the only type needed to drive playback, from any frontend and any thread.
///
/// Clones are cheap and control the same instance, which shuts down when the last clone is dropped.
///
/// Control methods return as soon as mpv has accepted the request; their effect is reported through
/// [`subscribe`](Self::subscribe) and [`state`](Self::state). Called while no media is loaded, they have no effect
/// and return `Ok`.
#[derive(Clone)]
pub struct Player {
    inner: Arc<Inner>,
}

struct Inner {
    mpv: Arc<Handle>,
    shared: Arc<Shared>,
    autoplay: Autoplay,
    video_output: VideoOutput,
    /// Set while a render context exists. Shared with it rather than kept here alone, so a render context that
    /// outlives the player does not keep the event thread and subscribers alive.
    render_active: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        // Stop playback even if a render context keeps the instance alive past this drop. Asynchronous, so the drop
        // never waits on mpv's core, which may be waiting on the render thread this drop runs on.
        let _ = self.mpv.command_async(0, &["stop"]);

        // Stop and join the event thread first, so no other thread is inside an mpv call when the last `Arc<Handle>`
        // (this one, unless a render context holds another) is dropped right after and destroys the instance.
        self.shared.stop.store(true, Ordering::Release);
        self.mpv.wakeup();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl fmt::Debug for Player {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Player").field("state", &self.state()).finish_non_exhaustive()
    }
}

impl Player {
    /// Starts building a player.
    pub fn builder() -> PlayerBuilder {
        PlayerBuilder::default()
    }

    /// Wraps an initialised instance: observes its properties and starts the event thread.
    pub(crate) fn start(mpv: Handle, state: State, autoplay: Autoplay, video_output: VideoOutput) -> Result<Self> {
        event::observe(&mpv, video_output == VideoOutput::Render).map_err(Error::Init)?;

        let mpv = Arc::new(mpv);
        let shared = Arc::new(Shared::new(state));
        let thread = {
            let (mpv, shared) = (Arc::clone(&mpv), Arc::clone(&shared));
            std::thread::Builder::new()
                .name("player-events".into())
                .spawn(move || event::run(&mpv, &shared, autoplay))
                .map_err(|e| Error::Init(format!("could not start the event thread: {e}")))?
        };

        let render_active = Arc::new(AtomicBool::new(false));
        let inner = Inner { mpv, shared, autoplay, video_output, render_active, thread: Some(thread) };
        Ok(Self { inner: Arc::new(inner) })
    }

    /// Whether `a` and `b` are copies of the same player, as [`Arc::ptr_eq`] tells for `Arc`s.
    pub fn ptr_eq(a: &Player, b: &Player) -> bool {
        Arc::ptr_eq(&a.inner, &b.inner)
    }

    fn mpv(&self) -> &Handle {
        &self.inner.mpv
    }

    /// The shared mpv handle, for a render context to keep the instance alive.
    pub(crate) fn handle(&self) -> &Arc<Handle> {
        &self.inner.mpv
    }

    pub(crate) fn video_output(&self) -> VideoOutput {
        self.inner.video_output
    }

    /// Set while the player has a render context.
    pub(crate) fn render_active(&self) -> &Arc<AtomicBool> {
        &self.inner.render_active
    }

    /// The state shared with the event thread, which a surface renderer tells of its existence.
    #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
    pub(crate) fn shared(&self) -> &Arc<Shared> {
        &self.inner.shared
    }

    /// Records that a surface renderer now exists, and loads the media again if it ended for want of one: media
    /// without audio ends as soon as its video output fails to start.
    #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
    pub(crate) fn surface_attached(&self) {
        if let Some(source) = self.inner.shared.surface_attached(true) {
            let _ = event::load(self.mpv(), self.inner.autoplay, &source);
        }
    }

    /// Loads `source` in place of any current media, then plays it or not according to the [`Autoplay`] mode.
    ///
    /// Returns once mpv has accepted the request, without waiting for network or disk. The outcome arrives as
    /// [`Event::Loaded`](crate::Event::Loaded) or [`Event::Ended`](crate::Event::Ended) with
    /// [`EndReason::Error`](crate::EndReason::Error); media this replaces ends with
    /// [`EndReason::Replaced`](crate::EndReason::Replaced).
    ///
    /// # Errors
    ///
    /// Returns [`Error::Property`] or [`Error::Command`] if mpv rejects the request.
    pub fn load(&self, source: Source) -> Result<()> {
        self.inner.shared.loading(&source, self.inner.render_active.load(Ordering::Acquire));
        event::load(self.mpv(), self.inner.autoplay, &source)
    }

    /// Starts or resumes playback. Media that has played to its end restarts from the beginning, as in Video.js.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Property`] or [`Error::Command`] if mpv rejects the request.
    pub fn play(&self) -> Result<()> {
        if self.mpv().get::<bool>("eof-reached").unwrap_or(false) {
            self.unless_idle(self.mpv().command(&["seek", "0", "absolute+exact"]))?;
        }
        self.mpv().set("pause", false)
    }

    /// Pauses playback.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Property`] if mpv rejects the request.
    pub fn pause(&self) -> Result<()> {
        self.mpv().set("pause", true)
    }

    /// Calls [`play`](Self::play) if paused, [`pause`](Self::pause) otherwise.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Property`] or [`Error::Command`] if mpv rejects the request.
    pub fn toggle_pause(&self) -> Result<()> {
        if self.mpv().get::<bool>("pause")? { self.play() } else { self.pause() }
    }

    /// Stops playback and unloads the media, which ends with [`EndReason::Stopped`](crate::EndReason::Stopped).
    ///
    /// # Errors
    ///
    /// Returns [`Error::Command`] if mpv rejects the request.
    pub fn stop(&self) -> Result<()> {
        self.mpv().command(&["stop"])
    }

    /// Moves the playback position, to the exact position requested (not the nearest keyframe).
    ///
    /// # Errors
    ///
    /// Returns [`Error::Command`] if mpv rejects the request while media is loaded, for example for a non-finite
    /// offset.
    pub fn seek(&self, seek: Seek) -> Result<()> {
        let (seconds, flags) = match seek {
            Seek::To(position) => (position.as_secs_f64(), "absolute+exact"),
            Seek::By(offset) => (offset, "relative+exact"),
        };
        self.unless_idle(self.mpv().command(&["seek", &seconds.to_string(), flags]))
    }

    /// Moves the playback position quickly to `position`, possibly landing on the nearest keyframe instead, for
    /// scrubbing: mpv executes only the latest of several pending seeks, so rapid calls do not queue up. Follow a
    /// scrub with an exact [`seek`](Self::seek) where the precise position matters.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Command`] if mpv rejects the request while media is loaded.
    pub fn scrub(&self, position: Duration) -> Result<()> {
        let seconds = position.as_secs_f64().to_string();
        self.unless_idle(self.mpv().command(&["seek", &seconds, "absolute+keyframes"]))
    }

    /// Sets the volume, clamped to `0.0..=1.0`. NaN is ignored.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Property`] if mpv rejects the request.
    pub fn set_volume(&self, volume: f64) -> Result<()> {
        if volume.is_nan() {
            return Ok(());
        }
        self.mpv().set("volume", volume.clamp(0.0, 1.0) * 100.0)
    }

    /// Mutes or unmutes the audio, keeping the volume.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Property`] if mpv rejects the request.
    pub fn set_muted(&self, muted: bool) -> Result<()> {
        self.mpv().set("mute", muted)
    }

    /// Sets the playback rate, clamped to `0.01..=100.0` (mpv's `speed` range). NaN is ignored.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Property`] if mpv rejects the request.
    pub fn set_rate(&self, rate: f64) -> Result<()> {
        if rate.is_nan() {
            return Ok(());
        }
        self.mpv().set("speed", rate.clamp(0.01, 100.0))
    }

    /// A snapshot of the current state. Cheap and never blocks on mpv, so it suits render paths.
    pub fn state(&self) -> State {
        self.inner.shared.state()
    }

    /// Subscribes to the events emitted from now on.
    ///
    /// Each subscription queues events without limit, and position changes arrive many times per second, so drop the
    /// [`Events`] once it is no longer read.
    pub fn subscribe(&self) -> Events {
        self.inner.shared.subscribe()
    }

    /// Reads any mpv property (see <https://mpv.io/manual/stable/#properties>) as a `bool`, `i64`, `f64` or
    /// `String`.
    ///
    /// This waits briefly for mpv's core; render paths should read [`state`](Self::state) instead.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Property`], with mpv's message, if the property does not exist, is unavailable right now, or
    /// cannot be read as `T`.
    pub fn get_property<T: PropertyValue>(&self, name: &str) -> Result<T> {
        self.mpv().get(name)
    }

    /// Writes any mpv property as a `bool`, `i64`, `f64` or `String`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Property`], with mpv's message, if the property does not exist or rejects the value.
    pub fn set_property<T: PropertyValue>(&self, name: &str, value: T) -> Result<()> {
        self.mpv().set(name, value)
    }

    /// Runs any mpv command (see <https://mpv.io/manual/stable/#list-of-input-commands>), given as its name followed
    /// by its arguments, and waits for it to finish.
    ///
    /// Loading media this way bypasses the player's bookkeeping, so media it replaces may be reported as
    /// [`EndReason::Stopped`](crate::EndReason::Stopped); prefer [`load`](Self::load).
    ///
    /// # Errors
    ///
    /// Returns [`Error::Command`], with mpv's message, if the command fails.
    pub fn command(&self, args: &[&str]) -> Result<()> {
        self.mpv().command(args)
    }

    /// Turns the error of a command that needs media into `Ok` when no media is loaded, including when the media was
    /// unloaded between the call and the command.
    fn unless_idle(&self, result: Result<()>) -> Result<()> {
        match result {
            Err(_) if self.mpv().get::<bool>("idle-active").unwrap_or(false) => Ok(()),
            result => result,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player() -> Player {
        Player::builder().mpv_option("ao", "null").build().unwrap()
    }

    #[test]
    fn runtime_values_are_clamped() {
        let player = player();
        player.set_volume(1.3).unwrap();
        assert_eq!(player.get_property::<f64>("volume").unwrap(), 100.0);
        player.set_volume(-2.0).unwrap();
        assert_eq!(player.get_property::<f64>("volume").unwrap(), 0.0);

        player.set_rate(0.0).unwrap();
        assert_eq!(player.get_property::<f64>("speed").unwrap(), 0.01);
        player.set_rate(1000.0).unwrap();
        assert_eq!(player.get_property::<f64>("speed").unwrap(), 100.0);
    }

    #[test]
    fn mute_keeps_the_volume() {
        let player = player();
        player.set_volume(0.6).unwrap();
        player.set_muted(true).unwrap();
        assert!(player.get_property::<bool>("mute").unwrap());
        assert_eq!(player.get_property::<f64>("volume").unwrap(), 60.0);
        player.set_muted(false).unwrap();
        assert!(!player.get_property::<bool>("mute").unwrap());
        assert_eq!(player.get_property::<f64>("volume").unwrap(), 60.0);
    }

    #[test]
    fn controls_without_media_do_nothing() {
        let player = player();
        player.seek(Seek::To(Duration::from_secs(1))).unwrap();
        player.seek(Seek::By(-5.0)).unwrap();
        player.scrub(Duration::from_secs(1)).unwrap();
        player.play().unwrap();
        player.pause().unwrap();
        player.toggle_pause().unwrap();
        player.stop().unwrap();
        assert!(player.get_property::<bool>("idle-active").unwrap());
    }

    #[test]
    fn handle_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Player>();
    }
}
