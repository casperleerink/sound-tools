//! Background work for an owner that runs its own loop, such as a window. Work runs on smol's
//! threads and hands what it found to the owner's [`Inbox`]; the [`Waker`] the window passed in
//! says there is something to take, and the owner takes it in its `poll` on the next frame.
//! A send comes before its wake, so a poll after a wake always finds it.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use smol::channel::{self, Receiver, Sender};
use smol::future;

/// Called from any thread once background work has something for the owner: the window asks
/// for a frame, whose `poll` takes it.
#[derive(Clone)]
pub struct Waker(Arc<dyn Fn() + Send + Sync>);

impl Waker {
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self(Arc::new(wake))
    }

    pub(crate) fn wake(&self) {
        (self.0)();
    }
}

/// What background work handed the owner, in the order it came.
pub(crate) struct Inbox<T> {
    sender: Sender<T>,
    receiver: Receiver<T>,
    waker: Waker,
}

impl<T> Inbox<T> {
    pub(crate) fn new(waker: Waker) -> Self {
        let (sender, receiver) = channel::unbounded();
        Self {
            sender,
            receiver,
            waker,
        }
    }

    pub(crate) fn outbox(&self) -> Outbox<T> {
        Outbox {
            sender: self.sender.clone(),
            waker: self.waker.clone(),
        }
    }

    /// The next thing handed in, without waiting.
    pub(crate) fn take(&self) -> Option<T> {
        self.receiver.try_recv().ok()
    }

    /// For something the owner did itself that the window must take on its next frame.
    pub(crate) fn wake(&self) {
        self.waker.wake();
    }
}

/// Hands things to an [`Inbox`] from the background.
pub(crate) struct Outbox<T> {
    sender: Sender<T>,
    waker: Waker,
}

impl<T> Clone for Outbox<T> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            waker: self.waker.clone(),
        }
    }
}

impl<T> Outbox<T> {
    /// Once the owner is gone nobody needs it, so a closed inbox is no error.
    pub(crate) fn send(&self, value: T) {
        if self.sender.try_send(value).is_ok() {
            self.waker.wake();
        }
    }
}

/// Work in the background. Dropping it cancels the work, and the processes it started end.
pub(crate) type Job = smol::Task<()>;

pub(crate) fn spawn(work: impl Future<Output = ()> + Send + 'static) -> Job {
    smol::spawn(work)
}

/// What `work` gives, or `None` once `limit` passed. The one that loses is dropped, which ends
/// a process it ran.
pub(crate) async fn within<T>(limit: Duration, work: impl Future<Output = T>) -> Option<T> {
    future::or(async { Some(work.await) }, async {
        sleep(limit).await;
        None
    })
    .await
}

pub(crate) async fn sleep(duration: Duration) {
    smol::Timer::after(duration).await;
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// A waker a test waits on.
    pub(crate) fn waker() -> (Waker, Receiver<()>) {
        let (sender, woken) = channel::unbounded();
        let waker = Waker::new(move || {
            sender.try_send(()).ok();
        });
        (waker, woken)
    }

    /// Waits for the next wake. The limit only ends a test that would hang.
    pub(crate) fn wait(woken: &Receiver<()>) {
        let woke = smol::block_on(within(Duration::from_secs(30), woken.recv()));
        assert!(matches!(woke, Some(Ok(()))), "nothing came in 30 s");
    }
}
