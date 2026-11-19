use crate::{UserEvent, desktop::WindowEvents, ios, scale_rotate::TrackpadDelta};
use objc2::{
    ClassType, DeclaredClass, declare_class, msg_send, msg_send_id,
    mutability::MainThreadOnly,
    rc::Retained,
    runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject},
    sel,
};
use objc2_foundation::{CGPoint, MainThreadMarker, NSArray, NSProcessInfo, NSSet};
use objc2_ui_kit::{
    UIAxis, UIBezierPath, UIEvent, UIEventButtonMask, UIEventType, UIGestureRecognizer,
    UIGestureRecognizerDelegate, UIGestureRecognizerState, UIHoverGestureRecognizer, UIInteraction,
    UIKeyModifierFlags, UIPanGestureRecognizer, UIPinchGestureRecognizer, UIPointerAccessory,
    UIPointerAccessoryPositionBottom, UIPointerAccessoryPositionLeft,
    UIPointerAccessoryPositionRight, UIPointerAccessoryPositionTop, UIPointerInteraction,
    UIPointerInteractionDelegate, UIPointerRegion, UIPointerRegionRequest, UIPointerShape,
    UIPointerStyle, UIRotationGestureRecognizer, UIScrollTypeMask, UITouch,
};
use std::{
    cell::{Cell, RefCell},
    time::{Duration, Instant},
};
use winit::{
    dpi::LogicalPosition,
    event::{MouseButton, TouchPhase},
    keyboard::ModifiersState,
    window::{CursorIcon, Window},
};

pub(crate) enum PointerInputEvent {
    Moved(LogicalPosition<f64>),
    Left,
    Button {
        button: MouseButton,
        position: LogicalPosition<f64>,
        phase: TouchPhase,
        modifiers: ModifiersState,
        at: Instant,
    },
    ButtonCancelled(MouseButton),
    Transform {
        position: Option<LogicalPosition<f64>>,
        delta: TrackpadDelta,
        phase: TouchPhase,
    },
    Scroll {
        position: LogicalPosition<f64>,
        delta: LogicalPosition<f64>,
        phase: TouchPhase,
    },
}

struct PointerTargetState {
    events: WindowEvents,
    translation: Cell<CGPoint>,
    scale: Cell<f64>,
    rotation: Cell<f64>,
    style: RefCell<Option<Retained<UIPointerStyle>>>,
}

declare_class!(
    struct PointerTarget;

    unsafe impl ClassType for PointerTarget {
        type Super = NSObject;
        type Mutability = MainThreadOnly;
        const NAME: &'static str = "XRackIndirectPointerTarget";
    }

    impl DeclaredClass for PointerTarget {
        type Ivars = PointerTargetState;
    }

    unsafe impl NSObjectProtocol for PointerTarget {}

    #[allow(non_snake_case)]
    unsafe impl UIGestureRecognizerDelegate for PointerTarget {
        #[method(gestureRecognizer:shouldReceiveEvent:)]
        unsafe fn gestureRecognizer_shouldReceiveEvent(
            &self,
            _: &UIGestureRecognizer,
            event: &UIEvent,
        ) -> bool {
            // SAFETY: UIKit provides the live event on its main thread.
            unsafe { event.r#type() == UIEventType::Transform }
        }

        #[method(gestureRecognizer:shouldRecognizeSimultaneouslyWithGestureRecognizer:)]
        unsafe fn gestureRecognizer_shouldRecognizeSimultaneouslyWithGestureRecognizer(
            &self,
            _: &UIGestureRecognizer,
            other: &UIGestureRecognizer,
        ) -> bool {
            // SAFETY: UIKit owns the recognizers during this delegate call.
            unsafe {
                let pinch: Bool = msg_send![other, isKindOfClass: UIPinchGestureRecognizer::class()];
                let rotate: Bool =
                    msg_send![other, isKindOfClass: UIRotationGestureRecognizer::class()];
                let hover: Bool = msg_send![other, isKindOfClass: UIHoverGestureRecognizer::class()];
                pinch.as_bool() || rotate.as_bool() || hover.as_bool()
            }
        }
    }

    #[allow(non_snake_case)]
    unsafe impl UIPointerInteractionDelegate for PointerTarget {
        #[method_id(pointerInteraction:regionForRequest:defaultRegion:)]
        unsafe fn pointerInteraction_regionForRequest_defaultRegion(
            &self,
            _: &UIPointerInteraction,
            _: &UIPointerRegionRequest,
            region: &UIPointerRegion,
        ) -> Option<Retained<UIPointerRegion>> {
            if self.ivars().style.borrow().is_some() {
                // SAFETY: UIKit borrows its default region for the delegate call.
                unsafe { Retained::retain(std::ptr::from_ref(region).cast_mut()) }
            } else {
                None
            }
        }

        #[method_id(pointerInteraction:styleForRegion:)]
        unsafe fn pointerInteraction_styleForRegion(
            &self,
            _: &UIPointerInteraction,
            _: &UIPointerRegion,
        ) -> Option<Retained<UIPointerStyle>> {
            self.ivars().style.borrow().clone()
        }
    }

    unsafe impl PointerTarget {
        #[method(hoverChanged:)]
        fn hover_changed(&self, hover: &UIHoverGestureRecognizer) {
            let event = match gesture_state(hover) {
                UIGestureRecognizerState::Began | UIGestureRecognizerState::Changed => {
                    let Some(point) = gesture_location(hover) else { return };
                    PointerInputEvent::Moved(point)
                }
                UIGestureRecognizerState::Ended | UIGestureRecognizerState::Cancelled => {
                    PointerInputEvent::Left
                }
                _ => return,
            };
            self.ivars().events.post(UserEvent::IosPointer(event));
        }

        #[method(buttonChanged:)]
        fn button_changed(&self, press: &PointerPress) {
            let Some(phase) = gesture_phase(press) else { return };
            let state = press.ivars();
            let button = state.button;
            let event = match (phase, gesture_location(press)) {
                (TouchPhase::Cancelled, _) | (_, None) => PointerInputEvent::ButtonCancelled(button),
                (phase, Some(position)) => {
                    // SAFETY: UIKit owns this recognizer during the action callback.
                    let flags = unsafe { press.modifierFlags() };
                    let modifiers = crate::ios_keyboard::modifiers(flags);
                    PointerInputEvent::Button {
                        button, position, phase, modifiers,
                        at: state.click_at.get().unwrap_or_else(Instant::now),
                    }
                }
            };
            self.ivars().events.post(UserEvent::IosPointer(event));
        }

        #[method(pinchChanged:)]
        fn pinch_changed(&self, pinch: &UIPinchGestureRecognizer) {
            let Some(phase) = gesture_phase(pinch) else { return };
            let scale = pinch.scale();
            let state = self.ivars();
            let previous = state.scale.get();
            let valid = scale.is_finite() && scale > 0.0;
            if let Some(next) = cumulative_sample(phase, scale, 1.0, valid) {
                state.scale.set(next);
            }
            let delta = if valid && phase != TouchPhase::Cancelled {
                let baseline = if phase == TouchPhase::Started { 1.0 } else { previous };
                scale / baseline - 1.0
            } else {
                0.0
            };
            state.events.post(UserEvent::IosPointer(PointerInputEvent::Transform {
                position: gesture_location(pinch),
                delta: TrackpadDelta::Scale(delta),
                phase,
            }));
        }

        #[method(rotationChanged:)]
        fn rotation_changed(&self, rotation: &UIRotationGestureRecognizer) {
            let Some(phase) = gesture_phase(rotation) else { return };
            let angle = rotation.rotation();
            let state = self.ivars();
            let previous = state.rotation.get();
            let valid = angle.is_finite();
            if let Some(next) = cumulative_sample(phase, angle, 0.0, valid) {
                state.rotation.set(next);
            }
            let delta = if valid && phase != TouchPhase::Cancelled {
                let baseline = if phase == TouchPhase::Started { 0.0 } else { previous };
                // UIKit uses clockwise radians. The shared desktop path takes
                // counterclockwise degrees, matching winit's rotation events.
                -(angle - baseline).to_degrees()
            } else {
                0.0
            };
            state.events.post(UserEvent::IosPointer(PointerInputEvent::Transform {
                position: gesture_location(rotation),
                delta: TrackpadDelta::Rotation(delta),
                phase,
            }));
        }

        #[method(scrollChanged:)]
        fn scroll_changed(&self, scroll: &UIPanGestureRecognizer) {
            let Some(phase) = gesture_phase(scroll) else { return };
            let state = self.ivars();
            // SAFETY: UIKit invokes this action on the main thread and owns the view.
            let translation = scroll.translationInView(unsafe { scroll.view() }.as_deref());
            let previous = state.translation.replace(if matches!(phase, TouchPhase::Ended | TouchPhase::Cancelled) {
                CGPoint::ZERO
            } else {
                translation
            });
            let delta = match phase {
                TouchPhase::Started => LogicalPosition::new(translation.x, translation.y),
                TouchPhase::Cancelled => LogicalPosition::new(0.0, 0.0),
                _ => LogicalPosition::new(translation.x - previous.x, translation.y - previous.y),
            };
            let Some(position) = gesture_location(scroll) else { return };
            if delta.x.is_finite() && delta.y.is_finite() {
                state.events.post(UserEvent::IosPointer(PointerInputEvent::Scroll { position, delta, phase }));
            }
        }
    }
);

struct PointerPressState {
    button: MouseButton,
    click_at: Cell<Option<Instant>>,
}

fn event_instant(event: &UIEvent) -> Instant {
    let now = Instant::now();
    // SAFETY: The live UIKit event and ProcessInfo use the same system uptime.
    let elapsed = unsafe { NSProcessInfo::processInfo().systemUptime() - event.timestamp() };
    Duration::try_from_secs_f64(elapsed.max(0.0))
        .ok()
        .and_then(|elapsed| now.checked_sub(elapsed))
        .unwrap_or(now)
}

declare_class!(
    struct PointerPress;

    unsafe impl ClassType for PointerPress {
        type Super = UIGestureRecognizer;
        type Mutability = MainThreadOnly;
        const NAME: &'static str = "XRackPointerPress";
    }

    impl DeclaredClass for PointerPress {
        type Ivars = PointerPressState;
    }

    unsafe impl NSObjectProtocol for PointerPress {}

    unsafe impl PointerPress {
        #[method(shouldReceiveEvent:)]
        fn should_receive_event(&self, event: &UIEvent) -> bool {
            // SAFETY: The event is live for this UIKit callback.
            unsafe {
                event.r#type() == UIEventType::Touches && {
                    let mask = event.buttonMask();
                    let control_click = event.modifierFlags() == UIKeyModifierFlags::UIKeyModifierControl;
                    match self.ivars().button {
                        MouseButton::Left => mask == UIEventButtonMask::Primary && !control_click,
                        MouseButton::Right => {
                            mask == UIEventButtonMask::Secondary
                                || (mask == UIEventButtonMask::Primary && control_click)
                        }
                        _ => false,
                    }
                }
            }
        }

        #[method(touchesBegan:withEvent:)]
        fn touches_began(&self, _: &NSSet<UITouch>, event: &UIEvent) {
            self.ivars().click_at.set(Some(event_instant(event)));
            // SAFETY: UIKit delivers the start of a matching press to this recognizer.
            unsafe { self.setState(UIGestureRecognizerState::Began) };
        }

        #[method(touchesMoved:withEvent:)]
        fn touches_moved(&self, _: &NSSet<UITouch>, _: &UIEvent) {
            // SAFETY: UIKit is updating the recognized press.
            unsafe { self.setState(UIGestureRecognizerState::Changed) };
        }

        #[method(touchesEnded:withEvent:)]
        fn touches_ended(&self, _: &NSSet<UITouch>, event: &UIEvent) {
            self.ivars().click_at.set(Some(event_instant(event)));
            // SAFETY: UIKit is ending the recognized press.
            unsafe { self.setState(UIGestureRecognizerState::Ended) };
        }

        #[method(touchesCancelled:withEvent:)]
        fn touches_cancelled(&self, _: &NSSet<UITouch>, _: &UIEvent) {
            // SAFETY: UIKit is cancelling the recognized press.
            unsafe { self.setState(UIGestureRecognizerState::Cancelled) };
        }
    }
);

fn gesture_state(recognizer: &UIGestureRecognizer) -> UIGestureRecognizerState {
    // SAFETY: UIKit exposes this readonly property on all gesture recognizers.
    unsafe { msg_send![recognizer, state] }
}

fn gesture_phase(recognizer: &UIGestureRecognizer) -> Option<TouchPhase> {
    Some(match gesture_state(recognizer) {
        UIGestureRecognizerState::Began => TouchPhase::Started,
        UIGestureRecognizerState::Changed => TouchPhase::Moved,
        UIGestureRecognizerState::Ended => TouchPhase::Ended,
        UIGestureRecognizerState::Cancelled => TouchPhase::Cancelled,
        _ => return None,
    })
}

fn cumulative_sample(phase: TouchPhase, sample: f64, reset: f64, valid: bool) -> Option<f64> {
    match phase {
        TouchPhase::Ended | TouchPhase::Cancelled => Some(reset),
        _ if valid => Some(sample),
        TouchPhase::Started => Some(reset),
        TouchPhase::Moved => None,
    }
}

fn gesture_location(recognizer: &UIGestureRecognizer) -> Option<LogicalPosition<f64>> {
    // SAFETY: The action and its attached view belong to the UIKit main thread.
    let point = unsafe {
        let view = recognizer.view()?;
        recognizer.locationInView(Some(&view))
    };
    (point.x.is_finite() && point.y.is_finite()).then(|| LogicalPosition::new(point.x, point.y))
}

fn pointer_style(cursor: CursorIcon, mtm: MainThreadMarker) -> Option<Retained<UIPointerStyle>> {
    // SAFETY: The pointer styles and paths are created on the UIKit main thread.
    unsafe {
        let shape = match cursor {
            CursorIcon::Text => {
                UIPointerShape::beamWithPreferredLength_axis(18.0, UIAxis::Vertical, mtm)
            }
            CursorIcon::Crosshair => {
                let path = UIBezierPath::bezierPath();
                path.moveToPoint(CGPoint::new(-1.0, -8.0));
                for (x, y) in [
                    (1.0, -8.0),
                    (1.0, -1.0),
                    (8.0, -1.0),
                    (8.0, 1.0),
                    (1.0, 1.0),
                    (1.0, 8.0),
                    (-1.0, 8.0),
                    (-1.0, 1.0),
                    (-8.0, 1.0),
                    (-8.0, -1.0),
                    (-1.0, -1.0),
                ] {
                    path.addLineToPoint(CGPoint::new(x, y));
                }
                path.closePath();
                UIPointerShape::shapeWithPath(&path, mtm)
            }
            CursorIcon::ColResize
            | CursorIcon::EwResize
            | CursorIcon::EResize
            | CursorIcon::WResize
            | CursorIcon::RowResize
            | CursorIcon::NsResize
            | CursorIcon::NResize
            | CursorIcon::SResize => {
                let horizontal = matches!(
                    cursor,
                    CursorIcon::ColResize
                        | CursorIcon::EwResize
                        | CursorIcon::EResize
                        | CursorIcon::WResize
                );
                let positions = if horizontal {
                    [
                        UIPointerAccessoryPositionLeft,
                        UIPointerAccessoryPositionRight,
                    ]
                } else {
                    [
                        UIPointerAccessoryPositionTop,
                        UIPointerAccessoryPositionBottom,
                    ]
                };
                let arrows = positions
                    .map(|position| UIPointerAccessory::arrowAccessoryWithPosition(position, mtm));
                let style = UIPointerStyle::systemPointerStyle(mtm);
                style.setAccessories(&NSArray::from_slice(&[&*arrows[0], &*arrows[1]]));
                return Some(style);
            }
            _ => return None,
        };
        Some(UIPointerStyle::styleWithShape_constrainedAxes(
            &shape,
            UIAxis::Neither,
        ))
    }
}

pub(super) struct PointerInput {
    hover: Retained<UIHoverGestureRecognizer>,
    scroll: Retained<UIPanGestureRecognizer>,
    presses: [Retained<PointerPress>; 2],
    pinch: Retained<UIPinchGestureRecognizer>,
    rotation: Retained<UIRotationGestureRecognizer>,
    interaction: Retained<UIPointerInteraction>,
    target: Retained<PointerTarget>,
}

impl PointerInput {
    pub(super) fn new(window: &Window, events: WindowEvents) -> Option<Self> {
        let view = ios::view(window)?;
        let mtm = MainThreadMarker::new()?;
        let target = mtm.alloc::<PointerTarget>().set_ivars(PointerTargetState {
            events,
            translation: Cell::new(CGPoint::ZERO),
            scale: Cell::new(1.0),
            rotation: Cell::new(0.0),
            style: RefCell::new(None),
        });
        // SAFETY: The recognizers, their target, and the view all belong to the UIKit main thread.
        unsafe {
            let target: Retained<PointerTarget> = msg_send_id![super(target), init];
            let hover = UIHoverGestureRecognizer::initWithTarget_action(
                mtm.alloc(),
                Some(&target),
                Some(sel!(hoverChanged:)),
            );
            hover.setCancelsTouchesInView(false);
            let scroll = UIPanGestureRecognizer::initWithTarget_action(
                mtm.alloc(),
                Some(&target),
                Some(sel!(scrollChanged:)),
            );
            scroll.setAllowedScrollTypesMask(UIScrollTypeMask::All);
            scroll.setAllowedTouchTypes(&NSArray::from_slice(&[]));
            scroll.setCancelsTouchesInView(false);
            view.addGestureRecognizer(&hover);
            view.addGestureRecognizer(&scroll);
            let presses = [MouseButton::Left, MouseButton::Right].map(|button| {
                let press = mtm.alloc::<PointerPress>().set_ivars(PointerPressState {
                    button,
                    click_at: Cell::new(None),
                });
                let press: Retained<PointerPress> =
                    msg_send_id![super(press), initWithTarget: &*target, action: sel!(buttonChanged:)];
                // Claim mouse presses before winit can turn them into touch.
                press.setDelaysTouchesBegan(true);
                press.setDelaysTouchesEnded(true);
                press.setCancelsTouchesInView(true);
                view.addGestureRecognizer(&press);
                press
            });
            let pinch = UIPinchGestureRecognizer::initWithTarget_action(
                mtm.alloc(),
                Some(&target),
                Some(sel!(pinchChanged:)),
            );
            let rotation = UIRotationGestureRecognizer::initWithTarget_action(
                mtm.alloc(),
                Some(&target),
                Some(sel!(rotationChanged:)),
            );
            for recognizer in [&*pinch as &UIGestureRecognizer, &*rotation] {
                recognizer.setDelegate(Some(ProtocolObject::from_ref(&*target)));
                recognizer.setAllowedTouchTypes(&NSArray::from_slice(&[]));
                recognizer.setCancelsTouchesInView(false);
                view.addGestureRecognizer(recognizer);
            }
            let interaction = UIPointerInteraction::initWithDelegate(
                mtm.alloc(),
                Some(ProtocolObject::from_ref(&*target)),
            );
            view.addInteraction(ProtocolObject::from_ref(&*interaction));
            Some(Self {
                hover,
                scroll,
                presses,
                pinch,
                rotation,
                interaction,
                target,
            })
        }
    }

    pub(super) fn set_cursor(&self, cursor: CursorIcon) {
        let style = pointer_style(cursor, MainThreadMarker::from(&*self.target));
        let custom = style.is_some();
        let previous = self.target.ivars().style.replace(style);
        if custom || previous.is_some() {
            // SAFETY: The interaction belongs to the UIKit main thread.
            unsafe { self.interaction.invalidate() };
        }
    }
}

impl Drop for PointerInput {
    fn drop(&mut self) {
        // SAFETY: The window drops its recognizers on the UIKit main thread.
        unsafe {
            if let Some(view) = self.interaction.view() {
                view.removeInteraction(ProtocolObject::from_ref(&*self.interaction));
            }
            for recognizer in [
                &*self.hover as &UIGestureRecognizer,
                &*self.scroll,
                &*self.presses[0],
                &*self.presses[1],
                &*self.pinch,
                &*self.rotation,
            ] {
                if let Some(view) = recognizer.view() {
                    view.removeGestureRecognizer(recognizer);
                }
            }
        }
    }
}
