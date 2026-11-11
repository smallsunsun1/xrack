use crate::{DialogRole, Node, Property};
use std::cell::{Cell, RefCell};

const SESSION: &str = "__picker-session";

thread_local! {
    static NEXT_SESSION: Cell<i64> = const { Cell::new(1) };
}

/// One visible modal session. Old snapshots cannot edit or finish a later session.
pub(crate) struct PickerSession<T> {
    initial: RefCell<Option<(i64, T)>>,
    draft: Property<Option<T>>,
}

impl<T> Default for PickerSession<T> {
    fn default() -> Self {
        Self {
            initial: RefCell::new(None),
            draft: Property::default(),
        }
    }
}

impl<T: Clone> PickerSession<T> {
    pub(crate) fn close(&self) {
        self.initial.take();
        self.draft.reset();
    }

    pub(crate) fn begin(&self, dialog: &mut Node, initial: impl FnOnce() -> T) -> Option<T> {
        if !dialog.is_visible() {
            self.close();
            return None;
        }
        let session = self.initial.borrow().clone();
        let (revision, initial) = session.unwrap_or_else(|| {
            let revision = NEXT_SESSION.with(|next| {
                let current = next.get();
                next.set(current.checked_add(1).expect("picker sessions exhausted"));
                current
            });
            let session = (revision, initial());
            self.initial.replace(Some(session.clone()));
            session
        });
        dialog.properties.insert(SESSION, revision.into());
        Some(self.draft.get(|| None).unwrap_or(initial))
    }

    pub(crate) fn stamp(&self, child: &mut Node) {
        if let Some((revision, _)) = self.initial.borrow().as_ref() {
            child.properties.insert(SESSION, (*revision).into());
        }
    }

    pub(crate) fn current(&self, scene: &Node) -> Option<T> {
        let initial = self.initial.borrow();
        let (revision, value) = initial.as_ref()?;
        (scene.int(SESSION) == Some(*revision))
            .then(|| self.draft.get(|| None).unwrap_or_else(|| value.clone()))
    }

    pub(crate) fn set(&self, draft: T) {
        self.draft.set(Some(draft));
    }

    pub(crate) fn finish(&self, dialog: &Node) -> Option<T> {
        let value = self.current(dialog)?;
        self.close();
        Some(value)
    }
}

pub(crate) fn append_picker_dialog(dialog: &mut Node, picker: Node) {
    let mut footer = Node::new("DialogButtonBox", None);
    for (role, property) in [
        (DialogRole::Reject, "cancel-text"),
        (DialogRole::Accept, "accept-text"),
    ] {
        let mut button = Node::new("DialogButton", None);
        button.properties.insert("role", role.into());
        if let Some(text) = dialog.text(property) {
            button.properties.insert("text", text.to_owned().into());
        }
        footer.children.push(button);
    }
    dialog.children.extend([picker, footer]);
}
