use crate::ChangeSignal;
use std::cell::{Cell, OnceCell, RefCell};

thread_local! {
    static CALLBACK_READS: Cell<u64> = const { Cell::new(0) };
}

pub(crate) fn callback_reads() -> u64 {
    CALLBACK_READS.get()
}

/// Stores a replaceable callback handle and tracks reactive readers of pure callbacks.
pub struct CallbackSlot<T> {
    callback: RefCell<Option<T>>,
    changed: OnceCell<ChangeSignal>,
}

impl<T> Default for CallbackSlot<T> {
    fn default() -> Self {
        Self::new(None)
    }
}

impl<T> CallbackSlot<T> {
    pub fn new(callback: Option<T>) -> Self {
        Self {
            callback: RefCell::new(callback),
            changed: OnceCell::new(),
        }
    }

    pub fn track(&self) {
        self.changed.get_or_init(ChangeSignal::default).track();
    }

    pub fn set(&self, callback: T) {
        let previous = self.callback.replace(Some(callback));
        if let Some(changed) = self.changed.get() {
            changed.notify();
        }
        drop(previous);
    }
}

impl<T> CallbackSlot<T> {
    #[doc(hidden)]
    pub fn has_handler(&self) -> bool {
        self.callback.borrow().is_some()
    }
}

impl<T: Clone> CallbackSlot<T> {
    /// Take a handle without borrowing the slot while user code runs.
    pub fn get(&self) -> Option<T> {
        let callback = self.callback.borrow().clone();
        if callback.is_some() {
            CALLBACK_READS.set(CALLBACK_READS.get().wrapping_add(1));
        }
        callback
    }
}

/// A host handler that may mutate its captured state during a call.
pub struct EffectfulCallback<T>(RefCell<T>);

#[derive(Debug, thiserror::Error)]
#[error("effectful host callback is already running")]
pub struct CallbackAlreadyRunning;

impl<T> EffectfulCallback<T> {
    pub fn new(callback: T) -> Self {
        Self(RefCell::new(callback))
    }

    /// Guard this handler, leaving its slot free to install a replacement.
    pub fn try_call<R>(
        &self,
        invoke: impl FnOnce(&mut T) -> R,
    ) -> Result<R, CallbackAlreadyRunning> {
        let mut callback = self
            .0
            .try_borrow_mut()
            .map_err(|_| CallbackAlreadyRunning)?;
        Ok(invoke(&mut callback))
    }
}
