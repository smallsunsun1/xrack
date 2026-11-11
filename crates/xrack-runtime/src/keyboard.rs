#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EventResult {
    Accept,
    #[default]
    Reject,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyModifiers {
    pub control: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
    pub primary: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyEvent {
    pub key: String,
    pub text: String,
    pub modifiers: KeyModifiers,
    pub repeat: bool,
}

/// A wheel movement in logical pixels, with position relative to its handler.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WheelEvent {
    pub x: crate::Length,
    pub y: crate::Length,
    pub delta_x: crate::Length,
    pub delta_y: crate::Length,
    pub lines_x: f64,
    pub lines_y: f64,
    pub is_line: bool,
    pub modifiers: KeyModifiers,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyPhase {
    CapturePressed,
    CaptureReleased,
    Pressed,
    Released,
}

impl KeyPhase {
    #[doc(hidden)]
    pub const fn mask(self) -> u8 {
        1 << self as u8
    }
}
