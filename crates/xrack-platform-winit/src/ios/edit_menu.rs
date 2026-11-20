use crate::{UserEvent, desktop::WindowEvents};
use block2::RcBlock;
use objc2::{
    ClassType, DeclaredClass, declare_class, msg_send, msg_send_id,
    mutability::MainThreadOnly,
    rc::Retained,
    runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject},
};
use objc2_foundation::{CGPoint, CGRect, CGSize, MainThreadMarker, NSArray, NSString};
use objc2_ui_kit::{
    UIAction, UIEditMenuConfiguration, UIEditMenuInteraction, UIEditMenuInteractionAnimating,
    UIEditMenuInteractionDelegate, UIInteraction, UIMenu, UIMenuElement,
};
use std::{cell::RefCell, ptr::NonNull};
use winit::window::Window;
use xrack_layout::{NativeTextMenu, Rect};

struct PresentedMenu {
    model: NativeTextMenu,
    identifier: Retained<NSString>,
    menu: Retained<UIMenu>,
    anchor: Rect,
}

impl PresentedMenu {
    fn matches(&self, configuration: &UIEditMenuConfiguration) -> bool {
        // SAFETY: UIKit returns the immutable identifier used by this configuration.
        unsafe {
            let matches: Bool = msg_send![&*configuration.identifier(), isEqual: &*self.identifier];
            matches.as_bool()
        }
    }
}

struct DelegateState {
    events: WindowEvents,
    active: RefCell<Option<PresentedMenu>>,
}

declare_class!(
    struct EditMenuDelegate;

    unsafe impl ClassType for EditMenuDelegate {
        type Super = NSObject;
        type Mutability = MainThreadOnly;
        const NAME: &'static str = "XRackEditMenuDelegate";
    }

    impl DeclaredClass for EditMenuDelegate {
        type Ivars = DelegateState;
    }

    unsafe impl NSObjectProtocol for EditMenuDelegate {}

    #[allow(non_snake_case)]
    unsafe impl UIEditMenuInteractionDelegate for EditMenuDelegate {
        #[method_id(editMenuInteraction:menuForConfiguration:suggestedActions:)]
        fn editMenuInteraction_menuForConfiguration_suggestedActions(
            &self,
            _: &UIEditMenuInteraction,
            configuration: &UIEditMenuConfiguration,
            _: &NSArray<UIMenuElement>,
        ) -> Option<Retained<UIMenu>> {
            self.ivars().active.borrow().as_ref()
                .filter(|active| active.matches(configuration))
                .map(|active| active.menu.clone())
        }

        #[method(editMenuInteraction:targetRectForConfiguration:)]
        fn editMenuInteraction_targetRectForConfiguration(
            &self,
            _: &UIEditMenuInteraction,
            configuration: &UIEditMenuConfiguration,
        ) -> CGRect {
            let active = self.ivars().active.borrow();
            if let Some(menu) = active.as_ref().filter(|menu| menu.matches(configuration)) {
                let rect = menu.anchor;
                CGRect::new(CGPoint::new(f64::from(rect.x), f64::from(rect.y)),
                            CGSize::new(f64::from(rect.width), f64::from(rect.height)))
            } else {
                // SAFETY: UIKit supplies its live configuration on the main thread.
                CGRect::new(unsafe { configuration.sourcePoint() }, CGSize::ZERO)
            }
        }

        #[method(editMenuInteraction:willDismissMenuForConfiguration:animator:)]
        fn editMenuInteraction_willDismissMenuForConfiguration_animator(
            &self,
            _: &UIEditMenuInteraction,
            configuration: &UIEditMenuConfiguration,
            animator: &ProtocolObject<dyn UIEditMenuInteractionAnimating>,
        ) {
            let state = self.ivars();
            let Some(session) = state.active.borrow().as_ref()
                .filter(|active| active.matches(configuration))
                .map(|active| active.model.session()) else { return };
            let events = state.events.clone();
            let complete = RcBlock::new(move || {
                events.post(UserEvent::IosTextMenu(session, None));
            });
            // SAFETY: The system retains its completion block across the transition.
            unsafe { animator.addCompletion(&complete) };
        }
    }
);

pub(crate) struct EditMenu {
    interaction: Retained<UIEditMenuInteraction>,
    delegate: Retained<EditMenuDelegate>,
}

impl EditMenu {
    pub(crate) fn new(window: &Window, events: WindowEvents) -> Option<Self> {
        let view = super::view(window)?;
        let mtm = MainThreadMarker::new()?;
        let delegate = mtm.alloc::<EditMenuDelegate>().set_ivars(DelegateState {
            events,
            active: RefCell::new(None),
        });
        // SAFETY: The view, delegate and interaction are owned by the UIKit main thread.
        unsafe {
            let delegate: Retained<EditMenuDelegate> = msg_send_id![super(delegate), init];
            let interaction = UIEditMenuInteraction::initWithDelegate(
                mtm.alloc(),
                Some(ProtocolObject::from_ref(&*delegate)),
            );
            view.addInteraction(ProtocolObject::from_ref(&*interaction));
            Some(Self {
                interaction,
                delegate,
            })
        }
    }

    pub(crate) fn present(&self, model: NativeTextMenu, anchor: Rect) {
        self.dismiss();
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let state = self.delegate.ivars();
        let session = model.session();
        let identifier = NSString::from_str(&session.to_string());
        // SAFETY: The interaction and each action belong to the UIKit main thread.
        unsafe {
            let children = model
                .entries()
                .map(|(index, title)| {
                    let events = state.events.clone();
                    let handle = RcBlock::new(move |_: NonNull<UIAction>| {
                        events.post(UserEvent::IosTextMenu(session, Some(index)));
                    });
                    let action =
                        UIAction::actionWithHandler(std::ptr::from_ref(&*handle).cast_mut(), mtm);
                    action.setTitle(&NSString::from_str(title));
                    Retained::into_super(action)
                })
                .collect::<Vec<_>>();
            if children.is_empty() {
                return;
            }
            let menu = UIMenu::menuWithChildren(&NSArray::from_vec(children), mtm);
            let configuration = UIEditMenuConfiguration::configurationWithIdentifier_sourcePoint(
                Some(ProtocolObject::from_ref(&*identifier)),
                CGPoint::new(
                    f64::from(anchor.x + anchor.width * 0.5),
                    f64::from(anchor.y + anchor.height * 0.5),
                ),
                mtm,
            );
            state.active.replace(Some(PresentedMenu {
                model,
                identifier,
                menu,
                anchor,
            }));
            self.interaction
                .presentEditMenuWithConfiguration(&configuration);
        }
    }

    pub(crate) fn take(&self, session: u64) -> Option<NativeTextMenu> {
        self.delegate
            .ivars()
            .active
            .borrow_mut()
            .take_if(|menu| menu.model.session() == session)
            .map(|menu| menu.model)
    }

    pub(crate) fn session(&self) -> Option<u64> {
        self.delegate
            .ivars()
            .active
            .borrow()
            .as_ref()
            .map(|menu| menu.model.session())
    }

    pub(crate) fn sync_anchor(&self, anchor: impl FnOnce(&NativeTextMenu) -> Option<Rect>) {
        let mut active = self.delegate.ivars().active.borrow_mut();
        let Some(menu) = active.as_mut() else { return };
        if let Some(rect) = anchor(&menu.model) {
            if menu.anchor == rect {
                return;
            }
            menu.anchor = rect;
            // Release the model borrow before UIKit calls the delegate for the rectangle.
            drop(active);
            // SAFETY: The interaction belongs to this UIKit thread.
            unsafe { self.interaction.updateVisibleMenuPositionAnimated(false) };
        } else {
            drop(active);
            self.dismiss();
        }
    }

    pub(crate) fn dismiss(&self) {
        if self.delegate.ivars().active.take().is_some() {
            // SAFETY: This interaction is only used on the UIKit main thread.
            unsafe { self.interaction.dismissMenu() };
        }
    }
}

impl Drop for EditMenu {
    fn drop(&mut self) {
        self.dismiss();
        // SAFETY: The interaction is detached on the UIKit main thread.
        unsafe {
            if let Some(view) = self.interaction.view() {
                view.removeInteraction(ProtocolObject::from_ref(&*self.interaction));
            }
        }
    }
}
