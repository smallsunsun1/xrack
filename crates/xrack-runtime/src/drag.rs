use crate::{KeyModifiers, Length};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DragAction {
    #[default]
    None,
    Copy,
    Move,
    Link,
}

impl KeyModifiers {
    #[doc(hidden)]
    pub fn drag_action(self) -> Option<DragAction> {
        match (self.control, self.shift) {
            (false, false) => None,
            (true, false) => Some(DragAction::Copy),
            (false, true) => Some(DragAction::Move),
            (true, true) => Some(DragAction::Link),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum DragEvent {
    Started,
    Finished(DragAction),
    Entered { data: String, action: DragAction },
    FilesEntered(Arc<[String]>),
    Moved { x: Length, y: Length },
    Exited,
    Dropped { data: String, action: DragAction },
    FilesDropped(Arc<[String]>),
}

impl DragEvent {
    #[doc(hidden)]
    pub fn target_kind(&self) -> &'static str {
        match self {
            Self::Started | Self::Finished(_) => "DragArea",
            _ => "DropArea",
        }
    }

    #[doc(hidden)]
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Exited | Self::Finished(_))
    }
}

pub fn is_drag_area(kind: &str) -> bool {
    matches!(kind, "DragArea" | "DropArea")
}
