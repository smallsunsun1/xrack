use super::text::CopyRevision;
use crate::{Application, TOUCH_HOLD_SLOP, UserEvent, desktop::WindowEvents};
use block2::RcBlock;
use objc2::{
    ClassType, DeclaredClass, declare_class, msg_send, msg_send_id,
    mutability::MainThreadOnly,
    rc::Retained,
    runtime::{NSObjectProtocol, ProtocolObject},
    sel,
};
use objc2_foundation::{
    CGPoint, CGRect, CGSize, MainThreadMarker, NSArray, NSError, NSItemProvider,
    NSItemProviderFileOptions, NSItemProviderReading, NSObject, NSProgress, NSSecureCoding,
    NSString, NSURL,
};
use objc2_ui_kit::{
    NSItemProviderUIKitAdditions, UIColor, UIDragDropSession, UIDragInteraction,
    UIDragInteractionDelegate, UIDragItem, UIDragPreview, UIDragSession, UIDropInteraction,
    UIDropInteractionDelegate, UIDropOperation, UIDropProposal, UIDropSession, UIGestureRecognizer,
    UIGestureRecognizerDelegate, UIGestureRecognizerState, UIInteraction, UILabel,
    UILongPressGestureRecognizer, UIPreferredPresentationStyle, UITouch, UITouchType, UIView,
};
use objc2_uniform_type_identifiers::{
    UTType, UTTypeData, UTTypeFileURL, UTTypePlainText, UTTypeURL,
};
use std::{
    cell::{Cell, RefCell},
    fs,
    path::{Component, Path},
    rc::Rc,
    sync::{Arc, Mutex},
};
use tempfile::TempDir;
use winit::window::{Window, WindowId};
use xrack_layout::{
    EditAction, EditSource, FileDragInput, FileDropScope, LayoutNode, Point, Rect, Target,
    TextDrop, TextEditContext,
};
use xrack_renderer_femtovg::PointerCaret;
use xrack_runtime::{Component as UiComponent, Node, TextCommand};

type Session = ProtocolObject<dyn UIDropSession>;
type DragSession = ProtocolObject<dyn UIDragSession>;

enum Payload {
    Text(Vec<Retained<NSItemProvider>>),
    Files(Vec<Representation>),
}

struct Snapshot {
    scene: Rc<Node>,
    layout: Rc<LayoutNode>,
    scope: FileDropScope,
}

pub(crate) struct TextDragSource {
    target: Target,
    context: TextEditContext,
    highlights: Vec<Rect>,
    native: Option<CopyRevision>,
}

impl TextDragSource {
    fn contains(&self, snapshot: &Snapshot, point: Point) -> bool {
        if self
            .native
            .as_ref()
            .is_some_and(|revision| !revision.is_current())
            || snapshot
                .scope
                .text_drag_source(&snapshot.scene, &snapshot.layout, point)
                .as_ref()
                != Some(&self.target)
        {
            return false;
        }
        snapshot
            .layout
            .at_path(&self.target.path)
            .is_some_and(|item| {
                item.contains_clip_point(point)
                    && item.map_from_window(point).is_some_and(|point| {
                        self.highlights.iter().any(|rect| rect.contains(point))
                    })
            })
    }
}

#[derive(Clone, Copy)]
struct Drag {
    session: usize,
    sequence: u64,
    performed: bool,
}

struct TextHold {
    origin: Point,
    target: Target,
    context: TextEditContext,
}

struct DelegateState {
    window: WindowId,
    events: WindowEvents,
    snapshot: RefCell<Option<Snapshot>>,
    text_source: RefCell<Option<TextDragSource>>,
    text_hold: RefCell<Option<TextHold>>,
    sequence: Cell<u64>,
    drag: Cell<Option<Drag>>,
}

declare_class!(
    struct FileDropDelegate;

    unsafe impl ClassType for FileDropDelegate {
        type Super = NSObject;
        type Mutability = MainThreadOnly;
        const NAME: &'static str = "XRackIOSFileDropDelegate";
    }

    impl DeclaredClass for FileDropDelegate {
        type Ivars = DelegateState;
    }

    unsafe impl NSObjectProtocol for FileDropDelegate {}

    unsafe impl UIGestureRecognizerDelegate for FileDropDelegate {
        #[method(gestureRecognizer:shouldReceiveTouch:)]
        fn receives_text_hold(&self, recognizer: &UIGestureRecognizer, touch: &UITouch) -> bool {
            // SAFETY: UIKit supplies the touch and its view on the main thread.
            unsafe { touch.r#type() == UITouchType::Direct && recognizer.view().is_some_and(|view| {
                self.with_text_source(view_point(&view, touch.locationInView(Some(&view))), |_| ()).is_some()
            }) }
        }

        #[method(gestureRecognizer:shouldRecognizeSimultaneouslyWithGestureRecognizer:)]
        fn allow_text_drag(&self, _: &UIGestureRecognizer, _: &UIGestureRecognizer) -> bool {
            true
        }
    }

    unsafe impl FileDropDelegate {
        #[method(xrackSelectedTextHeld:)]
        fn selected_text_held(&self, recognizer: &UILongPressGestureRecognizer) {
            self.update_text_hold(recognizer);
        }
    }

    unsafe impl UIDragInteractionDelegate for FileDropDelegate {
        #[method_id(dragInteraction:itemsForBeginningSession:)]
        fn start_text_drag(&self, interaction: &UIDragInteraction, session: &DragSession) -> Retained<NSArray<UIDragItem>> {
            self.drag_items(interaction, session)
        }

        #[method(dragInteraction:sessionAllowsMoveOperation:)]
        fn copy_only(&self, _: &UIDragInteraction, _: &DragSession) -> bool {
            false
        }

        #[method(dragInteraction:sessionWillBegin:)]
        fn text_drag_began(&self, _: &UIDragInteraction, _: &DragSession) {
            let state = self.ivars();
            state.text_hold.replace(None);
            state.events.post(UserEvent::IosTextDragStarted(state.window));
        }
    }

    unsafe impl UIDropInteractionDelegate for FileDropDelegate {
        #[method(dropInteraction:canHandleSession:)]
        fn can_handle(&self, _: &UIDropInteraction, session: &Session) -> bool {
            payload(session).is_some()
        }

        #[method_id(dropInteraction:sessionDidUpdate:)]
        fn did_update(&self, interaction: &UIDropInteraction, session: &Session) -> Retained<UIDropProposal> {
            let drag = self.drag(session);
            let accepted = !drag.performed && match self.offer(interaction, session) {
                Some((point, Payload::Files(files), _)) => self.post(FileDragInput::Offered {
                    sequence: drag.sequence, point, extensions: extensions(&files),
                }),
                offer => {
                    self.post(FileDragInput::Left { sequence: drag.sequence });
                    offer.is_some()
                }
            };
            // SAFETY: The delegate runs on UIKit's main thread.
            unsafe {
                UIDropProposal::initWithDropOperation(
                    MainThreadMarker::new_unchecked().alloc::<UIDropProposal>(),
                    if accepted { UIDropOperation::Copy } else { UIDropOperation::Cancel },
                )
            }
        }

        #[method(dropInteraction:performDrop:)]
        fn perform(&self, interaction: &UIDropInteraction, session: &Session) {
            let mut drag = self.drag(session);
            if drag.performed {
                return;
            }
            drag.performed = true;
            self.ivars().drag.set(Some(drag));
            self.post(FileDragInput::Left { sequence: drag.sequence });
            let Some((point, payload, target)) = self.offer(interaction, session) else {
                return;
            };
            let files = match payload {
                Payload::Text(providers) => {
                    self.load_text(drag.sequence, point, target, providers);
                    return;
                }
                Payload::Files(files) => files,
            };
            let extensions = extensions(&files);
            let Ok(directory) = tempfile::Builder::new().prefix("xrack-drop-").tempdir() else {
                self.post(FileDragInput::Failed {
                    sequence: drag.sequence,
                    point,
                    extensions,
                    target,
                    message: "Could not prepare storage for the dropped files.".into(),
                });
                return;
            };
            let batch = Arc::new(Batch {
                window: self.ivars().window,
                events: self.ivars().events.clone(),
                sequence: drag.sequence,
                point,
                target,
                extensions,
                directory: Arc::new(directory),
                pending: Mutex::new(Pending::new(files.len())),
            });
            for (index, file) in files.into_iter().enumerate() {
                file.load(index, batch.clone());
            }
        }

        #[method(dropInteraction:sessionDidExit:)]
        fn did_exit(&self, _: &UIDropInteraction, session: &Session) {
            self.leave(session);
        }

        #[method(dropInteraction:sessionDidEnd:)]
        fn did_end(&self, _: &UIDropInteraction, session: &Session) {
            self.leave(session);
        }
    }
);

fn text_preview(text: &str) -> *mut UIDragPreview {
    let Some(mtm) = MainThreadMarker::new() else {
        return std::ptr::null_mut();
    };
    // SAFETY: UIKit renders this label on the main thread. It only contains the drag payload.
    unsafe {
        let label: Retained<UILabel> = msg_send_id![mtm.alloc::<UILabel>(), init];
        label.setText(Some(&NSString::from_str(text)));
        label.setNumberOfLines(2);
        label.setBackgroundColor(Some(&UIColor::secondarySystemBackgroundColor()));
        label.setTextColor(Some(&UIColor::labelColor()));
        let size = label.sizeThatFits(CGSize::new(220.0, 64.0));
        label.setFrame(CGRect::new(
            Default::default(),
            CGSize::new(size.width.min(220.0), size.height.min(64.0)),
        ));
        Retained::autorelease_return(UIDragPreview::initWithView(mtm.alloc(), &label))
    }
}

fn view_point(view: &UIView, position: CGPoint) -> Point {
    let origin = view.bounds().origin;
    Point {
        x: (position.x - origin.x) as f32,
        y: (position.y - origin.y) as f32,
    }
}

impl FileDropDelegate {
    fn with_text_source<T>(
        &self,
        point: Point,
        read: impl FnOnce(&TextDragSource) -> T,
    ) -> Option<T> {
        let state = self.ivars();
        let snapshot = state.snapshot.borrow();
        let snapshot = snapshot.as_ref()?;
        let source = state.text_source.borrow();
        source
            .as_ref()
            .filter(|source| source.contains(snapshot, point))
            .map(read)
    }

    fn drag_items(
        &self,
        interaction: &UIDragInteraction,
        session: &DragSession,
    ) -> Retained<NSArray<UIDragItem>> {
        // SAFETY: UIKit supplies the attached view and session on its main thread.
        let Some(view) = (unsafe { interaction.view() }) else {
            return NSArray::new();
        };
        let point = view_point(&view, unsafe { session.locationInView(&view) });
        let Some(selection) =
            self.with_text_source(point, |source| source.context.selection().to_owned())
        else {
            return NSArray::new();
        };
        // SAFETY: This callback is main-thread-only and NSString implements NSItemProviderWriting.
        unsafe {
            let mtm = MainThreadMarker::new_unchecked();
            let text = NSString::from_str(&selection);
            let provider =
                NSItemProvider::initWithObject(mtm.alloc(), ProtocolObject::from_ref(&*text));
            provider.setPreferredPresentationStyle(UIPreferredPresentationStyle::Inline);
            let item = UIDragItem::initWithItemProvider(mtm.alloc(), &provider);
            let preview_text: String = selection.chars().take(120).collect();
            let preview = RcBlock::new(move || text_preview(&preview_text));
            item.setPreviewProvider(Some(&preview));
            NSArray::from_vec(vec![item])
        }
    }

    fn update_text_hold(&self, recognizer: &UILongPressGestureRecognizer) {
        let state = self.ivars();
        // SAFETY: UIKit calls the target on its main thread while the view is attached.
        let (phase, point) = unsafe {
            let Some(view) = recognizer.view() else {
                return;
            };
            (
                recognizer.state(),
                view_point(&view, recognizer.locationInView(Some(&view))),
            )
        };
        let mut hold = state.text_hold.borrow_mut();
        match phase {
            UIGestureRecognizerState::Began => {
                *hold = self.with_text_source(point, |source| TextHold {
                    origin: point,
                    target: source.target.clone(),
                    context: source.context.clone(),
                });
            }
            UIGestureRecognizerState::Changed | UIGestureRecognizerState::Ended => {
                hold.take_if(|hold| {
                    (point.x - hold.origin.x).hypot(point.y - hold.origin.y) > TOUCH_HOLD_SLOP
                });
                if phase == UIGestureRecognizerState::Ended
                    && let Some(hold) = hold.take()
                {
                    state.events.post(UserEvent::IosSelectedTextHeld {
                        window: state.window,
                        target: hold.target,
                        context: hold.context,
                        point: hold.origin,
                    });
                }
            }
            _ => *hold = None,
        }
    }

    fn drag(&self, session: &Session) -> Drag {
        let identity = std::ptr::from_ref(session) as usize;
        if let Some(drag) = self
            .ivars()
            .drag
            .get()
            .filter(|drag| drag.session == identity)
        {
            return drag;
        }
        let sequence = self.ivars().sequence.get().wrapping_add(1);
        self.ivars().sequence.set(sequence);
        let drag = Drag {
            session: identity,
            sequence,
            performed: false,
        };
        if let Some(previous) = self.ivars().drag.replace(Some(drag)) {
            self.post(FileDragInput::Left {
                sequence: previous.sequence,
            });
        }
        drag
    }

    fn leave(&self, session: &Session) {
        let identity = std::ptr::from_ref(session) as usize;
        if let Some(drag) = self
            .ivars()
            .drag
            .get()
            .filter(|drag| drag.session == identity)
        {
            self.ivars().drag.set(None);
            self.post(FileDragInput::Left {
                sequence: drag.sequence,
            });
        }
    }

    fn offer(
        &self,
        interaction: &UIDropInteraction,
        session: &Session,
    ) -> Option<(Point, Payload, Target)> {
        // SAFETY: UIKit supplies the interaction's view and session on the main thread.
        let view = unsafe { interaction.view()? };
        let location = unsafe { session.locationInView(&view) };
        let origin = view.bounds().origin;
        let point = Point {
            x: (location.x - origin.x) as f32,
            y: (location.y - origin.y) as f32,
        };
        let payload = payload(session)?;
        let snapshot = self.ivars().snapshot.borrow();
        let snapshot = snapshot.as_ref()?;
        let target = match &payload {
            Payload::Text(_) => {
                snapshot
                    .scope
                    .text_target(&snapshot.scene, &snapshot.layout, point)
            }
            Payload::Files(files) => snapshot.scope.target_for_extensions(
                &snapshot.scene,
                &snapshot.layout,
                point,
                &extensions(files),
            ),
        }?;
        Some((point, payload, target))
    }

    fn load_text(
        &self,
        sequence: u64,
        point: Point,
        target: Target,
        providers: Vec<Retained<NSItemProvider>>,
    ) {
        let window = self.ivars().window;
        let events = &self.ivars().events;
        if !events.post(UserEvent::IosTextDropStarted {
            window,
            sequence,
            point,
            target,
        }) {
            return;
        }
        let pending = Arc::new(Mutex::new(Pending::new(providers.len())));
        for (index, provider) in providers.into_iter().enumerate() {
            let pending = pending.clone();
            let events = events.clone();
            let callback = RcBlock::new(
                move |object: *mut ProtocolObject<dyn NSItemProviderReading>,
                      error: *mut NSError| {
                    // SAFETY: The provider owns the object during this call. Check its class
                    // before interpreting it as an NSString and transfer only Rust data.
                    let text = unsafe { object.as_ref() }
                        .filter(|object| {
                            error.is_null()
                                && unsafe { msg_send![*object, isKindOfClass: NSString::class()] }
                        })
                        .map(|object| {
                            unsafe { &*std::ptr::from_ref(object).cast::<NSString>() }.to_string()
                        });
                    let Some(parts) = pending
                        .lock()
                        .ok()
                        .and_then(|mut batch| batch.record(index, text))
                    else {
                        return;
                    };
                    events.post(UserEvent::IosTextDropLoaded {
                        window,
                        sequence,
                        text: parts.map(|parts| parts.join("\n")),
                    });
                },
            );
            // SAFETY: NSString conforms to NSItemProviderReading. Loading starts inside performDrop.
            let _: Retained<NSProgress> = unsafe {
                msg_send_id![&*provider, loadObjectOfClass: NSString::class(), completionHandler: &*callback]
            };
        }
    }

    fn post(&self, input: FileDragInput) -> bool {
        self.ivars().events.post(UserEvent::NativeFileDrag {
            window: self.ivars().window,
            input,
        })
    }
}

struct Representation {
    provider: Retained<NSItemProvider>,
    kind: LoadKind,
    filename: Option<String>,
}

enum LoadKind {
    File(Retained<NSString>),
    FileUrl,
}

fn payload(session: &Session) -> Option<Payload> {
    // SAFETY: The drag item metadata is valid during the delegate callback.
    let items = unsafe { session.items() };
    let (text, files): (Vec<_>, Vec<_>) = items
        .iter()
        .map(|item| unsafe { item.itemProvider() })
        .partition(|provider| inline_text(provider));
    match (text.is_empty(), files.is_empty()) {
        (false, true) => Some(Payload::Text(text)),
        (true, false) => representations(files).map(Payload::Files),
        _ => None,
    }
}

fn inline_text(provider: &NSItemProvider) -> bool {
    // SAFETY: These are synchronous metadata queries, and NSString supports object loading.
    unsafe {
        let style = provider.preferredPresentationStyle();
        let plain = UTTypePlainText.identifier();
        if style == UIPreferredPresentationStyle::Attachment
            || provider.hasItemConformingToTypeIdentifier(&UTTypeFileURL.identifier())
            || !provider.hasItemConformingToTypeIdentifier(&plain)
            || provider.hasRepresentationConformingToTypeIdentifier_fileOptions(
                &plain,
                NSItemProviderFileOptions::NSItemProviderFileOptionOpenInPlace,
            )
        {
            return false;
        }
        // A named text document can also vend an NSString. Keep it in the file route
        // unless the provider explicitly presents its payload as inline text.
        if style != UIPreferredPresentationStyle::Inline
            && provider
                .suggestedName()
                .is_some_and(|name| file_name(&name.to_string()).and_then(extension).is_some())
        {
            return false;
        }
        msg_send![provider, canLoadObjectOfClass: NSString::class()]
    }
}

fn representations(providers: Vec<Retained<NSItemProvider>>) -> Option<Vec<Representation>> {
    // SAFETY: Only provider metadata is accessed here.
    unsafe {
        let mut files = Vec::with_capacity(providers.len());
        for provider in providers {
            let mut filename = provider
                .suggestedName()
                .map(|name| name.to_string())
                .filter(|name| file_name(name).is_some());
            let file = filename.as_ref().and_then(|_| {
                provider
                    .registeredTypeIdentifiers()
                    .iter()
                    .find_map(|identifier| {
                        let ty = UTType::typeWithIdentifier(&identifier)?;
                        (ty.conformsToType(UTTypeData) && !ty.conformsToType(UTTypeURL))
                            .then_some(ty)
                    })
            });
            let kind = if let Some(ty) = file {
                if let Some(name) = &mut filename
                    && extension(name).is_none()
                    && let Some(ext) = ty.preferredFilenameExtension()
                {
                    name.push('.');
                    name.push_str(&ext.to_string());
                }
                LoadKind::File(ty.identifier())
            } else if provider.hasItemConformingToTypeIdentifier(&UTTypeFileURL.identifier()) {
                LoadKind::FileUrl
            } else {
                return None;
            };
            files.push(Representation {
                provider,
                kind,
                filename,
            });
        }
        Some(files)
    }
}

fn extensions(files: &[Representation]) -> Arc<[Option<String>]> {
    files
        .iter()
        .map(|file| {
            file.filename
                .as_deref()
                .and_then(extension)
                .map(str::to_owned)
        })
        .collect()
}

fn extension(name: &str) -> Option<&str> {
    Path::new(name).extension()?.to_str()
}

fn file_name(name: &str) -> Option<&str> {
    let mut components = Path::new(name).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(part)), None) if part == name => Some(name),
        _ => None,
    }
}

impl Representation {
    fn load(self, index: usize, batch: Arc<Batch>) {
        let Self {
            provider,
            filename,
            kind,
        } = self;
        // The callbacks can run on a provider queue. They capture only Rust
        // data and copy the file before the provider's URL expires.
        match kind {
            LoadKind::File(identifier) => {
                let callback = RcBlock::new(move |url: *mut NSURL, error: *mut NSError| {
                    // SAFETY: The provider keeps the delivered URL alive during this call.
                    let path = unsafe { url.as_ref() }
                        .filter(|_| error.is_null())
                        .and_then(|url| batch.copy(index, url, filename.as_deref()));
                    batch.finish(index, path);
                });
                // SAFETY: Request the declared file representation while handling the drop.
                unsafe {
                    provider.loadFileRepresentationForTypeIdentifier_completionHandler(
                        &identifier,
                        &callback,
                    )
                };
            }
            LoadKind::FileUrl => {
                let callback = RcBlock::new(
                    move |item: *mut ProtocolObject<dyn NSSecureCoding>, error: *mut NSError| {
                        // SAFETY: The callback borrow is valid until it returns and
                        // the dynamic class is checked before interpreting it as NSURL.
                        let path = unsafe { item.as_ref() }
                            .filter(|_| error.is_null())
                            .and_then(|item| {
                                let is_url: bool =
                                    unsafe { msg_send![item, isKindOfClass: NSURL::class()] };
                                if !is_url {
                                    return None;
                                }
                                let url = unsafe { &*std::ptr::from_ref(item).cast::<NSURL>() };
                                super::directory::with_file_url_access(url, |url| {
                                    batch.copy(index, url, None)
                                })
                            });
                        batch.finish(index, path);
                    },
                );
                // SAFETY: Request only file URLs. The callback independently validates the class and URL.
                unsafe {
                    provider.loadItemForTypeIdentifier_options_completionHandler(
                        &UTTypeFileURL.identifier(),
                        None,
                        std::ptr::from_ref(&*callback).cast_mut(),
                    );
                }
            }
        }
    }
}

struct Pending {
    remaining: usize,
    completed: Vec<Option<Option<String>>>,
}

impl Pending {
    fn new(count: usize) -> Self {
        Self {
            remaining: count,
            completed: vec![None; count],
        }
    }

    /// Return the ordered batch after every provider has answered.
    fn record(&mut self, index: usize, value: Option<String>) -> Option<Option<Vec<String>>> {
        let slot = self
            .completed
            .get_mut(index)
            .filter(|slot| slot.is_none())?;
        *slot = Some(value);
        self.remaining -= 1;
        (self.remaining == 0).then(|| {
            std::mem::take(&mut self.completed)
                .into_iter()
                .map(Option::flatten)
                .collect()
        })
    }
}

struct Batch {
    window: WindowId,
    events: WindowEvents,
    sequence: u64,
    point: Point,
    target: Target,
    extensions: Arc<[Option<String>]>,
    directory: Arc<TempDir>,
    pending: Mutex<Pending>,
}

impl Batch {
    fn copy(&self, index: usize, url: &NSURL, suggested: Option<&str>) -> Option<String> {
        // SAFETY: The immutable URL is borrowed from a provider completion.
        if !unsafe { url.isFileURL() }
            || unsafe { url.host() }
                .is_some_and(|host| !host.is_empty() && host.to_string() != "localhost")
        {
            return None;
        }
        let path = unsafe { url.path()? }.to_string();
        let source = Path::new(&path);
        if !source.is_absolute() || !fs::symlink_metadata(source).ok()?.is_file() {
            return None;
        }
        let name = suggested
            .or_else(|| source.file_name()?.to_str())
            .and_then(file_name)?;
        let item = self.directory.path().join(index.to_string());
        fs::create_dir(&item).ok()?;
        let destination = item.join(name);
        fs::copy(source, &destination).ok()?;
        Some(destination.to_str()?.to_owned())
    }

    fn finish(&self, index: usize, path: Option<String>) {
        let Some(paths) = self
            .pending
            .lock()
            .ok()
            .and_then(|mut batch| batch.record(index, path))
        else {
            return;
        };
        if let Some(paths) = paths {
            self.events.post(UserEvent::IosFilesCopied {
                window: self.window,
                input: FileDragInput::Copied {
                    sequence: self.sequence,
                    point: self.point,
                    paths: paths.into(),
                    target: self.target.clone(),
                },
                directory: self.directory.clone(),
            });
        } else {
            self.events.post(UserEvent::NativeFileDrag {
                window: self.window,
                input: FileDragInput::Failed {
                    sequence: self.sequence,
                    point: self.point,
                    extensions: self.extensions.clone(),
                    target: self.target.clone(),
                    message: "Could not read one or more dropped files.".into(),
                },
            });
        }
    }
}

pub(crate) struct FileDropBridge {
    view: Retained<UIView>,
    interaction: Retained<UIDropInteraction>,
    drag: Retained<UIDragInteraction>,
    text_hold: Retained<UILongPressGestureRecognizer>,
    delegate: Retained<FileDropDelegate>,
    directories: Vec<Arc<TempDir>>,
}

impl FileDropBridge {
    pub(crate) fn new(window: &Window, events: WindowEvents) -> Result<Self, String> {
        let view = super::view(window).ok_or("iOS window has no UIKit view")?;
        // SAFETY: The window supplies the live view, which is retained by the bridge.
        let view = unsafe { Retained::retain(std::ptr::from_ref(view).cast_mut()) }
            .ok_or("could not retain the UIKit view")?;
        let mtm = MainThreadMarker::new().ok_or("file drops require the UIKit main thread")?;
        let delegate = mtm.alloc::<FileDropDelegate>().set_ivars(DelegateState {
            window: window.id(),
            events,
            snapshot: RefCell::default(),
            text_source: RefCell::default(),
            text_hold: RefCell::default(),
            sequence: Cell::default(),
            drag: Cell::default(),
        });
        // SAFETY: These views and interactions are owned by the UIKit main thread.
        unsafe {
            let delegate: Retained<FileDropDelegate> = msg_send_id![super(delegate), init];
            let interaction = UIDropInteraction::initWithDelegate(
                mtm.alloc::<UIDropInteraction>(),
                ProtocolObject::from_ref(&*delegate),
            );
            interaction.setAllowsSimultaneousDropSessions(false);
            view.addInteraction(ProtocolObject::from_ref(&*interaction));
            let drag = UIDragInteraction::initWithDelegate(
                mtm.alloc(),
                ProtocolObject::from_ref(&*delegate),
            );
            drag.setEnabled(false);
            view.addInteraction(ProtocolObject::from_ref(&*drag));
            let text_hold = UILongPressGestureRecognizer::initWithTarget_action(
                mtm.alloc(),
                Some(&delegate),
                Some(sel!(xrackSelectedTextHeld:)),
            );
            text_hold.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            text_hold.setAllowableMovement(f64::from(TOUCH_HOLD_SLOP));
            text_hold.setCancelsTouchesInView(false);
            view.addGestureRecognizer(&text_hold);
            Ok(Self {
                view,
                interaction,
                drag,
                text_hold,
                delegate,
                directories: Vec::new(),
            })
        }
    }

    pub(crate) fn sync(&self, scene: Rc<Node>, layout: Rc<LayoutNode>, scope: FileDropScope) {
        self.delegate.ivars().snapshot.replace(Some(Snapshot {
            scene,
            layout,
            scope,
        }));
    }

    pub(crate) fn sync_text_source(&self, source: Option<TextDragSource>) {
        let enabled = source.is_some();
        self.delegate.ivars().text_source.replace(source);
        // SAFETY: The bridge is owned by the UIKit main thread.
        unsafe {
            if self.drag.isEnabled() != enabled {
                self.drag.setEnabled(enabled);
            }
        }
    }

    pub(crate) fn can_drag_text_at(&self, point: Point) -> bool {
        self.delegate.with_text_source(point, |_| ()).is_some()
    }

    pub(crate) fn keep(&mut self, directory: Arc<TempDir>) {
        self.directories.push(directory);
    }
}

impl Drop for FileDropBridge {
    fn drop(&mut self) {
        // SAFETY: The bridge and interaction are destroyed on the UIKit main thread.
        unsafe {
            self.view
                .removeInteraction(ProtocolObject::from_ref(&*self.interaction));
            self.view
                .removeInteraction(ProtocolObject::from_ref(&*self.drag));
            self.view.removeGestureRecognizer(&self.text_hold);
        };
    }
}

impl<C: UiComponent> Application<C> {
    pub(crate) fn sync_ios_text_drag(&self) {
        let Some(state) = &self.state else { return };
        let Some(bridge) = &state.file_drop else {
            return;
        };
        let source = (|| {
            if !self.ios_application_active
                || !state.window.has_focus()
                || self.selection_handle_drag.is_some()
            {
                return None;
            }
            let layout = self.layout.as_ref()?;
            let target = self.input.focused.as_ref()?;
            let node = target.node_in(&self.scene)?;
            if !(xrack_runtime::is_text_input(node.kind) || xrack_runtime::combo_editable(node)) {
                return None;
            }
            let context =
                TextEditContext::for_clipboard(&self.input, &self.scene, TextCommand::Copy)?;
            let native = match state
                .ios_text
                .as_ref()
                .filter(|native| native.editing(target))
            {
                Some(native) => Some(native.copy_revision()?),
                None => None,
            };
            let item = layout.at_path(&target.path)?;
            let highlights =
                state
                    .renderer
                    .text
                    .editor_selection_rects(node, item, &self.input, target);
            (!highlights.is_empty()).then_some(TextDragSource {
                target: target.clone(),
                context,
                highlights,
                native,
            })
        })();
        bridge.sync_text_source(source);
    }

    pub(crate) fn start_ios_text_drop(
        &mut self,
        window: WindowId,
        sequence: u64,
        point: Point,
        target: Target,
    ) {
        if self
            .state
            .as_ref()
            .is_none_or(|state| state.window.id() != window)
        {
            return;
        }
        self.pending_text_drop = self.layout.as_ref().and_then(|layout| {
            TextDrop::capture(&self.input, &self.scene, layout, point, target)
                .map(|drop| (sequence, drop))
        });
    }

    pub(crate) fn finish_ios_text_drop(
        &mut self,
        window: WindowId,
        sequence: u64,
        text: Option<String>,
    ) {
        let (Some(state), Some(layout)) = (&self.state, &self.layout) else {
            return;
        };
        if state.window.id() != window {
            return;
        }
        let Some((_, drop)) = self
            .pending_text_drop
            .take_if(|(pending, _)| *pending == sequence)
        else {
            return;
        };
        let Some(text) = text.filter(|text| !text.is_empty()) else {
            return;
        };
        let scope = self.input.file_drop_scope(layout);
        let Some((target, point)) = drop.destination(&self.input, &self.scene, layout, &scope)
        else {
            return;
        };
        let Some(caret) = PointerCaret::new(
            &self.input,
            &self.scene,
            layout,
            target,
            point,
            point,
            false,
        ) else {
            return;
        };
        if self
            .input
            .focus_text_from_touch(&self.scene, layout, target.clone())
            && caret.place(&state.renderer.text, &mut self.input, &self.scene, layout)
        {
            self.edit_text_from(EditAction::Insert(&text), EditSource::Atomic);
        }
    }
}
