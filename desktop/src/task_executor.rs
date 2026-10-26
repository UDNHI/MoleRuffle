//! Local futures stay owned by the UI thread. Background wakers send only a
//! notification, never a runnable that could drop a !Send future off-thread
//! when the window event loop has already closed.
use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures::future::LocalBoxFuture;
use futures::stream::{FuturesUnordered, StreamExt};
use futures::task::{ArcWake, waker};

struct Notify(Arc<dyn Fn() + Send + Sync>);
impl ArcWake for Notify {
    fn wake_by_ref(this: &Arc<Self>) {
        (this.0)();
    }
}

#[derive(Clone)]
pub struct LocalSpawner {
    pending: Rc<RefCell<Vec<LocalBoxFuture<'static, ()>>>>,
    closed: Rc<Cell<bool>>,
    notify: Arc<Notify>,
}

impl LocalSpawner {
    pub fn spawn(&self, future: impl Future<Output = ()> + 'static) {
        if self.closed.get() {
            return;
        }
        self.pending.borrow_mut().push(Box::pin(future));
        (self.notify.0)();
    }
}

pub struct LocalExecutor {
    spawner: LocalSpawner,
    active: FuturesUnordered<LocalBoxFuture<'static, ()>>,
}

impl LocalExecutor {
    pub fn new(notify: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            spawner: LocalSpawner {
                pending: Rc::new(RefCell::new(Vec::new())),
                closed: Rc::new(Cell::new(false)),
                notify: Arc::new(Notify(Arc::new(notify))),
            },
            active: FuturesUnordered::new(),
        }
    }

    pub fn spawner(&self) -> LocalSpawner {
        self.spawner.clone()
    }

    pub fn poll(&mut self) {
        if self.spawner.closed.get() {
            return;
        }
        let wake = waker(self.spawner.notify.clone());
        let mut context = Context::from_waker(&wake);
        // Keep GUI input responsive when a chain of cached loads completes.
        for _ in 0..64 {
            let incoming = std::mem::take(&mut *self.spawner.pending.borrow_mut());
            self.active.extend(incoming);
            match self.active.poll_next_unpin(&mut context) {
                Poll::Ready(Some(())) => {}
                Poll::Ready(None) | Poll::Pending => return,
            }
        }
        (self.spawner.notify.0)();
    }

    pub fn shutdown(&mut self) {
        self.spawner.closed.set(true);
        self.spawner.pending.borrow_mut().clear();
        // FuturesUnordered drops the futures here, on their owning thread.
        // Outstanding background wakers retain no live future after this.
        self.active = FuturesUnordered::new();
    }
}

impl Drop for LocalExecutor {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::pin::Pin;
    use std::sync::{Barrier, mpsc};
    use std::task::Waker;

    struct Waiting {
        owner: std::thread::ThreadId,
        dropped: mpsc::Sender<std::thread::ThreadId>,
        wake: Option<mpsc::Sender<Waker>>,
        // Makes the future genuinely !Send, like Ruffle's local futures.
        _local: Rc<()>,
    }
    impl Future for Waiting {
        type Output = ();
        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
            if let Some(tx) = self.wake.take() {
                tx.send(cx.waker().clone()).unwrap();
            }
            Poll::Pending
        }
    }
    impl Drop for Waiting {
        fn drop(&mut self) {
            let thread = std::thread::current().id();
            assert_eq!(self.owner, thread);
            self.dropped.send(thread).unwrap();
        }
    }

    #[test]
    fn background_wake_racing_with_shutdown_keeps_drop_on_owner() {
        for _ in 0..100 {
            let owner = std::thread::current().id();
            let (events, receiver) = mpsc::channel();
            let mut executor = LocalExecutor::new(move || {
                let _ = events.send(());
            });
            let (dropped, drop_rx) = mpsc::channel();
            let (wake, wake_rx) = mpsc::channel();
            executor.spawner().spawn(Waiting {
                owner,
                dropped,
                wake: Some(wake),
                _local: Rc::new(()),
            });
            executor.poll();
            let waker = wake_rx.recv().unwrap();
            let barrier = Arc::new(Barrier::new(2));
            let ready = barrier.clone();
            let worker = std::thread::spawn(move || {
                ready.wait();
                for _ in 0..16 {
                    waker.wake_by_ref();
                }
            });
            drop(receiver); // event loop has already closed
            barrier.wait();
            executor.shutdown();
            worker.join().unwrap();
            assert_eq!(drop_rx.recv().unwrap(), owner);
        }
    }

    #[test]
    fn late_wake_after_executor_is_dropped_is_safe() {
        let (dropped, drop_rx) = mpsc::channel();
        let (wake, wake_rx) = mpsc::channel();
        let owner = std::thread::current().id();
        let mut executor = LocalExecutor::new(|| {});
        executor.spawner().spawn(Waiting {
            owner,
            dropped,
            wake: Some(wake),
            _local: Rc::new(()),
        });
        executor.poll();
        let waker = wake_rx.recv().unwrap();
        drop(executor);
        std::thread::spawn(move || waker.wake()).join().unwrap();
        assert_eq!(drop_rx.recv().unwrap(), owner);
    }

    #[test]
    fn tokio_shutdown_with_closed_event_loop_does_not_drop_local_future() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .build()
            .unwrap();
        let (sender, response) = tokio::sync::oneshot::channel::<()>();
        runtime.spawn(async move {
            let _sender = sender;
            futures::future::pending::<()>().await;
        });
        let (events, event_loop) = mpsc::channel();
        let mut executor = LocalExecutor::new(move || {
            let _ = events.send(());
        });
        let owner = std::thread::current().id();
        let (dropped, drop_rx) = mpsc::channel();
        let guard = Waiting {
            owner,
            dropped,
            wake: None,
            _local: Rc::new(()),
        };
        executor.spawner().spawn(async move {
            let _guard = guard;
            let _ = response.await;
        });
        executor.poll();
        drop(event_loop);
        // Mirrors the recorded crash: cancelling Tokio work drops a response
        // sender on a worker after the GUI event loop has already disappeared.
        drop(runtime);
        assert!(drop_rx.try_recv().is_err()); // local future still belongs to UI
        executor.shutdown();
        assert_eq!(drop_rx.recv().unwrap(), owner);
    }

    #[test]
    fn background_response_wakes_and_completes_local_future() {
        let (events, event_loop) = mpsc::channel();
        let mut executor = LocalExecutor::new(move || {
            let _ = events.send(());
        });
        let (sender, response) = tokio::sync::oneshot::channel();
        let result = Rc::new(Cell::new(0));
        let completed = result.clone();
        executor.spawner().spawn(async move {
            completed.set(response.await.unwrap());
        });
        executor.poll();
        while event_loop.try_recv().is_ok() {}
        std::thread::spawn(move || sender.send(42).unwrap())
            .join()
            .unwrap();
        event_loop
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        executor.poll();
        assert_eq!(result.get(), 42);
    }

    #[test]
    fn polling_can_spawn_more_tasks_without_borrow_conflicts() {
        let mut executor = LocalExecutor::new(|| {});
        let spawner = executor.spawner();
        let ran = Rc::new(Cell::new(false));
        let result = ran.clone();
        executor.spawner().spawn(async move {
            spawner.spawn(async move {
                result.set(true);
            });
        });
        executor.poll();
        assert!(ran.get());
        executor.shutdown();
        executor
            .spawner()
            .spawn(async { panic!("must not run after shutdown") });
        executor.poll();
    }
}
