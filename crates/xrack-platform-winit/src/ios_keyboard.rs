use crate::{UserEvent, desktop::WindowEvents, ios_text::TextActions, keyboard::KeyEvent};
use objc2::{
    ClassType, DeclaredClass, declare_class, msg_send, msg_send_id,
    mutability::MainThreadOnly,
    rc::Retained,
    runtime::{AnyObject, Bool, NSObjectProtocol, ProtocolObject, Sel},
    sel,
};
use objc2_foundation::{
    CGPoint, CGRect, CGSize, MainThreadMarker, NSArray, NSRange, NSSet, NSString, NSUndoManager,
};
use objc2_ui_kit::{
    NSObjectUIAccessibility, UIKey, UIKeyModifierFlags, UIKeyboardHIDUsage, UIPress,
    UIPressesEvent, UITextField, UITextInput, UITextView, UIView,
};
use std::{cell::RefCell, rc::Rc};
use winit::{
    event::ElementState,
    keyboard::{Key, ModifiersState, NamedKey},
    window::Window,
};

use xrack_runtime::TextCommand;

pub(super) struct KeyboardInput(Retained<ControlResponder>);

impl KeyboardInput {
    pub(super) fn new(window: &Window, events: WindowEvents) -> Option<Self> {
        let view = crate::ios::view(window)?;
        let mtm = MainThreadMarker::new()?;
        let allocated = mtm.alloc::<ControlResponder>().set_ivars(events);
        // SAFETY: Initialize a main-thread UIView subclass with its Rust ivars.
        let responder: Retained<ControlResponder> = unsafe { msg_send_id![super(allocated), init] };
        // SAFETY: This view receives hardware keys while controls have logical focus.
        unsafe {
            responder.setFrame(CGRect::new(CGPoint::ZERO, CGSize::new(1.0, 1.0)));
            responder.setAlpha(0.02);
            responder.setIsAccessibilityElement(false);
            view.addSubview(&responder);
        }
        Some(Self(responder))
    }

    pub(super) fn focus(&self, active: bool) {
        // SAFETY: WindowState and the responder belong to the UIKit main thread.
        unsafe {
            if active && !self.0.isFirstResponder() {
                self.0.becomeFirstResponder();
            } else if !active && self.0.isFirstResponder() {
                self.0.resignFirstResponder();
            }
        }
    }
}

impl Drop for KeyboardInput {
    fn drop(&mut self) {
        self.focus(false);
        // SAFETY: The owning window releases its responder on the main thread.
        unsafe { self.0.removeFromSuperview() };
    }
}

declare_class!(
    struct ControlResponder;

    unsafe impl ClassType for ControlResponder {
        #[inherits(objc2_ui_kit::UIResponder, objc2::runtime::NSObject)]
        type Super = UIView;
        type Mutability = MainThreadOnly;
        const NAME: &'static str = "XRackControlKeyResponder";
    }

    impl DeclaredClass for ControlResponder {
        type Ivars = WindowEvents;
    }

    unsafe impl NSObjectProtocol for ControlResponder {}

    unsafe impl ControlResponder {
        #[method(canBecomeFirstResponder)]
        fn can_become_first_responder(&self) -> bool {
            true
        }

        #[method(pressesBegan:withEvent:)]
        fn presses_began(&self, presses: &NSSet<UIPress>, _: Option<&UIPressesEvent>) {
            self.deliver(presses, ElementState::Pressed);
        }

        #[method(pressesEnded:withEvent:)]
        fn presses_ended(&self, presses: &NSSet<UIPress>, _: Option<&UIPressesEvent>) {
            self.deliver(presses, ElementState::Released);
        }

        #[method(pressesCancelled:withEvent:)]
        fn presses_cancelled(&self, _: &NSSet<UIPress>, _: Option<&UIPressesEvent>) {
            self.ivars().post(UserEvent::IosKeyCancelled);
        }
    }
);

impl ControlResponder {
    fn deliver(&self, presses: &NSSet<UIPress>, state: ElementState) {
        for press in presses {
            // SAFETY: UIKit owns each press and its key during the responder callback.
            let Some(key) = (unsafe { press.key(MainThreadMarker::from(self)) }) else {
                continue;
            };
            let (event, modifiers) = key_event(&key, state);
            self.ivars()
                .post(UserEvent::IosControlKey(event, modifiers));
        }
    }
}

pub(super) type UndoManagerSlot = Rc<RefCell<Option<Retained<NSUndoManager>>>>;

struct TextResponderState {
    actions: TextActions,
    undo_manager: UndoManagerSlot,
}

pub(super) fn text_field(
    mtm: MainThreadMarker,
    actions: TextActions,
    undo_manager: UndoManagerSlot,
) -> Retained<UITextField> {
    let allocated = mtm.alloc::<FieldResponder>().set_ivars(TextResponderState {
        actions,
        undo_manager,
    });
    // SAFETY: Initialize the declared subclass and upcast to its UIKit superclass.
    unsafe {
        let field: Retained<FieldResponder> = msg_send_id![super(allocated), init];
        Retained::cast(field)
    }
}

pub(super) fn text_view(
    mtm: MainThreadMarker,
    actions: TextActions,
    undo_manager: UndoManagerSlot,
) -> Retained<UITextView> {
    let allocated = mtm
        .alloc::<MultilineResponder>()
        .set_ivars(TextResponderState {
            actions,
            undo_manager,
        });
    // SAFETY: Initialize the declared subclass and upcast to its UIKit superclass.
    unsafe {
        let view: Retained<MultilineResponder> = msg_send_id![super(allocated), init];
        Retained::cast(view)
    }
}

fn command_for_action(action: Sel) -> Option<TextCommand> {
    [
        (sel!(cut:), TextCommand::Cut),
        (sel!(copy:), TextCommand::Copy),
        (sel!(paste:), TextCommand::Paste),
        (sel!(select:), TextCommand::SelectWord),
        (sel!(selectAll:), TextCommand::SelectAll),
    ]
    .into_iter()
    .find_map(|(selector, command)| (selector == action).then_some(command))
}

macro_rules! text_responder {
    ($name:ident, $superclass:ident, $class_name:literal, $multiline:literal) => {
        declare_class!(
            struct $name;

            unsafe impl ClassType for $name {
                type Super = $superclass;
                type Mutability = MainThreadOnly;
                const NAME: &'static str = $class_name;
            }
            impl DeclaredClass for $name { type Ivars = TextResponderState; }
            unsafe impl NSObjectProtocol for $name {}

            unsafe impl $name {

                #[method(canPerformAction:withSender:)]
                fn can_perform_action(&self, action: Sel, sender: Option<&AnyObject>) -> bool {
                    if let Some(command) = command_for_action(action) {
                        self.editor_is_ready() && self.ivars().actions.can_run(command)
                    } else {
                        // SAFETY: Keep UIKit's behavior for actions outside XRack's edit commands.
                        let allowed: Bool = unsafe { msg_send![super(self), canPerformAction: action withSender: sender] };
                        allowed.as_bool()
                    }
                }

                #[method(cut:)]
                fn cut(&self, _: Option<&AnyObject>) {
                    self.run_command(TextCommand::Cut);
                }

                #[method(copy:)]
                fn copy(&self, _: Option<&AnyObject>) {
                    self.run_command(TextCommand::Copy);
                }

                #[method(paste:)]
                fn paste(&self, _: Option<&AnyObject>) {
                    self.run_command(TextCommand::Paste);
                }

                #[method(select:)]
                fn select(&self, _: Option<&AnyObject>) {
                    self.run_command(TextCommand::SelectWord);
                }

                #[method(selectAll:)]
                fn select_all(&self, _: Option<&AnyObject>) {
                    self.run_command(TextCommand::SelectAll);
                }

                #[method(setMarkedText:selectedRange:)]
                fn set_marked_text(&self, text: Option<&NSString>, range: NSRange) {
                    self.update_marked_text(|| {
                        // SAFETY: UIKit owns the responder and input data on its main thread.
                        unsafe { let _: () = msg_send![super(self), setMarkedText: text selectedRange: range]; }
                    });
                }

                #[method(unmarkText)]
                fn unmark_text(&self) {
                    self.update_marked_text(|| {
                        // SAFETY: Only the UIKit superclass mutates the marked range.
                        unsafe { let _: () = msg_send![super(self), unmarkText]; }
                    });
                }

                #[method_id(undoManager)]
                fn undo_manager(&self) -> Option<Retained<NSUndoManager>> {
                    // SAFETY: UIKit queries this responder on the main thread.
                    if unsafe { self.isFirstResponder() } {
                        self.ivars().undo_manager.borrow().clone()
                    } else {
                        None
                    }
                }

                #[method(pressesBegan:withEvent:)]
                fn presses_began(&self, presses: &NSSet<UIPress>, event: Option<&UIPressesEvent>) {
                    let unhandled = editor_keys(&self.ivars().actions.events, presses, ElementState::Pressed,
                        unsafe { self.markedTextRange().is_some() }, MainThreadMarker::from(self));
                    if !unhandled.is_empty() {
                        unsafe { let _: () = msg_send![super(self), pressesBegan: &*unhandled withEvent: event]; }
                    }
                }

                #[method(pressesEnded:withEvent:)]
                fn presses_ended(&self, presses: &NSSet<UIPress>, event: Option<&UIPressesEvent>) {
                    let unhandled = editor_keys(&self.ivars().actions.events, presses, ElementState::Released,
                        unsafe { self.markedTextRange().is_some() }, MainThreadMarker::from(self));
                    if !unhandled.is_empty() {
                        unsafe { let _: () = msg_send![super(self), pressesEnded: &*unhandled withEvent: event]; }
                    }
                }

                #[method(pressesCancelled:withEvent:)]
                fn presses_cancelled(&self, presses: &NSSet<UIPress>, event: Option<&UIPressesEvent>) {
                    self.ivars().actions.events.post(UserEvent::IosKeyCancelled);
                    unsafe { let _: () = msg_send![super(self), pressesCancelled: presses withEvent: event]; }
                }
            }
        );

        impl $name {
            fn update_marked_text(&self, update: impl FnOnce()) {
                let input = ProtocolObject::from_ref(&**self);
                self.ivars().actions.update_marked_text($multiline, input, update, || {
                    // SAFETY: Both UIKit responder classes expose text through this getter.
                    let value: Option<Retained<NSString>> = unsafe { msg_send_id![self, text] };
                    value.map_or_else(String::new, |text| text.to_string())
                });
            }

            fn editor_is_ready(&self) -> bool {
                // SAFETY: UIKit owns these responders on its main thread.
                unsafe { self.isFirstResponder() && self.markedTextRange().is_none() }
            }

            fn run_command(&self, command: TextCommand) {
                if self.editor_is_ready() {
                    self.ivars().actions.run(command);
                }
            }
        }
    };
}

text_responder!(FieldResponder, UITextField, "XRackKeyTextField", false);
text_responder!(MultilineResponder, UITextView, "XRackKeyTextView", true);

fn editor_keys(
    events: &WindowEvents,
    presses: &NSSet<UIPress>,
    state: ElementState,
    composing: bool,
    mtm: MainThreadMarker,
) -> Retained<NSSet<UIPress>> {
    let mut unhandled = Vec::new();
    for press in presses {
        // SAFETY: UIKit owns these keys during the main-thread press callback.
        let routed = unsafe { press.key(mtm) }.filter(|key| {
            let usage = unsafe { key.keyCode() };
            let modifier = matches!(
                usage,
                UIKeyboardHIDUsage::KeyboardLeftShift
                    | UIKeyboardHIDUsage::KeyboardRightShift
                    | UIKeyboardHIDUsage::KeyboardLeftControl
                    | UIKeyboardHIDUsage::KeyboardRightControl
                    | UIKeyboardHIDUsage::KeyboardLeftAlt
                    | UIKeyboardHIDUsage::KeyboardRightAlt
                    | UIKeyboardHIDUsage::KeyboardLeftGUI
                    | UIKeyboardHIDUsage::KeyboardRightGUI
            );
            if composing && !modifier {
                return false;
            }
            let flags = unsafe { key.modifierFlags() };
            let command = flags.intersects(
                UIKeyModifierFlags::UIKeyModifierControl | UIKeyModifierFlags::UIKeyModifierCommand,
            );
            let navigation = matches!(
                unsafe { key.keyCode() },
                UIKeyboardHIDUsage::KeyboardTab
                    | UIKeyboardHIDUsage::KeyboardEscape
                    | UIKeyboardHIDUsage::KeyboardLeftArrow
                    | UIKeyboardHIDUsage::KeyboardRightArrow
                    | UIKeyboardHIDUsage::KeyboardUpArrow
                    | UIKeyboardHIDUsage::KeyboardDownArrow
                    | UIKeyboardHIDUsage::KeyboardHome
                    | UIKeyboardHIDUsage::KeyboardEnd
                    | UIKeyboardHIDUsage::KeyboardPageUp
                    | UIKeyboardHIDUsage::KeyboardPageDown
                    | UIKeyboardHIDUsage::KeyboardReturnOrEnter
                    | UIKeyboardHIDUsage::KeyboardReturn
                    | UIKeyboardHIDUsage::KeypadEnter
            );
            command || navigation || modifier
        });
        if let Some(key) = routed {
            let (event, modifiers) = key_event(&key, state);
            events.post(UserEvent::IosControlKey(event, modifiers));
        } else {
            unhandled.push(press);
        }
    }
    // SAFETY: This is a subset of UIKit's press set, retaining the same press identities.
    unsafe { NSSet::setWithArray(&NSArray::from_slice(&unhandled)) }
}

fn key_event(key: &UIKey, state: ElementState) -> (KeyEvent, ModifiersState) {
    // SAFETY: The UIKey is borrowed from the UIKit responder callback.
    let (usage, flags, characters, unmodified) = unsafe {
        (
            key.keyCode(),
            key.modifierFlags(),
            key.characters(),
            key.charactersIgnoringModifiers(),
        )
    };
    let named = match usage {
        UIKeyboardHIDUsage::KeyboardLeftShift | UIKeyboardHIDUsage::KeyboardRightShift => {
            Some(NamedKey::Shift)
        }
        UIKeyboardHIDUsage::KeyboardLeftControl | UIKeyboardHIDUsage::KeyboardRightControl => {
            Some(NamedKey::Control)
        }
        UIKeyboardHIDUsage::KeyboardLeftAlt | UIKeyboardHIDUsage::KeyboardRightAlt => {
            Some(NamedKey::Alt)
        }
        UIKeyboardHIDUsage::KeyboardLeftGUI | UIKeyboardHIDUsage::KeyboardRightGUI => {
            Some(NamedKey::Super)
        }
        UIKeyboardHIDUsage::KeyboardTab => Some(NamedKey::Tab),
        UIKeyboardHIDUsage::KeyboardEscape => Some(NamedKey::Escape),
        UIKeyboardHIDUsage::KeyboardLeftArrow => Some(NamedKey::ArrowLeft),
        UIKeyboardHIDUsage::KeyboardRightArrow => Some(NamedKey::ArrowRight),
        UIKeyboardHIDUsage::KeyboardUpArrow => Some(NamedKey::ArrowUp),
        UIKeyboardHIDUsage::KeyboardDownArrow => Some(NamedKey::ArrowDown),
        UIKeyboardHIDUsage::KeyboardReturnOrEnter
        | UIKeyboardHIDUsage::KeyboardReturn
        | UIKeyboardHIDUsage::KeypadEnter => Some(NamedKey::Enter),
        UIKeyboardHIDUsage::KeyboardSpacebar => Some(NamedKey::Space),
        UIKeyboardHIDUsage::KeyboardDeleteOrBackspace => Some(NamedKey::Backspace),
        UIKeyboardHIDUsage::KeyboardDeleteForward => Some(NamedKey::Delete),
        UIKeyboardHIDUsage::KeyboardHome => Some(NamedKey::Home),
        UIKeyboardHIDUsage::KeyboardEnd => Some(NamedKey::End),
        UIKeyboardHIDUsage::KeyboardPageUp => Some(NamedKey::PageUp),
        UIKeyboardHIDUsage::KeyboardPageDown => Some(NamedKey::PageDown),
        _ => None,
    };
    let text = characters.to_string();
    let logical_key =
        named.map_or_else(|| Key::Character(unmodified.to_string().into()), Key::Named);
    (
        KeyEvent {
            unmodified_key: logical_key.clone(),
            logical_key,
            text: (state == ElementState::Pressed && !text.is_empty()).then_some(text),
            state,
            repeat: false,
        },
        modifiers(flags),
    )
}

pub(super) fn modifiers(flags: UIKeyModifierFlags) -> ModifiersState {
    let mut modifiers = ModifiersState::empty();
    for (native, flag) in [
        (
            UIKeyModifierFlags::UIKeyModifierShift,
            ModifiersState::SHIFT,
        ),
        (
            UIKeyModifierFlags::UIKeyModifierControl,
            ModifiersState::CONTROL,
        ),
        (
            UIKeyModifierFlags::UIKeyModifierAlternate,
            ModifiersState::ALT,
        ),
        (
            UIKeyModifierFlags::UIKeyModifierCommand,
            ModifiersState::SUPER,
        ),
    ] {
        modifiers.set(flag, flags.contains(native));
    }
    modifiers
}
