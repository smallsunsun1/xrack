use crate::{Component, Node, Property, RowKey};
use std::{
    any::Any,
    cell::{Cell, RefCell},
    collections::VecDeque,
    ops::Deref,
    rc::{Rc, Weak},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

type BuildStep = Box<dyn FnOnce() -> Result<(), String>>;

#[derive(Default)]
struct BuildSteps {
    children: VecDeque<BuildStep>,
    complete: Vec<BuildStep>,
}

/// Construction jobs owned by one deferred Loader request.
#[doc(hidden)]
#[derive(Clone, Default)]
pub struct LoaderBuild(Rc<RefCell<BuildSteps>>);

thread_local! {
    static CURRENT_BUILD: RefCell<Option<LoaderBuild>> = const { RefCell::new(None) };
}

impl LoaderBuild {
    pub fn current() -> Option<Self> {
        CURRENT_BUILD.with(|current| current.borrow().clone())
    }

    pub fn enqueue(&self, step: impl FnOnce() -> Result<(), String> + 'static) {
        self.0.borrow_mut().children.push_back(Box::new(step));
    }

    pub fn complete(&self, step: impl FnOnce() -> Result<(), String> + 'static) {
        self.0.borrow_mut().complete.push(Box::new(step));
    }

    fn scope<R>(&self, run: impl FnOnce() -> R) -> R {
        struct Restore(Option<LoaderBuild>);
        impl Drop for Restore {
            fn drop(&mut self) {
                CURRENT_BUILD.with(|current| current.replace(self.0.take()));
            }
        }
        let _restore = Restore(CURRENT_BUILD.with(|current| current.replace(Some(self.clone()))));
        run()
    }

    fn advance(&self, deadline: Option<Instant>, mut remaining: usize) -> Result<bool, String> {
        loop {
            let mut steps = self.0.borrow_mut();
            if steps.children.is_empty() && steps.complete.is_empty() {
                return Ok(true);
            }
            if deadline.is_some_and(|deadline| remaining == 0 || Instant::now() >= deadline) {
                return Ok(false);
            }
            let step = steps.children.pop_front().or_else(|| steps.complete.pop());
            drop(steps);
            if let Some(step) = step {
                self.scope(step)?;
            }
            remaining = remaining.saturating_sub(1);
        }
    }
}

/// Generated child storage. During a Loader build the child is filled by
/// a construction job before the containing component is published.
#[doc(hidden)]
pub struct LoaderChild<T>(ChildStorage<T>);

enum ChildStorage<T> {
    Ready(T),
    Pending(Rc<std::cell::OnceCell<T>>),
}

impl<T: 'static> LoaderChild<T> {
    pub fn new(create: impl FnOnce() -> T + 'static) -> Self {
        let Some(build) = LoaderBuild::current() else {
            return Self(ChildStorage::Ready(create()));
        };
        let value = Rc::new(std::cell::OnceCell::new());
        let target = Rc::downgrade(&value);
        build.enqueue(move || {
            if let Some(target) = target.upgrade() {
                let _ = target.set(create());
            }
            Ok(())
        });
        Self(ChildStorage::Pending(value))
    }
}

impl<T> Deref for LoaderChild<T> {
    type Target = T;

    #[expect(
        clippy::expect_used,
        reason = "a Loader publishes the component after all child jobs finish"
    )]
    fn deref(&self) -> &T {
        match &self.0 {
            ChildStorage::Ready(value) => value,
            ChildStorage::Pending(value) => value.get().expect("component child initialized"),
        }
    }
}

#[derive(Default)]
struct PendingBuild {
    gate: Rc<Property<bool>>,
    build: LoaderBuild,
    started: bool,
}

impl PendingBuild {
    fn waiting_scene(&self) -> Node {
        LoaderFrame::waiting(&self.gate)
    }
}

/// Stores the live candidate and the jobs required to finish a deferred candidate.
#[doc(hidden)]
#[derive(Default)]
pub struct LoaderSlot {
    instance: RefCell<Option<(usize, Rc<dyn Any>)>>,
    pending: RefCell<Option<(usize, PendingBuild)>>,
    mounted: Property<Option<usize>>,
}

impl LoaderSlot {
    pub fn get<T: 'static>(&self, candidate: usize) -> Option<Rc<T>> {
        if self.mounted.cached_value() != Some(Some(candidate)) {
            return None;
        }
        self.storage(candidate)
    }

    fn storage<T: 'static>(&self, candidate: usize) -> Option<Rc<T>> {
        let instance = self.instance.borrow();
        let (current, value) = instance.as_ref()?;
        if *current != candidate {
            return None;
        }
        value.clone().downcast().ok()
    }

    #[doc(hidden)]
    pub fn item<T: 'static>(
        &self,
        candidate: usize,
        component: crate::WeakComponent<T>,
    ) -> Option<LoaderItem<T>> {
        if self.mounted.cached_value() != Some(Some(candidate)) {
            return None;
        }
        let instance = self.instance.borrow();
        let (current, storage) = instance.as_ref()?;
        (*current == candidate).then(|| LoaderItem {
            mount: Rc::downgrade(storage),
            component,
        })
    }

    pub fn is_mounted(&self, candidate: i64) -> bool {
        self.mounted.get(|| None) == usize::try_from(candidate).ok() && candidate >= 0
    }

    pub fn render<T: Default + 'static>(
        &self,
        candidate: usize,
        deferred: bool,
        prepare: impl FnOnce(&T),
        render: impl FnOnce(&T) -> Node,
    ) -> Node {
        self.try_render(
            candidate,
            deferred,
            |value| Ok(prepare(value)),
            |value| Ok(render(value)),
            |error| panic!("generated component construction failed: {error}"),
        )
        .unwrap_or_else(|error: std::convert::Infallible| match error {})
    }

    pub fn try_render<T: Default + 'static, E>(
        &self,
        candidate: usize,
        deferred: bool,
        prepare: impl FnOnce(&T) -> Result<(), E>,
        render: impl FnOnce(&T) -> Result<Node, E>,
        build_error: impl FnOnce(String) -> E,
    ) -> Result<Node, E> {
        if let Some(value) = self.get::<T>(candidate) {
            return render(&value);
        }

        let stored = self.storage::<T>(candidate);
        if stored.is_none() {
            self.instance.take();
            self.mounted.set_if_changed(None);
        }
        let mut request = self
            .pending
            .take()
            .and_then(|(key, build)| (key == candidate).then_some(build));
        if deferred {
            let pending = request.get_or_insert_with(PendingBuild::default);
            if !pending.gate.get(|| false) {
                let scene = pending.waiting_scene();
                self.pending
                    .replace(request.map(|build| (candidate, build)));
                return Ok(scene);
            }
        }

        let value = stored.unwrap_or_else(|| {
            let value = Rc::new(T::default());
            self.instance.replace(Some((candidate, value.clone())));
            value
        });
        if let Some(mut pending) = request
            && (deferred || pending.started)
        {
            const STEPS_PER_FRAME: usize = 8;
            let deadline = deferred.then(|| Instant::now() + Duration::from_millis(2));
            let mut remaining = STEPS_PER_FRAME;
            if !pending.started {
                if let Err(error) = pending.build.scope(|| prepare(&value)) {
                    self.instance.take();
                    return Err(error);
                }
                pending.started = true;
                remaining -= 1;
            }
            match pending.build.advance(deadline, remaining) {
                Ok(true) => {}
                Ok(false) => {
                    pending.gate = Rc::default();
                    let scene = pending.waiting_scene();
                    self.pending.replace(Some((candidate, pending)));
                    return Ok(scene);
                }
                Err(error) => {
                    self.instance.take();
                    return Err(build_error(error));
                }
            }
        }
        let node = render(&value)?;
        self.mounted.set_if_changed(Some(candidate));
        Ok(node)
    }

    pub fn clear(&self, candidate: usize) {
        if self
            .instance
            .borrow()
            .as_ref()
            .is_some_and(|(key, _)| *key == candidate)
        {
            self.instance.take();
            self.mounted.set_if_changed(None);
        }
        if self
            .pending
            .borrow()
            .as_ref()
            .is_some_and(|(key, _)| *key == candidate)
        {
            self.pending.take();
        }
    }
}

/// A weak reference to a Loader candidate. Unmounting invalidates the handle.
pub struct LoaderItem<T> {
    mount: Weak<dyn Any>,
    component: crate::WeakComponent<T>,
}

impl<T: 'static> LoaderItem<T> {
    pub(crate) fn with_mount(mount: Weak<dyn Any>, component: crate::WeakComponent<T>) -> Self {
        Self { mount, component }
    }

    /// Obtain the component while its original mount remains live.
    pub fn upgrade(&self) -> Option<T> {
        let _mount = self.mount.upgrade()?;
        self.component.upgrade()
    }

    #[doc(hidden)]
    pub fn cast<U: 'static>(self) -> Option<LoaderItem<U>> {
        let component: Box<dyn Any> = Box::new(self.component);
        Some(LoaderItem {
            mount: self.mount,
            component: *component.downcast::<crate::WeakComponent<U>>().ok()?,
        })
    }
}

impl<T> Clone for LoaderItem<T> {
    fn clone(&self) -> Self {
        Self {
            mount: self.mount.clone(),
            component: self.component.clone(),
        }
    }
}

/// A scene can release the pending request only while its Loader still owns it.
#[doc(hidden)]
#[derive(Clone, Debug)]
pub struct LoaderFrame(Weak<Property<bool>>);

impl LoaderFrame {
    pub(crate) fn waiting(gate: &Rc<Property<bool>>) -> Node {
        gate.get(|| false);
        let mut node = Node::hidden();
        node.set_loader_frame(Self(Rc::downgrade(gate)));
        node
    }
}

impl PartialEq for LoaderFrame {
    fn eq(&self, other: &Self) -> bool {
        self.0.ptr_eq(&other.0)
    }
}

#[doc(hidden)]
pub fn finish_loader_frame(scene: &Node) -> bool {
    let mut advanced = false;
    if let Some(gate) = scene
        .loader_frame
        .as_ref()
        .and_then(|frame| frame.0.upgrade())
        && gate.cached_value() != Some(true)
    {
        gate.set(true);
        advanced = true;
    }
    for (_, child) in scene.materialized_children() {
        advanced |= finish_loader_frame(child);
    }
    advanced
}

/// Identity and pending notification for an instance owned by a Loader.
#[doc(hidden)]
#[derive(Clone, Debug)]
pub struct LoaderMount {
    pending: Rc<Cell<bool>>,
    pub(crate) key: RowKey,
}

impl Default for LoaderMount {
    fn default() -> Self {
        static NEXT_MOUNT: AtomicU64 = AtomicU64::new(1);
        Self {
            pending: Rc::new(Cell::new(true)),
            key: RowKey {
                model: 0,
                row: NEXT_MOUNT.fetch_add(1, Ordering::Relaxed),
                scope: 0,
            },
        }
    }
}

impl PartialEq for LoaderMount {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

/// Dispatch mount notifications from a committed scene. A handler can change
/// the selected component, so let the host prepare a fresh scene on mutation.
/// Returns whether the entire scene was visited.
#[doc(hidden)]
pub fn dispatch_loader_events(ui: &mut (impl Component + ?Sized), scene: &Node) -> bool {
    fn walk(
        ui: &mut (impl Component + ?Sized),
        scene: &Node,
        node: &Node,
        path: &mut Vec<usize>,
    ) -> bool {
        if node.kind == "Loader"
            && node
                .children
                .iter()
                .find_map(|candidate| candidate.children.first()?.loader_mount.as_ref())
                .is_some_and(|mount| mount.pending.replace(false))
        {
            ui.dispatch_at(scene, path, "loaded");
            if ui.scene_dirty() {
                return false;
            }
        }
        if node.kind == "DynamicContainer" {
            for child in &node.children {
                if child
                    .loader_mount
                    .as_ref()
                    .is_some_and(|mount| mount.pending.replace(false))
                {
                    ui.dispatch_at(scene, path, "mounted");
                    if ui.scene_dirty() {
                        return false;
                    }
                }
            }
        }
        for (index, child) in node.materialized_children() {
            path.push(index);
            let visited = walk(ui, scene, child, path);
            path.pop();
            if !visited {
                return false;
            }
        }
        true
    }
    !ui.scene_dirty() && walk(ui, scene, scene, &mut Vec::new())
}

/// Keep the route to a Loader candidate while laying out its single child.
#[doc(hidden)]
pub fn loader_candidate_scene(children: Vec<Node>) -> Node {
    if children.is_empty() {
        return Node::hidden();
    }
    let mut node = Node::new("LoaderCandidate", None);
    node.children = children;
    node
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switching_candidates_releases_previous_storage() {
        let slot = LoaderSlot::default();
        slot.render(0, false, |_: &usize| {}, |_: &usize| Node::hidden());
        let previous = Rc::downgrade(&slot.get::<usize>(0).unwrap());

        slot.render(
            1,
            false,
            |_: &String| {},
            |_: &String| {
                assert!(previous.upgrade().is_none());
                Node::hidden()
            },
        );
        slot.clear(0);
        assert!(slot.get::<String>(1).is_some());
        slot.clear(1);
        assert!(slot.get::<String>(1).is_none());
    }

    #[test]
    fn cancel_releases_partial_storage_and_discards_remaining_jobs() {
        let slot = LoaderSlot::default();
        let partial = RefCell::new(Weak::new());
        let advance = || {
            slot.render(
                0,
                true,
                |state: &RefCell<Option<Rc<()>>>| {
                    let marker = Rc::new(());
                    partial.replace(Rc::downgrade(&marker));
                    state.replace(Some(marker));
                    let build = LoaderBuild::current().unwrap();
                    for _ in 0..32 {
                        build.enqueue(|| Ok(()));
                    }
                },
                |_| panic!("published partial candidate"),
            )
        };

        let waiting = advance();
        assert!(finish_loader_frame(&waiting));
        let building = advance();
        assert!(partial.borrow().upgrade().is_some());
        assert!(!slot.is_mounted(0));
        assert!(!finish_loader_frame(&waiting));
        slot.clear(0);
        assert!(partial.borrow().upgrade().is_none());
        assert!(!finish_loader_frame(&building));
    }

    #[test]
    fn superseded_frame_cannot_release_new_candidate() {
        let slot = LoaderSlot::default();
        let old = slot.render(0, true, |_: &usize| {}, |_: &usize| panic!("old mount"));
        let new = slot.render(1, true, |_: &usize| {}, |_: &usize| panic!("early mount"));
        assert!(!finish_loader_frame(&old));
        assert!(!slot.is_mounted(1));
        assert!(finish_loader_frame(&new));
        assert!(!slot.is_mounted(1));
        slot.render(1, true, |_: &usize| {}, |_: &usize| Node::hidden());
        assert!(slot.is_mounted(1));
    }
}
