use crate::{Interaction, LayoutNode, Rect, ScrollLayout, view_cells::ViewCells};
use std::collections::BTreeMap;
use xrack_runtime::{
    Component, GeometryHandle, Length, Node, Orientation, ScrollAction, ScrollAlignment,
    ScrollRequest, ViewportHandle,
};

#[derive(Clone, Debug)]
pub(crate) struct UserScrollChange {
    target: ViewportHandle,
    before: [Option<f32>; 2],
}

impl UserScrollChange {
    pub(crate) fn record(
        changes: &mut Vec<Self>,
        item: &LayoutNode,
        axis: Orientation,
        offset: f32,
    ) {
        let Some(target) = item
            .viewport_handle
            .as_ref()
            .filter(|handle| handle.observes_user_scroll())
        else {
            return;
        };
        let index = usize::from(axis == Orientation::Vertical);
        if let Some(change) = changes.iter_mut().find(|change| &change.target == target) {
            change.before[index].get_or_insert(offset);
        } else {
            let mut before = [None; 2];
            before[index] = Some(offset);
            changes.push(Self {
                target: target.clone(),
                before,
            });
        }
    }

    pub(crate) fn commit(
        changes: &mut Vec<Self>,
        item: &LayoutNode,
        axis: Orientation,
        before: f32,
        after: f32,
    ) {
        item.write_bound_offset(axis, after);
        Self::record(changes, item, axis, before);
    }
}

impl LayoutNode {
    pub(crate) fn bound_offset(&self, axis: Orientation) -> Option<f32> {
        self.viewport_handle.as_ref()?.bound_offset(axis)
    }

    pub(crate) fn write_bound_offset(&self, axis: Orientation, offset: f32) {
        if let Some(handle) = &self.viewport_handle {
            handle.write_bound_offset(axis, offset);
        }
    }
}

/// Delivery is tied to the component instance that consumed the input.
pub struct ScrollDelivery {
    target: ViewportHandle,
    path: Vec<usize>,
}

impl ScrollDelivery {
    pub fn apply(&self, component: &mut impl Component, scene: &Node) -> bool {
        if scene
            .at_path(&self.path)
            .and_then(|node| node.viewport_handle.as_ref())
            == Some(&self.target)
        {
            component.dispatch_at(scene, &self.path, "scrolled")
        } else {
            false
        }
    }
}

#[derive(Default)]
pub struct LayoutObservers {
    present: Vec<ViewportHandle>,
    geometry: BTreeMap<usize, GeometryHandle>,
}

pub type ViewportObservers = LayoutObservers;

impl LayoutObservers {
    pub fn sync(&mut self, scene: &Node, layout: &LayoutNode) {
        fn publish(
            node: &Node,
            item: &LayoutNode,
            parent: Rect,
            viewports: &mut Vec<ViewportHandle>,
            geometry: &mut BTreeMap<usize, GeometryHandle>,
        ) {
            if let Some(handle) = &node.viewport_handle
                && item.visible
            {
                let viewport = viewport(item);
                let offset = |axis| item.scroll_on(axis).map_or(0.0, |scroll| scroll.offset);
                handle.publish(
                    Length(-f64::from(offset(Orientation::Horizontal))),
                    Length(-f64::from(offset(Orientation::Vertical))),
                    Length(f64::from(viewport.width)),
                    Length(f64::from(viewport.height)),
                );
                viewports.push(handle.clone());
            }
            if item.visible
                && (node.geometry_handle.is_some() || node.instance_geometry_handle.is_some())
            {
                let rect = item.bounds;
                let window = item.window_bounds(rect);
                for handle in node
                    .geometry_handle
                    .iter()
                    .chain(node.instance_geometry_handle.iter())
                {
                    handle.publish(
                        [
                            rect.x - parent.x,
                            rect.y - parent.y,
                            rect.width,
                            rect.height,
                        ],
                        [window.x, window.y, window.width, window.height],
                        true,
                    );
                    geometry.insert(handle.identity(), handle.clone());
                }
            }
            for (child, layout) in node.children.iter().zip(&item.children) {
                publish(child, layout, item.bounds, viewports, geometry);
            }
        }

        let mut next = Vec::new();
        let mut geometry = BTreeMap::new();
        publish(scene, layout, layout.bounds, &mut next, &mut geometry);
        for handle in self.present.drain(..) {
            if !next.contains(&handle) {
                reset(&handle);
            }
        }
        self.present = next;
        for (id, handle) in std::mem::take(&mut self.geometry) {
            if !geometry.contains_key(&id) {
                handle.clear();
            }
        }
        self.geometry = geometry;
    }

    pub fn clear(&mut self) {
        for handle in self.present.drain(..) {
            reset(&handle);
        }
        for handle in std::mem::take(&mut self.geometry).into_values() {
            handle.clear();
        }
    }
}

fn reset(handle: &ViewportHandle) {
    let zero = Length::default();
    handle.publish(zero, zero, zero, zero);
}

fn viewport(item: &LayoutNode) -> Rect {
    item.table.as_ref().map_or(item.bounds, |table| table.body)
}

fn find_path(node: &Node, handle: &ViewportHandle, path: &mut Vec<usize>) -> bool {
    if node.viewport_handle.as_ref() == Some(handle) {
        return true;
    }
    for (index, child) in node.children.iter().enumerate() {
        path.push(index);
        if find_path(child, handle, path) {
            return true;
        }
        path.pop();
    }
    false
}

impl Interaction {
    /// Resolve movement after layout clamps offsets and replaces virtual rows.
    pub fn take_scroll_events(&mut self, scene: &Node, layout: &LayoutNode) -> Vec<ScrollDelivery> {
        std::mem::take(&mut self.user_scroll_changes)
            .into_iter()
            .filter_map(|change| {
                let mut path = Vec::new();
                if !find_path(scene, &change.target, &mut path) {
                    return None;
                }
                let item = layout
                    .at_path(&path)
                    .filter(|item| item.visible && item.enabled)?;
                let moved = [Orientation::Horizontal, Orientation::Vertical]
                    .into_iter()
                    .zip(change.before)
                    .any(|(axis, offset)| {
                        offset
                            .zip(item.scroll_on(axis))
                            .is_some_and(|(offset, scroll)| offset != scroll.offset)
                    });
                moved.then_some(ScrollDelivery {
                    target: change.target,
                    path,
                })
            })
            .collect()
    }

    pub fn clear_scroll_events(&mut self) {
        self.user_scroll_changes.clear();
    }

    pub fn apply_scroll_requests(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        requests: impl IntoIterator<Item = ScrollRequest>,
    ) -> bool {
        let mut changed = false;
        for request in requests {
            let mut path = Vec::new();
            if !find_path(scene, &request.target, &mut path) {
                continue;
            }
            let (Some(node), Some(item)) = (
                scene.at_path(&path),
                layout.at_path(&path).filter(|item| item.visible),
            ) else {
                continue;
            };
            match request.action {
                ScrollAction::To { x, y } => {
                    for (axis, offset) in [(Orientation::Horizontal, x), (Orientation::Vertical, y)]
                    {
                        changed |= self.set_viewport_offset(&path, item, axis, offset.0);
                    }
                }
                ScrollAction::ToIndex { index, alignment } => {
                    let Some((list, scroll)) = node.list.zip(item.scroll) else {
                        continue;
                    };
                    let Ok(index) = usize::try_from(index) else {
                        continue;
                    };
                    if index >= list.row_count {
                        continue;
                    }
                    let viewport = viewport(item);
                    let cells = ViewCells::new(node, viewport.width);
                    let top = cells.top(index) as f32;
                    let current = item
                        .bound_offset(Orientation::Vertical)
                        .or_else(|| self.scroll_offsets().get(path.as_slice()).copied())
                        .unwrap_or(scroll.offset);
                    let current = ScrollLayout::new(current, scroll.max_offset);
                    let offset = position(
                        current,
                        top,
                        cells.height(index),
                        viewport.height,
                        alignment,
                    );
                    changed |= self.set_viewport_offset(
                        &path,
                        item,
                        Orientation::Vertical,
                        f64::from(offset),
                    );
                }
            }
        }
        changed
    }
}

fn position(
    scroll: ScrollLayout,
    top: f32,
    height: f32,
    viewport: f32,
    alignment: ScrollAlignment,
) -> f32 {
    match alignment {
        ScrollAlignment::Beginning => top,
        ScrollAlignment::Center => top + (height - viewport) * 0.5,
        ScrollAlignment::End => top + height - viewport,
        ScrollAlignment::Visible
            if top < scroll.offset + viewport && top + height > scroll.offset =>
        {
            scroll.offset
        }
        ScrollAlignment::Contain | ScrollAlignment::Visible if height > viewport => top,
        ScrollAlignment::Contain | ScrollAlignment::Visible => {
            scroll.revealing(top, height, viewport).offset
        }
    }
}
