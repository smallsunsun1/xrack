use std::{cell::RefCell, collections::BTreeMap};
use xrack_runtime::{ScaleRotateHandle, SwipeHandle, Value};

#[derive(Clone, Copy)]
pub(crate) enum Metric {
    Swiping,
    PressedX,
    PressedY,
    CurrentX,
    CurrentY,
    Active,
    Scale,
    Rotation,
    CenterX,
    CenterY,
}

impl Metric {
    pub(crate) fn parse(operation: &str, name: &str) -> Option<Self> {
        Some(match (operation, name) {
            ("xr.swipe_read", "swiping") => Self::Swiping,
            ("xr.swipe_read", "pressed-x") => Self::PressedX,
            ("xr.swipe_read", "pressed-y") => Self::PressedY,
            ("xr.swipe_read", "current-x") => Self::CurrentX,
            ("xr.swipe_read", "current-y") => Self::CurrentY,
            ("xr.scale_rotate_read", "active") => Self::Active,
            ("xr.scale_rotate_read", "scale") => Self::Scale,
            ("xr.scale_rotate_read", "rotation") => Self::Rotation,
            ("xr.scale_rotate_read", "center-x") => Self::CenterX,
            ("xr.scale_rotate_read", "center-y") => Self::CenterY,
            _ => return None,
        })
    }
}

#[derive(Default)]
pub(crate) struct Handles {
    swipes: RefCell<BTreeMap<String, SwipeHandle>>,
    scale_rotate: RefCell<BTreeMap<String, ScaleRotateHandle>>,
}

impl Handles {
    pub(crate) fn swipe(&self, key: &str) -> SwipeHandle {
        self.swipes
            .borrow_mut()
            .entry(key.into())
            .or_default()
            .clone()
    }

    pub(crate) fn scale_rotate(&self, key: &str) -> ScaleRotateHandle {
        self.scale_rotate
            .borrow_mut()
            .entry(key.into())
            .or_default()
            .clone()
    }

    pub(crate) fn read(&self, key: &str, metric: Metric) -> Value {
        match metric {
            Metric::Swiping => self.swipe(key).swiping().into(),
            Metric::PressedX => self.swipe(key).pressed_x().into(),
            Metric::PressedY => self.swipe(key).pressed_y().into(),
            Metric::CurrentX => self.swipe(key).current_x().into(),
            Metric::CurrentY => self.swipe(key).current_y().into(),
            Metric::Active => self.scale_rotate(key).active().into(),
            Metric::Scale => self.scale_rotate(key).scale().into(),
            Metric::Rotation => self.scale_rotate(key).rotation().into(),
            Metric::CenterX => self.scale_rotate(key).center_x().into(),
            Metric::CenterY => self.scale_rotate(key).center_y().into(),
        }
    }
}
