use std::{
    collections::VecDeque,
    sync::{Mutex, mpsc::TrySendError},
};

const BATCH_SIZE: usize = 64;

/// The empty-to-nonempty transition owns the wake. A partial drain rearms it.
pub(crate) struct EventQueue<T> {
    items: Mutex<Option<VecDeque<T>>>,
    capacity: usize,
    notify: Box<dyn Fn() + Send + Sync>,
}

impl<T> EventQueue<T> {
    pub(crate) fn new(capacity: usize, notify: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            items: Mutex::new(Some(VecDeque::new())),
            capacity,
            notify: Box::new(notify),
        }
    }

    pub(crate) fn try_push(&self, item: T) -> Result<(), TrySendError<T>> {
        let wake = {
            let mut items = self.items.lock().unwrap();
            let Some(items) = items.as_mut() else {
                return Err(TrySendError::Disconnected(item));
            };
            if items.len() == self.capacity {
                return Err(TrySendError::Full(item));
            }
            let was_empty = items.is_empty();
            items.push_back(item);
            was_empty
        };
        if wake {
            (self.notify)();
        }
        Ok(())
    }

    pub(crate) fn take_batch(&self) -> Vec<T> {
        let (batch, more) = {
            let mut items = self.items.lock().unwrap();
            let Some(items) = items.as_mut() else {
                return Vec::new();
            };
            let batch = items.drain(..items.len().min(BATCH_SIZE)).collect();
            (batch, !items.is_empty())
        };
        if more {
            (self.notify)();
        }
        batch
    }

    pub(crate) fn close(&self) {
        let items = self.items.lock().unwrap().take();
        drop(items);
    }
}
