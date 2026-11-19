use crate::{Property, StandardButtonKind, VecModel};
use std::{
    cell::Cell,
    collections::VecDeque,
    fmt,
    future::poll_fn,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    task::{Poll, Waker},
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
    #[expect(
        clippy::expect_used,
        reason = "native dialog request IDs must stay unique even if the counter reaches its limit"
    )]
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
                if let Some(selection) = selection {
                    selection.commit(&paths);
                }
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
    abandonment: Rc<DialogAbandonment>,
}

#[derive(Default)]
struct DialogAbandonment {
    abandoned: Cell<bool>,
    waiter: Cell<Option<Waker>>,
}

impl DialogAbandonment {
    fn abandon(&self) {
        if !self.abandoned.replace(true)
            && let Some(waiter) = self.waiter.take()
        {
            waiter.wake();
        }
    }
}

#[doc(hidden)]
pub struct NativeDialogPresentation {
    pub id: u64,
    pub options: Rc<NativeDialogOptions>,
    abandonment: Rc<DialogAbandonment>,
}

impl NativeDialogPresentation {
    pub fn is_abandoned(&self) -> bool {
        self.abandonment.abandoned.get()
    }

    /// Wait until the request's owning component leaves the active scene.
    /// A presentation has one asynchronous waiter on the UI thread.
    pub async fn wait_until_abandoned(&self) {
        poll_fn(|context| {
            if self.is_abandoned() {
                Poll::Ready(())
            } else {
                self.abandonment.waiter.set(Some(context.waker().clone()));
                Poll::Pending
            }
        })
        .await
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
            active.abandonment.abandon();
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
        let abandonment = Rc::new(DialogAbandonment::default());
        let request = NativeDialogPresentation {
            id: registration.id(),
            options: registration.options(),
            abandonment: abandonment.clone(),
        };
        self.active = Some(ActiveDialog {
            registration,
            abandonment,
        });
        Some(request)
    }

    pub fn is_active_request_live(&self, id: u64) -> bool {
        self.active.as_ref().is_some_and(|active| {
            active.registration.id() == id && !active.abandonment.abandoned.get()
        })
    }

    pub fn complete(&mut self, id: u64, result: NativeDialogResult) -> bool {
        let Some(active) = self.active.take_if(|active| {
            active.registration.id() == id
                && (active.abandonment.abandoned.get()
                    || active.registration.request.options.accepts_result(&result))
        }) else {
            return false;
        };
        active
            .registration
            .deliver(if active.abandonment.abandoned.get() {
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
    fn abandoning_an_active_request_wakes_its_presenter_before_advancing_the_queue() {
        use super::{NativeDialogQueue, NativeDialogResult};
        use std::{
            cell::RefCell,
            pin::pin,
            rc::Rc,
            sync::{
                Arc,
                atomic::{AtomicUsize, Ordering},
            },
            task::{Context, Wake, Waker},
        };

        #[derive(Default)]
        struct WakeCount(AtomicUsize);
        impl Wake for WakeCount {
            fn wake(self: Arc<Self>) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }

        let result = Rc::new(RefCell::new(None));
        let first = NativeDialogSlot::default();
        first.open(|| NativeDialogOptions::File(FileDialogOptions::default()));
        let first = first
            .register({
                let result = result.clone();
                move |_, value| {
                    result.replace(Some(value));
                    true
                }
            })
            .unwrap();
        let next = NativeDialogSlot::default();
        next.open(|| NativeDialogOptions::File(FileDialogOptions::default()));
        let next = next.register(|_, _| true).unwrap();
        let mut queue = NativeDialogQueue::default();
        queue.sync(&[&first, &next]);
        let active = queue.begin_next().unwrap();
        let mut abandoned = pin!(active.wait_until_abandoned());
        let wakes = Arc::new(WakeCount::default());
        let waker = Waker::from(wakes.clone());
        let mut context = Context::from_waker(&waker);

        assert!(abandoned.as_mut().poll(&mut context).is_pending());
        queue.sync(&[&next]);
        queue.sync(&[&next]);
        assert_eq!(wakes.0.load(Ordering::Relaxed), 1);
        assert!(abandoned.as_mut().poll(&mut context).is_ready());
        assert!(queue.begin_next().is_none());

        assert!(queue.complete(active.id, NativeDialogResult::Files(vec!["old.txt".into()])));
        assert_eq!(
            result.borrow().as_ref(),
            Some(&NativeDialogResult::Abandoned)
        );
        assert_eq!(queue.begin_next().unwrap().id, next.id());
    }

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
