use crate::{
    ChangeSignal, Component, ExternalComponent, ExternalLoader, LoadedExternal, LoaderMount,
    LoaderSlot, Node, Property, WeakComponent,
};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::atomic::{AtomicI64, Ordering},
};

type Mounted = Rc<RefCell<LoadedExternal>>;
type DeferredValue = RefCell<Option<Mounted>>;
type Factory = Box<dyn FnOnce() -> Result<LoadedExternal, String>>;

enum Content {
    Ready(Mounted),
    Deferred {
        factory: RefCell<Option<Factory>>,
        slot: LoaderSlot,
        failed: Property<bool>,
    },
    External {
        loader: ExternalLoader,
        source: String,
    },
}

struct Child {
    id: i64,
    content: Content,
    mount: LoaderMount,
}

impl Child {
    fn status(&self) -> &'static str {
        match &self.content {
            Content::Ready(_) => "ready",
            Content::Deferred { slot, failed, .. } => {
                if failed.get(|| false) {
                    "error"
                } else if slot.is_mounted(0) {
                    "ready"
                } else {
                    "loading"
                }
            }
            Content::External { loader, source } => match loader.status(source).as_str() {
                "ready" => "ready",
                "error" => "error",
                "null" => "null",
                _ => "loading",
            },
        }
    }

    fn get<T: 'static>(&self) -> Option<T> {
        let component = match &self.content {
            Content::Ready(component) => component.clone(),
            Content::Deferred { slot, .. } => {
                slot.is_mounted(0);
                slot.get::<DeferredValue>(0)?.borrow().as_ref()?.clone()
            }
            Content::External { loader, .. } => return loader.item::<T>()?.upgrade(),
        };
        component
            .try_borrow()
            .ok()?
            .item
            .downcast_ref::<WeakComponent<T>>()?
            .upgrade()
    }

    fn render_component(&self, component: &Mounted) -> Node {
        let mut node = component.borrow().ui.render();
        node.set_loader_mount(self.mount.clone());
        node.set_external_component(ExternalComponent(Rc::downgrade(component)));
        node
    }

    fn render(&self) -> Node {
        match &self.content {
            Content::Ready(component) => self.render_component(component),
            Content::Deferred {
                factory,
                slot,
                failed,
            } => {
                if failed.get(|| false) {
                    return Node::hidden();
                }
                let rendered = slot.try_render::<DeferredValue, String>(
                    0,
                    true,
                    |value| {
                        let factory = factory
                            .take()
                            .ok_or("missing dynamic component constructor")?;
                        value.replace(Some(Rc::new(RefCell::new(factory()?))));
                        Ok(())
                    },
                    |value| {
                        let value = value.borrow();
                        let component = value.as_ref().ok_or("incomplete dynamic component")?;
                        Ok(self.render_component(component))
                    },
                    |error| error,
                );
                match rendered {
                    Ok(node) => node,
                    Err(error) => {
                        failed.set_if_changed(true);
                        let mut node = Node::hidden();
                        node.properties.insert("load-error", error.into());
                        node
                    }
                }
            }
            Content::External { loader, source } => {
                let node = loader.render(source);
                if node.kind == "LoaderCandidate" {
                    node.children.first().cloned().unwrap_or_else(Node::hidden)
                } else {
                    node
                }
            }
        }
    }
}

/// Owns the mounted children of a DynamicContainer.
#[doc(hidden)]
#[derive(Default)]
pub struct DynamicChildren {
    children: RefCell<Vec<Rc<Child>>>,
    changes: ChangeSignal,
}

impl DynamicChildren {
    fn insert(&self, content: Content) -> i64 {
        static NEXT_ID: AtomicI64 = AtomicI64::new(1);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        self.children.borrow_mut().push(Rc::new(Child {
            id,
            content,
            mount: LoaderMount::default(),
        }));
        self.changes.notify();
        id
    }

    pub fn mount<T: Component + 'static>(&self, ui: T, weak: WeakComponent<T>) -> i64 {
        self.insert(Content::Ready(Rc::new(RefCell::new(LoadedExternal::new(
            ui, weak,
        )))))
    }

    pub fn prepare(
        &self,
        factory: impl FnOnce() -> Result<LoadedExternal, String> + 'static,
    ) -> i64 {
        self.insert(Content::Deferred {
            factory: RefCell::new(Some(Box::new(factory))),
            slot: LoaderSlot::default(),
            failed: Property::default(),
        })
    }

    pub fn mount_url(&self, source: String) -> i64 {
        self.insert(Content::External {
            loader: ExternalLoader::default(),
            source,
        })
    }

    fn child(&self, id: i64) -> Option<Rc<Child>> {
        self.changes.track();
        self.children
            .borrow()
            .iter()
            .find(|child| child.id == id)
            .cloned()
    }

    pub fn get<T: 'static>(&self, id: i64) -> Option<T> {
        self.child(id)?.get()
    }

    pub fn contains(&self, id: i64) -> bool {
        self.status(id) == "ready"
    }

    pub fn status(&self, id: i64) -> &'static str {
        self.child(id).map_or("null", |child| child.status())
    }

    pub fn remove(&self, id: i64) -> bool {
        let mut children = self.children.borrow_mut();
        let Some(index) = children.iter().position(|child| child.id == id) else {
            return false;
        };
        let removed = children.remove(index);
        drop(children);
        self.changes.notify();
        drop(removed);
        true
    }

    pub fn move_to(&self, id: i64, index: i64) -> bool {
        let Ok(index) = usize::try_from(index) else {
            return false;
        };
        let mut children = self.children.borrow_mut();
        if index >= children.len() {
            return false;
        }
        let Some(previous) = children.iter().position(|child| child.id == id) else {
            return false;
        };
        if previous != index {
            let child = children.remove(previous);
            children.insert(index, child);
            self.changes.notify();
        }
        true
    }

    pub fn render(&self) -> Vec<Node> {
        self.changes.track();
        let children = self.children.borrow().clone();
        children.iter().map(|child| child.render()).collect()
    }
}
