use crate::Property;
use std::{cell::RefCell, fmt, ops::Range, rc::Rc};

/// Identity of a focus target in one component instance.
#[derive(Clone, Default)]
pub struct FocusHandle(Rc<FocusSignals>);

#[derive(Default)]
struct FocusSignals {
    direct: Property<bool>,
    within: Property<bool>,
    text: Option<Box<TextSelectionSignals>>,
    commands: Option<Box<TextCommandSignals>>,
}

#[derive(Default)]
struct TextSelectionSignals {
    offsets: [Property<i64>; 3],
    selected: Property<Rc<str>>,
}

#[derive(Default)]
struct TextCommandSignals {
    undo: Property<bool>,
    redo: Property<bool>,
    cut: Property<bool>,
    copy: Property<bool>,
}

/// Availability of commands on the active text editor.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextCommandAvailability {
    pub undo: bool,
    pub redo: bool,
    pub cut: bool,
    pub copy: bool,
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
    #[doc(hidden)]
    pub fn with_text_selection() -> Self {
        Self::with_text_state(true, false)
    }

    #[doc(hidden)]
    pub fn with_text_state(selection: bool, commands: bool) -> Self {
        Self(Rc::new(FocusSignals {
            text: selection.then(Box::default),
            commands: commands.then(Box::default),
            ..FocusSignals::default()
        }))
    }

    #[doc(hidden)]
    pub fn observes_text_commands(&self) -> bool {
        self.0.commands.is_some()
    }

    #[doc(hidden)]
    pub fn publish_text_commands(&self, commands: TextCommandAvailability) {
        if let Some(signals) = &self.0.commands {
            signals.undo.set_if_changed(commands.undo);
            signals.redo.set_if_changed(commands.redo);
            signals.cut.set_if_changed(commands.cut);
            signals.copy.set_if_changed(commands.copy);
        }
    }

    #[doc(hidden)]
    pub fn clear_text_state(&self) {
        self.publish_text_selection(0, 0..0, "");
        self.publish_text_commands(TextCommandAvailability::default());
    }

    pub fn can_undo(&self) -> bool {
        self.0
            .commands
            .as_ref()
            .is_some_and(|signals| signals.undo.get(|| false))
    }

    pub fn can_redo(&self) -> bool {
        self.0
            .commands
            .as_ref()
            .is_some_and(|signals| signals.redo.get(|| false))
    }

    pub fn can_cut(&self) -> bool {
        self.0
            .commands
            .as_ref()
            .is_some_and(|signals| signals.cut.get(|| false))
    }

    pub fn can_copy(&self) -> bool {
        self.0
            .commands
            .as_ref()
            .is_some_and(|signals| signals.copy.get(|| false))
    }

    #[doc(hidden)]
    pub fn observes_text_selection(&self) -> bool {
        self.0.text.is_some()
    }

    #[doc(hidden)]
    pub fn publish_text_selection(&self, caret: usize, selection: Range<usize>, text: &str) {
        if let Some(signals) = &self.0.text {
            for (offset, value) in
                signals
                    .offsets
                    .iter()
                    .zip([caret, selection.start, selection.end])
            {
                offset.set_if_changed(value as i64);
            }
            if signals.selected.explicit_value().as_deref() != Some(text) {
                signals.selected.set(Rc::from(text));
            }
        }
    }

    fn text_offset(&self, index: usize) -> i64 {
        self.0
            .text
            .as_ref()
            .map_or(0, |signals| signals.offsets[index].get(|| 0))
    }

    pub fn cursor_offset(&self) -> i64 {
        self.text_offset(0)
    }

    pub fn selection_start_offset(&self) -> i64 {
        self.text_offset(1)
    }

    pub fn selection_end_offset(&self) -> i64 {
        self.text_offset(2)
    }

    pub fn selected_text(&self) -> String {
        self.0.text.as_ref().map_or_else(String::new, |signals| {
            signals.selected.get(|| Rc::from("")).to_string()
        })
    }

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
    fn selection_start_and_selected_text_track_independently() {
        let handle = FocusHandle::with_text_selection();
        let start = Property::default();
        let selected = Property::default();
        handle.publish_text_selection(3, 1..3, "bc");
        assert_eq!(start.get(|| handle.selection_start_offset()), 1);
        assert_eq!(selected.get(|| handle.selected_text()), "bc");

        handle.publish_text_selection(4, 1..4, "bcd");
        assert!(!start.is_dirty());
        assert!(selected.is_dirty());
        assert_eq!(selected.get(|| handle.selected_text()), "bcd");

        handle.publish_text_selection(1, 1..4, "bcd");
        assert!(!start.is_dirty());
        assert!(!selected.is_dirty());
    }

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
