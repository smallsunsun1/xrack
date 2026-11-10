use crate::{Affine, Interaction, LayoutNode, Point, scale_rotate::gesture_target_live};
use xrack_runtime::{Component, Length, Node, RowKey, SwipeHandle, SwipeSample};

pub(crate) fn has_swipe_directions(node: &Node) -> bool {
    [
        "handle-swipe-left",
        "handle-swipe-right",
        "handle-swipe-up",
        "handle-swipe-down",
    ]
    .into_iter()
    .any(|name| node.boolean(name, false))
}

#[derive(Clone, Debug)]
pub(crate) struct SwipeTarget {
    path: Vec<usize>,
    handle: SwipeHandle,
    row: Option<RowKey>,
    to_local: Affine,
    origin: Point,
    sample: SwipeSample,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SwipePhase {
    Moved,
    Swiped,
    Cancelled,
}

#[derive(Clone, Debug)]
pub struct SwipeDelivery {
    path: Vec<usize>,
    handle: SwipeHandle,
    row: Option<RowKey>,
    sample: SwipeSample,
    phase: SwipePhase,
}

impl SwipeDelivery {
    pub fn matches(&self, scene: &Node) -> bool {
        scene.row_key_at(&self.path) == self.row
            && scene
                .at_path(&self.path)
                .and_then(|node| node.swipe_handle.as_ref())
                == Some(&self.handle)
    }

    pub fn apply(&self, component: &mut impl Component, scene: &Node) {
        if self.phase == SwipePhase::Moved
            && (self.handle.cancel_requested() || !self.matches(scene))
        {
            return;
        }
        let event = match self.phase {
            SwipePhase::Moved => "moved",
            SwipePhase::Swiped if !self.handle.cancel_requested() => "swiped",
            _ => "cancelled",
        };
        self.handle
            .publish(self.sample, self.phase == SwipePhase::Moved);
        if self.matches(scene) {
            component.dispatch_at(scene, &self.path, event);
        }
    }
}

impl SwipeTarget {
    pub fn new(scene: &Node, layout: &LayoutNode, path: &[usize], point: Point) -> Option<Self> {
        let node = scene.interactive_at_path(path)?;
        let handle = node.swipe_handle.clone()?;
        if !has_swipe_directions(node) {
            return None;
        }
        let item = layout.at_path(path)?;
        let to_local = item.world_transform().inverse()?;
        let local = to_local.map_point(point);
        if !item.visible
            || !item.enabled
            || !item.bounds.contains(local)
            || !item.contains_clip_point(point)
        {
            return None;
        }
        let origin = Point {
            x: item.bounds.x,
            y: item.bounds.y,
        };
        let x = Length(f64::from(local.x - origin.x));
        let y = Length(f64::from(local.y - origin.y));
        Some(Self {
            path: path.to_vec(),
            handle,
            row: scene.row_key_at(path),
            to_local,
            origin,
            sample: SwipeSample {
                pressed_x: x,
                pressed_y: y,
                current_x: x,
                current_y: y,
            },
        })
    }

    pub fn accepts(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        input: &Interaction,
        point: Point,
    ) -> bool {
        if !self.is_live(scene, layout, input) {
            return false;
        }
        let [x, y] = self.coordinates(point);
        let dx = x.0 - self.sample.pressed_x.0;
        let dy = y.0 - self.sample.pressed_y.0;
        if !dx.is_finite() || !dy.is_finite() || dx == 0.0 && dy == 0.0 {
            return false;
        }
        let direction = if dx.abs() >= dy.abs() {
            if dx < 0.0 {
                "handle-swipe-left"
            } else {
                "handle-swipe-right"
            }
        } else if dy < 0.0 {
            "handle-swipe-up"
        } else {
            "handle-swipe-down"
        };
        scene
            .at_path(&self.path)
            .is_some_and(|node| node.boolean(direction, false))
    }

    pub fn is_live(&self, scene: &Node, layout: &LayoutNode, input: &Interaction) -> bool {
        gesture_target_live(scene, layout, input, &self.path, self.row)
            && scene
                .interactive_at_path(&self.path)
                .and_then(|node| node.swipe_handle.as_ref())
                == Some(&self.handle)
    }

    pub fn cancel_requested(&self) -> bool {
        self.handle.cancel_requested()
    }

    pub fn update(&mut self, point: Point) -> Option<SwipeDelivery> {
        let [x, y] = self.coordinates(point);
        if !x.0.is_finite()
            || !y.0.is_finite()
            || (x == self.sample.current_x && y == self.sample.current_y)
        {
            return None;
        }
        self.sample.current_x = x;
        self.sample.current_y = y;
        Some(self.delivery(SwipePhase::Moved))
    }

    pub fn delivery(&self, phase: SwipePhase) -> SwipeDelivery {
        SwipeDelivery {
            path: self.path.clone(),
            handle: self.handle.clone(),
            row: self.row,
            sample: self.sample,
            phase,
        }
    }

    fn coordinates(&self, point: Point) -> [Length; 2] {
        // Keep the press coordinate frame even when a callback moves the handler.
        let local = self.to_local.map_point(point);
        [
            Length(f64::from(local.x - self.origin.x)),
            Length(f64::from(local.y - self.origin.y)),
        ]
    }
}
