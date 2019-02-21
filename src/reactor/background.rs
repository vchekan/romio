use super::{Handle, Reactor};

use futures::task::{AtomicWaker, Waker};
use futures::{executor, Future, Poll};
use log::debug;

use std::io;
use std::pin::Pin;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::Arc;
use std::thread;

/// Handle to the reactor running on a background thread.
///
/// Instances are created by calling [`Reactor::background`].
///
/// [`Reactor::background`]: struct.Reactor.html#method.background
#[derive(Debug)]
pub struct Background {
    /// When `None`, the reactor thread will run until the process terminates.
    inner: Option<Inner>,
}

/// Future that resolves when the reactor thread has shutdown.
#[derive(Debug)]
pub struct Shutdown {
    inner: Inner,
}

/// Actual Background handle.
#[derive(Debug)]
struct Inner {
    /// Handle to the reactor
    handle: Handle,

    /// Shared state between the background handle and the reactor thread.
    shared: Arc<Shared>,
}

#[derive(Debug)]
struct Shared {
    /// Signal the reactor thread to shutdown.
    shutdown: AtomicUsize,

    /// Task to notify when the reactor thread enters a shutdown state.
    shutdown_task: AtomicWaker,
}

/// Notifies the reactor thread to shutdown once the reactor becomes idle.
const SHUTDOWN_IDLE: usize = 1;

/// Notifies the reactor thread to shutdown immediately.
const SHUTDOWN_NOW: usize = 2;

/// The reactor is currently shutdown.
const SHUTDOWN: usize = 3;

// ===== impl Background =====

impl Background {
    /// Launch a reactor in the background and return a handle to the thread.
    pub(super) fn new(reactor: Reactor) -> io::Result<Background> {
        // Grab a handle to the reactor
        let handle = reactor.handle().clone();

        // Create the state shared between the background handle and the reactor
        // thread.
        let shared = Arc::new(Shared {
            shutdown: AtomicUsize::new(0),
            shutdown_task: AtomicWaker::new(),
        });

        // For the reactor thread
        let shared2 = shared.clone();

        // Start the reactor thread
        thread::Builder::new().spawn(move || run(reactor, shared2))?;

        Ok(Background {
            inner: Some(Inner { handle, shared }),
        })
    }

    /// Run the reactor on its thread until the process terminates.
    pub fn forget(mut self) {
        drop(self.inner.take());
    }
}

impl Drop for Background {
    fn drop(&mut self) {
        let inner = match self.inner.take() {
            Some(i) => i,
            None => return,
        };

        inner.shutdown_now();

        let shutdown = Shutdown { inner };
        let _ = executor::block_on(shutdown);
    }
}

// ===== impl Shutdown =====

impl Future for Shutdown {
    type Output = Result<(), ()>;

    fn poll(self: Pin<&mut Self>, lw: &Waker) -> Poll<Self::Output> {
        self.inner.shared.shutdown_task.register(lw);

        if !self.inner.is_shutdown() {
            return Poll::Pending;
        }

        Poll::Ready(Ok(()))
    }
}

// ===== impl Inner =====

impl Inner {
    /// Returns true if the reactor thread is shutdown.
    fn is_shutdown(&self) -> bool {
        self.shared.shutdown.load(SeqCst) == SHUTDOWN
    }

    /// Notify the reactor thread to shutdown immediately.
    fn shutdown_now(&self) {
        let mut curr = self.shared.shutdown.load(SeqCst);

        loop {
            if curr >= SHUTDOWN_NOW {
                return;
            }

            let act = self
                .shared
                .shutdown
                .compare_and_swap(curr, SHUTDOWN_NOW, SeqCst);

            if act == curr {
                self.handle.wakeup();
                return;
            }

            curr = act;
        }
    }
}

// ===== impl Reactor thread =====

fn run(mut reactor: Reactor, shared: Arc<Shared>) {
    debug!("starting background reactor");
    loop {
        let shutdown = shared.shutdown.load(SeqCst);

        if shutdown == SHUTDOWN_NOW {
            debug!("shutting background reactor down NOW");
            break;
        }

        if shutdown == SHUTDOWN_IDLE && reactor.is_idle() {
            debug!("shutting background reactor on idle");
            break;
        }

        reactor.turn(None).unwrap();
    }

    drop(reactor);

    // Transition the state to shutdown
    shared.shutdown.store(SHUTDOWN, SeqCst);

    // Notify any waiters
    shared.shutdown_task.wake();

    debug!("background reactor has shutdown");
}
