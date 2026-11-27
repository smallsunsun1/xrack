use crate::{ControlInput, LayoutNode, Point, Rect, Target, scroll_gesture::SCROLL_FRAME_INTERVAL};
use std::time::Instant;
use xrack_runtime::{ControlAction, LayoutDirection, Node, Orientation, TabMove, group_item};

#[derive(Clone, Debug)]
pub(crate) struct TabDrag {
    pub source: Target,
    pub start: Point,
    stamp: (u64, u64),
    pointer: Option<Point>,
    pub wake: Option<Instant>,
}

impl TabDrag {
    pub fn new(source: &Target, scene: &Node, layout: &LayoutNode, start: Point) -> Option<Self> {
        if source.tab_close.is_some() {
            return None;
        }
        let (&slot, path) = source.path.split_last()?;
        let bar = scene.at_path(path)?;
        let item = layout.at_path(path)?;
        if !matches!(bar.kind, "TabBar" | "TabWidget")
            || !bar.boolean("reorderable", false)
            || group_item(bar, slot)?.row_key.is_none()
            || !item.visible
            || !item.enabled
        {
            return None;
        }
        Some(Self {
            source: source.clone(),
            start,
            stamp: bar.standard_list_stamp?,
            pointer: None,
            wake: None,
        })
    }

    pub fn path(&self) -> &[usize] {
        &self.source.path[..self.source.path.len() - 1]
    }

    pub fn accepts_direction(&self, scene: &Node, layout: &LayoutNode, point: Point) -> bool {
        let Some((bar, item)) = self.resolve(scene, layout) else {
            return false;
        };
        let Some(delta) = item.map_delta_from_window(Point {
            x: point.x - self.start.x,
            y: point.y - self.start.y,
        }) else {
            return false;
        };
        match bar.orientation() {
            Orientation::Horizontal => delta.x.abs() > delta.y.abs(),
            Orientation::Vertical => delta.y.abs() > delta.x.abs(),
        }
    }

    fn resolve<'a, 'b>(
        &self,
        scene: &'a Node,
        layout: &'b LayoutNode,
    ) -> Option<(&'a Node, &'b LayoutNode)> {
        self.source.node_in(scene)?;
        let bar = scene.at_path(self.path())?;
        let item = layout.at_path(self.path())?;
        self.live(bar, item).then_some((bar, item))
    }

    fn live(&self, bar: &Node, item: &LayoutNode) -> bool {
        matches!(bar.kind, "TabBar" | "TabWidget")
            && bar.boolean("reorderable", false)
            && bar.standard_list_stamp == Some(self.stamp)
            && item.visible
            && item.enabled
    }

    pub fn update(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
        now: Instant,
    ) -> bool {
        let previous = self.pointer;
        self.pointer = Some(point);
        if self.scroll_target(scene, layout).is_some() {
            self.wake.get_or_insert(now + SCROLL_FRAME_INTERVAL);
        } else {
            self.wake = None;
        }
        previous != self.pointer
    }

    fn move_at(&self, bar: &Node, item: &LayoutNode) -> Option<TabMove> {
        if !self.live(bar, item) {
            return None;
        }
        let point = item.map_from_window(self.pointer?)?;
        let viewport = item.tab_viewport().intersect(item.clip);
        if !viewport.contains(point) {
            return None;
        }
        let vertical = bar.orientation() == Orientation::Vertical;
        let axis = |point: Point| if vertical { point.y } else { point.x };
        let to = (0..item.children.len())
            .filter_map(|index| {
                group_item(bar, index)?;
                let visible = item.tab_item_rect(index)?.intersect(viewport);
                if visible.width <= 0.0 || visible.height <= 0.0 {
                    return None;
                }
                let center = Point {
                    x: visible.x + visible.width / 2.0,
                    y: visible.y + visible.height / 2.0,
                };
                Some((index, (axis(center) - axis(point)).abs()))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))?
            .0;
        bar.tab_move(*self.source.path.last()?, to)
    }

    pub fn marker(&self, bar: &Node, item: &LayoutNode) -> Option<Rect> {
        let movement = self.move_at(bar, item)?;
        let viewport = item.clip.intersect(item.tab_viewport());
        let target = item.tab_item_rect(movement.to)?.intersect(viewport);
        let after = movement.to > movement.from;
        let line = match bar.orientation() {
            Orientation::Vertical => {
                let y = if after {
                    target.y + target.height
                } else {
                    target.y
                };
                Rect::new(target.x + 3.0, y - 1.5, (target.width - 6.0).max(0.0), 3.0)
            }
            Orientation::Horizontal => {
                let reversed = item.layout_direction == LayoutDirection::RightToLeft;
                let x = if after != reversed {
                    target.x + target.width
                } else {
                    target.x
                };
                Rect::new(x - 1.5, target.y + 3.0, 3.0, (target.height - 6.0).max(0.0))
            }
        };
        let clip = line.intersect(viewport);
        (clip.width > 0.0 && clip.height > 0.0).then_some(clip)
    }

    pub fn finish(&self, scene: &Node, layout: &LayoutNode) -> Option<ControlInput> {
        let (bar, item) = self.resolve(scene, layout)?;
        let movement = self.move_at(bar, item)?;
        Some(ControlInput {
            path: self.path().to_vec(),
            action: ControlAction::MoveTab(movement),
        })
    }

    pub fn scroll_target(&self, scene: &Node, layout: &LayoutNode) -> Option<(Orientation, f32)> {
        let (bar, item) = self.resolve(scene, layout)?;
        let point = item.map_from_window(self.pointer?)?;
        let viewport = item.tab_viewport().intersect(item.clip);
        if !viewport.contains(point) {
            return None;
        }
        let orientation = bar.orientation();
        let axis = item.scroll_on(orientation)?;
        let (position, start, extent) = match orientation {
            Orientation::Horizontal => (point.x, viewport.x, viewport.width),
            Orientation::Vertical => (point.y, viewport.y, viewport.height),
        };
        let edge = (extent * 0.25).min(26.0);
        if edge <= 0.0 {
            return None;
        }
        let direction = ((position - (start + extent - edge)) / edge).max(0.0)
            - ((start + edge - position) / edge).max(0.0);
        // TabBar offsets use physical coordinates, including in RTL.
        let offset = (axis.offset + direction * 12.0).clamp(0.0, axis.max_offset);
        (offset != axis.offset).then_some((orientation, offset))
    }
}
