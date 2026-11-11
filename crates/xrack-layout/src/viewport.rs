use crate::{
    Interaction, LayoutNode, Rect, ScrollLayout, scroll_gesture::PanDevice, view_cells::ViewCells,
};
use std::collections::BTreeMap;
use xrack_runtime::{
    Component, GeometryHandle, Length, Node, Orientation, ScrollAction, ScrollAlignment,
    ScrollRequest, ViewportHandle, ViewportMotion,
};

pub(crate) type MotionOwners = Vec<(ViewportHandle, [bool; 2])>;

pub(crate) fn record_motion(owners: &mut MotionOwners, item: &LayoutNode, axis: Orientation) {
    let Some(handle) = item
        .viewport_handle
        .as_ref()
        .filter(|handle| handle.observes_motion())
    else {
        return;
    };
    record_motion_owner(owners, handle, axis);
}

pub(crate) fn record_motion_owner(
    owners: &mut MotionOwners,
    handle: &ViewportHandle,
    axis: Orientation,
) {
    let index = usize::from(axis == Orientation::Vertical);
    if let Some((_, axes)) = owners.iter_mut().find(|(owner, _)| owner == handle) {
        axes[index] = true;
    } else {
        let mut axes = [false; 2];
        axes[index] = true;
        owners.push((handle.clone(), axes));
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct MotionState {
    owners: MotionOwners,
}

impl MotionState {
    pub fn update(&mut self, owners: MotionOwners, motion: ViewportMotion) {
        for (handle, _) in self.owners.drain(..) {
            if !owners.iter().any(|(owner, _)| owner == &handle) {
                handle.publish_motion(ViewportMotion::Idle);
            }
        }
        self.owners = owners;
        self.set_phase(motion);
    }

    pub fn set_phase(&self, motion: ViewportMotion) {
        for (handle, _) in &self.owners {
            handle.publish_motion(motion);
        }
    }

    pub fn retain_axes(&mut self, axes: [bool; 2]) {
        self.owners.retain_mut(|(handle, owned)| {
            for (owned, axis) in owned.iter_mut().zip(axes) {
                *owned &= axis;
            }
            let keep = owned.contains(&true);
            if !keep {
                handle.publish_motion(ViewportMotion::Idle);
            }
            keep
        });
    }

    pub fn contains(&self, handle: &ViewportHandle) -> bool {
        self.owners.iter().any(|(owner, _)| owner == handle)
    }

    pub fn clear(&mut self) {
        self.set_phase(ViewportMotion::Idle);
        self.owners.clear();
    }

    pub fn reconcile(&mut self, layout: &LayoutNode, device: PanDevice) {
        fn live(
            item: &LayoutNode,
            handle: &ViewportHandle,
            axes: [bool; 2],
            device: PanDevice,
        ) -> bool {
            if !item.visible || !item.enabled {
                return false;
            }
            if item.viewport_handle.as_ref() == Some(handle) {
                let bounds = item.clip_rect(item.bounds);
                bounds.width > 0.0
                    && bounds.height > 0.0
                    && [Orientation::Horizontal, Orientation::Vertical]
                        .into_iter()
                        .zip(axes)
                        .any(|(axis, moved)| {
                            moved
                                && item.accepts_pan(axis, device)
                                && item
                                    .scroll_on(axis)
                                    .is_some_and(|scroll| scroll.max_offset > 0.0)
                        })
            } else {
                item.children
                    .iter()
                    .any(|child| live(child, handle, axes, device))
            }
        }
        self.owners.retain(|(handle, axes)| {
            let keep = live(layout, handle, *axes, device);
            if !keep {
                handle.publish_motion(ViewportMotion::Idle);
            }
            keep
        });
    }
}

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
    pub(crate) fn scroll_viewport(&self, axis: Orientation) -> Rect {
        self.table
            .as_deref()
            .map_or(self.bounds, |table| match axis {
                Orientation::Horizontal => table.scrollable_body(),
                Orientation::Vertical => table.rows_area(),
            })
    }

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
        scene
            .at_path(&self.path)
            .filter(|node| node.viewport_handle.as_ref() == Some(&self.target))
            .is_some_and(|node| {
                component.dispatch_at(
                    scene,
                    &self.path,
                    if node.kind == "Flickable" {
                        "flicked"
                    } else {
                        "scrolled"
                    },
                )
            })
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
                for axis in [Orientation::Horizontal, Orientation::Vertical] {
                    let rect = item.scroll_viewport(axis);
                    let page = match axis {
                        Orientation::Horizontal => rect.width,
                        Orientation::Vertical => rect.height,
                    };
                    let scroll = item.scroll_on(axis).unwrap_or(ScrollLayout {
                        offset: 0.0,
                        max_offset: 0.0,
                    });
                    handle.publish_scroll_range(axis, scroll.offset, page, scroll.max_offset);
                }
                viewports.push(handle.clone());
            }
            if item.visible
                && (node.geometry_handle.is_some() || node.instance_geometry_handle.is_some())
            {
                let rect = item.bounds;
                let window = item.window_bounds(rect);
                let available = if xrack_runtime::is_pane(node.kind) {
                    let inner = crate::linear::Padding::read(node).inner_rect(rect);
                    [inner.width, inner.height]
                } else if node.application_window {
                    item.content_bounds
                        .as_deref()
                        .map_or([0.0; 2], |rect| [rect.width, rect.height])
                } else {
                    [0.0; 2]
                };
                for handle in node
                    .geometry_handle
                    .iter()
                    .chain(node.instance_geometry_handle.iter())
                {
                    handle.publish_with_content(
                        [
                            rect.x - parent.x,
                            rect.y - parent.y,
                            rect.width,
                            rect.height,
                        ],
                        [window.x, window.y, window.width, window.height],
                        available,
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
    for axis in [Orientation::Horizontal, Orientation::Vertical] {
        handle.publish_scroll_range(axis, 0.0, 0.0, 0.0);
    }
}

fn viewport(item: &LayoutNode) -> Rect {
    item.table.as_ref().map_or(item.bounds, |table| table.body)
}

fn find_path(node: &Node, handle: &ViewportHandle, path: &mut Vec<usize>) -> bool {
    if node.viewport_handle.as_ref() == Some(handle) {
        return true;
    }
    for (index, child) in node.materialized_children() {
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
                ScrollAction::CancelFlick => {
                    changed |= self.cancel_viewport_flick(&request.target);
                }
                ScrollAction::ReturnToBounds => {
                    changed |= self.return_viewport_to_bounds(&request.target);
                }
                ScrollAction::To { x, y } => {
                    for (axis, offset) in [(Orientation::Horizontal, x), (Orientation::Vertical, y)]
                    {
                        changed |= self.set_viewport_offset(&path, item, axis, offset.0);
                    }
                }
                ScrollAction::ToIndex { index, alignment } => {
                    if let Ok(index) = usize::try_from(index) {
                        changed |= self.position_row(&path, node, item, index, alignment);
                    }
                }
                ScrollAction::ToRowKey {
                    key,
                    source_index,
                    alignment,
                } => {
                    if let Some(index) = node.displayed_row_for_key(key, source_index) {
                        changed |= self.position_row(&path, node, item, index, alignment);
                    }
                }
                ScrollAction::ToColumn { column, alignment } => {
                    changed |= self.position_column(&path, item, column, alignment);
                }
                ScrollAction::ToCell {
                    row,
                    column,
                    horizontal,
                    vertical,
                } => {
                    if let Ok(row) = usize::try_from(row) {
                        changed |= self.position_cell(
                            &path,
                            node,
                            item,
                            (row, column),
                            (horizontal, vertical),
                        );
                    }
                }
                ScrollAction::ToCellAtRowKey {
                    key,
                    source_index,
                    column,
                    horizontal,
                    vertical,
                } => {
                    if let Some(row) = node.displayed_row_for_key(key, source_index) {
                        changed |= self.position_cell(
                            &path,
                            node,
                            item,
                            (row, column),
                            (horizontal, vertical),
                        );
                    }
                }
            }
        }
        changed
    }

    fn position_cell(
        &mut self,
        path: &[usize],
        node: &Node,
        item: &LayoutNode,
        (row, column): (usize, i64),
        (horizontal, vertical): (ScrollAlignment, ScrollAlignment),
    ) -> bool {
        // Geometry comes from the fresh scene, including column visibility and resizing.
        if !node
            .list
            .zip(item.table.as_ref())
            .is_some_and(|(list, table)| {
                row < list.row_count
                    && usize::try_from(column).is_ok_and(|column| table.is_column_visible(column))
            })
        {
            return false;
        }
        let x = self.position_column(path, item, column, horizontal);
        let y = self.position_row(path, node, item, row, vertical);
        x || y
    }

    fn position_row(
        &mut self,
        path: &[usize],
        node: &Node,
        item: &LayoutNode,
        index: usize,
        alignment: ScrollAlignment,
    ) -> bool {
        if !node.list.is_some_and(|list| index < list.row_count) {
            return false;
        }
        let cells = ViewCells::new(node, viewport(item).width);
        self.position_axis(
            path,
            item,
            Orientation::Vertical,
            cells.top(index) as f32,
            cells.height(index),
            alignment,
        )
    }

    fn position_column(
        &mut self,
        path: &[usize],
        item: &LayoutNode,
        column: i64,
        alignment: ScrollAlignment,
    ) -> bool {
        let Some((column, table)) = usize::try_from(column).ok().zip(item.table.as_ref()) else {
            return false;
        };
        let Some(bounds) = table
            .columns
            .get(column)
            .filter(|bounds| bounds.width > 0.0 && !table.is_frozen(column))
        else {
            return false;
        };
        let left = bounds.x + table.horizontal.offset - table.scrollable_body().x;
        self.position_axis(
            path,
            item,
            Orientation::Horizontal,
            left,
            bounds.width,
            alignment,
        )
    }

    fn position_axis(
        &mut self,
        path: &[usize],
        item: &LayoutNode,
        axis: Orientation,
        start: f32,
        length: f32,
        alignment: ScrollAlignment,
    ) -> bool {
        let Some(scroll) = item.scroll_on(axis) else {
            return false;
        };
        let offsets = match axis {
            Orientation::Horizontal => self.horizontal_scroll_offsets(),
            Orientation::Vertical => self.scroll_offsets(),
        };
        // Earlier commands in this delivery may have moved the same axis already.
        let current = item
            .bound_offset(axis)
            .or_else(|| offsets.get(path).copied())
            .unwrap_or(scroll.offset);
        let viewport = item.scroll_viewport(axis);
        let page = match axis {
            Orientation::Horizontal => viewport.width,
            Orientation::Vertical => viewport.height,
        };
        if page <= 0.0 {
            return false;
        }
        let offset = position(
            ScrollLayout::new(current, scroll.max_offset),
            start,
            length,
            page,
            alignment,
        );
        self.set_viewport_offset(path, item, axis, f64::from(offset))
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
