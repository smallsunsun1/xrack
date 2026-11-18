use crate::keyboard::KeyEvent;
use winit::{
    event::ElementState,
    keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey},
};

#[derive(Default)]
pub(crate) struct Keyboard {
    held: u8,
}

impl Keyboard {
    pub fn clear(&mut self) -> ModifiersState {
        self.held = 0;
        ModifiersState::empty()
    }

    pub fn translate(
        &mut self,
        physical: PhysicalKey,
        mut event: KeyEvent,
    ) -> (KeyEvent, ModifiersState) {
        let bit = match physical {
            PhysicalKey::Code(KeyCode::ShiftLeft) => 1,
            PhysicalKey::Code(KeyCode::ShiftRight) => 2,
            PhysicalKey::Code(KeyCode::ControlLeft) => 4,
            PhysicalKey::Code(KeyCode::ControlRight) => 8,
            PhysicalKey::Code(KeyCode::AltLeft) => 16,
            PhysicalKey::Code(KeyCode::AltRight) => 32,
            PhysicalKey::Code(KeyCode::SuperLeft) => 64,
            PhysicalKey::Code(KeyCode::SuperRight) => 128,
            _ => match event.logical_key {
                Key::Named(NamedKey::Shift) => 1,
                Key::Named(NamedKey::Control) => 4,
                Key::Named(NamedKey::Alt | NamedKey::AltGraph) => 16,
                Key::Named(NamedKey::Super | NamedKey::Meta) => 64,
                _ => 0,
            },
        };
        if event.state == ElementState::Pressed {
            self.held |= bit;
        } else {
            self.held &= !bit;
        }
        let mut modifiers = ModifiersState::empty();
        modifiers.set(ModifiersState::SHIFT, self.held & 3 != 0);
        modifiers.set(ModifiersState::CONTROL, self.held & 12 != 0);
        modifiers.set(ModifiersState::ALT, self.held & 48 != 0);
        modifiers.set(ModifiersState::SUPER, self.held & 192 != 0);

        // Android key maps may return C0 characters for editing keys or Ctrl+A–Z.
        let named = match physical {
            PhysicalKey::Code(KeyCode::ShiftLeft | KeyCode::ShiftRight) => Some(NamedKey::Shift),
            PhysicalKey::Code(KeyCode::ControlLeft | KeyCode::ControlRight) => {
                Some(NamedKey::Control)
            }
            PhysicalKey::Code(KeyCode::AltLeft | KeyCode::AltRight) => {
                Some(if event.logical_key == Key::Named(NamedKey::AltGraph) {
                    NamedKey::AltGraph
                } else {
                    NamedKey::Alt
                })
            }
            PhysicalKey::Code(KeyCode::SuperLeft | KeyCode::SuperRight) => Some(NamedKey::Super),
            PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => Some(NamedKey::Enter),
            PhysicalKey::Code(KeyCode::Backspace) => Some(NamedKey::Backspace),
            PhysicalKey::Code(KeyCode::Delete) => Some(NamedKey::Delete),
            PhysicalKey::Code(KeyCode::Tab) => Some(NamedKey::Tab),
            PhysicalKey::Code(KeyCode::Escape) => Some(NamedKey::Escape),
            _ => None,
        };
        if let Some(key) = named {
            event.logical_key = Key::Named(key);
        } else if modifiers.control_key()
            && !modifiers.alt_key()
            && let Key::Character(text) = &event.logical_key
        {
            let mut chars = text.chars();
            if let Some(ch) = chars.next().filter(|_| chars.next().is_none())
                && (1..=26).contains(&u32::from(ch))
            {
                let letter = char::from(b'a' + ch as u8 - 1);
                let letter = if modifiers.shift_key() {
                    letter.to_ascii_uppercase()
                } else {
                    letter
                };
                event.logical_key = Key::Character(letter.to_string().into());
            }
        }
        event.unmodified_key = event.logical_key.clone();
        if bit != 0 || (modifiers.control_key() || modifiers.super_key()) && !modifiers.alt_key() {
            event.text = None;
        }
        (event, modifiers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(key: Key, state: ElementState) -> KeyEvent {
        KeyEvent {
            unmodified_key: key.clone(),
            text: key.to_text().map(str::to_owned),
            logical_key: key,
            state,
            repeat: false,
        }
    }

    #[test]
    fn ctrl_shortcuts_preserve_key_identity_without_inserting_control_text() {
        let mut keyboard = Keyboard::default();
        let (_, state) = keyboard.translate(
            KeyCode::ControlLeft.into(),
            event(Key::Named(NamedKey::Control), ElementState::Pressed),
        );
        assert!(state.control_key());
        for key in ["a", "\u{1}"] {
            let (key, state) = keyboard.translate(
                KeyCode::KeyA.into(),
                event(Key::Character(key.into()), ElementState::Pressed),
            );
            assert_eq!(key.logical_key, Key::Character("a".into()));
            assert!(key.text.is_none());
            assert!(state.control_key());
        }
        keyboard.translate(
            KeyCode::ControlLeft.into(),
            event(Key::Named(NamedKey::Control), ElementState::Released),
        );
        let (key, state) = keyboard.translate(
            KeyCode::KeyA.into(),
            event(Key::Character("a".into()), ElementState::Pressed),
        );
        assert!(state.is_empty());
        assert_eq!(key.text.as_deref(), Some("a"));
    }

    #[test]
    fn either_modifier_side_can_stay_pressed_and_focus_loss_clears_both() {
        let mut keyboard = Keyboard::default();
        for code in [KeyCode::ControlLeft, KeyCode::ControlRight] {
            keyboard.translate(
                code.into(),
                event(Key::Named(NamedKey::Control), ElementState::Pressed),
            );
        }
        let (_, state) = keyboard.translate(
            KeyCode::ControlLeft.into(),
            event(Key::Named(NamedKey::Control), ElementState::Released),
        );
        assert!(state.control_key());
        assert!(keyboard.clear().is_empty());
        let (_, state) = keyboard.translate(
            KeyCode::KeyV.into(),
            event(Key::Character("v".into()), ElementState::Pressed),
        );
        assert!(state.is_empty());
    }

    #[test]
    fn shift_is_available_for_selection_and_altgr_text_remains_text() {
        let mut keyboard = Keyboard::default();
        let (_, state) = keyboard.translate(
            KeyCode::ShiftLeft.into(),
            event(Key::Named(NamedKey::Shift), ElementState::Pressed),
        );
        assert!(state.shift_key());
        keyboard.clear();
        keyboard.translate(
            KeyCode::ControlLeft.into(),
            event(Key::Named(NamedKey::Control), ElementState::Pressed),
        );
        keyboard.translate(
            KeyCode::AltRight.into(),
            event(Key::Named(NamedKey::Alt), ElementState::Pressed),
        );
        let (key, state) = keyboard.translate(
            KeyCode::KeyQ.into(),
            event(Key::Character("@".into()), ElementState::Pressed),
        );
        assert!(state.control_key() && state.alt_key());
        assert_eq!(key.text.as_deref(), Some("@"));
        assert_eq!(key.logical_key, Key::Character("@".into()));
    }

    #[test]
    fn named_editing_keys_do_not_become_ctrl_letter_shortcuts() {
        let mut keyboard = Keyboard::default();
        for (physical, text, named) in [
            (KeyCode::Enter, "\r", NamedKey::Enter),
            (KeyCode::Backspace, "\u{8}", NamedKey::Backspace),
            (KeyCode::Tab, "\t", NamedKey::Tab),
            (KeyCode::Escape, "\u{1b}", NamedKey::Escape),
        ] {
            let (key, _) = keyboard.translate(
                physical.into(),
                event(Key::Character(text.into()), ElementState::Pressed),
            );
            assert_eq!(key.logical_key, Key::Named(named));
        }
    }

    #[test]
    fn a_modifier_key_cannot_insert_a_keymap_control_character() {
        let mut keyboard = Keyboard::default();
        let (key, state) = keyboard.translate(
            KeyCode::ShiftLeft.into(),
            event(Key::Character("\0".into()), ElementState::Pressed),
        );
        assert_eq!(key.logical_key, Key::Named(NamedKey::Shift));
        assert!(key.text.is_none());
        assert!(state.shift_key());
    }
}
