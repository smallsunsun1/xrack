use crate::Component;
use std::{
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

#[doc(hidden)]
pub struct SecondaryWindowSlot(u64);

impl Default for SecondaryWindowSlot {
    fn default() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(
            NEXT.try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .expect("secondary window identities exhausted"),
        )
    }
}

impl SecondaryWindowSlot {
    pub fn register(
        &self,
        mount: impl Fn() -> Option<Box<dyn Component>> + 'static,
        close: impl Fn() + 'static,
    ) -> SecondaryWindowRegistration {
        SecondaryWindowRegistration {
            id: self.0,
            mount: Rc::new(mount),
            close: Rc::new(close),
        }
    }
}

/// A parent's request to host one of its component instances as a native window.
#[doc(hidden)]
#[derive(Clone)]
pub struct SecondaryWindowRegistration {
    id: u64,
    mount: Rc<dyn Fn() -> Option<Box<dyn Component>>>,
    close: Rc<dyn Fn()>,
}

impl SecondaryWindowRegistration {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn mount(&self) -> Option<Box<dyn Component>> {
        (self.mount)()
    }

    pub fn closed(&self) {
        (self.close)();
    }
}
