use crate::Property;
use std::{cell::RefCell, fmt, rc::Rc};

/// Identity of a focus target in one component instance.
#[derive(Clone, Default)]
pub struct FocusHandle(Rc<FocusSignals>);

#[derive(Default)]
struct FocusSignals {
    direct: Property<bool>,
    within: Property<bool>,
}

impl fmt::Debug for FocusHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FocusHandle").finish_non_exhaustive()
    }
}

impl PartialEq for FocusHandle {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusAction {
    Focus,
    Clear,
    Text(TextCommand),
    EditTableCell { row: i64, column: i64 },
    CancelTableEdit,
}

/// A programmatic edit using the same selection, validation and history as keyboard input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextCommand {
    SelectAll,
    ClearSelection,
    SetSelection { anchor: i64, focus: i64 },
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
}

/// Commands lowered from XR focus and editor methods.
#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub enum FocusCommandMethod {
    Focus,
    Clear,
    SelectAll,
    ClearSelection,
    SetSelection,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    EditTableCell,
    CancelTableEdit,
}

impl FocusCommandMethod {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "focus" => Self::Focus,
            "clear" => Self::Clear,
            "select-all" => Self::SelectAll,
            "clear-selection" => Self::ClearSelection,
            "set-selection-offsets" => Self::SetSelection,
            "undo" => Self::Undo,
            "redo" => Self::Redo,
            "cut" => Self::Cut,
            "copy" => Self::Copy,
            "paste" => Self::Paste,
            "edit-cell" => Self::EditTableCell,
            "cancel-edit" => Self::CancelTableEdit,
            _ => return None,
        })
    }

    pub fn action(self, operands: &[i64]) -> Option<FocusAction> {
        let text = match (self, operands) {
            (Self::Focus, []) => return Some(FocusAction::Focus),
            (Self::Clear, []) => return Some(FocusAction::Clear),
            (Self::SelectAll, []) => TextCommand::SelectAll,
            (Self::ClearSelection, []) => TextCommand::ClearSelection,
            (Self::SetSelection, &[anchor, focus]) => TextCommand::SetSelection { anchor, focus },
            (Self::Undo, []) => TextCommand::Undo,
            (Self::Redo, []) => TextCommand::Redo,
            (Self::Cut, []) => TextCommand::Cut,
            (Self::Copy, []) => TextCommand::Copy,
            (Self::Paste, []) => TextCommand::Paste,
            (Self::EditTableCell, &[row, column]) => {
                return Some(FocusAction::EditTableCell { row, column });
            }
            (Self::CancelTableEdit, []) => return Some(FocusAction::CancelTableEdit),
            _ => return None,
        };
        Some(FocusAction::Text(text))
    }
}

#[doc(hidden)]
pub fn is_focus_handler(name: &str) -> bool {
    FocusHandler::ALL
        .iter()
        .any(|handler| handler.name() == name)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FocusReason {
    #[default]
    Programmatic,
    TabNavigation,
    PointerClick,
    PopupActivation,
    WindowActivation,
    Accessibility,
    LayoutChange,
    KeyboardNavigation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusEvent {
    Gained(FocusReason),
    Lost(FocusReason),
    Within { focused: bool, reason: FocusReason },
}

#[derive(Clone, Copy, Debug)]
#[doc(hidden)]
pub enum FocusHandler {
    Gained,
    Lost,
    Changed,
    Within,
}

impl FocusHandler {
    pub const ALL: [Self; 4] = [Self::Gained, Self::Lost, Self::Changed, Self::Within];

    pub fn name(self) -> &'static str {
        match self {
            Self::Gained => "focus-gained",
            Self::Lost => "focus-lost",
            Self::Changed => "focus-changed-event",
            Self::Within => "focus-within-changed",
        }
    }
}

impl FocusEvent {
    #[doc(hidden)]
    pub fn is_loss(self) -> bool {
        matches!(self, Self::Lost(_) | Self::Within { focused: false, .. })
    }

    #[doc(hidden)]
    pub fn try_deliver<E>(
        self,
        mut notify: impl FnMut(FocusHandler, bool, FocusReason) -> Result<bool, E>,
    ) -> Result<bool, E> {
        let (handler, focused, reason) = match self {
            Self::Gained(reason) => (FocusHandler::Gained, true, reason),
            Self::Lost(reason) => (FocusHandler::Lost, false, reason),
            Self::Within { focused, reason } => {
                return notify(FocusHandler::Within, focused, reason);
            }
        };
        let handled = notify(handler, focused, reason)?;
        Ok(notify(FocusHandler::Changed, focused, reason)? || handled)
    }

    #[doc(hidden)]
    pub fn deliver(self, mut notify: impl FnMut(FocusHandler, bool, FocusReason) -> bool) -> bool {
        self.try_deliver(|handler, focused, reason| {
            Ok::<_, std::convert::Infallible>(notify(handler, focused, reason))
        })
        .unwrap_or_else(|error| match error {})
    }
}

pub struct FocusRequest {
    pub target: FocusHandle,
    pub action: FocusAction,
}

/// A window's commands, drained against the scene prepared after the callback.
#[derive(Clone, Default)]
pub struct FocusRequests(Rc<RefCell<Vec<FocusRequest>>>);

thread_local! {
    static ACTIVE: RefCell<Option<FocusRequests>> = const { RefCell::new(None) };
}

pub struct FocusContext(Option<FocusRequests>);

impl FocusRequests {
    pub fn enter(&self) -> FocusContext {
        FocusContext(ACTIVE.with(|active| active.replace(Some(self.clone()))))
    }

    pub fn drain(&self) -> Vec<FocusRequest> {
        std::mem::take(&mut *self.0.borrow_mut())
    }

    pub fn is_empty(&self) -> bool {
        self.0.borrow().is_empty()
    }
}

impl Drop for FocusContext {
    fn drop(&mut self) {
        ACTIVE.with(|active| active.replace(self.0.take()));
    }
}

impl FocusHandle {
    pub fn has_focus(&self) -> bool {
        self.0.direct.get(|| false)
    }

    pub fn focus_within(&self) -> bool {
        self.0.within.get(|| false)
    }

    #[doc(hidden)]
    pub fn publish(&self, direct: bool, within: bool) {
        self.0.direct.set_if_changed(direct);
        self.0.within.set_if_changed(within);
    }

    pub fn request(&self, action: FocusAction) {
        ACTIVE.with(|active| {
            if let Some(requests) = &*active.borrow() {
                requests.0.borrow_mut().push(FocusRequest {
                    target: self.clone(),
                    action,
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_and_within_focus_have_independent_dependencies() {
        let handle = FocusHandle::default();
        let direct = Property::default();
        let within = Property::default();
        assert!(!direct.get(|| handle.has_focus()));
        assert!(!within.get(|| handle.focus_within()));
        handle.publish(false, true);
        assert!(!direct.is_dirty());
        assert!(within.is_dirty());
        assert!(within.get(|| handle.focus_within()));
        handle.publish(true, true);
        assert!(direct.is_dirty());
        assert!(!within.is_dirty());
    }
}
