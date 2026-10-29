//! What every platform's surface renderer shares: the signals between its render thread and the rest of the program,
//! and what it does for media loaded before it existed.

use crate::event;
use crate::player::Player;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

/// The host's notification, called on the render thread after each publication.
pub(crate) type OnFrame = Box<dyn Fn() + Send + Sync + 'static>;

/// What a render thread and its renderer share: the signals waking the thread, and the newest published frame.
pub(crate) struct Shared<F> {
    signals: Mutex<Signals>,
    wake: Condvar,
    latest: Mutex<Option<F>>,
}

#[derive(Default)]
struct Signals {
    /// mpv announced a frame or asked for a redraw.
    mpv_frame: bool,
    /// The size requested by the host, before any rounding by the renderer.
    size: Option<(u32, u32)>,
    /// `size` changed since the last frame.
    resized: bool,
    shutdown: bool,
}

impl<F> Default for Shared<F> {
    fn default() -> Self {
        Shared { signals: Mutex::default(), wake: Condvar::new(), latest: Mutex::new(None) }
    }
}

impl<F: Clone> Shared<F> {
    fn signals(&self) -> MutexGuard<'_, Signals> {
        self.signals.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// From mpv's threads: only sets a flag and wakes the render thread, as mpv requires.
    pub(crate) fn mpv_frame(&self) {
        self.signals().mpv_frame = true;
        self.wake.notify_one();
    }

    pub(crate) fn set_size(&self, width: u32, height: u32) {
        let mut signals = self.signals();
        if signals.size != Some((width, height)) {
            signals.size = Some((width, height));
            signals.resized = true;
            self.wake.notify_one();
        }
    }

    pub(crate) fn shutdown(&self) {
        self.signals().shutdown = true;
        self.wake.notify_one();
    }

    /// Waits for something to render, and returns the size to render at; `None` once shutting down.
    pub(crate) fn wait(&self) -> Option<Option<(u32, u32)>> {
        let mut signals = self.signals();
        while !(signals.mpv_frame || signals.resized || signals.shutdown) {
            signals = self.wake.wait(signals).unwrap_or_else(PoisonError::into_inner);
        }
        if signals.shutdown {
            return None;
        }
        signals.mpv_frame = false;
        signals.resized = false;
        Some(signals.size)
    }

    pub(crate) fn latest(&self) -> Option<F> {
        self.latest.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub(crate) fn publish(&self, frame: F) {
        *self.latest.lock().unwrap_or_else(PoisonError::into_inner) = Some(frame);
    }
}

/// Called once a surface renderer for `player` exists: shows the video of media loaded before it (see
/// [`reselect_video`]), and loads media without audio again if it already ended for want of a renderer.
pub(crate) fn attached(player: &Player) {
    reselect_video(player);
    player.surface_attached();
}

/// Called when a surface renderer goes away. Detaching never asks for a reload.
pub(crate) fn detached(player: &event::Shared) {
    let _ = player.surface_attached(false);
}

/// Media loaded while a player has no render context plays without video: mpv deselects the video track when its
/// video output fails to start, although the `vid` option still asks for one. Selecting the first video track again
/// restarts the video output, now with this render context, while audio and the position carry on.
fn reselect_video(player: &Player) {
    let string = |name: &str| player.get_property::<String>(name).ok();
    if string("vid").as_deref() != Some("no") || string("options/vid").as_deref() == Some("no") {
        return;
    }
    let count = player.get_property::<i64>("track-list/count").unwrap_or(0);
    let video = (0..count).find(|track| string(&format!("track-list/{track}/type")).as_deref() == Some("video"));
    if let Some(id) = video.and_then(|track| player.get_property::<i64>(&format!("track-list/{track}/id")).ok()) {
        let _ = player.set_property("vid", id);
    }
}
