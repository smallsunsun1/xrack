//! AppKit reports the position and pasteboard together for each native drag.
use crate::{UserEvent, desktop::WindowEvents, window_owner::native_window};
use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{NSObject, NSObjectProtocol, ProtocolObject, Sel},
};
use objc2_app_kit::{
    NSDragOperation, NSDraggingDestination, NSDraggingInfo, NSPasteboard, NSPasteboardTypeFileURL,
    NSWindow, NSWindowDelegate,
};
use objc2_foundation::{NSArray, NSURL};
use std::{cell::RefCell, ffi::CStr, ptr::NonNull, rc::Rc, sync::Arc};
use winit::window::{Window, WindowId};
use xrack_layout::{FileDragInput, FileDropScope, LayoutNode, Point};
use xrack_runtime::Node;

type DragInfo = ProtocolObject<dyn NSDraggingInfo>;
type WindowDelegate = ProtocolObject<dyn NSWindowDelegate>;

struct Snapshot {
    scene: Rc<Node>,
    layout: Rc<LayoutNode>,
    scope: FileDropScope,
}

struct PasteboardBatch {
    sequence: u64,
    change: isize,
    paths: Arc<[String]>,
}

struct DelegateState {
    original: Retained<WindowDelegate>,
    id: WindowId,
    events: WindowEvents,
    snapshot: RefCell<Option<Snapshot>>,
    batch: RefCell<Option<PasteboardBatch>>,
}

// Replace only the window delegate slot. The original winit delegate still
// handles every selector outside this bridge's drag-and-drop protocol.
define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "XRackFileDropDelegate"]
    #[ivars = DelegateState]
    struct FileDropDelegate;

    unsafe impl NSObjectProtocol for FileDropDelegate {
        #[unsafe(method(respondsToSelector:))]
        fn responds_to_selector(&self, selector: Sel) -> bool {
            // Ask NSObject about the selectors implemented by this subclass.
            let implemented: bool = unsafe { msg_send![super(self), respondsToSelector: selector] };
            implemented || self.ivars().original.respondsToSelector(selector)
        }
    }

    unsafe impl NSWindowDelegate for FileDropDelegate {}

    impl FileDropDelegate {
        #[unsafe(method_id(forwardingTargetForSelector:))]
        fn forward_to_window_delegate(&self, selector: Sel) -> Option<Retained<WindowDelegate>> {
            self.ivars().original.respondsToSelector(selector).then(|| self.ivars().original.clone())
        }
    }

    unsafe impl NSDraggingDestination for FileDropDelegate {
        #[unsafe(method(draggingEntered:))]
        fn dragging_entered(&self, sender: &DragInfo) -> NSDragOperation {
            self.observe(sender)
        }

        #[unsafe(method(draggingUpdated:))]
        fn dragging_updated(&self, sender: &DragInfo) -> NSDragOperation {
            self.observe(sender)
        }

        #[unsafe(method(draggingExited:))]
        fn dragging_exited(&self, sender: Option<&DragInfo>) {
            let sequence = sender.map(sequence).or_else(|| self.ivars().batch.borrow().as_ref().map(|batch| batch.sequence));
            if let Some(sequence) = sequence {
                self.leave(sequence);
            }
        }

        #[unsafe(method(draggingEnded:))]
        fn dragging_ended(&self, sender: &DragInfo) {
            self.leave(sequence(sender));
        }

        #[unsafe(method(prepareForDragOperation:))]
        fn prepare_for_drag_operation(&self, sender: &DragInfo) -> bool {
            let accepted = location(sender).is_some_and(|point| self.target(point, &self.paths(sender)).is_some());
            if !accepted { self.leave(sequence(sender)); }
            accepted
        }

        #[unsafe(method(performDragOperation:))]
        fn perform_drag_operation(&self, sender: &DragInfo) -> bool {
            let sequence = sequence(sender);
            let paths = self.paths(sender);
            self.ivars().batch.borrow_mut().take();
            if let Some(point) = location(sender)
                && let Some(target) = self.target(point, &paths)
            {
                self.post(FileDragInput::Dropped { sequence, point, paths, target })
            } else {
                self.leave(sequence);
                false
            }
        }
    }
);

impl FileDropDelegate {
    fn leave(&self, sequence: u64) {
        let mut batch = self.ivars().batch.borrow_mut();
        if batch
            .as_ref()
            .is_some_and(|batch| batch.sequence == sequence)
        {
            batch.take();
        }
        self.post(FileDragInput::Left { sequence });
    }

    fn post(&self, input: FileDragInput) -> bool {
        self.ivars().events.post(UserEvent::NativeFileDrag {
            window: self.ivars().id,
            input,
        })
    }

    fn target(&self, point: Point, paths: &[String]) -> Option<xrack_layout::Target> {
        let snapshot = self.ivars().snapshot.borrow();
        let snapshot = snapshot.as_ref()?;
        snapshot
            .scope
            .target(&snapshot.scene, &snapshot.layout, point, paths)
    }

    fn observe(&self, sender: &DragInfo) -> NSDragOperation {
        let sequence = sequence(sender);
        let Some(point) = location(sender) else {
            self.post(FileDragInput::Left { sequence });
            return NSDragOperation::None;
        };
        let paths = self.paths(sender);
        let accepts = self.target(point, &paths).is_some();
        let posted = self.post(FileDragInput::Moved {
            sequence,
            point,
            paths,
        });
        if accepts && posted {
            NSDragOperation::Copy
        } else {
            NSDragOperation::None
        }
    }

    fn paths(&self, sender: &DragInfo) -> Arc<[String]> {
        if !sender
            .draggingSourceOperationMask()
            .contains(NSDragOperation::Copy)
        {
            return Arc::default();
        }
        let pasteboard = sender.draggingPasteboard();
        let sequence = sequence(sender);
        let change = pasteboard.changeCount();
        if let Some(batch) = &*self.ivars().batch.borrow()
            && batch.sequence == sequence
            && batch.change == change
        {
            return batch.paths.clone();
        }
        let paths = file_paths(&pasteboard).unwrap_or_default();
        self.ivars().batch.replace(Some(PasteboardBatch {
            sequence,
            change,
            paths: paths.clone(),
        }));
        paths
    }
}

fn sequence(sender: &DragInfo) -> u64 {
    sender.draggingSequenceNumber() as u64
}

fn location(sender: &DragInfo) -> Option<Point> {
    let view = sender.draggingDestinationWindow()?.contentView()?;
    let point = view.convertPoint_fromView(sender.draggingLocation(), None);
    let bounds = view.bounds();
    let y = point.y - bounds.origin.y;
    Some(Point {
        x: (point.x - bounds.origin.x) as f32,
        y: (if view.isFlipped() {
            y
        } else {
            bounds.size.height - y
        }) as f32,
    })
}

fn file_paths(pasteboard: &NSPasteboard) -> Option<Arc<[String]>> {
    let items = pasteboard.pasteboardItems()?;
    let mut paths = Vec::with_capacity(items.len());
    for item in &*items {
        // Read every item explicitly so a mixed file/URL batch cannot be truncated.
        let value = item.stringForType(unsafe { NSPasteboardTypeFileURL })?;
        let url = NSURL::URLWithString(&value)?;
        if !url.isFileURL()
            || url
                .host()
                .is_some_and(|host| !host.is_empty() && host.to_string() != "localhost")
        {
            return None;
        }
        // Darwin PATH_MAX includes the terminator. AppKit reports conversion
        // failure rather than returning a pointer for malformed file URLs.
        let mut buffer = [0u8; 1024];
        let destination = NonNull::from(&mut buffer[0]).cast();
        if !unsafe { url.getFileSystemRepresentation_maxLength(destination, buffer.len()) } {
            return None;
        }
        let path = CStr::from_bytes_until_nul(&buffer).ok()?.to_str().ok()?;
        paths.push(path.to_owned());
    }
    Some(paths.into())
}

pub(crate) struct FileDropBridge {
    window: Retained<NSWindow>,
    delegate: Retained<FileDropDelegate>,
}

impl FileDropBridge {
    pub(crate) fn new(window: &Window, events: WindowEvents) -> Result<Self, String> {
        let id = window.id();
        let window = native_window(window)?;
        let original = window.delegate().ok_or("AppKit window has no delegate")?;
        let mtm = MainThreadMarker::new().ok_or("file-drop bridge must run on the main thread")?;
        let delegate = FileDropDelegate::alloc(mtm).set_ivars(DelegateState {
            original,
            id,
            events,
            snapshot: RefCell::default(),
            batch: RefCell::default(),
        });
        // NSObject's initializer returns the same object or reports allocation failure.
        let delegate: Retained<FileDropDelegate> = unsafe { msg_send![super(delegate), init] };
        window.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        window.registerForDraggedTypes(&NSArray::from_slice(&[unsafe { NSPasteboardTypeFileURL }]));
        Ok(Self { window, delegate })
    }

    pub(crate) fn sync(&self, scene: Rc<Node>, layout: Rc<LayoutNode>, scope: FileDropScope) {
        self.delegate.ivars().snapshot.replace(Some(Snapshot {
            scene,
            layout,
            scope,
        }));
    }
}

impl Drop for FileDropBridge {
    fn drop(&mut self) {
        let current: &WindowDelegate = ProtocolObject::from_ref(&*self.delegate);
        if self
            .window
            .delegate()
            .as_deref()
            .is_some_and(|delegate| std::ptr::eq(delegate, current))
        {
            self.window
                .setDelegate(Some(&self.delegate.ivars().original));
            self.window.unregisterDraggedTypes();
            // Restore the registration installed by the pinned winit version.
            #[allow(deprecated)]
            self.window
                .registerForDraggedTypes(&NSArray::from_slice(&[unsafe {
                    objc2_app_kit::NSFilenamesPboardType
                }]));
        }
    }
}
