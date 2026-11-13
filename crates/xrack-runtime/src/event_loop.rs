use std::{
    any::Any,
    cell::{OnceCell, RefCell},
    collections::HashMap,
    marker::PhantomData,
    rc::Rc,
    sync::{Arc, Mutex},
    thread::{self, ThreadId},
};

type UiCallback = Box<dyn FnOnce() + Send>;
type Post = dyn Fn(UiCallback) -> Result<(), EventLoopError> + Send + Sync;
static EVENT_LOOP: Mutex<Option<Arc<Post>>> = Mutex::new(None);

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EventLoopError {
    #[error("the XRack UI event loop is not running")]
    NotRunning,
    #[error("the XRack UI event loop queue is full")]
    QueueFull,
    #[error("an XRack UI event loop is already running")]
    AlreadyRunning,
}

/// Schedule a closure on the UI event loop from any thread.
/// A successful return means the closure was queued.
/// The queue holds at most 256 callbacks on the desktop host. Pending callbacks
/// are discarded when the loop exits.
pub fn invoke_from_event_loop(
    callback: impl FnOnce() + Send + 'static,
) -> Result<(), EventLoopError> {
    let post = EVENT_LOOP.lock().unwrap().clone();
    post.ok_or(EventLoopError::NotRunning)?(Box::new(callback))
}

/// Keeps the platform's event loop registered until it exits.
#[doc(hidden)]
pub struct EventLoopRegistration(());

impl EventLoopRegistration {
    pub fn new(
        post: impl Fn(UiCallback) -> Result<(), EventLoopError> + Send + Sync + 'static,
    ) -> Result<Self, EventLoopError> {
        let mut active = EVENT_LOOP.lock().unwrap();
        if active.is_some() {
            return Err(EventLoopError::AlreadyRunning);
        }
        *active = Some(Arc::new(post));
        Ok(Self(()))
    }
}

impl Drop for EventLoopRegistration {
    fn drop(&mut self) {
        let previous = EVENT_LOOP.lock().unwrap().take();
        drop(previous);
    }
}

type ComponentLookup<C> = Rc<dyn Fn() -> Option<C>>;
thread_local! {
    static COMPONENTS: RefCell<HashMap<usize, Box<dyn Any>>> = RefCell::default();
}

#[derive(Default)]
pub(crate) struct WeakRegistration(OnceCell<Arc<()>>);

impl Drop for WeakRegistration {
    fn drop(&mut self) {
        if let Some(identity) = self.0.get() {
            // The state and its registered Rc weak reference stay on this thread.
            let _ = COMPONENTS.try_with(|components| {
                components
                    .borrow_mut()
                    .remove(&(Arc::as_ptr(identity) as usize))
            });
        }
    }
}

/// A handle that can be sent to a worker without keeping the component alive.
pub struct WeakComponent<C> {
    // Keep this allocation alive until the last handle is dropped so a stale
    // handle cannot identify a later component at the same address.
    identity: Arc<()>,
    owner: ThreadId,
    component: PhantomData<fn() -> C>,
}

impl<C> Clone for WeakComponent<C> {
    fn clone(&self) -> Self {
        Self {
            identity: self.identity.clone(),
            owner: self.owner,
            component: PhantomData,
        }
    }
}

impl<C: 'static> WeakComponent<C> {
    #[doc(hidden)]
    pub fn new<S: 'static>(
        state: &Rc<S>,
        lifetime: &crate::ComponentLifetime,
        wrap: fn(Rc<S>) -> C,
    ) -> Self {
        let identity = lifetime.weak.0.get_or_init(|| {
            let identity = Arc::new(());
            let state = Rc::downgrade(state);
            let lookup: ComponentLookup<C> = Rc::new(move || state.upgrade().map(wrap));
            COMPONENTS.with(|components| {
                components
                    .borrow_mut()
                    .insert(Arc::as_ptr(&identity) as usize, Box::new(lookup));
            });
            identity
        });
        Self {
            identity: identity.clone(),
            owner: thread::current().id(),
            component: PhantomData,
        }
    }

    /// Obtain the component on its owning thread while it is still alive.
    pub fn upgrade(&self) -> Option<C> {
        if self.owner != thread::current().id() {
            return None;
        }
        let lookup = COMPONENTS
            .try_with(|components| {
                components
                    .borrow()
                    .get(&(Arc::as_ptr(&self.identity) as usize))?
                    .downcast_ref::<ComponentLookup<C>>()
                    .cloned()
            })
            .ok()
            .flatten()?;
        lookup()
    }

    /// Queue an update and run it on the UI thread if the component still exists.
    pub fn upgrade_in_event_loop(
        &self,
        callback: impl FnOnce(C) + Send + 'static,
    ) -> Result<(), EventLoopError> {
        let weak = self.clone();
        invoke_from_event_loop(move || {
            if let Some(component) = weak.upgrade() {
                callback(component);
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ComponentLifetime;
    use std::{cell::Cell, sync::mpsc};

    #[derive(Default)]
    struct ComponentState {
        lifetime: ComponentLifetime,
        value: Cell<u32>,
    }

    #[test]
    fn worker_posts_to_the_owner_and_a_destroyed_component_is_skipped() {
        assert_eq!(
            invoke_from_event_loop(|| {}),
            Err(EventLoopError::NotRunning)
        );
        let (send, receive) = mpsc::channel::<UiCallback>();
        let registration = EventLoopRegistration::new(move |callback| {
            send.send(callback).map_err(|_| EventLoopError::NotRunning)
        })
        .unwrap();
        let ui_thread = thread::current().id();
        let state = Rc::new(ComponentState::default());
        let weak = WeakComponent::new(&state, &state.lifetime, |state| state);
        let gone = Rc::new(ComponentState::default());
        let gone_weak = WeakComponent::new(&gone, &gone.lifetime, |state| state);
        let gone_identity = Arc::as_ptr(&gone_weak.identity) as usize;
        let (done, observed) = mpsc::channel();

        thread::spawn(move || {
            assert!(weak.upgrade().is_none());
            invoke_from_event_loop(move || done.send(thread::current().id()).unwrap()).unwrap();
            weak.upgrade_in_event_loop(move |state| {
                assert_eq!(thread::current().id(), ui_thread);
                state.value.set(42);
            })
            .unwrap();
            gone_weak
                .upgrade_in_event_loop(|_| panic!("component was destroyed"))
                .unwrap();
        })
        .join()
        .unwrap();

        drop(gone);
        COMPONENTS.with(|components| assert!(!components.borrow().contains_key(&gone_identity)));
        assert_eq!(state.value.get(), 0);
        for _ in 0..3 {
            receive.try_recv().unwrap()();
        }
        assert_eq!(observed.try_recv().unwrap(), ui_thread);
        assert_eq!(state.value.get(), 42);
        drop(registration);
        assert_eq!(
            invoke_from_event_loop(|| {}),
            Err(EventLoopError::NotRunning)
        );
    }
}
