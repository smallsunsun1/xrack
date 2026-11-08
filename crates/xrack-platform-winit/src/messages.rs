use crate::queue::EventQueue;
use std::sync::{Arc, mpsc::TrySendError};

const CAPACITY: usize = 256;

/// A thread-safe handle for posting owned messages to the UI event loop.
pub struct UiSender<M>(Arc<EventQueue<M>>);

impl<M> Clone for UiSender<M> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<M: Send + 'static> UiSender<M> {
    /// Enqueue without blocking for space. Returns the message if full or closed.
    pub fn try_send(&self, message: M) -> Result<(), TrySendError<M>> {
        self.0.try_push(message)
    }
}

pub(crate) struct Inbox<M>(Arc<EventQueue<M>>);

impl<M> Inbox<M> {
    pub(crate) fn new(notify: impl Fn() + Send + Sync + 'static) -> Self {
        Self(Arc::new(EventQueue::new(CAPACITY, notify)))
    }

    pub(crate) fn sender(&self) -> UiSender<M> {
        UiSender(self.0.clone())
    }
    pub(crate) fn take_batch(&self) -> Vec<M> {
        self.0.take_batch()
    }
    pub(crate) fn close(&self) {
        self.0.close();
    }
}

impl<M> Drop for Inbox<M> {
    fn drop(&mut self) {
        self.close();
    }
}
