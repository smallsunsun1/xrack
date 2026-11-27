//! Lazy property bindings with dynamically tracked dependencies.
use std::{
    cell::{Cell, OnceCell, RefCell},
    collections::{HashMap, hash_map::Entry},
    rc::{Rc, Weak},
};

thread_local! {
    static INPUT_PATHS: RefCell<Vec<InputPath>> = const { RefCell::new(Vec::new()) };
    static EVALUATING: RefCell<EvaluationContext> = const {
        RefCell::new(EvaluationContext { binding: None, watches: Vec::new() })
    };
}

#[derive(Default)]
struct InputPath {
    root_overrides: Vec<Weak<DependencyNode>>,
    availability: Option<Rc<ReadWatch>>,
    states: Vec<InputState>,
}

struct InputState {
    component: Weak<DependencyNode>,
    selector: Box<str>,
    selected: i64,
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
    frozen: Cell<bool>,
}

impl ReadWatch {
    fn observe(&self, node: &Rc<DependencyNode>) {
        if self.frozen.get() {
            return;
        }
        let mut sources = self.sources.borrow_mut();
        if sources.len() >= self.next_prune.get() {
            sources.retain(|_, source| source.strong_count() != 0);
            self.next_prune.set(sources.len().saturating_mul(2).max(64));
        }
        let Entry::Vacant(entry) = sources.entry(Rc::as_ptr(node)) else {
            return;
        };
        entry.insert(Rc::downgrade(node));
        if node.dirty.get() {
            return;
        }
        // Clean bindings may reuse their value without reading their inputs again.
        let mut pending: Vec<_> = node
            .sources
            .borrow()
            .active_nodes()
            .iter()
            .filter_map(Weak::upgrade)
            .collect();
        while let Some(node) = pending.pop() {
            if let Entry::Vacant(source) = sources.entry(Rc::as_ptr(&node)) {
                source.insert(Rc::downgrade(&node));
                if !node.dirty.get() {
                    pending.extend(
                        node.sources
                            .borrow()
                            .active_nodes()
                            .iter()
                            .filter_map(Weak::upgrade),
                    );
                }
            }
        }
    }

    fn invalidate(&self, source: *const DependencyNode) {
        if self.sources.borrow().contains_key(&source) {
            self.changed.set(true);
        }
    }

    pub(crate) fn changed(&self) -> bool {
        self.changed.get()
    }

    pub(crate) fn clear_changes(&self) {
        self.changed.set(false);
    }

    // Continue tracking writes to the existing dependencies without observing new reads.
    pub(crate) fn freeze(&self) {
        self.frozen.set(true);
    }

    pub(crate) fn without_new_reads<T>(&self, operation: impl FnOnce() -> T) -> T {
        struct Restore<'a>(&'a Cell<bool>, bool);
        impl Drop for Restore<'_> {
            fn drop(&mut self) {
                self.0.set(self.1);
            }
        }
        let _restore = Restore(&self.frozen, self.frozen.replace(true));
        operation()
    }
}

pub(crate) fn watch_reads<T>(operation: impl FnOnce(&ReadWatch) -> T) -> T {
    let watch = Rc::new(ReadWatch::default());
    with_read_watch(&watch, || operation(&watch))
}

pub(crate) fn with_read_watch<T>(watch: &Rc<ReadWatch>, operation: impl FnOnce() -> T) -> T {
    struct Scope;
    impl Drop for Scope {
        fn drop(&mut self) {
            EVALUATING.with(|context| context.borrow_mut().watches.pop());
        }
    }

    EVALUATING.with(|context| context.borrow_mut().watches.push(watch.clone()));
    let _scope = Scope;
    operation()
}

/// Synchronize a delegate's cached row without making the input depend on the whole row.
pub(crate) fn without_read_watches<T>(operation: impl FnOnce() -> T) -> T {
    struct Restore(Vec<Rc<ReadWatch>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            EVALUATING.with(|context| context.borrow_mut().watches = std::mem::take(&mut self.0));
        }
    }
    let _restore =
        Restore(EVALUATING.with(|context| std::mem::take(&mut context.borrow_mut().watches)));
    operation()
}

pub(crate) fn with_input_path<T>(operation: impl FnOnce() -> T) -> T {
    struct Scope;
    impl Drop for Scope {
        fn drop(&mut self) {
            INPUT_PATHS.with(|paths| paths.borrow_mut().pop());
        }
    }

    INPUT_PATHS.with(|paths| paths.borrow_mut().push(InputPath::default()));
    let _scope = Scope;
    operation()
}

pub(crate) fn track_input_availability<T>(check: impl FnOnce() -> T) -> T {
    let watch = INPUT_PATHS.with(|paths| {
        paths
            .borrow_mut()
            .last_mut()
            .map(|path| path.availability.get_or_insert_with(Rc::default).clone())
    });
    match watch {
        Some(watch) => with_read_watch(&watch, check),
        None => check(),
    }
}

/// Linked row writes revalidate availability while checking individual field conflicts separately.
pub(crate) fn input_path_is_current() -> bool {
    INPUT_PATHS.with(|paths| {
        paths
            .borrow()
            .last()
            .and_then(|path| path.availability.as_ref())
            .is_none_or(|watch| !watch.changed())
    })
}

const SOURCE_INDEX_THRESHOLD: usize = 64;

struct SourceIndex {
    slots: HashMap<*const DependencyNode, usize>,
    observed: usize,
}

enum SourceLink {
    Recorded,
    Install,
    DiscardUnread,
}

#[derive(Default)]
struct DependencySources {
    nodes: Vec<Weak<DependencyNode>>,
    index: Option<Box<SourceIndex>>,
}

impl DependencySources {
    fn begin_evaluation(&mut self) -> Option<Vec<Weak<DependencyNode>>> {
        if let Some(index) = &mut self.index {
            index.observed = 0;
            None
        } else {
            Some(std::mem::take(&mut self.nodes))
        }
    }

    // Reconciliation keeps the inputs read in this evaluation in a prefix.
    // Retain existing reverse links for that prefix and discard the rest
    // once the evaluation ends.
    fn active_nodes(&self) -> &[Weak<DependencyNode>] {
        &self.nodes[..self
            .index
            .as_ref()
            .map_or(self.nodes.len(), |index| index.observed)]
    }

    fn observes(&self, source: *const DependencyNode) -> bool {
        if let Some(index) = &self.index {
            index
                .slots
                .get(&source)
                .is_some_and(|slot| *slot < index.observed)
        } else {
            self.nodes.iter().any(|node| node.as_ptr() == source)
        }
    }

    fn observe(&mut self, node: &Rc<DependencyNode>) -> SourceLink {
        let address = Rc::as_ptr(node);
        if let Some(index) = &mut self.index {
            let next = index.observed;
            if self
                .nodes
                .get(next)
                .is_some_and(|source| source.as_ptr() == address)
            {
                // Stable bindings usually read their inputs in the same order.
                index.observed += 1;
                return SourceLink::Recorded;
            }
            return match index.slots.entry(address) {
                Entry::Occupied(mut entry) => {
                    let slot = *entry.get();
                    if slot >= next {
                        *entry.get_mut() = next;
                        self.nodes.swap(slot, next);
                        index.slots.insert(self.nodes[slot].as_ptr(), slot);
                        index.observed += 1;
                    }
                    SourceLink::Recorded
                }
                Entry::Vacant(entry) if next == self.nodes.len() => {
                    entry.insert(next);
                    self.nodes.push(Rc::downgrade(node));
                    index.observed += 1;
                    SourceLink::Install
                }
                // A new input can signal a branch switch. Drop the unread
                // suffix once instead of moving an old input for every new one.
                Entry::Vacant(_) => SourceLink::DiscardUnread,
            };
        }
        if self.nodes.iter().any(|source| source.as_ptr() == address) {
            return SourceLink::Recorded;
        }
        // The weak references keep addresses from being reused while they
        // are present in the index.
        self.nodes.push(Rc::downgrade(node));
        if self.nodes.len() == SOURCE_INDEX_THRESHOLD {
            self.index = Some(Box::new(SourceIndex {
                slots: self
                    .nodes
                    .iter()
                    .enumerate()
                    .map(|(i, source)| (source.as_ptr(), i))
                    .collect(),
                observed: self.nodes.len(),
            }));
        }
        SourceLink::Install
    }

    fn discard_unread(&mut self) -> Vec<Weak<DependencyNode>> {
        let Some(index) = &mut self.index else {
            return Vec::new();
        };
        if index.observed == self.nodes.len() {
            return Vec::new();
        }
        let stale = self.nodes.split_off(index.observed);
        if index.observed < SOURCE_INDEX_THRESHOLD {
            self.index = None;
            self.nodes.shrink_to_fit();
        } else {
            for source in &stale {
                index.slots.remove(&source.as_ptr());
            }
        }
        stale
    }
}

#[derive(Default)]
struct DependencyNode {
    dirty: Cell<bool>,
    evaluating: Cell<bool>,
    sources: RefCell<DependencySources>,
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
            let link = binding.sources.borrow_mut().observe(self);
            let install = match link {
                SourceLink::Recorded => false,
                SourceLink::Install => true,
                SourceLink::DiscardUnread => {
                    let unread = binding.sources.borrow_mut().discard_unread();
                    binding.unlink_sources(unread);
                    matches!(
                        binding.sources.borrow_mut().observe(self),
                        SourceLink::Install
                    )
                }
            };
            if install {
                self.dependents.borrow_mut().push(Rc::downgrade(binding));
            }
        });
    }

    fn detach(&self) {
        self.unlink_sources(self.sources.take().nodes);
    }

    fn unlink_sources(&self, sources: Vec<Weak<Self>>) {
        let address = self as *const Self;
        for source in sources {
            if let Some(source) = source.upgrade() {
                source
                    .dependents
                    .borrow_mut()
                    .retain(|binding| binding.as_ptr() != address);
            }
        }
    }

    fn observes_source(&self, source: *const Self) -> bool {
        // The unobserved suffix during reevaluation retains reverse links
        // for reuse. It does not participate in the current binding.
        !self.evaluating.get() || self.sources.borrow().observes(source)
    }

    fn push_dependents(&self, pending: &mut Vec<Rc<Self>>) {
        let source = std::ptr::from_ref(self);
        self.dependents.borrow_mut().retain(|weak| {
            if let Some(binding) = weak.upgrade() {
                if binding.observes_source(source) {
                    pending.push(binding);
                }
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
                watch.invalidate(address);
            }
            INPUT_PATHS.with(|paths| {
                for path in paths.borrow().iter() {
                    if let Some(watch) = &path.availability {
                        watch.invalidate(address);
                    }
                }
            });
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
        let detached = self.sources.borrow_mut().begin_evaluation();
        let reconcile = detached.is_none();
        if let Some(detached) = detached {
            self.unlink_sources(detached);
        }
        self.evaluating.set(true);
        self.dirty.set(false);
        let mut evaluation = Evaluation {
            node: self,
            previous: EVALUATING.with(|current| current.borrow_mut().binding.replace(self.clone())),
            completed: false,
            reconcile,
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
    reconcile: bool,
}

impl Drop for Evaluation<'_> {
    fn drop(&mut self) {
        EVALUATING.with(|current| {
            current.borrow_mut().binding = self.previous.take();
        });
        if self.reconcile {
            let stale = self.node.sources.borrow_mut().discard_unread();
            self.node.unlink_sources(stale);
        }
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
        let address = Rc::as_ptr(&self.0);
        self.0.dependents.borrow().iter().any(|weak| {
            weak.upgrade()
                .is_some_and(|binding| binding.observes_source(address))
        }) || EVALUATING.with(|context| {
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
pub struct ComponentLifetime {
    loader_mount: OnceCell<crate::LoaderMount>,
    signal: OnceCell<ChangeSignal>,
    pub(crate) weak: crate::event_loop::WeakRegistration,
}

impl ComponentLifetime {
    #[doc(hidden)]
    pub fn loader_mount(&self) -> &crate::LoaderMount {
        self.loader_mount.get_or_init(crate::LoaderMount::default)
    }

    pub fn track(&self) {
        self.signal.get_or_init(ChangeSignal::default).track();
    }

    /// Share the selected case between availability checks and input preparation.
    #[doc(hidden)]
    pub fn try_input_state<E>(
        &self,
        selector: &str,
        read: impl FnOnce() -> Result<i64, E>,
    ) -> Result<i64, E> {
        let component = &self.signal.get_or_init(ChangeSignal::default).0;
        let cached = INPUT_PATHS.with(|paths| {
            paths.borrow().last()?.states.iter().find_map(|entry| {
                (entry.component.as_ptr() == Rc::as_ptr(component)
                    && entry.selector.as_ref() == selector)
                    .then_some(entry.selected)
            })
        });
        if let Some(selected) = cached {
            return Ok(selected);
        }
        let selected = read()?;
        INPUT_PATHS.with(|paths| {
            if let Some(path) = paths.borrow_mut().last_mut() {
                path.states.push(InputState {
                    component: Rc::downgrade(component),
                    selector: selector.into(),
                    selected,
                });
            }
        });
        Ok(selected)
    }

    #[doc(hidden)]
    pub fn input_state(&self, selector: &str, read: impl FnOnce() -> i64) -> i64 {
        self.try_input_state(selector, || Ok::<_, std::convert::Infallible>(read()))
            .unwrap_or_else(|error| match error {})
    }

    pub(crate) fn record_input_root_override(&self) {
        let signal = self.signal.get_or_init(ChangeSignal::default);
        INPUT_PATHS.with(|paths| {
            if let Some(path) = paths.borrow_mut().last_mut() {
                path.root_overrides.push(Rc::downgrade(&signal.0));
            }
        });
    }

    /// Read the effective root visibility while a routed input writes through a linked property.
    #[doc(hidden)]
    pub fn try_input_root_visible<E>(
        &self,
        visible: impl FnOnce() -> Result<bool, E>,
    ) -> Result<bool, E> {
        let overridden = self.signal.get().is_some_and(|signal| {
            INPUT_PATHS.with(|paths| {
                paths.borrow().last().is_some_and(|path| {
                    path.root_overrides
                        .iter()
                        .any(|owner| owner.as_ptr() == Rc::as_ptr(&signal.0))
                })
            })
        });
        if overridden { Ok(true) } else { visible() }
    }

    #[doc(hidden)]
    pub fn input_root_visible(&self, visible: impl FnOnce() -> bool) -> bool {
        self.try_input_root_visible(|| Ok::<_, std::convert::Infallible>(visible()))
            .unwrap_or_else(|error| match error {})
    }
}

enum PropertyValue<T> {
    Owned(T),
    Shared(Rc<T>),
}

impl<T> PropertyValue<T> {
    fn value(&self) -> &T {
        match self {
            Self::Owned(value) => value,
            Self::Shared(value) => value,
        }
    }

    fn into_shared(self) -> Rc<T> {
        match self {
            Self::Owned(value) => Rc::new(value),
            Self::Shared(value) => value,
        }
    }
}

enum Stored<T> {
    Unset,
    Explicit(PropertyValue<T>),
    Cached(PropertyValue<T>),
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
        self.replace(Stored::Explicit(PropertyValue::Owned(value)), true);
    }

    /// Inspect whether a cached binding needs reevaluation without tracking a read.
    pub fn is_dirty(&self) -> bool {
        self.dependency.dirty.get()
    }

    /// Inspect the current explicit or clean cached value without evaluating a binding.
    pub(crate) fn matches_current(&self, predicate: impl FnOnce(&T) -> bool) -> bool {
        match &*self.value.borrow() {
            Stored::Explicit(value) => predicate(value.value()),
            Stored::Cached(value) if !self.dependency.dirty.get() => predicate(value.value()),
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
                if equal(current.value(), &value) {
                    return;
                }
                true
            }
            Stored::Cached(current) if !self.dependency.dirty.get() => {
                !equal(current.value(), &value)
            }
            Stored::Unset | Stored::Cached(_) => true,
        };
        self.replace(Stored::Explicit(PropertyValue::Owned(value)), changed);
    }
}

impl<T: PartialEq> Property<T> {
    /// Override the binding and invalidate dependents only when its known value changes.
    pub fn set_if_changed(&self, value: T) {
        self.set_if_changed_by(value, T::eq);
    }
}

impl<T: Clone> Property<T> {
    /// Return an explicit or clean cached value without evaluating or tracking the binding.
    #[doc(hidden)]
    pub fn cached_value(&self) -> Option<T> {
        match &*self.value.borrow() {
            Stored::Explicit(value) => Some(value.value().clone()),
            Stored::Cached(value) if !self.dependency.dirty.get() => Some(value.value().clone()),
            _ => None,
        }
    }

    /// Observe an explicit override while leaving the default to the caller.
    pub fn explicit_value(&self) -> Option<T> {
        self.dependency.track();
        match &*self.value.borrow() {
            Stored::Explicit(value) => Some(value.value().clone()),
            _ => None,
        }
    }

    pub fn get(&self, default: impl FnOnce() -> T) -> T {
        self.try_get(|| Ok::<_, std::convert::Infallible>(default()))
            .unwrap_or_else(|error| match error {})
    }

    /// Store the initialization result as a value, detaching its dependencies.
    #[doc(hidden)]
    pub fn get_once(&self, initialize: impl FnOnce() -> T) -> T {
        self.try_get_once(|| Ok::<_, std::convert::Infallible>(initialize()))
            .unwrap_or_else(|error| match error {})
    }

    #[doc(hidden)]
    pub fn try_get_once<E>(&self, initialize: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
        if let Some(value) = self.cached_value() {
            self.dependency.track();
            return Ok(value);
        }
        let value = self.try_get(initialize)?;
        // A snapshot is the stored initial value. Keep it distinct from a later
        // write so reload can use a revised initializer for untouched storage.
        self.replace(Stored::Cached(PropertyValue::Owned(value.clone())), false);
        self.dependency.dirty.set(false);
        Ok(value)
    }

    /// Evaluate a binding without caching a failed result.
    #[doc(hidden)]
    pub fn try_get<E>(&self, default: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
        self.try_get_with(|| match &self.binding {
            Some(binding) => Ok(binding()),
            None => default(),
        })
    }

    /// Get an immutable value handle without cloning its payload. Shared
    /// handles retain the previous value when the property is updated.
    #[doc(hidden)]
    pub fn get_shared(&self, default: impl FnOnce() -> T) -> Rc<T> {
        self.try_get_shared(|| Ok::<_, std::convert::Infallible>(default()))
            .unwrap_or_else(|error| match error {})
    }

    #[doc(hidden)]
    pub fn try_get_shared<E>(&self, default: impl FnOnce() -> Result<T, E>) -> Result<Rc<T>, E> {
        assert!(
            !self.dependency.evaluating.get(),
            "cyclic property evaluation"
        );
        self.dependency.track();
        if let Some(value) = self.share_current() {
            crate::update_profile::property(false);
            return Ok(value);
        }
        let value = Rc::new(self.evaluate_binding(|| match &self.binding {
            Some(binding) => Ok(binding()),
            None => default(),
        })?);
        if !self.dependency.dirty.get() {
            self.value
                .replace(Stored::Cached(PropertyValue::Shared(value.clone())));
        }
        Ok(value)
    }

    fn share_current(&self) -> Option<Rc<T>> {
        let mut stored = self.value.borrow_mut();
        match &*stored {
            Stored::Unset => return None,
            Stored::Cached(_) if self.dependency.dirty.get() => return None,
            Stored::Explicit(PropertyValue::Shared(value))
            | Stored::Cached(PropertyValue::Shared(value)) => return Some(value.clone()),
            _ => {}
        }
        let (explicit, value) = match std::mem::replace(&mut *stored, Stored::Unset) {
            Stored::Explicit(value) => (true, value.into_shared()),
            Stored::Cached(value) => (false, value.into_shared()),
            Stored::Unset => return None,
        };
        let shared = PropertyValue::Shared(value.clone());
        *stored = if explicit {
            Stored::Explicit(shared)
        } else {
            Stored::Cached(shared)
        };
        Some(value)
    }

    fn try_get_with<E>(&self, read: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
        assert!(
            !self.dependency.evaluating.get(),
            "cyclic property evaluation"
        );
        self.dependency.track();
        match &*self.value.borrow() {
            Stored::Explicit(value) => return Ok(value.value().clone()),
            Stored::Cached(value) if !self.dependency.dirty.get() => {
                crate::update_profile::property(false);
                return Ok(value.value().clone());
            }
            Stored::Unset | Stored::Cached(_) => {}
        }

        let value = self.evaluate_binding(read)?;
        if !self.dependency.dirty.get() {
            let cached = value.clone();
            if !self.dependency.dirty.get() {
                self.value
                    .replace(Stored::Cached(PropertyValue::Owned(cached)));
            }
        }
        Ok(value)
    }

    fn evaluate_binding<E>(&self, read: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
        crate::update_profile::property(true);
        self.dependency.evaluate(read).inspect_err(|_| {
            if !self.dependency.dirty.replace(true) {
                self.dependency.notify();
            }
        })
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
    ReadOnly(Box<Self>),
    Initial {
        value: Property<T>,
        read: Box<dyn Fn() -> Option<T>>,
    },
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

    /// Supply a value at construction without retaining the caller's binding.
    #[doc(hidden)]
    pub fn initial_optional(read: impl Fn() -> Option<T> + 'static) -> Self {
        Self::Initial {
            value: Property::default(),
            read: Box::new(read),
        }
    }

    pub fn linked(read: impl Fn() -> Option<T> + 'static, write: impl Fn(T) + 'static) -> Self {
        Self::Linked {
            read: Box::new(read),
            write: Box::new(write),
        }
    }

    /// Preserve a supplied binding when the receiving component tries to write.
    #[doc(hidden)]
    pub fn protect_binding(self) -> Self {
        match self {
            Self::State { .. } => Self::ReadOnly(Box::new(self)),
            Self::Local(property) if property.binding.is_some() => {
                Self::ReadOnly(Box::new(Self::Local(property)))
            }
            _ => self,
        }
    }

    #[doc(hidden)]
    pub fn is_writable(&self) -> bool {
        !matches!(self, Self::ReadOnly(_))
    }

    /// Add a lazy state binding above an existing input. An explicit write
    /// replaces this binding until reset, including when the value is equal.
    pub fn with_state(self, read: impl Fn() -> Option<T> + 'static) -> Self {
        let protected = !self.is_writable();
        let state = Self::State {
            value: Property::default(),
            read: Box::new(read),
            base: Box::new(self),
        };
        if protected {
            state.protect_binding()
        } else {
            state
        }
    }

    pub fn reset(&self) {
        match self {
            Self::Local(value) | Self::State { value, .. } => value.reset(),
            Self::Initial { .. } | Self::Linked { .. } | Self::ReadOnly(_) => {}
        }
    }

    #[doc(hidden)]
    pub fn set_if_changed_by(&self, value: T, equal: impl FnOnce(&T, &T) -> bool) {
        match self {
            Self::Local(property)
            | Self::Initial {
                value: property, ..
            }
            | Self::State {
                value: property, ..
            } => property.set_if_changed_by(value, equal),
            Self::Linked { write, .. } => write(value),
            Self::ReadOnly(_) => {}
        }
    }
}

impl<T: Clone> InputProperty<T> {
    /// Evaluate connected initializers after the owning component graph exists.
    #[doc(hidden)]
    pub fn freeze_initial(&self) {
        match self {
            Self::Initial { value, read } if value.cached_value().is_none() => {
                if let Some(initial) = read() {
                    value.get_once(|| initial);
                }
            }
            Self::State { base, .. } | Self::ReadOnly(base) => base.freeze_initial(),
            _ => {}
        }
    }

    #[doc(hidden)]
    pub fn get_shared(&self, default: impl FnOnce() -> T) -> Rc<T> {
        match self {
            Self::Local(property) => property.get_shared(default),
            Self::ReadOnly(base) => base.get_shared(default),
            Self::Linked { read, .. } => Rc::new(read().unwrap_or_else(default)),
            Self::Initial { value, read } => {
                Rc::new(value.get_once(|| read().unwrap_or_else(default)))
            }
            Self::State { value, read, base } => {
                value.get_shared(|| read().unwrap_or_else(|| base.get(default)))
            }
        }
    }

    pub fn get(&self, default: impl FnOnce() -> T) -> T {
        match self {
            Self::Local(property) => property.get(default),
            Self::ReadOnly(base) => base.get(default),
            Self::Linked { read, .. } => read().unwrap_or_else(default),
            Self::Initial { value, read } => value.get_once(|| read().unwrap_or_else(default)),
            Self::State { value, read, base } => {
                value.get(|| read().unwrap_or_else(|| base.get(default)))
            }
        }
    }
}

impl<T: Clone, E: Clone> InputProperty<Result<T, E>> {
    #[doc(hidden)]
    pub fn freeze_initial_result(&self) -> Result<(), E> {
        match self {
            Self::Initial { value, read } if value.cached_value().is_none() => {
                if let Some(initial) = read() {
                    let initial = initial?;
                    let _ = value.get_once(|| Ok(initial));
                }
            }
            Self::State { base, .. } | Self::ReadOnly(base) => base.freeze_initial_result()?,
            _ => {}
        }
        Ok(())
    }

    /// A failed input read leaves the binding dirty so it can be retried.
    #[doc(hidden)]
    pub fn get_result(&self, default: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
        match self {
            Self::Local(property) => property.get_result(default),
            Self::ReadOnly(base) => base.get_result(default),
            Self::Linked { read, .. } => read().unwrap_or_else(default),
            Self::Initial { value, read } => {
                value.try_get_once(|| read().unwrap_or_else(default).map(Ok))?
            }
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

    #[test]
    fn large_bindings_deduplicate_reads_and_detach_unused_inputs() {
        let wide = Property::default();
        wide.set(true);
        let inputs: Vec<Property<i32>> = (0..128).map(|_| Property::default()).collect();
        let total = Property::default();
        let sum = || {
            if wide.get(|| false) {
                inputs
                    .iter()
                    .map(|input| input.get(|| 1) + input.get(|| 1))
                    .sum()
            } else {
                inputs[0].get(|| 1)
            }
        };

        assert_eq!(total.get(sum), 256);
        {
            let dependencies = total.dependency.sources.borrow();
            assert_eq!(dependencies.nodes.len(), 129);
            assert_eq!(dependencies.index.as_ref().unwrap().slots.len(), 129);
        }
        inputs[127].set(7);
        assert!(total.is_dirty());
        assert_eq!(total.get(sum), 268);

        wide.set(false);
        assert_eq!(total.get(sum), 1);
        assert!(total.dependency.sources.borrow().index.is_none());
        inputs[127].set(9);
        assert!(!total.is_dirty());
        inputs[0].set(4);
        assert!(total.is_dirty());
        assert_eq!(total.get(sum), 4);
    }

    #[test]
    fn large_bindings_reconcile_reordered_reads_and_ignore_unread_writes() {
        let inputs = (0..256_i64)
            .map(|value| {
                let input = Property::default();
                input.set(value);
                input
            })
            .collect::<Vec<_>>();
        let use_even = Property::default();
        let change_after_read = Cell::new(false);
        let result = Property::<i64>::default();
        let evaluate = || {
            if !use_even.get(|| false) {
                return inputs.iter().map(|input| input.get(|| 0)).sum();
            }
            // This source participated in the previous evaluation only.
            inputs[1].set(999);
            (0..inputs.len())
                .step_by(2)
                .rev()
                .map(|index| {
                    let value = inputs[index].get(|| 0);
                    if index == 0 && change_after_read.replace(false) {
                        inputs[index].set(value + 1);
                    }
                    value
                })
                .sum()
        };
        assert_eq!(result.get(evaluate), (0..256_i64).sum::<i64>());
        use_even.set(true);
        let even: i64 = (0..256_i64).step_by(2).sum();
        assert_eq!(result.get(evaluate), even);
        assert!(!result.is_dirty());
        inputs[1].set(500);
        assert!(!result.is_dirty());

        inputs[2].set(22);
        assert!(result.is_dirty());
        change_after_read.set(true);
        assert_eq!(result.get(evaluate), even + 20);
        assert!(result.is_dirty());
        assert_eq!(result.get(evaluate), even + 21);
        assert!(!result.is_dirty());
    }

    #[test]
    fn failed_indexed_evaluation_unlinks_inputs_that_were_not_read() {
        let inputs = (0..128)
            .map(|_| ChangeSignal::default())
            .collect::<Vec<_>>();
        let result = Property::<i64>::default();
        assert_eq!(
            result.get(|| {
                for input in &inputs {
                    input.track();
                }
                1
            }),
            1,
        );
        inputs[0].notify();
        let rejected = result.try_get(|| {
            inputs[0].track();
            Err("interrupted binding")
        });
        assert_eq!(rejected, Err("interrupted binding"));
        assert!(result.is_dirty());
        assert!(inputs[0].has_observers());
        assert!(!inputs[1].has_observers());
        assert!(result.dependency.sources.borrow().index.is_none());
        assert_eq!(result.get(|| 2), 2);
    }

    #[test]
    fn indexed_binding_observes_new_inputs_after_a_branch_switch() {
        let previous = (0..128)
            .map(|_| ChangeSignal::default())
            .collect::<Vec<_>>();
        let current = (0..128)
            .map(|_| ChangeSignal::default())
            .collect::<Vec<_>>();
        let switched = Property::default();
        let result = Property::<bool>::default();
        let evaluate = || {
            let switched = switched.get(|| false);
            for input in if switched { &current } else { &previous } {
                input.track();
            }
            switched
        };
        assert!(!result.get(evaluate));
        switched.set(true);
        assert!(result.get(evaluate));
        assert!(!previous[0].has_observers());
        previous[127].notify();
        assert!(!result.is_dirty());
        current[127].notify();
        assert!(result.is_dirty());
        assert!(result.get(evaluate));
        switched.set(false);
        assert!(!result.get(evaluate));
        assert!(!current[127].has_observers());
    }

    #[test]
    fn shared_string_reads_reuse_the_cached_buffer_until_invalidated() {
        let input = Property::default();
        let binding = Property::default();
        input.set("first value".to_string());
        let first = binding.get_shared(|| input.get(String::new));
        let same = binding.get_shared(|| panic!("shared value was already computed"));
        assert!(Rc::ptr_eq(&first, &same));
        assert_eq!(binding.get(String::new), "first value");

        input.set("second value".to_string());
        assert!(binding.is_dirty());
        let second = binding.get_shared(|| input.get(String::new));
        assert!(!Rc::ptr_eq(&first, &second));
        assert_eq!(first.as_str(), "first value");
        assert_eq!(second.as_str(), "second value");
    }

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
