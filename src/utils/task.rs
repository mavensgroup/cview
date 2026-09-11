// src/utils/task.rs
//
// Running analysis off the GTK main thread.
//
// Everything in `physics::analysis` is a pure function of a Structure, and
// the heavy ones (void distance field, interstitial site search) take
// seconds on a supercell. Called directly from a widget handler they block
// the main loop, so the window stops redrawing and the compositor marks it
// unresponsive — the computation is parallel internally, but the UI still
// freezes for its whole duration.
//
// `spawn` moves the work to a worker thread and delivers the result back on
// the main loop, where it is safe to touch widgets. A job is cancellable and
// is cancelled automatically when its handle is dropped, so a slider that
// fires ten times in a second leaves one live computation rather than ten.

use gtk4::glib;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

/// Shared "stop what you are doing" flag, polled by long-running work.
///
/// Cloning shares the flag; cancelling any clone cancels them all.
#[derive(Clone, Debug)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
}

impl CancelToken {
    pub fn new() -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// A token that is never tripped, for callers running work synchronously.
    pub fn never() -> Self {
        Self::new()
    }

    pub fn cancel(&self) {
        self.flag.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::Relaxed)
    }
}

impl Default for CancelToken {
    fn default() -> Self {
        Self::new()
    }
}

/// A running background job.
///
/// Dropping the handle cancels the job, which is what makes "recompute on
/// every control change" safe: hold one handle per panel, and starting a new
/// job replaces — and so cancels — the previous one.
pub struct JobHandle {
    cancel: CancelToken,
}

impl JobHandle {
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }
}

impl Drop for JobHandle {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// Run `work` on a worker thread; call `on_done` with its result on the main
/// thread once it finishes.
///
/// `work` receives a [`CancelToken`] and should poll it periodically, returning
/// `None` if it gives up. `on_done` is skipped entirely for a cancelled job,
/// so callers never have to defend against a stale result overwriting a fresh
/// one.
///
/// Delivery is by polling a channel from a main-loop timeout rather than an
/// async channel: it keeps the dependency set unchanged, and the latency it
/// costs is invisible next to the multi-second jobs this exists for.
pub fn spawn<T, F, G>(work: F, on_done: G) -> JobHandle
where
    T: Send + 'static,
    F: FnOnce(CancelToken) -> Option<T> + Send + 'static,
    G: FnOnce(T) + 'static,
{
    let cancel = CancelToken::new();
    let worker_token = cancel.clone();
    let ui_token = cancel.clone();

    let (tx, rx) = mpsc::channel::<T>();

    std::thread::spawn(move || {
        if let Some(result) = work(worker_token) {
            // A send error just means the receiver is gone — the job was
            // cancelled and nobody is waiting. Nothing to report.
            let _ = tx.send(result);
        }
    });

    // `on_done` is FnOnce, so it has to be moved out of the closure when it
    // fires; Option::take is how a repeating source consumes it exactly once.
    let mut on_done = Some(on_done);

    glib::timeout_add_local(Duration::from_millis(30), move || {
        if ui_token.is_cancelled() {
            return glib::ControlFlow::Break;
        }

        match rx.try_recv() {
            Ok(result) => {
                if let Some(f) = on_done.take() {
                    f(result);
                }
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            // Worker finished without sending (cancelled) or panicked.
            Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        }
    });

    JobHandle { cancel }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropping_the_handle_cancels_the_job() {
        let handle = JobHandle {
            cancel: CancelToken::new(),
        };
        let observer = handle.cancel.clone();

        assert!(!observer.is_cancelled());
        drop(handle);
        assert!(observer.is_cancelled());
    }

    #[test]
    fn cancellation_is_shared_between_clones() {
        let a = CancelToken::new();
        let b = a.clone();

        assert!(!b.is_cancelled());
        a.cancel();
        assert!(b.is_cancelled());
    }
}
