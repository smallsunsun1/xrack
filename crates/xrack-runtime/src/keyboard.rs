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
