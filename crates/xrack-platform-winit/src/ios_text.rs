use crate::{UserEvent, desktop::WindowEvents, ios, text_indices::byte_offset};
use block2::RcBlock;
use objc2::{
    ClassType, DeclaredClass, declare_class, msg_send_id,
    mutability::MainThreadOnly,
    rc::Retained,
    runtime::{NSObjectProtocol, ProtocolObject},
    sel,
};
use objc2_foundation::{
    CGPoint, CGRect, CGSize, MainThreadMarker, NSArray, NSNotification, NSNotificationCenter,
    NSObject, NSOperationQueue, NSString,
};
use objc2_ui_kit::{
    NSObjectUIAccessibility, UIBarButtonItem, UIBarButtonItemStyle, UIBarButtonSystemItem,
    UIKeyboardDidChangeFrameNotification, UIKeyboardType, UIReturnKeyType,
    UITextAutocapitalizationType, UITextAutocorrectionType, UITextContentType,
    UITextContentTypeEmailAddress, UITextContentTypeTelephoneNumber, UITextContentTypeURL,
    UITextField, UITextFieldDelegate, UITextFieldTextDidChangeNotification, UITextInput,
    UITextInputTraits, UITextRange, UITextSpellCheckingType, UITextView,
    UITextViewTextDidChangeNotification, UIToolbar, UIView,
};
use std::{cell::Cell, ops::Range, ptr::NonNull, rc::Rc};
use winit::window::Window;
use xrack_layout::Target;
use xrack_runtime::{InputType, Node, ReturnKey, TableCellEditor, is_multiline_text_input};

pub(crate) struct NativeEdit {
    epoch: u64,
    serial: u64,
    pub(super) input: NativeInput,
}

pub(super) enum NativeInput {
    Preedit {
        text: String,
        selection: Range<usize>,
    },
    Commit {
        value: String,
        selection: Range<usize>,
    },
    Submit,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    SingleLine,
    Multiline,
    Password,
    Integer,
    Decimal,
    SignedNumber,
    Search,
    Email,
    Url,
    Tel,
}

impl Mode {
    pub(super) fn for_node(node: &Node) -> Self {
        if matches!(
            node.table_cell_editor,
            Some(TableCellEditor::Int | TableCellEditor::Float)
        ) {
            return Self::SignedNumber;
        }
        match (node.kind, node.input_type()) {
            ("SpinBox", _) if node.int("minimum").is_some_and(|minimum| minimum < 0) => {
                Self::SignedNumber
            }
            ("DoubleSpinBox", _) if node.number("minimum").is_some_and(|minimum| minimum < 0.0) => {
                Self::SignedNumber
            }
            ("SpinBox", _) | (_, InputType::Number) => Self::Integer,
            ("DoubleSpinBox", _) => Self::Decimal,
            (_, InputType::Decimal) => Self::SignedNumber,
            ("SearchField", _) | (_, InputType::Search) => Self::Search,
            (_, InputType::Password) => Self::Password,
            (_, InputType::Email) => Self::Email,
            (_, InputType::Url) => Self::Url,
            (_, InputType::Tel) => Self::Tel,
            (kind, _) if is_multiline_text_input(kind) => Self::Multiline,
            _ => Self::SingleLine,
        }
    }

    fn keyboard(self) -> UIKeyboardType {
        match self {
            Self::SingleLine | Self::Multiline | Self::Password | Self::Search => {
                UIKeyboardType::Default
            }
            Self::Integer => UIKeyboardType::ASCIICapableNumberPad,
            Self::Decimal => UIKeyboardType::DecimalPad,
            Self::SignedNumber => UIKeyboardType::NumbersAndPunctuation,
            Self::Email => UIKeyboardType::EmailAddress,
            Self::Url => UIKeyboardType::URL,
            Self::Tel => UIKeyboardType::PhonePad,
        }
    }

    fn has_done_toolbar(self) -> bool {
        matches!(
            self,
            Self::Integer | Self::Decimal | Self::SignedNumber | Self::Tel
        )
    }

    fn return_key(self, key: ReturnKey) -> UIReturnKeyType {
        match key {
            ReturnKey::Next => return UIReturnKeyType::UIReturnKeyNext,
            ReturnKey::Done => return UIReturnKeyType::UIReturnKeyDone,
            ReturnKey::Auto => {}
        }
        match self {
            Self::Search => UIReturnKeyType::UIReturnKeySearch,
            mode if mode.has_done_toolbar() => UIReturnKeyType::UIReturnKeyDone,
            _ => UIReturnKeyType::UIReturnKeyDefault,
        }
    }

    fn content_type(self) -> Option<&'static UITextContentType> {
        // SAFETY: UIKit's text content type constants are immutable strings.
        unsafe {
            match self {
                Self::Email => Some(UITextContentTypeEmailAddress),
                Self::Url => Some(UITextContentTypeURL),
                Self::Tel => Some(UITextContentTypeTelephoneNumber),
                _ => None,
            }
        }
    }
}

#[derive(Clone)]
enum Responder {
    Field(Retained<UITextField>),
    View(Retained<UITextView>),
}

impl Responder {
    fn view(&self) -> &UIView {
        match self {
            Self::Field(field) => field,
            Self::View(view) => view,
        }
    }

    fn input(&self) -> &ProtocolObject<dyn UITextInput> {
        match self {
            Self::Field(field) => ProtocolObject::from_ref(&**field),
            Self::View(view) => ProtocolObject::from_ref(&**view),
        }
    }

    fn value(&self) -> String {
        // SAFETY: The responder is owned and used by the UIKit main thread.
        unsafe {
            match self {
                Self::Field(field) => field
                    .text()
                    .map_or_else(String::new, |value| value.to_string()),
                Self::View(view) => view.text().to_string(),
            }
        }
    }

    fn set_value(&self, value: &str) {
        let string = NSString::from_str(value);
        // SAFETY: The responder is owned and used by the UIKit main thread.
        unsafe {
            match self {
                Self::Field(field) => field.setText(Some(&string)),
                Self::View(view) => view.setText(Some(&string)),
            }
        }
    }
}

#[derive(Default)]
struct Revision {
    active: Cell<Option<Mode>>,
    epoch: Cell<u64>,
    serial: Cell<u64>,
}

struct SubmitAction {
    revision: Rc<Revision>,
    events: WindowEvents,
}

impl SubmitAction {
    fn post(&self) {
        if self
            .revision
            .active
            .get()
            .is_some_and(|mode| mode != Mode::Multiline)
        {
            self.events.post(UserEvent::IosText(NativeEdit {
                epoch: self.revision.epoch.get(),
                serial: self.revision.serial.get(),
                input: NativeInput::Submit,
            }));
        }
    }
}

declare_class!(
    struct FieldDelegate;

    unsafe impl ClassType for FieldDelegate {
        type Super = NSObject;
        type Mutability = MainThreadOnly;
        const NAME: &'static str = "XRackTextFieldDelegate";
    }

    impl DeclaredClass for FieldDelegate {
        type Ivars = SubmitAction;
    }

    unsafe impl NSObjectProtocol for FieldDelegate {}

    unsafe impl FieldDelegate {
        #[method(submitFromToolbar:)]
        fn submit_from_toolbar(&self, _: &UIBarButtonItem) {
            self.ivars().post();
        }
    }

    #[allow(non_snake_case)]
    unsafe impl UITextFieldDelegate for FieldDelegate {
        #[method(textFieldShouldReturn:)]
        unsafe fn textFieldShouldReturn(&self, _: &UITextField) -> bool {
            self.ivars().post();
            // XRack resolves validation and focus before changing the responder.
            false
        }
    }
);

pub(super) struct TextInput {
    field: Responder,
    multiline: Responder,
    submit_toolbar: SubmitToolbar,
    _field_delegate: Retained<FieldDelegate>,
    center: Retained<NSNotificationCenter>,
    observers: [Retained<NSObject>; 3],
    revision: Rc<Revision>,
    applied: u64,
    focused: Option<(Target, Mode)>,
    return_key: ReturnKey,
}

impl TextInput {
    pub(super) fn new(window: &Window, events: &WindowEvents) -> Option<Self> {
        let view = ios::view(window)?;
        let mtm = MainThreadMarker::new()?;
        let revision = Rc::new(Revision::default());

        // SAFETY: UIKit views and notifications here are confined to the main thread.
        unsafe {
            view.keyboardLayoutGuide().setFollowsUndockedKeyboard(true);
            let field = crate::ios_keyboard::text_field(mtm, events.clone());
            let action = SubmitAction {
                revision: revision.clone(),
                events: events.clone(),
            };
            let delegate = mtm.alloc::<FieldDelegate>().set_ivars(action);
            let field_delegate: Retained<FieldDelegate> = msg_send_id![super(delegate), init];
            field.setDelegate(Some(ProtocolObject::from_ref(&*field_delegate)));
            let submit_toolbar = submit_toolbar(mtm, &field_delegate);
            let field = Responder::Field(field);
            let multiline = Responder::View(crate::ios_keyboard::text_view(mtm, events.clone()));
            for responder in [&field, &multiline] {
                let input_view = responder.view();
                input_view.setFrame(CGRect::new(CGPoint::ZERO, CGSize::new(1.0, 1.0)));
                input_view.setAlpha(0.02);
                input_view.setIsAccessibilityElement(false);
                view.addSubview(input_view);
            }

            let center = NSNotificationCenter::defaultCenter();
            let queue = NSOperationQueue::mainQueue();
            let field_change = text_changes(field.clone(), false, &revision, events);
            let view_change = text_changes(multiline.clone(), true, &revision, events);
            let geometry_update = {
                let events = events.clone();
                RcBlock::new(move |_: NonNull<NSNotification>| {
                    events.post(UserEvent::IosKeyboardGeometry);
                })
            };
            let observers = [
                center.addObserverForName_object_queue_usingBlock(
                    Some(UITextFieldTextDidChangeNotification),
                    Some(field.view()),
                    Some(&queue),
                    &field_change,
                ),
                center.addObserverForName_object_queue_usingBlock(
                    Some(UITextViewTextDidChangeNotification),
                    Some(multiline.view()),
                    Some(&queue),
                    &view_change,
                ),
                center.addObserverForName_object_queue_usingBlock(
                    Some(UIKeyboardDidChangeFrameNotification),
                    None,
                    Some(&queue),
                    &geometry_update,
                ),
            ];
            Some(Self {
                field,
                multiline,
                submit_toolbar,
                _field_delegate: field_delegate,
                center,
                observers,
                revision,
                applied: 0,
                focused: None,
                return_key: ReturnKey::Auto,
            })
        }
    }

    fn responder(&self, mode: Mode) -> &Responder {
        match mode {
            Mode::Multiline => &self.multiline,
            _ => &self.field,
        }
    }

    pub(super) fn focus(
        &mut self,
        target: &Target,
        mode: Mode,
        return_key: ReturnKey,
        value: &str,
        selection: Range<usize>,
    ) -> bool {
        let changed = self
            .focused
            .as_ref()
            .is_none_or(|(current, kind)| current != target || *kind != mode);
        if changed {
            self.revision.active.set(None);
            if let Some((_, previous)) = self.focused.take() {
                // SAFETY: Clear the old UIKit composition on the main thread before handing over focus.
                unsafe { self.responder(previous).input().unmarkText() };
                self.responder(previous).set_value("");
            }
            self.focused = Some((target.clone(), mode));
            self.advance_focus();
        }

        let update_key = changed || self.return_key != return_key;
        if update_key {
            self.return_key = return_key;
        }
        let responder = self.responder(mode);
        let input = responder.input();
        // SAFETY: All responder access is on the UIKit main thread.
        let active = unsafe {
            if update_key {
                if let Responder::Field(field) = responder {
                    let keyboard = mode.keyboard();
                    let key_type = mode.return_key(return_key);
                    let content_type = mode.content_type();
                    let secure = mode == Mode::Password;
                    let needs_reload = input.keyboardType() != keyboard
                        || input.returnKeyType() != key_type
                        || input.isSecureTextEntry() != secure;
                    input.setSecureTextEntry(secure);
                    input.setKeyboardType(keyboard);
                    input.setReturnKeyType(key_type);
                    input.setTextContentType(content_type);
                    if content_type.is_some() {
                        input.setAutocapitalizationType(UITextAutocapitalizationType::None);
                        input.setAutocorrectionType(UITextAutocorrectionType::No);
                        input.setSpellCheckingType(UITextSpellCheckingType::No);
                    } else {
                        input.setAutocapitalizationType(UITextAutocapitalizationType::Sentences);
                        input.setAutocorrectionType(UITextAutocorrectionType::Default);
                        input.setSpellCheckingType(UITextSpellCheckingType::Default);
                    }
                    field.setInputAccessoryView(mode.has_done_toolbar().then(|| {
                        self.submit_toolbar
                            .view
                            .setItems(Some(if return_key == ReturnKey::Next {
                                &self.submit_toolbar.next
                            } else {
                                &self.submit_toolbar.done
                            }));
                        let toolbar: &UIView = &self.submit_toolbar.view;
                        toolbar
                    }));
                    if needs_reload && field.isFirstResponder() {
                        field.reloadInputViews();
                    }
                }
            }
            // Preserve UIKit's marked text and any newer edit waiting in the event loop.
            if self.applied == self.revision.serial.get() && input.markedTextRange().is_none() {
                if responder.value() != value {
                    responder.set_value(value);
                }
                if selected_bytes(input, value) != selection {
                    let origin = input.beginningOfDocument();
                    let start = input.positionFromPosition_offset(
                        &origin,
                        value[..selection.start].encode_utf16().count() as isize,
                    );
                    let end = input.positionFromPosition_offset(
                        &origin,
                        value[..selection.end].encode_utf16().count() as isize,
                    );
                    if let Some((start, end)) = start.zip(end) {
                        let range = input.textRangeFromPosition_toPosition(&start, &end);
                        input.setSelectedTextRange(range.as_deref());
                    }
                }
            }
            responder.view().isFirstResponder() || responder.view().becomeFirstResponder()
        };
        self.revision.active.set(active.then_some(mode));
        active
    }

    pub(super) fn clear_focus(&mut self) {
        let Some((_, mode)) = self.focused.take() else {
            return;
        };
        self.revision.active.set(None);
        self.advance_focus();
        let responder = self.responder(mode);
        // SAFETY: Responder changes are confined to the UIKit main thread.
        unsafe { responder.view().resignFirstResponder() };
        responder.set_value("");
    }

    pub(super) fn editing(&self, target: &Target) -> bool {
        self.revision.active.get().is_some()
            && self
                .focused
                .as_ref()
                .is_some_and(|(focused, _)| focused == target)
    }

    pub(super) fn accessory_frame(&self, view: &UIView) -> Option<CGRect> {
        if !self
            .revision
            .active
            .get()
            .is_some_and(Mode::has_done_toolbar)
        {
            return None;
        }
        // SAFETY: The toolbar and the window's view are owned by UIKit's main
        // thread. Convert through their windows because UIKit may host the
        // keyboard accessory in a different window from the application.
        unsafe {
            let toolbar = &self.submit_toolbar.view;
            let source = toolbar.window()?;
            let destination = view.window()?;
            let bounds = toolbar.convertRect_toView(toolbar.bounds(), None);
            let bounds = source.convertRect_toWindow(bounds, Some(&destination));
            Some(view.convertRect_fromView(bounds, None))
        }
    }

    fn advance_focus(&mut self) {
        self.revision
            .epoch
            .set(self.revision.epoch.get().wrapping_add(1));
        self.applied = self.revision.serial.get();
    }

    pub(super) fn accept(&mut self, target: &Target, edit: &NativeEdit) -> bool {
        if self
            .focused
            .as_ref()
            .is_none_or(|(current, _)| current != target)
            || edit.epoch != self.revision.epoch.get()
            || edit.serial != self.revision.serial.get()
        {
            return false;
        }
        self.applied = edit.serial;
        true
    }
}

impl Drop for TextInput {
    fn drop(&mut self) {
        // SAFETY: WindowState is created and destroyed on the UIKit main thread.
        unsafe {
            for observer in &self.observers {
                self.center.removeObserver(observer);
            }
            self.field.view().removeFromSuperview();
            self.multiline.view().removeFromSuperview();
        }
    }
}

struct SubmitToolbar {
    view: Retained<UIToolbar>,
    done: Retained<NSArray<UIBarButtonItem>>,
    next: Retained<NSArray<UIBarButtonItem>>,
}

fn submit_toolbar(mtm: MainThreadMarker, delegate: &FieldDelegate) -> SubmitToolbar {
    // SAFETY: The toolbar and its UIKit action are created and used on the main thread.
    unsafe {
        let toolbar = UIToolbar::new(mtm);
        let action = Some(sel!(submitFromToolbar:));
        let done = UIBarButtonItem::initWithBarButtonSystemItem_target_action(
            mtm.alloc(),
            UIBarButtonSystemItem::Done,
            Some(delegate),
            action,
        );
        let next = UIBarButtonItem::initWithTitle_style_target_action(
            mtm.alloc(),
            Some(&NSString::from_str("Next")),
            UIBarButtonItemStyle::Plain,
            Some(delegate),
            action,
        );
        let space = UIBarButtonItem::initWithBarButtonSystemItem_target_action(
            mtm.alloc(),
            UIBarButtonSystemItem::FlexibleSpace,
            None,
            None,
        );
        toolbar.setFrame(CGRect::new(CGPoint::ZERO, CGSize::new(320.0, 44.0)));
        SubmitToolbar {
            view: toolbar,
            done: NSArray::from_slice(&[&*space, &*done]),
            next: NSArray::from_slice(&[&*space, &*next]),
        }
    }
}

fn text_changes(
    responder: Responder,
    multiline: bool,
    revision: &Rc<Revision>,
    events: &WindowEvents,
) -> RcBlock<dyn Fn(NonNull<NSNotification>)> {
    let revision = revision.clone();
    let events = events.clone();
    RcBlock::new(move |_: NonNull<NSNotification>| {
        if !revision
            .active
            .get()
            .is_some_and(|mode| (mode == Mode::Multiline) == multiline)
        {
            return;
        }
        let value = responder.value();
        let input = responder.input();
        let selection = selected_bytes(input, &value);
        // SAFETY: The notification observer runs on the UIKit main queue.
        let update = if let Some(marked) = unsafe { input.markedTextRange() } {
            let range = range_bytes(input, &value, &marked);
            let start = selection.start.saturating_sub(range.start).min(range.len());
            let end = selection.end.saturating_sub(range.start).min(range.len());
            NativeInput::Preedit {
                text: value[range].to_owned(),
                selection: start..end,
            }
        } else {
            NativeInput::Commit { value, selection }
        };
        let serial = revision.serial.get().wrapping_add(1);
        revision.serial.set(serial);
        events.post(UserEvent::IosText(NativeEdit {
            epoch: revision.epoch.get(),
            serial,
            input: update,
        }));
    })
}

fn selected_bytes(field: &ProtocolObject<dyn UITextInput>, value: &str) -> Range<usize> {
    // SAFETY: Selection is sampled from a retained field on the UIKit main thread.
    unsafe {
        let Some(range) = field.selectedTextRange() else {
            return value.len()..value.len();
        };
        range_bytes(field, value, &range)
    }
}

fn range_bytes(
    field: &ProtocolObject<dyn UITextInput>,
    value: &str,
    range: &UITextRange,
) -> Range<usize> {
    // SAFETY: The text positions are from the same retained field and read on the UIKit main thread.
    unsafe {
        let origin = field.beginningOfDocument();
        let start = field.offsetFromPosition_toPosition(&origin, &range.start());
        let end = field.offsetFromPosition_toPosition(&origin, &range.end());
        let start = byte_offset(value, start.max(0) as usize);
        let end = byte_offset(value, end.max(0) as usize);
        start.min(end)..start.max(end)
    }
}
