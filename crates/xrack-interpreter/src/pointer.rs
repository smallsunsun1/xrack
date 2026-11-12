use xrack_runtime::{HoverHandle, PointerHandle, Value};

#[derive(Clone, Copy)]
pub(crate) enum HoverRead {
    Hovered,
    X,
    Y,
}

impl HoverRead {
    pub(crate) fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "hovered" => Self::Hovered,
            "hover-x" => Self::X,
            "hover-y" => Self::Y,
            _ => return None,
        })
    }

    pub(crate) fn read(self, handle: &HoverHandle) -> Value {
        match self {
            Self::Hovered => handle.hovered().into(),
            Self::X => handle.hover_x().into(),
            Self::Y => handle.hover_y().into(),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Read {
    Pressed,
    ContainsPress,
    HasHover,
    MouseX,
    MouseY,
    PressedX,
    PressedY,
    Count,
    Points,
}

impl Read {
    pub(crate) fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "pressed" => Self::Pressed,
            "contains-press" => Self::ContainsPress,
            "has-hover" => Self::HasHover,
            "mouse-x" => Self::MouseX,
            "mouse-y" => Self::MouseY,
            "pressed-x" => Self::PressedX,
            "pressed-y" => Self::PressedY,
            "active-touch-count" => Self::Count,
            "touch-points" => Self::Points,
            _ => return None,
        })
    }

    pub(crate) fn read(self, handle: &PointerHandle) -> Value {
        match self {
            Self::Pressed => handle.pressed().into(),
            Self::ContainsPress => handle.contains_press().into(),
            Self::HasHover => handle.has_hover().into(),
            Self::MouseX => handle.mouse_x().into(),
            Self::MouseY => handle.mouse_y().into(),
            Self::PressedX => handle.pressed_x().into(),
            Self::PressedY => handle.pressed_y().into(),
            Self::Count => handle.active_touch_count().into(),
            Self::Points => Value::TouchPoints(handle.touch_points()),
        }
    }
}
