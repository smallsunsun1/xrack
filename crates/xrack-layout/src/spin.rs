use crate::{Point, Rect};
use xrack_runtime::SpinAction;

pub struct SpinGeometry {
    pub text: Rect,
    pub up: Rect,
    pub down: Rect,
}

impl SpinGeometry {
    pub fn new(bounds: Rect) -> Self {
        let width = 28.0_f32.min(bounds.width * 0.5);
        let half_height = bounds.height * 0.5;
        let x = bounds.x + bounds.width - width;
        Self {
            text: Rect::new(bounds.x, bounds.y, bounds.width - width, bounds.height),
            up: Rect::new(x, bounds.y, width, half_height),
            down: Rect::new(x, bounds.y + half_height, width, half_height),
        }
    }

    pub fn action_at(&self, point: Point) -> Option<SpinAction> {
        if self.up.contains(point) {
            Some(SpinAction::Increase)
        } else if self.down.contains(point) {
            Some(SpinAction::Decrease)
        } else {
            None
        }
    }
}
