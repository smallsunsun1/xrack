//! Lazy property bindings with dynamically tracked dependencies.
use std::{
    cell::{Cell, OnceCell, RefCell},
    collections::HashMap,
    rc::{Rc, Weak},
};

thread_local! {
    static EVALUATING: RefCell<EvaluationContext> = const {
        RefCell::new(EvaluationContext { binding: None, watches: Vec::new() })
    };
}

struct EvaluationContext {
    binding: Option<Rc<DependencyNode>>,
    watches: Vec<Rc<ReadWatch>>,
}

/// Observe writes to dependencies read while preparing a transaction.
#[derive(Default)]
pub(crate) struct ReadWatch {
    sources: RefCell<HashMap<*const DependencyNode, Weak<DependencyNode>>>,
    next_prune: Cell<usize>,
    changed: Cell<bool>,
}

impl ReadWatch {
    fn observe(&self, node: &Rc<DependencyNode>) {
        let mut sources = self.sources.borrow_mut();
        if sources.contains_key(&Rc::as_ptr(node)) {
            return;
        }
        if sources.len() >= self.next_prune.get() {
            sources.retain(|_, source| source.strong_count() != 0);
            self.next_prune.set(sources.len().saturating_mul(2).max(64));
        }
        // Clean bindings may use their cached value without reading their inputs again.
        let mut pending = Vec::new();
        let mut next = Some(node.clone());
        while let Some(node) = next {
            if let std::collections::hash_map::Entry::Vacant(source) =
                sources.entry(Rc::as_ptr(&node))
            {
                source.insert(Rc::downgrade(&node));
                if !node.dirty.get() {
                    pending.extend(node.sources.borrow().iter().filter_map(Weak::upgrade));
                }
            }
            next = pending.pop();
        }
    }

    pub(crate) fn changed(&self) -> bool {
        self.changed.get()
    }
}

pub(crate) fn watch_reads<T>(operation: impl FnOnce(&ReadWatch) -> T) -> T {
    struct Scope;
    impl Drop for Scope {
        fn drop(&mut self) {
            EVALUATING.with(|context| context.borrow_mut().watches.pop());
        }
    }

    let watch = Rc::new(ReadWatch::default());
    EVALUATING.with(|context| context.borrow_mut().watches.push(watch.clone()));
    let _scope = Scope;
    operation(&watch)
}

#[derive(Default)]
struct DependencyNode {
    dirty: Cell<bool>,
    evaluating: Cell<bool>,
    sources: RefCell<Vec<Weak<Self>>>,
    dependents: RefCell<Vec<Weak<Self>>>,
}

impl DependencyNode {
    fn track(self: &Rc<Self>) {
        EVALUATING.with(|current| {
            let current = current.borrow();
            for watch in &current.watches {
                watch.observe(self);
            }
            let Some(binding) = current.binding.as_ref() else {
                return;
            };
            if Rc::ptr_eq(self, binding) {
                return;
            }
            let mut sources = binding.sources.borrow_mut();
            let source = Rc::downgrade(self);
            if !sources.iter().any(|existing| existing.ptr_eq(&source)) {
                sources.push(source);
                self.dependents.borrow_mut().push(Rc::downgrade(binding));
            }
        });
    }

    fn detach(&self) {
        let address = self as *const Self;
        for source in self.sources.take() {
            if let Some(source) = source.upgrade() {
                source
                    .dependents
                    .borrow_mut()
                    .retain(|binding| binding.as_ptr() != address);
            }
        }
    }

    fn push_dependents(&self, pending: &mut Vec<Rc<Self>>) {
        self.dependents.borrow_mut().retain(|weak| {
            if let Some(binding) = weak.upgrade() {
                pending.push(binding);
                true
            } else {
                false
            }
        });
    }

    fn notify(&self) {
        EVALUATING.with(|context| {
            let address = std::ptr::from_ref(self);
            for watch in &context.borrow().watches {
                if watch.sources.borrow().contains_key(&address) {
                    watch.changed.set(true);
                }
            }
        });
        self.invalidate_dependents();
    }

    fn invalidate_dependents(&self) {
        let mut pending = Vec::new();
        self.push_dependents(&mut pending);
        while let Some(binding) = pending.pop() {
            if !binding.dirty.replace(true) {
                binding.push_dependents(&mut pending);
            }
        }
    }

    fn evaluate<T>(self: &Rc<Self>, binding: impl FnOnce() -> T) -> T {
        self.detach();
        self.evaluating.set(true);
        self.dirty.set(false);
        let mut evaluation = Evaluation {
            node: self,
            previous: EVALUATING.with(|current| current.borrow_mut().binding.replace(self.clone())),
            completed: false,
        };
        let value = binding();
        evaluation.completed = true;
        value
    }
}

impl Drop for DependencyNode {
    fn drop(&mut self) {
        self.detach();
        // Ending a temporary binding is not a write to an input used by a transaction.
        self.invalidate_dependents();
    }
}

struct Evaluation<'a> {
    node: &'a DependencyNode,
    previous: Option<Rc<DependencyNode>>,
    completed: bool,
}

impl Drop for Evaluation<'_> {
    fn drop(&mut self) {
        EVALUATING.with(|current| {
            current.borrow_mut().binding = self.previous.take();
        });
        self.node.evaluating.set(false);
        if !self.completed && !self.node.dirty.replace(true) {
            self.node.notify();
        }
    }
}

/// A dependency on mutable state that is stored outside a property.
#[derive(Default)]
pub struct ChangeSignal(Rc<DependencyNode>);

impl ChangeSignal {
    pub(crate) fn is_tracking() -> bool {
        EVALUATING.with(|current| {
            let current = current.borrow();
            current.binding.is_some() || !current.watches.is_empty()
        })
    }

    pub(crate) fn has_observers(&self) -> bool {
        !self.0.dependents.borrow().is_empty()
            || EVALUATING.with(|context| {
                let address = Rc::as_ptr(&self.0);
                context
                    .borrow()
                    .watches
                    .iter()
                    .any(|watch| watch.sources.borrow().contains_key(&address))
            })
    }

    pub fn track(&self) {
        self.0.track();
    }

    pub fn notify(&self) {
        self.0.notify();
    }
}

/// A dependency on the lifetime of a component that owns a weak binding.
#[doc(hidden)]
#[derive(Default)]
pub struct ComponentLifetime(OnceCell<ChangeSignal>);

impl ComponentLifetime {
    pub fn track(&self) {
        self.0.get_or_init(ChangeSignal::default).track();
    }
}

enum Stored<T> {
    Unset,
    Explicit(T),
    Cached(T),
}

/// Stores an explicit value or a lazily computed binding result.
///
/// The default closure supplied to `get` describes the same binding on each read.
pub struct Property<T> {
    value: RefCell<Stored<T>>,
    binding: Option<Box<dyn Fn() -> T>>,
    dependency: Rc<DependencyNode>,
}

impl<T> Default for Property<T> {
    fn default() -> Self {
        Self::new(None)
    }
}

impl<T> Property<T> {
    pub fn new(binding: Option<Box<dyn Fn() -> T>>) -> Self {
        Self {
            value: RefCell::new(Stored::Unset),
            binding,
            dependency: Rc::default(),
        }
    }

    pub fn set(&self, value: T) {
        self.replace(Stored::Explicit(value), true);
    }

    /// Inspect whether a cached binding needs reevaluation without tracking a read.
    pub fn is_dirty(&self) -> bool {
        self.dependency.dirty.get()
    }

    /// Inspect the current explicit or clean cached value without evaluating a binding.
    pub(crate) fn matches_current(&self, predicate: impl FnOnce(&T) -> bool) -> bool {
        match &*self.value.borrow() {
            Stored::Explicit(value) => predicate(value),
            Stored::Cached(value) if !self.dependency.dirty.get() => predicate(value),
            _ => false,
        }
    }

    pub fn reset(&self) {
        if matches!(*self.value.borrow(), Stored::Explicit(_)) {
            self.replace(Stored::Unset, true);
        }
    }

    fn replace(&self, value: Stored<T>, changed: bool) {
        let previous = self.value.replace(value);
        self.dependency.detach();
        self.dependency.dirty.set(true);
        if changed {
            self.dependency.notify();
        }
        drop(previous);
    }
    #[doc(hidden)]
    pub fn set_if_changed_by(&self, value: T, equal: impl FnOnce(&T, &T) -> bool) {
        let changed = match &*self.value.borrow() {
            Stored::Explicit(current) => {
                if equal(current, &value) {
                    return;
                }
                true
            }
            Stored::Cached(current) if !self.dependency.dirty.get() => !equal(current, &value),
            Stored::Unset | Stored::Cached(_) => true,
        };
        self.replace(Stored::Explicit(value), changed);
    }
}

impl<T: PartialEq> Property<T> {
    /// Override the binding and invalidate dependents only when its known value changes.
    pub fn set_if_changed(&self, value: T) {
        self.set_if_changed_by(value, T::eq);
    }
}

impl<T: Clone> Property<T> {
    /// Observe an explicit override while leaving the default to the caller.
    pub fn explicit_value(&self) -> Option<T> {
        self.dependency.track();
        match &*self.value.borrow() {
            Stored::Explicit(value) => Some(value.clone()),
            _ => None,
        }
    }

    pub fn get(&self, default: impl FnOnce() -> T) -> T {
        self.try_get(|| Ok::<_, std::convert::Infallible>(default()))
            .unwrap_or_else(|error| match error {})
    }

    /// Evaluate a binding without caching a failed result.
    #[doc(hidden)]
    pub fn try_get<E>(&self, default: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
        self.try_get_with(|| match &self.binding {
            Some(binding) => Ok(binding()),
            None => default(),
        })
    }

    fn try_get_with<E>(&self, read: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
        assert!(
            !self.dependency.evaluating.get(),
            "cyclic property evaluation"
        );
        self.dependency.track();
        match &*self.value.borrow() {
            Stored::Explicit(value) => return Ok(value.clone()),
            Stored::Cached(value) if !self.dependency.dirty.get() => return Ok(value.clone()),
            Stored::Unset | Stored::Cached(_) => {}
        }

        let value = self.dependency.evaluate(read);
        let value = match value {
            Ok(value) => value,
            Err(error) => {
                if !self.dependency.dirty.replace(true) {
                    self.dependency.notify();
                }
                return Err(error);
            }
        };
        if !self.dependency.dirty.get() {
            let cached = value.clone();
            if !self.dependency.dirty.get() {
                self.value.replace(Stored::Cached(cached));
            }
        }
        Ok(value)
    }
}

impl<T: Clone, E: Clone> Property<Result<T, E>> {
    fn get_result(&self, default: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
        self.try_get_with(|| {
            match &self.binding {
                Some(binding) => binding(),
                None => default(),
            }
            .map(Ok)
        })?
    }
}

/// Storage for a generated component input. Linked inputs track the actual
/// source property and forward writes without allocating a second value slot.
#[doc(hidden)]
pub enum InputProperty<T> {
    Local(Property<T>),
    Linked {
        read: Box<dyn Fn() -> Option<T>>,
        write: Box<dyn Fn(T)>,
    },
    State {
        value: Property<T>,
        read: Box<dyn Fn() -> Option<T>>,
        base: Box<Self>,
    },
}

impl<T> Default for InputProperty<T> {
    fn default() -> Self {
        Self::Local(Property::default())
    }
}

impl<T> InputProperty<T> {
    pub fn bound(read: impl Fn() -> T + 'static) -> Self {
        Self::Local(Property::new(Some(Box::new(read))))
    }

    /// Evaluate a supplied binding while it is available, then use the
    /// component's own property default.
    #[doc(hidden)]
    pub fn bound_optional(read: impl Fn() -> Option<T> + 'static) -> Self {
        Self::default().with_state(read)
    }

    pub fn linked(read: impl Fn() -> Option<T> + 'static, write: impl Fn(T) + 'static) -> Self {
        Self::Linked {
            read: Box::new(read),
            write: Box::new(write),
        }
    }

    /// Add a lazy state binding above an existing input. An explicit write
    /// replaces this binding until reset, including when the value is equal.
    pub fn with_state(self, read: impl Fn() -> Option<T> + 'static) -> Self {
        Self::State {
            value: Property::default(),
            read: Box::new(read),
            base: Box::new(self),
        }
    }

    pub fn reset(&self) {
        match self {
            Self::Local(value) | Self::State { value, .. } => value.reset(),
            Self::Linked { .. } => {}
        }
    }

    #[doc(hidden)]
    pub fn set_if_changed_by(&self, value: T, equal: impl FnOnce(&T, &T) -> bool) {
        match self {
            Self::Local(property)
            | Self::State {
                value: property, ..
            } => property.set_if_changed_by(value, equal),
            Self::Linked { write, .. } => write(value),
        }
    }
}

impl<T: Clone> InputProperty<T> {
    pub fn get(&self, default: impl FnOnce() -> T) -> T {
        match self {
            Self::Local(property) => property.get(default),
            Self::Linked { read, .. } => read().unwrap_or_else(default),
            Self::State { value, read, base } => {
                value.get(|| read().unwrap_or_else(|| base.get(default)))
            }
        }
    }
}

impl<T: Clone, E: Clone> InputProperty<Result<T, E>> {
    /// A failed input read leaves the binding dirty so it can be retried.
    #[doc(hidden)]
    pub fn get_result(&self, default: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
        match self {
            Self::Local(property) => property.get_result(default),
            Self::Linked { read, .. } => read().unwrap_or_else(default),
            Self::State { value, read, base } => {
                value.get_result(|| read().unwrap_or_else(|| base.get_result(default)))
            }
        }
    }
}

impl<T: PartialEq> InputProperty<T> {
    pub fn set_if_changed(&self, value: T) {
        self.set_if_changed_by(value, T::eq);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    #[test]
    fn read_watch_keeps_live_inputs_of_a_dropped_cached_binding() {
        let input = Rc::new(Property::default());
        input.set(1);
        let sampled = Property::new(Some(Box::new({
            let input = input.clone();
            move || input.get(|| 0)
        })));
        assert_eq!(sampled.get(|| 0), 1);

        watch_reads(|reads| {
            assert_eq!(sampled.get(|| 0), 1);
            drop(sampled);
            assert!(!reads.changed());
            input.set(2);
            assert!(reads.changed());
        });
    }

    #[test]
    fn state_input_restores_the_live_fallback_after_reset() {
        let active = Rc::new(Property::default());
        active.set(true);
        let base = Property::default();
        base.set(1);
        let state = active.clone();
        let input = InputProperty::default().with_state(move || state.get(|| false).then_some(2));
        let read = || input.get(|| base.get(|| 0));

        assert_eq!(read(), 2);
        input.set_if_changed(2);
        active.set(false);
        base.set(3);
        assert_eq!(read(), 2);
        input.reset();
        assert_eq!(read(), 3);
        active.set(true);
        assert_eq!(read(), 2);
    }

    #[test]
    fn invalidation_reaches_a_diamond_once() {
        let source = Property::default();
        let left = Property::default();
        let right = Property::default();
        let total = Property::default();
        let evaluations = Cell::new(0);

        source.set(1);
        let value = || {
            total.get(|| {
                evaluations.set(evaluations.get() + 1);
                let left = left.get(|| source.get(|| 0) + source.get(|| 0));
                left + right.get(|| source.get(|| 0) * 3)
            })
        };
        assert_eq!(value(), 5);
        source.set(2);
        source.set(3);
        assert_eq!(evaluations.get(), 1);
        assert_eq!(value(), 15);
        assert_eq!(value(), 15);
        assert_eq!(evaluations.get(), 2);
    }

    #[test]
    fn unwinding_a_nested_evaluation_allows_retry_and_restores_tracking() {
        let source = Property::default();
        let inner = Property::default();
        let outer = Property::default();
        let first = Cell::new(true);
        source.set(7);

        let value = || {
            outer.get(|| {
                catch_unwind(AssertUnwindSafe(|| {
                    inner.get(|| {
                        let number = source.get(|| 0);
                        assert!(!first.replace(false), "transient failure");
                        number
                    })
                }))
                .unwrap_or(0)
            })
        };
        assert_eq!(value(), 0);
        assert_eq!(value(), 7);
        source.set(9);
        assert_eq!(value(), 9);
    }

    #[test]
    fn invalidation_during_evaluation_does_not_cache_an_obsolete_result() {
        let source = Property::default();
        let derived = Property::default();
        source.set(1);

        assert_eq!(
            derived.get(|| {
                let previous = source.get(|| 0);
                source.set(2);
                previous
            }),
            1
        );
        assert_eq!(derived.get(|| source.get(|| 0)), 2);
        assert_eq!(derived.get(|| panic!("cache should be valid")), 2);
    }

    #[test]
    fn dropping_a_source_invalidates_bindings_that_read_through_weak_references() {
        let source = Rc::new(Property::default());
        source.set(8);
        let weak = Rc::downgrade(&source);
        let derived = Property::default();
        let value = || derived.get(|| weak.upgrade().map_or(0, |source| source.get(|| 0)));
        assert_eq!(value(), 8);
        drop(source);
        assert_eq!(value(), 0);
    }
}
