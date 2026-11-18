use winit::{event::ElementState, keyboard::Key};

/// Keyboard data shared by winit and native platform responders.
pub(crate) struct KeyEvent {
    pub(crate) logical_key: Key,
    pub(crate) unmodified_key: Key,
    pub(crate) text: Option<String>,
    pub(crate) state: ElementState,
    pub(crate) repeat: bool,
}

impl From<winit::event::KeyEvent> for KeyEvent {
    fn from(event: winit::event::KeyEvent) -> Self {
        #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
        let unmodified_key = {
            use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
            event.key_without_modifiers()
        };
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        let unmodified_key = event.logical_key.clone();
        Self {
            logical_key: event.logical_key,
            unmodified_key,
            text: event.text.map(|text| text.to_string()),
            state: event.state,
            repeat: event.repeat,
        }
    }
}
