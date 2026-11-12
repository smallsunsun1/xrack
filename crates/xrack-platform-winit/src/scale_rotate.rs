use winit::event::{DeviceId, TouchPhase};
pub(crate) use xrack_layout::scale_rotate_gestures::{ContactId, Context, Delivery, TrackpadDelta};

pub(crate) type Gestures = xrack_layout::scale_rotate_gestures::Gestures<DeviceId>;

pub(crate) fn phase(phase: TouchPhase) -> xrack_layout::scale_rotate_gestures::TouchPhase {
    use xrack_layout::scale_rotate_gestures::TouchPhase as SharedPhase;
    match phase {
        TouchPhase::Started => SharedPhase::Started,
        TouchPhase::Moved => SharedPhase::Moved,
        TouchPhase::Ended => SharedPhase::Ended,
        TouchPhase::Cancelled => SharedPhase::Cancelled,
    }
}
