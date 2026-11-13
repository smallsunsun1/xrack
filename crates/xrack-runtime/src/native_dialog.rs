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

impl FileDialogOptions {
    #[doc(hidden)]
    pub fn read_bindings(
        mode: impl FnOnce() -> FileDialogMode,
        title: impl FnOnce() -> String,
        directory: impl FnOnce() -> String,
        file_name: impl FnOnce() -> String,
        name_filters: impl FnOnce() -> Vec<String>,
    ) -> Self {
        Self::try_read_bindings(
            || Ok::<_, std::convert::Infallible>(mode()),
            || Ok(title()),
            || Ok(directory()),
            || Ok(file_name()),
            || Ok(name_filters()),
        )
        .unwrap_or_else(|error| match error {})
    }

    #[doc(hidden)]
    pub fn try_read_bindings<E>(
        mode: impl FnOnce() -> Result<FileDialogMode, E>,
        title: impl FnOnce() -> Result<String, E>,
        directory: impl FnOnce() -> Result<String, E>,
        file_name: impl FnOnce() -> Result<String, E>,
        name_filters: impl FnOnce() -> Result<Vec<String>, E>,
    ) -> Result<Self, E> {
        Ok(Self {
            mode: mode()?,
            title: title()?,
            directory: directory()?,
            file_name: file_name()?,
            name_filters: name_filters()?,
        })
    }

    #[doc(hidden)]
    pub fn filters(&self) -> Result<Vec<(&str, Vec<&str>)>, &'static str> {
        if matches!(
            self.mode,
            FileDialogMode::SelectFolder | FileDialogMode::SelectFolders
        ) {
            return Ok(Vec::new());
        }
        self.name_filters
            .iter()
            .map(|filter| parse_filter(filter))
            .collect::<Option<_>>()
            .ok_or("Invalid name filter. Use a label and extension patterns, for example Text files (*.txt *.md).")
    }
}

fn parse_filter(filter: &str) -> Option<(&str, Vec<&str>)> {
    let (label, patterns) = filter.trim().strip_suffix(')')?.rsplit_once('(')?;
    let label = label.trim();
    let extensions = patterns
        .split_whitespace()
        .map(|pattern| {
            if pattern == "*" || pattern == "*.*" {
                return Some("*");
            }
            let extension = pattern.strip_prefix("*.")?;
            (!extension.is_empty()
                && extension
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')))
            .then_some(extension)
        })
        .collect::<Option<Vec<_>>>()?;
    (!label.is_empty() && !extensions.is_empty()).then_some((label, extensions))
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

impl MessageDialogOptions {
    #[doc(hidden)]
    pub fn read_bindings(
        level: impl FnOnce() -> MessageLevel,
        buttons: impl FnOnce() -> MessageButtons,
        title: impl FnOnce() -> String,
        text: impl FnOnce() -> String,
        informative_text: impl FnOnce() -> String,
    ) -> Self {
        Self::try_read_bindings(
            || Ok::<_, std::convert::Infallible>(level()),
            || Ok(buttons()),
            || Ok(title()),
            || Ok(text()),
            || Ok(informative_text()),
        )
        .unwrap_or_else(|error| match error {})
    }

    #[doc(hidden)]
    pub fn try_read_bindings<E>(
        level: impl FnOnce() -> Result<MessageLevel, E>,
        buttons: impl FnOnce() -> Result<MessageButtons, E>,
        title: impl FnOnce() -> Result<String, E>,
        text: impl FnOnce() -> Result<String, E>,
        informative_text: impl FnOnce() -> Result<String, E>,
    ) -> Result<Self, E> {
        Ok(Self {
            level: level()?,
            buttons: buttons()?,
            title: title()?,
            text: text()?,
            informative_text: informative_text()?,
        })
    }
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

impl NativeDialogOptions {
    fn accepts_result(&self, result: &NativeDialogResult) -> bool {
        match result {
            NativeDialogResult::Files(_) | NativeDialogResult::Canceled => {
                matches!(self, Self::File(_))
            }
            NativeDialogResult::Button(_) => matches!(self, Self::Message(_)),
            NativeDialogResult::Failed(_) | NativeDialogResult::Abandoned => true,
        }
    }
}

#[doc(hidden)]
pub enum NativeDialogEvent {
    FilesSelected(VecModel<String>),
    FileCanceled,
    MessageAccepted,
    MessageRejected,
    MessageFinished(StandardButtonKind),
    Failed(String),
}

#[derive(Clone, Debug)]
struct Request {
    id: u64,
    options: Rc<NativeDialogOptions>,
}

/// The component owns the pending request. Render snapshots only register it with the host.
#[doc(hidden)]
#[derive(Default)]
pub struct NativeDialogSlot {
    request: Property<Option<Request>>,
    opening: Cell<bool>,
}

struct OpeningGuard<'a>(&'a Cell<bool>);

impl Drop for OpeningGuard<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

impl NativeDialogSlot {
    pub fn pending(&self) -> bool {
        self.request.get(|| None).is_some()
    }

    pub fn open(&self, options: impl FnOnce() -> NativeDialogOptions) {
        let _: Result<(), std::convert::Infallible> = self.try_open(|| Ok(options()));
    }

    pub fn try_open<E>(
        &self,
        options: impl FnOnce() -> Result<NativeDialogOptions, E>,
    ) -> Result<(), E> {
        self.try_open_if(|| Ok(true), options)
    }

    #[doc(hidden)]
    pub fn open_if(
        &self,
        enabled: impl FnOnce() -> bool,
        options: impl FnOnce() -> NativeDialogOptions,
    ) {
        let _: Result<(), std::convert::Infallible> =
            self.try_open_if(|| Ok(enabled()), || Ok(options()));
    }

    /// Prevent reentry and publish only while the permission and options remain current.
    #[doc(hidden)]
    pub fn try_open_if<E>(
        &self,
        enabled: impl FnOnce() -> Result<bool, E>,
        options: impl FnOnce() -> Result<NativeDialogOptions, E>,
    ) -> Result<(), E> {
        if self.opening.replace(true) {
            return Ok(());
        }
        let _opening = OpeningGuard(&self.opening);
        let request = crate::property::watch_reads(|reads| {
            if self.pending() || !enabled()? || reads.changed() {
                return Ok(None);
            }
            static NEXT: AtomicU64 = AtomicU64::new(1);
            let id = NEXT
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .expect("native dialog request identities exhausted");
            let options = options()?;
            Ok((!reads.changed()).then(|| Request {
                id,
                options: Rc::new(options),
            }))
        })?;
        if let Some(request) = request {
            self.request.set(Some(request));
        }
        Ok(())
    }

    pub fn register(
        &self,
        deliver: impl Fn(u64, NativeDialogResult) -> bool + 'static,
    ) -> Option<NativeDialogRegistration> {
        Some(NativeDialogRegistration {
            request: self.request.get(|| None)?,
            deliver: Rc::new(deliver),
        })
    }

    #[doc(hidden)]
    pub fn complete(
        &self,
        id: u64,
        result: NativeDialogResult,
        selection: Option<&FileDialogSelection>,
        mut notify: impl FnMut(NativeDialogEvent),
    ) -> bool {
        self.try_complete(id, result, selection, |event| {
            notify(event);
            Ok::<_, std::convert::Infallible>(())
        })
        .unwrap_or_else(|error| match error {})
    }

    /// Clear the pending request and commit the selection before notifying handlers.
    #[doc(hidden)]
    pub fn try_complete<E>(
        &self,
        id: u64,
        result: NativeDialogResult,
        selection: Option<&FileDialogSelection>,
        mut notify: impl FnMut(NativeDialogEvent) -> Result<(), E>,
    ) -> Result<bool, E> {
        let Some(request) = self.request.get(|| None).filter(|request| request.id == id) else {
            return Ok(false);
        };
        if !request.options.accepts_result(&result) {
            return Ok(false);
        }
        let selection_matches = match &result {
            NativeDialogResult::Files(_) | NativeDialogResult::Canceled => selection.is_some(),
            NativeDialogResult::Button(_) => selection.is_none(),
            NativeDialogResult::Failed(_) | NativeDialogResult::Abandoned => true,
        };
        if !selection_matches {
            return Ok(false);
        }
        self.request.set(None);
        match result {
            NativeDialogResult::Files(paths) => {
                let selection = selection.expect("validated file dialog selection");
                selection.commit(&paths);
                notify(NativeDialogEvent::FilesSelected(VecModel::from(paths)))?;
            }
            NativeDialogResult::Canceled => {
                notify(NativeDialogEvent::FileCanceled)?;
            }
            NativeDialogResult::Button(button) => {
                notify(if button.accepts_message() {
                    NativeDialogEvent::MessageAccepted
                } else {
                    NativeDialogEvent::MessageRejected
                })?;
                notify(NativeDialogEvent::MessageFinished(button))?;
            }
            NativeDialogResult::Failed(message) => notify(NativeDialogEvent::Failed(message))?,
            NativeDialogResult::Abandoned => {}
        }
        Ok(true)
    }

    pub fn finish(&self, id: u64) -> bool {
        if self
            .request
            .get(|| None)
            .is_none_or(|request| request.id != id)
        {
            return false;
        }
        self.request.set(None);
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
        let Some(active) = self.active.take_if(|active| {
            active.registration.id() == id
                && (active.abandoned.get()
                    || active.registration.request.options.accepts_result(&result))
        }) else {
            return false;
        };
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
    use super::{
        FileDialogOptions, FileDialogSelection, NativeDialogOptions, NativeDialogSlot, parse_filter,
    };
    use std::panic::{AssertUnwindSafe, catch_unwind};

    #[test]
    fn failed_open_bindings_release_the_slot() {
        let slot = NativeDialogSlot::default();
        assert_eq!(
            slot.try_open(|| Err("invalid options")),
            Err("invalid options")
        );
        assert!(
            catch_unwind(AssertUnwindSafe(
                || slot.open(|| panic!("option callback failed"))
            ))
            .is_err()
        );
        assert!(!slot.pending());

        slot.open(|| NativeDialogOptions::File(FileDialogOptions::default()));
        assert!(slot.pending());
    }

    #[test]
    fn name_filters_preserve_labels_and_validate_extensions() {
        assert_eq!(
            parse_filter("Text (source) (*.txt *.md)"),
            Some(("Text (source)", vec!["txt", "md"]))
        );
        assert_eq!(
            parse_filter("All files (*.*)"),
            Some(("All files", vec!["*"]))
        );
        for invalid in [
            "*.txt",
            "Text ()",
            "(*.txt)",
            "Text (file*.txt)",
            "Text (*../x)",
        ] {
            assert_eq!(parse_filter(invalid), None);
        }
    }

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
