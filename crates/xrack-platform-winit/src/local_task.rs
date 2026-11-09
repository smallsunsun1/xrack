use crate::queue::EventQueue;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    fmt,
    future::Future,
    pin::Pin,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
};

type TaskId = u64;

thread_local! {
    static EXECUTOR: RefCell<Option<Rc<LocalExecutor>>> = const { RefCell::new(None) };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpawnLocalError;

impl fmt::Display for SpawnLocalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("spawn_local requires a running XRack UI event loop on this thread")
    }
}

impl std::error::Error for SpawnLocalError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskCancelled;

impl fmt::Display for TaskCancelled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the local UI task was cancelled")
    }
}

impl std::error::Error for TaskCancelled {}

/// Poll a future on the thread running XRack, starting on a subsequent event.
pub fn spawn_local<F: Future + 'static>(
    future: F,
) -> Result<LocalTask<F::Output>, SpawnLocalError> {
    let executor = EXECUTOR
        .with(|current| current.borrow().clone())
        .ok_or(SpawnLocalError)?;
    executor.spawn(future)
}

/// A handle to a UI task. Dropping the handle leaves the task scheduled.
pub struct LocalTask<T> {
    outcome: Rc<RefCell<Outcome<T>>>,
    cancelled: Rc<Cell<bool>>,
    wake: Arc<TaskWake>,
}

impl<T> LocalTask<T> {
    /// Arrange to drop the future on the UI thread the next time it is scheduled.
    pub fn abort(&self) {
        if matches!(*self.outcome.borrow(), Outcome::Pending(_)) {
            self.cancelled.set(true);
            self.wake.wake_by_ref();
        }
    }
}

impl<T> Future for LocalTask<T> {
    type Output = Result<T, TaskCancelled>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut outcome = self.outcome.borrow_mut();
        match &mut *outcome {
            Outcome::Pending(waker) => {
                if let Some(waker) = waker {
                    waker.clone_from(cx.waker());
                } else {
                    *waker = Some(cx.waker().clone());
                }
                Poll::Pending
            }
            Outcome::Ready(_) => {
                let Outcome::Ready(result) = std::mem::replace(&mut *outcome, Outcome::Taken)
                else {
                    unreachable!()
                };
                Poll::Ready(result)
            }
            Outcome::Taken => panic!("local task result already taken"),
        }
    }
}

enum Outcome<T> {
    Pending(Option<Waker>),
    Ready(Result<T, TaskCancelled>),
    Taken,
}

struct Report<T>(Option<Rc<RefCell<Outcome<T>>>>);

impl<T> Report<T> {
    fn complete(&mut self, result: Result<T, TaskCancelled>) {
        if let Some(outcome) = self.0.take() {
            let previous = outcome.replace(Outcome::Ready(result));
            if let Outcome::Pending(Some(waker)) = previous {
                waker.wake();
            }
        }
    }
}

impl<T> Drop for Report<T> {
    fn drop(&mut self) {
        self.complete(Err(TaskCancelled));
    }
}

// Only this scheduling token crosses threads. The future stays in LocalExecutor.
struct TaskWake {
    id: TaskId,
    scheduled: AtomicBool,
    ready: Arc<EventQueue<TaskId>>,
}

impl Wake for TaskWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        if !self.scheduled.swap(true, Ordering::AcqRel) {
            let _ = self.ready.try_push(self.id);
        }
    }
}

struct Task {
    future: Pin<Box<dyn Future<Output = ()>>>,
    wake: Arc<TaskWake>,
    cancelled: Rc<Cell<bool>>,
}

impl Drop for Task {
    fn drop(&mut self) {
        // Ignore any wakers held by external I/O after completion or cancellation.
        self.wake.scheduled.store(true, Ordering::Release);
    }
}

pub(crate) struct LocalExecutor {
    ready: Arc<EventQueue<TaskId>>,
    tasks: RefCell<BTreeMap<TaskId, Task>>,
    next_id: Cell<TaskId>,
    closed: Cell<bool>,
}

impl LocalExecutor {
    pub(crate) fn new(notify: impl Fn() + Send + Sync + 'static) -> Rc<Self> {
        Rc::new(Self {
            ready: Arc::new(EventQueue::new(usize::MAX, notify)),
            tasks: RefCell::default(),
            next_id: Cell::default(),
            closed: Cell::default(),
        })
    }

    pub(crate) fn enter(self: &Rc<Self>) -> impl Drop + use<> {
        struct Scope(Option<Rc<LocalExecutor>>);
        impl Drop for Scope {
            fn drop(&mut self) {
                EXECUTOR.with(|current| current.replace(self.0.take()));
            }
        }
        Scope(EXECUTOR.with(|current| current.replace(Some(self.clone()))))
    }

    pub(crate) fn spawn<F: Future + 'static>(
        &self,
        future: F,
    ) -> Result<LocalTask<F::Output>, SpawnLocalError> {
        if self.closed.get() {
            return Err(SpawnLocalError);
        }
        let id = self.next_id.get();
        self.next_id
            .set(id.checked_add(1).expect("local task ID exhausted"));
        let wake = Arc::new(TaskWake {
            id,
            scheduled: AtomicBool::new(false),
            ready: self.ready.clone(),
        });
        let outcome = Rc::new(RefCell::new(Outcome::Pending(None)));
        let cancelled = Rc::new(Cell::new(false));
        let mut report = Report(Some(outcome.clone()));
        let future = Box::pin(async move {
            report.complete(Ok(future.await));
        });
        self.tasks.borrow_mut().insert(
            id,
            Task {
                future,
                wake: wake.clone(),
                cancelled: cancelled.clone(),
            },
        );
        wake.wake_by_ref();
        Ok(LocalTask {
            outcome,
            cancelled,
            wake,
        })
    }

    pub(crate) fn poll_ready(&self) -> bool {
        let mut processed = false;
        for id in self.ready.take_batch() {
            let task = self.tasks.borrow_mut().remove(&id);
            let Some(mut task) = task else {
                continue;
            };
            processed = true;
            if task.cancelled.get() {
                continue;
            }
            task.wake.scheduled.store(false, Ordering::Release);
            let waker = Waker::from(task.wake.clone());
            if task
                .future
                .as_mut()
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
                && !task.cancelled.get()
            {
                self.tasks.borrow_mut().insert(id, task);
            }
        }
        processed
    }

    pub(crate) fn close(&self) {
        self.closed.set(true);
        self.ready.close();
        let tasks = self.tasks.take();
        drop(tasks);
    }
}

impl Drop for LocalExecutor {
    fn drop(&mut self) {
        self.close();
    }
}
