use crate::{ControlInput, LayoutNode, Point, Rect, Size, Target, TextMeasurer, linear};
use xrack_runtime::{ControlAction, Node, RowKey, SwipeSide};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SwipeDelegateGeometry {
    bounds: Rect,
    widths: [f32; 2],
    pub side: SwipeSide,
    offset: f32,
    preview: bool,
}

pub(crate) struct Region {
    pub(crate) bounds: Rect,
    pub(crate) clip: Rect,
    pub(crate) enabled: bool,
}

impl SwipeDelegateGeometry {
    pub(crate) fn new(node: &Node, bounds: Rect, text: &mut impl TextMeasurer) -> Self {
        let available = Size::new(bounds.width, bounds.height);
        let limit = (bounds.width - 24.0).max(0.0);
        let widths = [SwipeSide::Left, SwipeSide::Right].map(|side| {
            node.swipe_panel(side).map_or(0.0, |panel| {
                linear::measure(panel, available, text)
                    .x
                    .preferred
                    .min(limit)
            })
        });
        let mut geometry = Self {
            bounds,
            widths,
            side: SwipeSide::Closed,
            offset: 0.0,
            preview: false,
        };
        let side = node.swipe_side();
        if geometry.supports(side) {
            geometry.side = side;
            geometry.offset = geometry.position(side);
        }
        geometry
    }

    pub fn supports(&self, side: SwipeSide) -> bool {
        side == SwipeSide::Closed || self.position(side) != 0.0
    }

    pub fn toggle(&self) -> Option<(Rect, ControlAction)> {
        if self.preview || self.widths == [0.0, 0.0] {
            return None;
        }
        let content = self.region("SwipeContent").bounds.intersect(self.bounds);
        let width = content.width.min(28.0);
        let height = content.height.min(32.0);
        let rect = Rect::new(
            content.x + content.width - width,
            content.y + (content.height - height) * 0.5,
            width,
            height,
        );
        let side = if self.side != SwipeSide::Closed {
            SwipeSide::Closed
        } else if self.supports(SwipeSide::Right) {
            SwipeSide::Right
        } else {
            SwipeSide::Left
        };
        Some((rect, ControlAction::SwipeDelegate(side)))
    }

    fn position(&self, side: SwipeSide) -> f32 {
        match side {
            SwipeSide::Closed => 0.0,
            SwipeSide::Left => self.widths[0],
            SwipeSide::Right => -self.widths[1],
        }
    }

    pub(crate) fn set_preview(&mut self, offset: f32) {
        self.offset = offset.clamp(-self.widths[1], self.widths[0]);
        self.preview = true;
    }

    pub(crate) fn region(&self, kind: &str) -> Region {
        let bounds = self.bounds;
        let (bounds, clip, enabled) = match kind {
            "SwipeContent" => (
                Rect {
                    x: bounds.x + self.offset,
                    ..bounds
                },
                bounds,
                true,
            ),
            "SwipeLeft" => (
                Rect {
                    width: self.widths[0],
                    ..bounds
                },
                Rect {
                    width: self.offset.max(0.0),
                    ..bounds
                },
                self.side == SwipeSide::Left && !self.preview,
            ),
            "SwipeRight" => {
                let right = bounds.x + bounds.width;
                (
                    Rect {
                        x: right - self.widths[1],
                        width: self.widths[1],
                        ..bounds
                    },
                    Rect {
                        x: right + self.offset.min(0.0),
                        width: (-self.offset).max(0.0),
                        ..bounds
                    },
                    self.side == SwipeSide::Right && !self.preview,
                )
            }
            _ => (Rect::default(), Rect::default(), false),
        };
        Region {
            bounds,
            clip,
            enabled,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SwipeDrag {
    pub(crate) target: Target,
    start: Point,
    initial: SwipeSide,
    geometry: SwipeDelegateGeometry,
    offset: Option<f32>,
}

impl SwipeDrag {
    pub(crate) fn new(
        target: Target,
        node: &Node,
        item: &LayoutNode,
        start: Point,
    ) -> Option<Self> {
        let geometry = *item.swipe_delegate.as_deref()?;
        (geometry.widths != [0.0, 0.0]).then_some(Self {
            target,
            start,
            initial: node.swipe_side(),
            geometry,
            offset: None,
        })
    }

    fn offset_at(&self, point: Point) -> f32 {
        (self.geometry.position(self.geometry.side) + point.x - self.start.x)
            .clamp(-self.geometry.widths[1], self.geometry.widths[0])
    }

    pub(crate) fn accepts_direction(&self, point: Point) -> bool {
        (point.x - self.start.x).abs() > (point.y - self.start.y).abs()
            && self.offset_at(point) != self.geometry.position(self.geometry.side)
    }

    fn matches(&self, node: &Node, geometry: &SwipeDelegateGeometry, row: Option<RowKey>) -> bool {
        node.kind == "SwipeDelegate"
            && self.target.matches_node(node)
            && node.swipe_side() == self.initial
            && row == self.target.row_key
            && self.geometry.bounds == geometry.bounds
            && self.geometry.widths == geometry.widths
    }

    fn is_current(&self, scene: &Node, layout: &LayoutNode) -> bool {
        let path = &self.target.path;
        scene
            .interactive_at_path(path)
            .zip(layout.at_path(path))
            .is_some_and(|(node, item)| {
                item.enabled
                    && item.visible
                    && item.swipe_delegate.as_deref().is_some_and(|geometry| {
                        self.matches(node, geometry, scene.row_key_at(path))
                    })
            })
    }

    pub(crate) fn update(&mut self, scene: &Node, layout: &LayoutNode, point: Point) -> bool {
        let offset = self
            .is_current(scene, layout)
            .then(|| self.offset_at(point));
        let changed = self.offset != offset;
        self.offset = offset;
        changed
    }

    pub(crate) fn preview(
        &self,
        path: &[usize],
        node: &Node,
        geometry: &SwipeDelegateGeometry,
        row: Option<RowKey>,
    ) -> Option<f32> {
        self.offset
            .filter(|_| self.target.path == path && self.matches(node, geometry, row))
    }

    pub(crate) fn has_preview(&self) -> bool {
        self.offset.is_some()
    }

    pub(crate) fn preview_state(&self) -> Option<(&[usize], Option<RowKey>, f32)> {
        Some((&self.target.path, self.target.row_key, self.offset?))
    }

    pub(crate) fn finish(&self, scene: &Node, layout: &LayoutNode) -> Option<ControlInput> {
        let offset = self.offset?;
        let side = if offset > 0.0 && offset >= self.geometry.widths[0] * 0.5 {
            SwipeSide::Left
        } else if offset < 0.0 && -offset >= self.geometry.widths[1] * 0.5 {
            SwipeSide::Right
        } else {
            SwipeSide::Closed
        };
        (side != self.initial && self.is_current(scene, layout)).then(|| ControlInput {
            path: self.target.path.clone(),
            action: ControlAction::SwipeDelegate(side),
        })
    }
}
