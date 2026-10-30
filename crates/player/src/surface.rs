//! What every platform's surface renderer shares: its render thread's start, loop and stop, the signals between that
//! thread and the rest of the program, and what it does for media loaded before it existed.

use crate::error::{Error, Result};
use crate::event;
use crate::player::Player;
use crate::render::RenderContext;
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;

/// The host's notification, called on the render thread after each publication.
pub(crate) type OnFrame = Box<dyn Fn() + Send + Sync + 'static>;

/// Frames in a renderer's ring: a published frame is overwritten only after two newer ones are published.
pub(crate) const RING: usize = 3;

/// What a render thread runs.
pub(crate) type Job = Box<dyn FnOnce() + Send + 'static>;

/// A render thread, as [`start`] runs it.
pub(crate) trait Thread {
    /// Waits for the thread's job to finish.
    fn join(self);
}

impl Thread for JoinHandle<()> {
    fn join(self) {
        let _ = JoinHandle::join(self);
    }
}

/// Spawns a render thread.
pub(crate) fn spawn_thread(f: impl FnOnce() + Send + 'static) -> Result<JoinHandle<()>> {
    std::thread::Builder::new()
        .name("player-surface".into())
        .spawn(f)
        .map_err(|error| Error::Render(format!("could not start the render thread: {error}")))
}

/// The platform's part of a render thread: its GL context and ring of [`RING`] frames. It lives on the render thread,
/// where its context is current, and is dropped after mpv's render context.
pub(crate) trait Backend {
    type Frame;

    /// The size of the frames rendered for the size the host asked for.
    fn frame_size(&self, width: u32, height: u32) -> (u32, u32) {
        (width, height)
    }

    /// Allocates the ring, and anything else sized, at the new size, after [`release`](Self::release).
    fn rebuild(&mut self, width: u32, height: u32) -> Result<()>;

    /// Renders the current picture into the ring's frame `slot`, waits for the GPU to finish it and returns it as the
    /// frame numbered `sequence`.
    fn draw(&mut self, render: &mut RenderContext, slot: usize, size: (u32, u32), sequence: u64)
    -> Result<Self::Frame>;

    /// Deletes the ring and anything else sized. Frames the host holds keep their own memory alive.
    fn release(&mut self);
}

/// A render thread's state: renders each frame into the next of the backend's ring, publishes it and notifies.
struct Driver<B: Backend> {
    /// Dropped first, while the backend's context is current, as mpv requires.
    render: Option<RenderContext>,
    backend: B,
    /// The size of the frames in the ring.
    size: Option<(u32, u32)>,
    next: usize,
    sequence: u64,
    shared: Arc<Shared<B::Frame>>,
    on_frame: OnFrame,
}

impl<B: Backend> Driver<B> {
    fn run(mut self) {
        while let Some(size) = self.shared.wait() {
            let render = self.render.as_mut().expect("the render context lives until the drop");
            // Clears mpv's update flag. A redraw request is rendered like a new frame.
            render.update();
            if let Some((width, height)) = size {
                // A frame that cannot be drawn is skipped; the next signal tries again.
                let _ = self.draw(self.backend.frame_size(width, height));
            }
        }
    }

    fn draw(&mut self, size: (u32, u32)) -> Result<()> {
        if self.size != Some(size) {
            self.release();
            self.backend.rebuild(size.0, size.1)?;
            self.size = Some(size);
        }
        let slot = self.next;
        self.next = (self.next + 1) % RING;

        let render = self.render.as_mut().expect("the render context lives until the drop");
        let frame = self.backend.draw(render, slot, size, self.sequence + 1)?;
        self.sequence += 1;
        self.shared.publish(frame);
        render.report_swap();
        (self.on_frame)();
        Ok(())
    }

    fn release(&mut self) {
        self.size = None;
        self.next = 0;
        self.backend.release();
    }
}

impl<B: Backend> Drop for Driver<B> {
    fn drop(&mut self) {
        // mpv's render context first, with the GL context current, as mpv requires; then the ring. The backend's own
        // state follows, with the fields.
        drop(self.render.take());
        self.release();
    }
}

/// Starts a render thread for `player` with `spawn`, and waits for it to be ready, so that its errors are returned
/// here. `build` creates the backend and mpv's render context on the thread, which wakes the thread through the
/// [`Shared`] it is given, and returns what the caller is told about them. The player is only used to create the
/// render context, then dropped: a renderer must not keep the player from stopping.
pub(crate) fn start<B, T, J>(
    player: &Player,
    spawn: impl FnOnce(Job) -> Result<J>,
    on_frame: OnFrame,
    build: impl FnOnce(&Player, &Arc<Shared<B::Frame>>) -> Result<(B, RenderContext, T)> + Send + 'static,
) -> Result<(Attachment<B::Frame, J>, T)>
where
    B: Backend,
    B::Frame: Send + 'static,
    T: Send + 'static,
    J: Thread,
{
    let shared = Arc::new(Shared::default());
    let (ready, initialised) = mpsc::sync_channel(1);
    let thread_player = player.clone();
    let thread_shared = Arc::clone(&shared);
    let thread = spawn(Box::new(move || {
        let built = build(&thread_player, &thread_shared);
        drop(thread_player);
        match built {
            Ok((backend, render, info)) => {
                let _ = ready.send(Ok(info));
                Driver {
                    render: Some(render),
                    backend,
                    size: None,
                    next: 0,
                    sequence: 0,
                    shared: thread_shared,
                    on_frame,
                }
                .run();
            }
            Err(error) => {
                let _ = ready.send(Err(error));
            }
        }
    }))?;

    match initialised.recv() {
        Ok(Ok(info)) => Ok((Attachment { shared, thread: Some(thread), player: Arc::clone(player.shared()) }, info)),
        Ok(Err(error)) => {
            thread.join();
            Err(error)
        }
        Err(_) => {
            thread.join();
            Err(Error::Render("the render thread stopped while starting".into()))
        }
    }
}

/// A started render thread, as a surface renderer holds it. Dropping it detaches the renderer from its player and
/// stops the thread.
pub(crate) struct Attachment<F, J: Thread> {
    shared: Arc<Shared<F>>,
    thread: Option<J>,
    /// The player's state shared with its event thread, told when the renderer goes away. Holding it does not keep
    /// the player alive.
    player: Arc<event::Shared>,
}

impl<F: Clone, J: Thread> Attachment<F, J> {
    pub(crate) fn set_size(&self, width: u32, height: u32) {
        self.shared.set_size(width, height);
    }

    pub(crate) fn latest(&self) -> Option<F> {
        self.shared.latest()
    }
}

impl<F, J: Thread> Drop for Attachment<F, J> {
    fn drop(&mut self) {
        detached(&self.player);
        self.shared.shutdown();
        if let Some(thread) = self.thread.take() {
            thread.join();
        }
    }
}

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

impl<F> Shared<F> {
    fn signals(&self) -> MutexGuard<'_, Signals> {
        self.signals.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// From mpv's threads: only sets a flag and wakes the render thread, as mpv requires.
    pub(crate) fn mpv_frame(&self) {
        self.signals().mpv_frame = true;
        self.wake.notify_one();
    }

    fn set_size(&self, width: u32, height: u32) {
        let mut signals = self.signals();
        if signals.size != Some((width, height)) {
            signals.size = Some((width, height));
            signals.resized = true;
            self.wake.notify_one();
        }
    }

    fn shutdown(&self) {
        self.signals().shutdown = true;
        self.wake.notify_one();
    }

    /// Waits for something to render, and returns the size to render at, if it has no zero dimension; `None` once
    /// shutting down.
    fn wait(&self) -> Option<Option<(u32, u32)>> {
        let mut signals = self.signals();
        while !(signals.mpv_frame || signals.resized || signals.shutdown) {
            signals = self.wake.wait(signals).unwrap_or_else(PoisonError::into_inner);
        }
        if signals.shutdown {
            return None;
        }
        signals.mpv_frame = false;
        signals.resized = false;
        Some(signals.size.filter(|&(width, height)| width > 0 && height > 0))
    }

    fn publish(&self, frame: F) {
        *self.latest.lock().unwrap_or_else(PoisonError::into_inner) = Some(frame);
    }
}

impl<F: Clone> Shared<F> {
    fn latest(&self) -> Option<F> {
        self.latest.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

/// Called once a surface renderer for `player` exists: shows the video of media loaded before it (see
/// [`reselect_video`]), and loads media without audio again if it already ended for want of a renderer.
pub(crate) fn attached(player: &Player) {
    reselect_video(player);
    player.surface_attached();
}

/// Called when a surface renderer goes away. Detaching never asks for a reload.
fn detached(player: &event::Shared) {
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
