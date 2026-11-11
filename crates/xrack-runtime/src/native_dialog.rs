use crate::{Property, StandardButtonKind, VecModel};
use std::{
    cell::Cell,
    collections::VecDeque,
    fmt,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FileDialogMode {
    #[default]
    OpenFile,
    OpenFiles,
    SaveFile,
    SelectFolder,
    SelectFolders,
}

#[derive(Debug, Default)]
pub struct FileDialogOptions {
    pub mode: FileDialogMode,
    pub title: String,
    pub directory: String,
    pub file_name: String,
    pub name_filters: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MessageLevel {
    #[default]
    Info,
    Warning,
    Error,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MessageButtons {
    #[default]
    Ok,
    OkCancel,
    YesNo,
    YesNoCancel,
}

#[derive(Debug, Default)]
pub struct MessageDialogOptions {
    pub level: MessageLevel,
    pub buttons: MessageButtons,
    pub title: String,
    pub text: String,
    pub informative_text: String,
}

#[doc(hidden)]
#[derive(Debug)]
pub enum NativeDialogOptions {
    File(FileDialogOptions),
    Message(MessageDialogOptions),
}

#[doc(hidden)]
#[derive(Debug, PartialEq, Eq)]
pub enum NativeDialogResult {
    Files(Vec<String>),
    Button(StandardButtonKind),
    Canceled,
    Failed(String),
    Abandoned,
}

#[derive(Clone, Debug)]
struct Request {
    id: u64,
    options: Rc<NativeDialogOptions>,
}

/// The component owns the pending request. Render snapshots only register it with the host.
#[doc(hidden)]
#[derive(Default)]
pub struct NativeDialogSlot(Property<Option<Request>>);

impl NativeDialogSlot {
    pub fn pending(&self) -> bool {
        self.0.get(|| None).is_some()
    }

    pub fn open(&self, options: impl FnOnce() -> NativeDialogOptions) {
        if self.pending() {
            return;
        }
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let id = NEXT
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .expect("native dialog request identities exhausted");
        self.0.set(Some(Request {
            id,
            options: Rc::new(options()),
        }));
    }

    pub fn register(
        &self,
        deliver: impl Fn(u64, NativeDialogResult) -> bool + 'static,
    ) -> Option<NativeDialogRegistration> {
        Some(NativeDialogRegistration {
            request: self.0.get(|| None)?,
            deliver: Rc::new(deliver),
        })
    }

    pub fn finish(&self, id: u64) -> bool {
        if self.0.get(|| None).is_none_or(|request| request.id != id) {
            return false;
        }
        self.0.set(None);
        true
    }
}

/// The last accepted result, independent of mutable lists handed to callbacks.
#[doc(hidden)]
#[derive(Default)]
pub struct FileDialogSelection(Property<Rc<[String]>>);

impl FileDialogSelection {
    pub fn commit(&self, paths: &[String]) {
        self.0.set_if_changed(Rc::from(paths));
    }

    pub fn selected_file(&self) -> String {
        self.0.get(Rc::default).first().cloned().unwrap_or_default()
    }

    pub fn selected_files(&self) -> VecModel<String> {
        VecModel::from(self.0.get(Rc::default).to_vec())
    }
}

#[doc(hidden)]
#[derive(Clone)]
pub struct NativeDialogRegistration {
    request: Request,
    deliver: Rc<dyn Fn(u64, NativeDialogResult) -> bool>,
}

impl NativeDialogRegistration {
    pub fn id(&self) -> u64 {
        self.request.id
    }

    pub fn options(&self) -> Rc<NativeDialogOptions> {
        self.request.options.clone()
    }

    pub fn deliver(self, result: NativeDialogResult) -> bool {
        (self.deliver)(self.request.id, result)
    }
}

impl fmt::Debug for NativeDialogRegistration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeDialogRegistration")
            .field("id", &self.id())
            .finish()
    }
}

impl PartialEq for NativeDialogRegistration {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}

struct ActiveDialog {
    registration: NativeDialogRegistration,
    abandoned: Rc<Cell<bool>>,
}

#[doc(hidden)]
pub struct NativeDialogPresentation {
    pub id: u64,
    pub options: Rc<NativeDialogOptions>,
    abandoned: Rc<Cell<bool>>,
}

impl NativeDialogPresentation {
    pub fn is_abandoned(&self) -> bool {
        self.abandoned.get()
    }
}

/// Serializes native panels and keeps delayed results tied to their original request.
#[doc(hidden)]
#[derive(Default)]
pub struct NativeDialogQueue {
    waiting: VecDeque<NativeDialogRegistration>,
    active: Option<ActiveDialog>,
}

impl NativeDialogQueue {
    pub fn close(&mut self) {
        self.sync(&[]);
        if let Some(active) = self.active.take() {
            active.registration.deliver(NativeDialogResult::Abandoned);
        }
    }

    pub fn sync(&mut self, live: &[&NativeDialogRegistration]) {
        let is_live = |id| live.iter().any(|entry| entry.id() == id);
        if let Some(active) = &self.active
            && !is_live(active.registration.id())
        {
            active.abandoned.set(true);
        }
        self.waiting.retain(|entry| {
            if !is_live(entry.id()) {
                entry.clone().deliver(NativeDialogResult::Abandoned);
                return false;
            }
            true
        });
        let queued = self.waiting.len();
        for entry in live {
            if self
                .active
                .as_ref()
                .is_some_and(|active| active.registration.id() == entry.id())
                || self.waiting.iter().any(|queued| queued.id() == entry.id())
            {
                continue;
            }
            self.waiting.push_back((*entry).clone());
        }
        if self.waiting.len() > queued {
            // Registration traversal follows scene order, which can differ from request order.
            self.waiting
                .make_contiguous()
                .sort_by_key(NativeDialogRegistration::id);
        }
    }

    pub fn begin_next(&mut self) -> Option<NativeDialogPresentation> {
        if self.active.is_some() {
            return None;
        }
        let registration = self.waiting.pop_front()?;
        let abandoned = Rc::new(Cell::new(false));
        let request = NativeDialogPresentation {
            id: registration.id(),
            options: registration.options(),
            abandoned: abandoned.clone(),
        };
        self.active = Some(ActiveDialog {
            registration,
            abandoned,
        });
        Some(request)
    }

    pub fn complete(&mut self, id: u64, result: NativeDialogResult) -> bool {
        if self
            .active
            .as_ref()
            .is_none_or(|active| active.registration.id() != id)
        {
            return false;
        }
        let active = self.active.take().expect("matching active native dialog");
        active.registration.deliver(if active.abandoned.get() {
            NativeDialogResult::Abandoned
        } else {
            result
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::FileDialogSelection;

    #[test]
    fn accepted_paths_are_independent_of_the_returned_model() {
        let selection = FileDialogSelection::default();
        assert!(selection.selected_file().is_empty());
        assert!(selection.selected_files().is_empty());
        selection.commit(&["/workspace/src".into(), "/workspace/tests".into()]);
        let model = selection.selected_files();
        assert!(model.set_row(0, "/modified".into()));
        assert_eq!(selection.selected_file(), "/workspace/src");
        assert_eq!(
            selection.selected_files().snapshot(),
            ["/workspace/src", "/workspace/tests"]
        );
    }
}
