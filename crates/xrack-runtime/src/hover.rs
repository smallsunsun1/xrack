use crate::{Length, Property};
use std::{fmt, rc::Rc};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorShape {
    #[default]
    Auto,
    Default,
    Pointer,
    Text,
    Crosshair,
    Move,
    Grab,
    Grabbing,
    NotAllowed,
    Wait,
    Help,
    ResizeHorizontal,
    ResizeVertical,
    ZoomIn,
    ZoomOut,
}

#[derive(Default)]
struct HoverSignals {
    hovered: Property<bool>,
    x: Property<Length>,
    y: Property<Length>,
}

/// Reactive hover state for one instance of a handler.
#[doc(hidden)]
#[derive(Clone, Default)]
pub struct HoverHandle(Rc<HoverSignals>);

impl fmt::Debug for HoverHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HoverHandle").finish_non_exhaustive()
    }
}

impl PartialEq for HoverHandle {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl HoverHandle {
    pub fn hovered(&self) -> bool {
        self.0.hovered.get(|| false)
    }

    pub fn hover_x(&self) -> Length {
        self.0.x.get(Length::default)
    }

    pub fn hover_y(&self) -> Length {
        self.0.y.get(Length::default)
    }

    pub fn publish(&self, position: Option<(Length, Length)>) {
        let (x, y) = position.unwrap_or_default();
        self.0.x.set_if_changed(x);
        self.0.y.set_if_changed(y);
        self.0.hovered.set_if_changed(position.is_some());
    }
}
