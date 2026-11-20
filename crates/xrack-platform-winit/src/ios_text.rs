use crate::{
    UserEvent,
    desktop::WindowEvents,
    ios,
    text_indices::{byte_offset, cover_graphemes},
};
use block2::RcBlock;
use objc2::{
    ClassType, DeclaredClass, declare_class, msg_send_id,
    mutability::MainThreadOnly,
    rc::Retained,
    runtime::{NSObjectProtocol, ProtocolObject},
    sel,
};
use objc2_foundation::{
    CGPoint, CGRect, CGSize, MainThreadMarker, NSArray, NSBundle, NSNotification,
    NSNotificationCenter, NSObject, NSOperationQueue, NSString, NSUndoManager,
};
use objc2_ui_kit::{
    NSObjectUIAccessibility, UIBarButtonItem, UIBarButtonItemStyle, UIBarButtonSystemItem,
    UIKeyboardDidChangeFrameNotification, UIKeyboardType, UIPasteboard, UIReturnKeyType,
    UIScrollViewDelegate, UITextAutocapitalizationType, UITextAutocorrectionType,
    UITextContentType, UITextContentTypeEmailAddress, UITextContentTypeNewPassword,
    UITextContentTypeOneTimeCode, UITextContentTypePassword, UITextContentTypeTelephoneNumber,
    UITextContentTypeURL, UITextContentTypeUsername, UITextField, UITextFieldDelegate,
    UITextFieldTextDidChangeNotification, UITextInput, UITextInputTraits, UITextRange,
    UITextSpellCheckingType, UITextView, UITextViewDelegate, UITextViewTextDidChangeNotification,
    UIToolbar, UIView,
};
use std::{
    cell::{Cell, RefCell},
    ops::Range,
    ptr::NonNull,
    rc::Rc,
};
use winit::window::Window;
use xrack_layout::{Target, TextEdit, TextPosition};
use xrack_runtime::{
    InputPurpose, InputType, KeyboardCapitalization, Node, ReturnKey, TableCellEditor, TextCommand,
    TextCommandAvailability, Value, is_multiline_text_input,
};

pub(crate) struct NativeEdit {
    epoch: u64,
    serial: u64,
    pub(super) input: NativeInput,
}

pub(super) enum NativeInput {
    Preedit {
        value: String,
        marked: Range<usize>,
        selection: Range<usize>,
    },
    Commit {
        value: String,
        selection: Range<usize>,
    },
    Dismissed {
        committed: Option<(String, Range<usize>)>,
    },
    Command(TextCommand),
    Submit,
    NavigateField {
        reverse: bool,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct EditorKeyboard {
    pub(super) return_key: ReturnKey,
    pub(super) fields: Option<[bool; 2]>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
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
    fn for_node(node: &Node) -> Self {
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

    fn content_type(self, purpose: InputPurpose) -> Option<&'static UITextContentType> {
        // SAFETY: UIKit's text content type constants are immutable strings.
        unsafe {
            match (purpose, self) {
                (InputPurpose::Username, _) => Some(UITextContentTypeUsername),
                (InputPurpose::CurrentPassword, _) => Some(UITextContentTypePassword),
                (InputPurpose::NewPassword, _) => Some(UITextContentTypeNewPassword),
                (InputPurpose::OneTimeCode, _) => Some(UITextContentTypeOneTimeCode),
                (InputPurpose::Auto, Self::Email) => Some(UITextContentTypeEmailAddress),
                (InputPurpose::Auto, Self::Url) => Some(UITextContentTypeURL),
                (InputPurpose::Auto, Self::Tel) => Some(UITextContentTypeTelephoneNumber),
                _ => None,
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct KeyboardAssistance {
    capitalization: UITextAutocapitalizationType,
    correction: UITextAutocorrectionType,
    spelling: UITextSpellCheckingType,
}

impl KeyboardAssistance {
    fn for_node(node: &Node, mode: Mode, purpose: InputPurpose) -> Self {
        if mode == Mode::Password
            || matches!(
                purpose,
                InputPurpose::CurrentPassword
                    | InputPurpose::NewPassword
                    | InputPurpose::OneTimeCode
            )
        {
            return Self {
                capitalization: UITextAutocapitalizationType::None,
                correction: UITextAutocorrectionType::No,
                spelling: UITextSpellCheckingType::No,
            };
        }
        let prose = purpose == InputPurpose::Auto
            && matches!(mode, Mode::SingleLine | Mode::Multiline | Mode::Search);
        let capitalization = match node.keyboard_capitalization() {
            KeyboardCapitalization::Auto if prose => UITextAutocapitalizationType::Sentences,
            KeyboardCapitalization::Auto | KeyboardCapitalization::None => {
                UITextAutocapitalizationType::None
            }
            KeyboardCapitalization::Words => UITextAutocapitalizationType::Words,
            KeyboardCapitalization::Sentences => UITextAutocapitalizationType::Sentences,
            KeyboardCapitalization::Characters => UITextAutocapitalizationType::AllCharacters,
        };
        let correction = match node.properties.get("auto-correct") {
            Some(Value::Bool(true)) => UITextAutocorrectionType::Yes,
            Some(Value::Bool(false)) => UITextAutocorrectionType::No,
            _ if prose => UITextAutocorrectionType::Default,
            _ => UITextAutocorrectionType::No,
        };
        let spelling = match node.properties.get("spell-check") {
            Some(Value::Bool(true)) => UITextSpellCheckingType::Yes,
            Some(Value::Bool(false)) => UITextSpellCheckingType::No,
            _ if prose => UITextSpellCheckingType::Default,
            _ => UITextSpellCheckingType::No,
        };
        Self {
            capitalization,
            correction,
            spelling,
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

    fn set_accessory_view(&self, accessory: Option<&UIView>) -> bool {
        // SAFETY: Both responders and their shared toolbar belong to the UIKit main thread.
        unsafe {
            match self {
                Self::Field(field)
                    if field.inputAccessoryView().is_some() != accessory.is_some() =>
                {
                    field.setInputAccessoryView(accessory);
                }
                Self::View(view) if view.inputAccessoryView().is_some() != accessory.is_some() => {
                    view.setInputAccessoryView(accessory);
                }
                _ => return false,
            }
        }
        true
    }
}

#[derive(Default)]
struct Revision {
    active: Cell<Option<Mode>>,
    ended: Cell<bool>,
    epoch: Cell<u64>,
    serial: Cell<u64>,
    marked: Cell<bool>,
    updating_marked_text: Cell<bool>,
    commands: Cell<TextCommandAvailability>,
    select_word: Cell<bool>,
    select_all: Cell<bool>,
}

#[derive(Clone)]
pub(super) struct TextActions {
    revision: Rc<Revision>,
    pub(super) events: WindowEvents,
}

pub(super) struct CopyRevision {
    revision: Rc<Revision>,
    epoch: u64,
    serial: u64,
}

impl CopyRevision {
    pub(super) fn is_current(&self) -> bool {
        self.revision.epoch.get() == self.epoch
            && self.revision.serial.get() == self.serial
            && self.revision.active.get().is_some()
            && !self.revision.ended.get()
            && !self.revision.marked.get()
            && self.revision.commands.get().copy
    }
}

impl TextActions {
    fn post(&self, input: NativeInput) {
        self.events.post(UserEvent::IosText(NativeEdit {
            epoch: self.revision.epoch.get(),
            serial: self.revision.serial.get(),
            input,
        }));
    }

    pub(super) fn can_run(&self, command: TextCommand) -> bool {
        let revision = &self.revision;
        if revision.active.get().is_none() || revision.ended.get() || revision.marked.get() {
            return false;
        }
        let commands = revision.commands.get();
        match command {
            TextCommand::Undo => commands.undo,
            TextCommand::Redo => commands.redo,
            TextCommand::Cut => commands.cut,
            TextCommand::Copy => commands.copy,
            TextCommand::Paste => {
                // SAFETY: UIKit calls from the main thread. Check only the type;
                // the clipboard port reads data after the user invokes Paste.
                unsafe { UIPasteboard::generalPasteboard().hasStrings() }
            }
            TextCommand::SelectWord => revision.select_word.get(),
            TextCommand::SelectAll => revision.select_all.get(),
            _ => false,
        }
    }

    pub(super) fn run(&self, command: TextCommand) {
        if self.can_run(command) {
            self.post(NativeInput::Command(command));
        }
    }

    fn submit(&self) {
        if !self.revision.ended.get()
            && self
                .revision
                .active
                .get()
                .is_some_and(|mode| mode != Mode::Multiline)
        {
            self.post(NativeInput::Submit);
        }
    }

    fn navigate_field(&self, reverse: bool) {
        if !self.revision.ended.get()
            && !self.revision.marked.get()
            && self.revision.active.get().is_some()
        {
            self.post(NativeInput::NavigateField { reverse });
        }
    }

    pub(super) fn update_marked_text(
        &self,
        multiline: bool,
        input: &ProtocolObject<dyn UITextInput>,
        update: impl FnOnce(),
        read_value: impl FnOnce() -> String,
    ) {
        // UIKit can report the selected text's removal while installing its replacement.
        let nested = self.revision.updating_marked_text.replace(true);
        update();
        if !nested {
            self.revision.updating_marked_text.set(false);
            self.capture(multiline, input, read_value);
        }
    }

    fn capture(
        &self,
        multiline: bool,
        input: &ProtocolObject<dyn UITextInput>,
        read_value: impl FnOnce() -> String,
    ) {
        if self.revision.updating_marked_text.get() || !self.active_editor(multiline) {
            return;
        }
        let value = read_value();
        let selection = selected_bytes(input, &value);
        // SAFETY: UIKit delivers the notifications and delegate calls on the main thread.
        let marked = unsafe { input.markedTextRange() };
        self.revision.marked.set(marked.is_some());
        let update = if let Some(marked) = marked {
            let range = range_bytes(input, &value, &marked);
            let start = selection.start.saturating_sub(range.start).min(range.len());
            let end = selection.end.saturating_sub(range.start).min(range.len());
            NativeInput::Preedit {
                value,
                marked: range,
                selection: start..end,
            }
        } else {
            NativeInput::Commit { value, selection }
        };
        let serial = self.revision.serial.get().wrapping_add(1);
        self.revision.serial.set(serial);
        self.post(update);
    }

    fn active_editor(&self, multiline: bool) -> bool {
        !self.revision.ended.get()
            && self
                .revision
                .active
                .get()
                .is_some_and(|mode| (mode == Mode::Multiline) == multiline)
    }

    fn finish_editing(
        &self,
        multiline: bool,
        input: &ProtocolObject<dyn UITextInput>,
        read_value: impl FnOnce() -> String,
    ) {
        if !self.active_editor(multiline) {
            return;
        }
        // Latch UIKit's resignation before a keyboard-frame notification can relayout.
        self.revision.ended.set(true);
        // SAFETY: UIKit calls this delegate on the main thread after editing ends.
        let committed = unsafe { input.markedTextRange().is_none() }.then(|| {
            let value = read_value();
            let selection = selected_bytes(input, &value);
            (value, selection)
        });
        self.revision
            .serial
            .set(self.revision.serial.get().wrapping_add(1));
        self.post(NativeInput::Dismissed { committed });
    }
}

struct UndoState {
    actions: TextActions,
    epoch: u64,
}

impl UndoState {
    fn available(&self, command: TextCommand) -> bool {
        self.actions.revision.epoch.get() == self.epoch && self.actions.can_run(command)
    }

    fn replay(&self, command: TextCommand) {
        if self.available(command) {
            self.actions.post(NativeInput::Command(command));
        }
    }
}

declare_class!(
    struct TextUndoManager;

    unsafe impl ClassType for TextUndoManager {
        type Super = NSUndoManager;
        type Mutability = MainThreadOnly;
        const NAME: &'static str = "XRackTextUndoManager";
    }

    impl DeclaredClass for TextUndoManager {
        type Ivars = UndoState;
    }

    unsafe impl NSObjectProtocol for TextUndoManager {}

    unsafe impl TextUndoManager {
        #[method(canUndo)]
        fn can_undo(&self) -> bool {
            self.ivars().available(TextCommand::Undo)
        }

        #[method(canRedo)]
        fn can_redo(&self) -> bool {
            self.ivars().available(TextCommand::Redo)
        }

        #[method(undo)]
        fn undo(&self) {
            self.ivars().replay(TextCommand::Undo);
        }

        #[method(redo)]
        fn redo(&self) {
            self.ivars().replay(TextCommand::Redo);
        }
    }
);

declare_class!(
    struct TextDelegate;

    unsafe impl ClassType for TextDelegate {
        type Super = NSObject;
        type Mutability = MainThreadOnly;
        const NAME: &'static str = "XRackTextInputDelegate";
    }

    impl DeclaredClass for TextDelegate {
        type Ivars = TextActions;
    }

    unsafe impl NSObjectProtocol for TextDelegate {}

    unsafe impl TextDelegate {
        #[method(submitFromToolbar:)]
        fn submit_from_toolbar(&self, _: &UIBarButtonItem) {
            self.ivars().submit();
        }

        #[method(previousField:)]
        fn previous_field(&self, _: &UIBarButtonItem) {
            self.ivars().navigate_field(true);
        }

        #[method(nextField:)]
        fn next_field(&self, _: &UIBarButtonItem) {
            self.ivars().navigate_field(false);
        }
    }

    #[allow(non_snake_case)]
    unsafe impl UITextFieldDelegate for TextDelegate {
        #[method(textFieldShouldReturn:)]
        unsafe fn textFieldShouldReturn(&self, _: &UITextField) -> bool {
            self.ivars().submit();
            // XRack resolves validation and focus before changing the responder.
            false
        }

        #[method(textFieldDidChangeSelection:)]
        unsafe fn textFieldDidChangeSelection(&self, field: &UITextField) {
            self.ivars().capture(false, ProtocolObject::from_ref(field), || {
                // SAFETY: UIKit owns the field for this main-thread delegate call.
                unsafe { field.text() }.map_or_else(String::new, |value| value.to_string())
            });
        }

        #[method(textFieldDidEndEditing:)]
        unsafe fn textFieldDidEndEditing(&self, field: &UITextField) {
            self.ivars().finish_editing(false, ProtocolObject::from_ref(field), || {
                // SAFETY: UIKit owns the field during this main-thread delegate call.
                unsafe { field.text() }.map_or_else(String::new, |value| value.to_string())
            });
        }
    }

    unsafe impl UIScrollViewDelegate for TextDelegate {}

    #[allow(non_snake_case)]
    unsafe impl UITextViewDelegate for TextDelegate {
        #[method(textViewDidChangeSelection:)]
        unsafe fn textViewDidChangeSelection(&self, view: &UITextView) {
            self.ivars().capture(true, ProtocolObject::from_ref(view), || {
                // SAFETY: UIKit owns the view for this main-thread delegate call.
                unsafe { view.text() }.to_string()
            });
        }

        #[method(textViewDidEndEditing:)]
        unsafe fn textViewDidEndEditing(&self, view: &UITextView) {
            self.ivars().finish_editing(true, ProtocolObject::from_ref(view), || {
                // SAFETY: UIKit owns the view during this main-thread delegate call.
                unsafe { view.text() }.to_string()
            });
        }
    }
);

pub(super) struct TextInput {
    field: Responder,
    multiline: Responder,
    submit_toolbar: SubmitToolbar,
    delegate: Retained<TextDelegate>,
    undo_manager: crate::ios_keyboard::UndoManagerSlot,
    center: Retained<NSNotificationCenter>,
    observers: [Retained<NSObject>; 3],
    revision: Rc<Revision>,
    applied: u64,
    focused: Option<(Target, Mode)>,
    observed_value: String,
    keyboard: EditorKeyboard,
    purpose: InputPurpose,
    assistance: Option<KeyboardAssistance>,
}

impl TextInput {
    pub(super) fn new(window: &Window, events: &WindowEvents) -> Option<Self> {
        let view = ios::view(window)?;
        let mtm = MainThreadMarker::new()?;
        let revision = Rc::new(Revision::default());

        // SAFETY: UIKit views and notifications here are confined to the main thread.
        unsafe {
            view.keyboardLayoutGuide().setFollowsUndockedKeyboard(true);
            let undo_manager = Rc::new(RefCell::new(None));
            let actions = TextActions {
                revision: revision.clone(),
                events: events.clone(),
            };
            let field = crate::ios_keyboard::text_field(mtm, actions.clone(), undo_manager.clone());
            let delegate = mtm.alloc::<TextDelegate>().set_ivars(actions.clone());
            let delegate: Retained<TextDelegate> = msg_send_id![super(delegate), init];
            field.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            let submit_toolbar = submit_toolbar(mtm, &delegate);
            let field = Responder::Field(field);
            let multiline =
                crate::ios_keyboard::text_view(mtm, actions.clone(), undo_manager.clone());
            multiline.setScrollsToTop(false);
            multiline.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            let multiline = Responder::View(multiline);
            for responder in [&field, &multiline] {
                let input_view = responder.view();
                input_view.setFrame(CGRect::new(CGPoint::ZERO, CGSize::new(1.0, 1.0)));
                input_view.setAlpha(0.02);
                input_view.setIsAccessibilityElement(false);
                view.addSubview(input_view);
            }

            let center = NSNotificationCenter::defaultCenter();
            let queue = NSOperationQueue::mainQueue();
            let field_change = text_changes(field.clone(), false, actions.clone());
            let view_change = text_changes(multiline.clone(), true, actions);
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
                delegate,
                undo_manager,
                center,
                observers,
                revision,
                applied: 0,
                focused: None,
                observed_value: String::new(),
                keyboard: EditorKeyboard {
                    return_key: ReturnKey::Auto,
                    fields: None,
                },
                purpose: InputPurpose::Auto,
                assistance: None,
            })
        }
    }

    fn responder(&self, mode: Mode) -> &Responder {
        match mode {
            Mode::Multiline => &self.multiline,
            _ => &self.field,
        }
    }

    pub(super) fn discard_stale_edits(&mut self, target: &Target, value: &str) -> bool {
        let Some((_, mode)) = self
            .focused
            .as_ref()
            .filter(|(current, _)| current == target)
        else {
            return false;
        };
        let mode = *mode;
        if self.revision.active.get() != Some(mode) || self.observed_value == value {
            return false;
        }
        // SAFETY: The installed responder is owned by the UIKit main thread.
        let marked = unsafe { self.responder(mode).input().markedTextRange().is_some() };
        if !marked && self.applied == self.revision.serial.get() {
            return false;
        }

        // A change from XRack supersedes this native session and its queued edits.
        let ended = self.revision.ended.get();
        self.revision.active.set(None);
        self.invalidate_edits();
        if ended {
            self.revision.ended.set(true);
            self.delegate
                .ivars()
                .post(NativeInput::Dismissed { committed: None });
        }
        if marked {
            // SAFETY: This main-thread call may notify UIKit delegates immediately.
            // Capture is disabled while the composition is cleared.
            unsafe { self.responder(mode).input().unmarkText() };
        }
        true
    }

    pub(super) fn focus(
        &mut self,
        target: &Target,
        node: &Node,
        keyboard: EditorKeyboard,
        value: &str,
        selection: Range<usize>,
        commands: TextCommandAvailability,
    ) -> bool {
        let mode = Mode::for_node(node);
        let purpose = node.input_purpose();
        let assistance = KeyboardAssistance::for_node(node, mode, purpose);
        let changed = self
            .focused
            .as_ref()
            .is_none_or(|(current, kind)| current != target || *kind != mode);
        if !changed && self.revision.ended.get() {
            // The queued end-edit event still owns the editor until it is reconciled.
            return true;
        }
        if changed {
            self.revision.active.set(None);
            if let Some((_, previous)) = self.focused.take() {
                let previous_responder = self.responder(previous);
                // SAFETY: Clear the old UIKit composition on the main thread before handing over focus.
                unsafe { previous_responder.input().unmarkText() };
                previous_responder.set_value("");
                if (previous == Mode::Multiline) != (mode == Mode::Multiline) {
                    previous_responder.set_accessory_view(None);
                }
            }
            self.focused = Some((target.clone(), mode));
            self.invalidate_edits();
        }

        self.revision.commands.set(commands);
        self.revision
            .select_word
            .set(!value.is_empty() && selection.is_empty());
        self.revision.select_all.set(selection.len() < value.len());
        if self.undo_manager.borrow().is_none() {
            let mtm = MainThreadMarker::from(&*self.delegate);
            let manager = mtm.alloc::<TextUndoManager>().set_ivars(UndoState {
                actions: self.delegate.ivars().clone(),
                epoch: self.revision.epoch.get(),
            });
            // SAFETY: The responder and manager are created on the UIKit main thread.
            unsafe {
                let manager: Retained<TextUndoManager> = msg_send_id![super(manager), init];
                // UIKit edits a mirrored buffer. XRack alone records user undo history.
                manager.disableUndoRegistration();
                self.undo_manager
                    .replace(Some(Retained::into_super(manager)));
            }
        }

        // The keyboard can be reloaded after a composition commits.
        let purpose_changed = self.purpose != purpose;
        // SAFETY: The responder is owned and used by the UIKit main thread.
        let marked = unsafe { self.responder(mode).input().markedTextRange().is_some() };
        let update_traits = !marked
            && (changed
                || self.keyboard != keyboard
                || purpose_changed
                || self.assistance != Some(assistance));
        if update_traits {
            self.keyboard = keyboard;
            self.purpose = purpose;
            self.assistance = Some(assistance);
        }
        self.submit_toolbar
            .set_navigation_enabled(self.keyboard.fields, !marked);
        let responder = self.responder(mode);
        let input = responder.input();
        // UIKit can notify its text observers during selection synchronization.
        // Those notifications reflect the responder before our update completes.
        self.revision.active.set(None);
        // SAFETY: All responder access is on the UIKit main thread.
        let active = unsafe {
            if update_traits {
                let keyboard_type = mode.keyboard();
                let key_type = mode.return_key(keyboard.return_key);
                let content_type = mode.content_type(purpose);
                let secure = mode == Mode::Password;
                let secure_changed = input.isSecureTextEntry() != secure;
                let mut needs_reload = input.keyboardType() != keyboard_type
                    || input.returnKeyType() != key_type
                    || secure_changed
                    || purpose_changed
                    || input.autocapitalizationType() != assistance.capitalization
                    || input.autocorrectionType() != assistance.correction
                    || input.spellCheckingType() != assistance.spelling;
                if secure_changed {
                    input.setSecureTextEntry(secure);
                }
                input.setKeyboardType(keyboard_type);
                input.setReturnKeyType(key_type);
                input.setTextContentType(content_type);
                input.setAutocapitalizationType(assistance.capitalization);
                input.setAutocorrectionType(assistance.correction);
                input.setSpellCheckingType(assistance.spelling);
                let uses_toolbar = self.submit_toolbar.configure(mode, keyboard);
                let toolbar: &UIView = &self.submit_toolbar.view;
                needs_reload |= responder.set_accessory_view(uses_toolbar.then_some(toolbar));
                if needs_reload && responder.view().isFirstResponder() {
                    responder.view().reloadInputViews();
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
        if self.observed_value != value {
            value.clone_into(&mut self.observed_value);
        }
        self.revision.active.set(active.then_some(mode));
        active
    }

    pub(super) fn clear_focus(&mut self) {
        let Some((_, mode)) = self.focused.take() else {
            return;
        };
        self.observed_value = String::new();
        self.revision.active.set(None);
        self.invalidate_edits();
        let responder = self.responder(mode);
        // SAFETY: Responder changes are confined to the UIKit main thread.
        unsafe { responder.view().resignFirstResponder() };
        responder.set_accessory_view(None);
        responder.set_value("");
    }

    pub(super) fn editing(&self, target: &Target) -> bool {
        self.revision.active.get().is_some()
            && self
                .focused
                .as_ref()
                .is_some_and(|(focused, _)| focused == target)
    }

    pub(super) fn copy_revision(&self) -> Option<CopyRevision> {
        let revision = CopyRevision {
            revision: self.revision.clone(),
            epoch: self.revision.epoch.get(),
            serial: self.applied,
        };
        revision.is_current().then_some(revision)
    }

    pub(super) fn can_dismiss_for_scroll(&self, target: &Target) -> bool {
        if self.applied != self.revision.serial.get() {
            return false;
        }
        self.focused.as_ref().is_some_and(|(focused, mode)| {
            focused == target
                && self.revision.active.get() == Some(*mode)
                // SAFETY: The editor belongs to the UIKit main thread.
                && unsafe { self.responder(*mode).input().markedTextRange().is_none() }
        })
    }

    pub(super) fn accessory_frame(&self, view: &UIView) -> Option<CGRect> {
        if !self
            .revision
            .active
            .get()
            .is_some_and(|mode| mode.has_done_toolbar() || self.keyboard.fields.is_some())
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

    fn invalidate_edits(&mut self) {
        self.undo_manager.take();
        self.revision.ended.set(false);
        self.revision.marked.set(false);
        self.revision
            .commands
            .set(TextCommandAvailability::default());
        self.revision.select_word.set(false);
        self.revision.select_all.set(false);
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
        self.revision.active.set(None);
        self.invalidate_edits();
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
    previous: Retained<UIBarButtonItem>,
    following: Retained<UIBarButtonItem>,
    done: Retained<UIBarButtonItem>,
    submit_next: Retained<UIBarButtonItem>,
    space: Retained<UIBarButtonItem>,
    arrangement: Cell<Option<(bool, Option<ReturnKey>)>>,
}

impl SubmitToolbar {
    fn configure(&self, mode: Mode, keyboard: EditorKeyboard) -> bool {
        let show_navigation = keyboard.fields.is_some();
        let submit = mode
            .has_done_toolbar()
            .then_some(if keyboard.return_key == ReturnKey::Next {
                ReturnKey::Next
            } else {
                ReturnKey::Done
            });
        let show = show_navigation || submit.is_some();
        let requested = show.then_some((show_navigation, submit));
        if self.arrangement.replace(requested) != requested && show {
            let mut items = Vec::new();
            if show_navigation {
                items.push(&*self.previous);
                if submit != Some(ReturnKey::Next) {
                    items.push(&*self.following);
                }
            }
            if let Some(submit) = submit {
                items.push(&*self.space);
                items.push(if submit == ReturnKey::Next {
                    &*self.submit_next
                } else {
                    &*self.done
                });
            }
            // SAFETY: The toolbar and its items belong to the UIKit main thread.
            unsafe { self.view.setItems(Some(&NSArray::from_slice(&items))) };
        }
        show
    }

    fn set_navigation_enabled(&self, fields: Option<[bool; 2]>, ready: bool) {
        let [previous, next] = fields.unwrap_or_default();
        // SAFETY: UIKit owns these toolbar items on the application's main thread.
        unsafe {
            for (item, enabled) in [(&self.previous, previous), (&self.following, next)] {
                let enabled = ready && enabled;
                if item.isEnabled() != enabled {
                    item.setEnabled(enabled);
                }
            }
        }
    }
}

fn submit_toolbar(mtm: MainThreadMarker, delegate: &TextDelegate) -> SubmitToolbar {
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
        let bundle = NSBundle::mainBundle();
        let button = |name, action| {
            let title =
                bundle.localizedStringForKey_value_table(&NSString::from_str(name), None, None);
            UIBarButtonItem::initWithTitle_style_target_action(
                mtm.alloc(),
                Some(&title),
                UIBarButtonItemStyle::Plain,
                Some(delegate),
                Some(action),
            )
        };
        let space = UIBarButtonItem::initWithBarButtonSystemItem_target_action(
            mtm.alloc(),
            UIBarButtonSystemItem::FlexibleSpace,
            None,
            None,
        );
        toolbar.setFrame(CGRect::new(CGPoint::ZERO, CGSize::new(320.0, 44.0)));
        SubmitToolbar {
            view: toolbar,
            previous: button("Previous field", sel!(previousField:)),
            following: button("Next field", sel!(nextField:)),
            done,
            submit_next: button("Next", sel!(submitFromToolbar:)),
            space,
            arrangement: Cell::new(None),
        }
    }
}

fn text_changes(
    responder: Responder,
    multiline: bool,
    actions: TextActions,
) -> RcBlock<dyn Fn(NonNull<NSNotification>)> {
    RcBlock::new(move |_: NonNull<NSNotification>| {
        actions.capture(multiline, responder.input(), || responder.value());
    })
}

pub(super) fn apply_marked_text(
    editor: &mut TextEdit,
    committed: &str,
    value: &str,
    marked: Range<usize>,
    selection: Range<usize>,
) {
    let prefix = &value[..marked.start];
    let suffix = &value[marked.end..];
    let text = &value[marked];
    // The selection snapshot may have been superseded by this marked-text
    // snapshot. Recover the replaced bytes from the unchanged surroundings.
    if let Some(replaced) = committed
        .strip_prefix(prefix)
        .and_then(|rest| rest.strip_suffix(suffix))
    {
        let end = prefix.len() + replaced.len();
        let range = cover_graphemes(committed, prefix.len()..end);
        let before = &committed[range.start..prefix.len()];
        let after = &committed[end..range.end];
        editor.set_selection(
            committed,
            range.start,
            TextPosition {
                offset: range.end,
                trailing: false,
            },
        );
        editor.set_preedit(
            [before, text, after].concat(),
            Some((selection.start + before.len(), selection.end + before.len())),
        );
    } else {
        editor.set_preedit(text.to_owned(), Some((selection.start, selection.end)));
    }
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
