use crate::{DialogRole, InputRoute, Node, Property};
use std::cell::{Cell, RefCell};

const SESSION: &str = "__picker-session";
const DRAFT_REVISION: &str = "__picker-draft-revision";

thread_local! {
    static NEXT_SESSION: Cell<i64> = const { Cell::new(1) };
}

/// One visible modal session. Old snapshots cannot edit or finish a later session.
pub(crate) struct PickerSession<T> {
    initial: RefCell<Option<(i64, T)>>,
    draft: Property<Option<T>>,
    draft_revision: Cell<i64>,
}

impl<T> Default for PickerSession<T> {
    fn default() -> Self {
        Self {
            initial: RefCell::new(None),
            draft: Property::default(),
            draft_revision: Cell::new(0),
        }
    }
}

impl<T: Clone> PickerSession<T> {
    pub(crate) fn close(&self) {
        self.initial.take();
        self.draft_revision.set(0);
        self.draft.reset();
    }

    pub(crate) fn try_begin<E>(
        &self,
        dialog: &mut Node,
        initial: impl FnOnce() -> Result<T, E>,
    ) -> Result<Option<T>, E> {
        if !dialog.is_visible() {
            self.close();
            return Ok(None);
        }
        let session = self.initial.borrow().clone();
        let (revision, initial) = if let Some(session) = session {
            session
        } else {
            let revision = NEXT_SESSION.with(|next| {
                let current = next.get();
                next.set(current.checked_add(1).expect("picker sessions exhausted"));
                current
            });
            let session = (revision, initial()?);
            self.initial.replace(Some(session.clone()));
            session
        };
        dialog.properties.insert(SESSION, revision.into());
        dialog
            .properties
            .insert(DRAFT_REVISION, self.draft_revision.get().into());
        Ok(Some(self.draft.get(|| None).unwrap_or(initial)))
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

    pub(crate) fn accepts_input(&self, scene: &Node, input: InputRoute<'_>) -> bool {
        input.is_current()
            && self
                .initial
                .borrow()
                .as_ref()
                .is_some_and(|(revision, _)| scene.int(SESSION) == Some(*revision))
    }

    pub(crate) fn set(&self, draft: T) {
        self.draft_revision.set(
            self.draft_revision
                .get()
                .checked_add(1)
                .expect("picker draft revisions exhausted"),
        );
        self.draft.set(Some(draft));
    }

    pub(crate) fn try_finish_input<E>(
        &self,
        dialog: &Node,
        role: DialogRole,
        input: InputRoute<'_>,
        accept: impl FnOnce(T) -> Result<Option<T>, E>,
    ) -> Result<Option<T>, E> {
        if !self.accepts_input(dialog, input) {
            return Ok(None);
        }
        if role == DialogRole::Reject {
            return Ok(self.finish(dialog, role));
        }
        if dialog.int(DRAFT_REVISION) != Some(self.draft_revision.get()) {
            return Ok(None);
        }
        let Some(draft) = self.current(dialog) else {
            return Ok(None);
        };
        let Some(value) = accept(draft)? else {
            return Ok(None);
        };
        // Bindings sampled by confirmation may invalidate the route or change the session.
        if !self.accepts_input(dialog, input)
            || dialog.int(DRAFT_REVISION) != Some(self.draft_revision.get())
        {
            return Ok(None);
        }
        self.close();
        Ok(Some(value))
    }

    pub(crate) fn finish(&self, dialog: &Node, role: DialogRole) -> Option<T> {
        if role == DialogRole::Accept
            && dialog.int(DRAFT_REVISION) != Some(self.draft_revision.get())
        {
            return None;
        }
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
