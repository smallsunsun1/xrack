use std::{fmt, str::FromStr};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShortcutModifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_key: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShortcutKey {
    Character(char),
    Function(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuShortcut {
    key: ShortcutKey,
    modifiers: ShortcutModifiers,
}

impl MenuShortcut {
    pub fn new(key: ShortcutKey, modifiers: ShortcutModifiers) -> Option<Self> {
        let key = match key {
            ShortcutKey::Character(key)
                if key.is_ascii_alphanumeric()
                    && (modifiers.ctrl || modifiers.alt || modifiers.super_key) =>
            {
                ShortcutKey::Character(key.to_ascii_uppercase())
            }
            ShortcutKey::Function(1..=12) => key,
            _ => return None,
        };
        Some(Self { key, modifiers })
    }
}

impl FromStr for MenuShortcut {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut parts = value.split('+');
        let key = parts.next_back().ok_or(())?.trim();
        let key = if let Some(function) = key.strip_prefix(['F', 'f']).filter(|_| key.len() > 1) {
            ShortcutKey::Function(function.parse().map_err(|_| ())?)
        } else {
            let mut chars = key.chars();
            let key = chars.next().ok_or(())?;
            if chars.next().is_some() {
                return Err(());
            }
            ShortcutKey::Character(key)
        };
        let mut modifiers = ShortcutModifiers::default();
        for modifier in parts {
            let flag = match modifier.trim().to_ascii_lowercase().as_str() {
                "ctrl" => &mut modifiers.ctrl,
                "alt" => &mut modifiers.alt,
                "shift" => &mut modifiers.shift,
                "super" => &mut modifiers.super_key,
                "primary" if cfg!(target_os = "macos") => &mut modifiers.super_key,
                "primary" => &mut modifiers.ctrl,
                _ => return Err(()),
            };
            if std::mem::replace(flag, true) {
                return Err(());
            }
        }
        Self::new(key, modifiers).ok_or(())
    }
}

impl fmt::Display for MenuShortcut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.modifiers.ctrl {
            f.write_str("Ctrl+")?;
        }
        if self.modifiers.alt {
            f.write_str(if cfg!(target_os = "macos") {
                "Option+"
            } else {
                "Alt+"
            })?;
        }
        if self.modifiers.shift {
            f.write_str("Shift+")?;
        }
        if self.modifiers.super_key {
            f.write_str(if cfg!(target_os = "macos") {
                "Cmd+"
            } else {
                "Super+"
            })?;
        }
        match self.key {
            ShortcutKey::Character(key) => write!(f, "{key}"),
            ShortcutKey::Function(key) => write!(f, "F{key}"),
        }
    }
}
