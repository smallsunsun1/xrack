use crate::{ControlInput, LayoutNode, Target, menu::MenuPopup};
use std::{
    fmt,
    str::FromStr,
    time::{Duration, Instant},
};
use xrack_runtime::{ActionKey, ControlAction, Node, RowKey};

const MAX_STROKES: usize = 4;
const STROKE_TIMEOUT: Duration = Duration::from_secs(1);

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

/// Routing result for one chord in the active window scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShortcutOutcome {
    Unmatched,
    Consumed,
    Activate(ControlInput),
    Ambiguous(Vec<ControlInput>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Binding {
    keys: Vec<MenuShortcut>,
    target: ControlInput,
    element_id: Option<&'static str>,
    row_key: Option<RowKey>,
    command: Option<ActionKey>,
    auto_repeat: bool,
}

pub(crate) fn bindings(
    scene: &Node,
    layout: &LayoutNode,
    scope: &[usize],
    popup: Option<&MenuPopup>,
    focus: Option<&Target>,
) -> Vec<Binding> {
    let mut bindings = Vec::new();
    crate::menu::visit_shortcut_presentations(scene, layout, scope, popup, |node, path| {
        let action = match node.kind {
            "Shortcut" => ControlAction::ActivateShortcut,
            "MenuItem" => ControlAction::ActivateMenuItem,
            _ => ControlAction::ActivateAction,
        };
        if node.boolean("focus-within", false)
            && !focus.is_some_and(|focus| {
                let parent = path.split_last().map_or(path, |(_, parent)| parent);
                focus.path.starts_with(parent) && crate::input::valid(focus, scene, layout)
            })
        {
            return;
        }
        let single = if node.properties.contains_key("sequences") {
            None
        } else {
            node.text("sequence").or_else(|| node.text("shortcut"))
        };
        let limit = if action == ControlAction::ActivateShortcut {
            MAX_STROKES
        } else {
            1
        };
        for value in node
            .strings("sequences")
            .iter()
            .map(String::as_str)
            .chain(single)
        {
            let Ok(keys) = value
                .split(',')
                .take(limit + 1)
                .map(str::parse)
                .collect::<Result<Vec<_>, _>>()
            else {
                continue;
            };
            if keys.len() > limit {
                continue;
            }
            bindings.push(Binding {
                keys,
                target: ControlInput {
                    path: path.to_vec(),
                    action,
                },
                element_id: node.id,
                row_key: scene.row_key_at(path),
                command: node.action,
                auto_repeat: node.boolean("auto-repeat", false),
            });
        }
    });
    bindings
}

#[derive(Clone, Debug)]
pub(crate) struct PendingShortcut {
    bindings: Vec<Binding>,
    scope: Vec<usize>,
    matched: usize,
    expires: Instant,
}

impl PendingShortcut {
    pub(crate) fn retain_active(&mut self, scope: &[usize], active: &[Binding]) -> bool {
        if self.scope != scope || self.expires <= Instant::now() {
            return false;
        }
        self.bindings.retain(|binding| active.contains(binding));
        !self.bindings.is_empty()
    }
}

pub(crate) fn press(
    pending: &mut Option<PendingShortcut>,
    active: Vec<Binding>,
    scope: &[usize],
    key: MenuShortcut,
    repeat: bool,
    now: Instant,
) -> ShortcutOutcome {
    let previous = pending
        .take()
        .filter(|old| old.expires > now && old.scope == scope);
    if repeat && previous.is_some() {
        *pending = previous;
        return ShortcutOutcome::Consumed;
    }
    let mut matched = 1;
    let mut candidates = Vec::new();
    if let Some(old) = previous {
        candidates = old
            .bindings
            .into_iter()
            .filter(|binding| {
                binding.keys.get(old.matched) == Some(&key) && active.contains(binding)
            })
            .collect();
        if !candidates.is_empty() {
            matched = old.matched + 1;
        }
    }
    if candidates.is_empty() {
        candidates = active
            .into_iter()
            .filter(|binding| binding.keys.first() == Some(&key))
            .collect();
    }
    let (exact, prefixes): (Vec<_>, Vec<_>) = candidates
        .into_iter()
        .partition(|binding| binding.keys.len() == matched);
    let has_prefix = !prefixes.is_empty();
    if has_prefix && !repeat {
        *pending = Some(PendingShortcut {
            bindings: prefixes,
            scope: scope.to_vec(),
            matched,
            expires: now + STROKE_TIMEOUT,
        });
    }
    resolve(exact, has_prefix, repeat)
}

pub(crate) fn single_action(bindings: Vec<Binding>, key: MenuShortcut) -> Option<ControlInput> {
    let (exact, prefixes): (Vec<_>, Vec<_>) = bindings
        .into_iter()
        .filter(|binding| binding.keys.first() == Some(&key))
        .partition(|binding| binding.keys.len() == 1);
    match resolve(exact, !prefixes.is_empty(), false) {
        ShortcutOutcome::Activate(action) => Some(action),
        _ => None,
    }
}

fn resolve(exact: Vec<Binding>, has_prefix: bool, repeat: bool) -> ShortcutOutcome {
    let mut distinct: Vec<Binding> = Vec::new();
    for binding in exact {
        if let Some(same) = distinct.iter_mut().find(|other| {
            binding.target == other.target
                || binding.command.is_some() && binding.command == other.command
        }) {
            if repeat && binding.auto_repeat {
                *same = binding;
            }
        } else {
            distinct.push(binding);
        }
    }
    match (distinct.len(), has_prefix, repeat) {
        (0, false, _) => ShortcutOutcome::Unmatched,
        (0, true, _) => ShortcutOutcome::Consumed,
        (1, false, _) => {
            let binding = distinct.pop().expect("one shortcut target");
            if !repeat || binding.auto_repeat && binding.keys.len() == 1 {
                ShortcutOutcome::Activate(binding.target)
            } else {
                ShortcutOutcome::Consumed
            }
        }
        (_, _, true) => ShortcutOutcome::Consumed,
        _ => ShortcutOutcome::Ambiguous(
            distinct
                .into_iter()
                .filter_map(|binding| {
                    (binding.command.is_none()
                        && binding.target.action == ControlAction::ActivateShortcut)
                        .then_some(ControlInput {
                            path: binding.target.path,
                            action: ControlAction::AmbiguousShortcut,
                        })
                })
                .collect(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_complete_sequence_that_is_also_a_prefix_reports_ambiguity() {
        let first = "F11".parse().unwrap();
        let second = "F12".parse().unwrap();
        let binding = |index, keys| Binding {
            keys,
            target: ControlInput {
                path: vec![index],
                action: ControlAction::ActivateShortcut,
            },
            element_id: None,
            row_key: None,
            command: None,
            auto_repeat: false,
        };
        let short = binding(0, vec![first]);
        let long = binding(1, vec![first, second]);
        let active = vec![short, long.clone()];
        let mut pending = None;
        let now = Instant::now();
        assert_eq!(
            press(&mut pending, active.clone(), &[], first, false, now),
            ShortcutOutcome::Ambiguous(vec![ControlInput {
                path: vec![0],
                action: ControlAction::AmbiguousShortcut
            }]),
        );
        assert_eq!(
            press(&mut pending, active, &[], second, false, now),
            ShortcutOutcome::Activate(long.target),
        );
    }
}
