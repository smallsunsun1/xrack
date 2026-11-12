use crate::{Error, Result};
use xrack_runtime::{FocusAction, TextCommand, Value};

pub(crate) fn is_handler(name: &str) -> bool {
    matches!(
        name,
        "focus-gained" | "focus-lost" | "focus-changed-event" | "focus-within-changed"
    )
}

#[derive(Clone, Copy)]
pub(crate) enum Command {
    Focus,
    Clear,
    Text(TextCommand),
    SetSelection,
    EditTableCell,
    CancelTableEdit,
}

impl Command {
    pub(crate) fn parse(method: &str) -> Option<Self> {
        Some(match method {
            "focus" => Self::Focus,
            "clear" => Self::Clear,
            "select-all" => Self::Text(TextCommand::SelectAll),
            "clear-selection" => Self::Text(TextCommand::ClearSelection),
            "undo" => Self::Text(TextCommand::Undo),
            "redo" => Self::Text(TextCommand::Redo),
            "cut" => Self::Text(TextCommand::Cut),
            "copy" => Self::Text(TextCommand::Copy),
            "paste" => Self::Text(TextCommand::Paste),
            "set-selection-offsets" => Self::SetSelection,
            "edit-cell" => Self::EditTableCell,
            "cancel-edit" => Self::CancelTableEdit,
            _ => return None,
        })
    }

    pub(crate) fn action(self, args: &[Value]) -> Result<FocusAction> {
        Ok(match (self, args) {
            (Self::Focus, []) => FocusAction::Focus,
            (Self::Clear, []) => FocusAction::Clear,
            (Self::Text(command), []) => FocusAction::Text(command),
            (Self::SetSelection, [Value::Int(anchor), Value::Int(focus)]) => {
                FocusAction::Text(TextCommand::SetSelection {
                    anchor: *anchor,
                    focus: *focus,
                })
            }
            (Self::EditTableCell, [Value::Int(row), Value::Int(column)]) => {
                FocusAction::EditTableCell {
                    row: *row,
                    column: *column,
                }
            }
            (Self::CancelTableEdit, []) => FocusAction::CancelTableEdit,
            _ => return Err(Error::Evaluation("invalid focus command arguments".into())),
        })
    }
}
