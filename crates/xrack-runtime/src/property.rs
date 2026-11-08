//! Lazy property bindings with dynamically tracked dependencies.
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

thread_local! {
    static EVALUATING: RefCell<Option<Rc<DependencyNode>>> = const { RefCell::new(None) };
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
            let Some(binding) = current.as_ref() else {
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
            previous: EVALUATING.with(|current| current.replace(Some(self.clone()))),
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
        self.notify();
    }
}

struct Evaluation<'a> {
    node: &'a DependencyNode,
    previous: Option<Rc<DependencyNode>>,
    completed: bool,
}

impl Drop for Evaluation<'_> {
    fn drop(&mut self) {
        EVALUATING.with(|current| current.replace(self.previous.take()));
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
        EVALUATING.with(|current| current.borrow().is_some())
    }

    pub(crate) fn has_dependents(&self) -> bool {
        !self.0.dependents.borrow().is_empty()
    }

    pub fn track(&self) {
        self.0.track();
    }

    pub fn notify(&self) {
        self.0.notify();
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
}

impl<T: PartialEq> Property<T> {
    /// Override the binding and invalidate dependents only when its known value changes.
    pub fn set_if_changed(&self, value: T) {
        let changed = match &*self.value.borrow() {
            Stored::Explicit(current) => {
                if *current == value {
                    return;
                }
                true
            }
            Stored::Cached(current) if !self.dependency.dirty.get() => *current != value,
            Stored::Unset | Stored::Cached(_) => true,
        };
        self.replace(Stored::Explicit(value), changed);
    }
}

impl<T: Clone> Property<T> {
    pub fn get(&self, default: impl FnOnce() -> T) -> T {
        assert!(
            !self.dependency.evaluating.get(),
            "cyclic property evaluation"
        );
        self.dependency.track();
        match &*self.value.borrow() {
            Stored::Explicit(value) => return value.clone(),
            Stored::Cached(value) if !self.dependency.dirty.get() => return value.clone(),
            Stored::Unset | Stored::Cached(_) => {}
        }

        let value = self.dependency.evaluate(|| match &self.binding {
            Some(binding) => binding(),
            None => default(),
        });
        if !self.dependency.dirty.get() {
            let cached = value.clone();
            if !self.dependency.dirty.get() {
                self.value.replace(Stored::Cached(cached));
            }
        }
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::{AssertUnwindSafe, catch_unwind};

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
