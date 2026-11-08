use crate::{Point, Rect};
use xrack_runtime::{ControlAction, Node, text_input_value};

pub struct SearchGeometry {
    pub submit: Rect,
    pub entry: Rect,
    pub clear: Rect,
}

impl SearchGeometry {
    pub fn new(bounds: Rect) -> Self {
        let button = (bounds.width * 0.25).min(30.0);
        let entry = bounds.width - 2.0 * button;
        Self {
            submit: Rect::new(bounds.x, bounds.y, button, bounds.height),
            entry: Rect::new(bounds.x + button, bounds.y, entry, bounds.height),
            clear: Rect::new(bounds.x + button + entry, bounds.y, button, bounds.height),
        }
    }

    pub fn actions(&self, node: &Node) -> impl Iterator<Item = (Rect, ControlAction)> {
        let clear = (!node.boolean("read-only", false) && !text_input_value(node).is_empty())
            .then_some((self.clear, ControlAction::ClearSearch));
        [(self.submit, ControlAction::SubmitSearch)]
            .into_iter()
            .chain(clear)
    }

    pub fn action_at(&self, node: &Node, point: Point) -> Option<ControlAction> {
        self.actions(node)
            .find_map(|(rect, action)| rect.contains(point).then_some(action))
    }
}
