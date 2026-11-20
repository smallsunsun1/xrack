use crate::{AccessibilityEscape, UserEvent, desktop::WindowEvents};
use objc2::{
    ClassType, DeclaredClass, declare_class, msg_send_id,
    mutability::MainThreadOnly,
    rc::Retained,
    runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject},
    sel,
};
use objc2_foundation::MainThreadMarker;
use objc2_ui_kit::{
    UIGestureRecognizer, UIGestureRecognizerDelegate, UIGestureRecognizerState,
    UIPanGestureRecognizer, UITouch, UITouchType,
};
use std::cell::{Cell, RefCell};
use winit::window::Window;
use xrack_runtime::LayoutDirection;

const LEADING_EDGE_WIDTH: f64 = 20.0;
const COMMIT_DISTANCE: f64 = 64.0;

struct BackTargetState {
    events: WindowEvents,
    direction: Cell<LayoutDirection>,
    available: RefCell<Option<AccessibilityEscape>>,
    active: RefCell<Option<AccessibilityEscape>>,
}

impl BackTargetState {
    fn toward_content(&self, x: f64) -> f64 {
        match self.direction.get() {
            LayoutDirection::LeftToRight => x,
            LayoutDirection::RightToLeft => -x,
        }
    }
}

declare_class!(
    struct BackTarget;

    unsafe impl ClassType for BackTarget {
        type Super = NSObject;
        type Mutability = MainThreadOnly;
        const NAME: &'static str = "XRackEdgeBackTarget";
    }

    impl DeclaredClass for BackTarget {
        type Ivars = BackTargetState;
    }

    unsafe impl NSObjectProtocol for BackTarget {}

    unsafe impl UIGestureRecognizerDelegate for BackTarget {
        #[method(gestureRecognizer:shouldReceiveTouch:)]
        fn should_receive_touch(&self, gesture: &UIGestureRecognizer, touch: &UITouch) -> Bool {
            if touch.r#type() != UITouchType::Direct {
                return Bool::NO;
            }
            let state = self.ivars();
            // SAFETY: UIKit borrows the live gesture and touch on the main thread here.
            let Some(view) = (unsafe { gesture.view() }) else { return Bool::NO };
            let point = touch.locationInView(Some(&view));
            let bounds = view.bounds();
            let distance = match state.direction.get() {
                LayoutDirection::LeftToRight => point.x - bounds.origin.x,
                LayoutDirection::RightToLeft => bounds.origin.x + bounds.size.width - point.x,
            };
            if !(0.0..=LEADING_EDGE_WIDTH).contains(&distance) {
                return Bool::NO;
            }
            let request = state.available.borrow().clone();
            let available = request.is_some();
            state.active.replace(request);
            Bool::new(available)
        }

        #[method(gestureRecognizerShouldBegin:)]
        fn should_begin(&self, gesture: &UIPanGestureRecognizer) -> Bool {
            let state = self.ivars();
            let request = state.active.borrow();
            if request.is_none() || *request != *state.available.borrow() {
                return Bool::NO;
            }
            let mut movement = gesture.velocityInView(None);
            if movement.x == 0.0 && movement.y == 0.0 {
                movement = gesture.translationInView(None);
            }
            Bool::new(state.toward_content(movement.x) > movement.y.abs())
        }
    }

    unsafe impl BackTarget {
        #[method(backChanged:)]
        fn back_changed(&self, gesture: &UIPanGestureRecognizer) {
            let state = self.ivars();
            match gesture.state() {
                UIGestureRecognizerState::Ended => {
                    let Some(request) = state.active.take() else { return };
                    let translation = gesture.translationInView(None);
                    let forward = state.toward_content(translation.x);
                    if forward >= COMMIT_DISTANCE && forward >= 1.25 * translation.y.abs() {
                        state.events.post(UserEvent::IosBackRequested(request));
                    }
                }
                UIGestureRecognizerState::Cancelled | UIGestureRecognizerState::Failed => {
                    state.active.take();
                }
                _ => {}
            }
        }
    }
);

pub(crate) struct EdgeBack {
    gesture: Retained<UIPanGestureRecognizer>,
    target: Retained<BackTarget>,
}

impl EdgeBack {
    pub(crate) fn new(window: &Window, events: WindowEvents) -> Option<Self> {
        let parent = super::view(window)?;
        let mtm = MainThreadMarker::new()?;
        let target = mtm.alloc::<BackTarget>().set_ivars(BackTargetState {
            events,
            direction: Cell::new(LayoutDirection::LeftToRight),
            available: RefCell::new(None),
            active: RefCell::new(None),
        });
        // SAFETY: The view, target and recognizer are owned by UIKit's main thread.
        unsafe {
            let target: Retained<BackTarget> = msg_send_id![super(target), init];
            let gesture = UIPanGestureRecognizer::initWithTarget_action(
                mtm.alloc(),
                Some(&target),
                Some(sel!(backChanged:)),
            );
            gesture.setDelegate(Some(ProtocolObject::from_ref(&*target)));
            gesture.setMaximumNumberOfTouches(1);
            gesture.setCancelsTouchesInView(true);
            gesture.setEnabled(false);
            parent.addGestureRecognizer(&gesture);
            Some(Self { gesture, target })
        }
    }

    pub(crate) fn prepare(&self, direction: LayoutDirection, request: Option<AccessibilityEscape>) {
        let state = self.target.ivars();
        let enabled = request.is_some();
        state.available.replace(request);
        // SAFETY: Window geometry is published on the UIKit main thread.
        unsafe {
            if state.direction.replace(direction) != direction {
                self.gesture.setEnabled(false);
                state.active.take();
            }
            if self.gesture.isEnabled() != enabled {
                self.gesture.setEnabled(enabled);
            }
        }
    }
}

impl Drop for EdgeBack {
    fn drop(&mut self) {
        // SAFETY: The window and its recognizer are dropped on the UIKit main thread.
        unsafe {
            if let Some(parent) = self.gesture.view() {
                parent.removeGestureRecognizer(&self.gesture);
            }
        }
    }
}
