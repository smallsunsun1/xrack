use crate::{
    Interaction, LayoutNode, Point, Rect, ScrollBarGeometry, ScrollLayout,
    scroll_gesture::PanDevice, view_cells::ViewCells,
};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
use xrack_runtime::{
    Component, GeometryHandle, Length, Node, Orientation, ScrollAction, ScrollAlignment,
    ScrollRequest, ViewportHandle, ViewportMotion, is_window_overlay,
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
    #[doc(hidden)]
    pub fn scroll_axis_reversed(&self, axis: Orientation) -> bool {
        axis == Orientation::Horizontal
            && (self.horizontal_scroll_reversed
                || self.table.as_ref().is_some_and(|table| table.mirrored()))
    }

    #[doc(hidden)]
    pub fn scroll_viewport(&self, axis: Orientation) -> Rect {
        if let Some(strip) = self.tab_strip {
            return strip;
        }
        self.table.as_deref().map_or_else(
            || {
                self.content_bounds
                    .as_deref()
                    .copied()
                    .unwrap_or(self.bounds)
            },
            |table| match axis {
                Orientation::Horizontal => table.scrollable_body(),
                Orientation::Vertical => table.rows_area(),
            },
        )
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
    event: &'static str,
}

impl ScrollDelivery {
    pub fn apply(&self, component: &mut impl Component, scene: &Node) -> bool {
        scene.at_path(&self.path).is_some_and(|node| {
            node.viewport_handle.as_ref() == Some(&self.target)
                && (self.event != "refresh-requested" || node.can_pull_to_refresh())
                && component.dispatch_at(scene, &self.path, self.event)
        })
    }
}

struct ScrollActivity {
    viewport: Option<ViewportHandle>,
    axis: Orientation,
    offset: f32,
    until: Option<Instant>,
}

#[derive(Default)]
pub struct LayoutObservers {
    present: Vec<ViewportHandle>,
    geometry: BTreeMap<usize, GeometryHandle>,
    scroll_controls: Vec<(Vec<usize>, GeometryHandle)>,
    scroll_activity: BTreeMap<usize, ScrollActivity>,
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
            scroll_controls: &mut Vec<(Vec<usize>, GeometryHandle)>,
            path: &mut Vec<usize>,
        ) {
            if let Some(handle) = &node.range_position_handle {
                handle.publish_direction(item.layout_direction);
            }
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
                if node.kind == "ScrollView" {
                    let mut width = Length::default();
                    let mut height = Length::default();
                    for bar in item.scroll_bars(node) {
                        match bar.orientation {
                            Orientation::Horizontal => height = Length(f64::from(bar.track.height)),
                            Orientation::Vertical => width = Length(f64::from(bar.track.width)),
                        }
                    }
                    handle.publish_scroll_bar_extent(width, height);
                }
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
                if let Some(handle) = &node.geometry_handle
                    && matches!(node.kind, "ScrollBar" | "ScrollIndicator")
                {
                    let (position, size) = item
                        .scroll_control_geometry(node)
                        .map_or((0.0, 0.0), |bar| bar.normalized_thumb());
                    handle.publish_scroll_thumb(position, size);
                    scroll_controls.push((path.clone(), handle.clone()));
                }
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
            for (index, child, layout) in item.children_in_scene_order(node) {
                path.push(index);
                publish(
                    child,
                    layout,
                    item.bounds,
                    viewports,
                    geometry,
                    scroll_controls,
                    path,
                );
                path.pop();
            }
        }

        let mut next = Vec::new();
        let mut geometry = BTreeMap::new();
        self.scroll_controls.clear();
        publish(
            scene,
            layout,
            layout.bounds,
            &mut next,
            &mut geometry,
            &mut self.scroll_controls,
            &mut Vec::new(),
        );
        for handle in self.present.drain(..) {
            if !next.contains(&handle) {
                reset(&handle);
            }
        }
        self.present = next;
        for (id, handle) in std::mem::take(&mut self.geometry) {
            if !geometry.contains_key(&id) {
                handle.clear();
                self.scroll_activity.remove(&id);
            }
        }
        self.geometry = geometry;
    }

    /// Samples input against the same mounted bars used for hit testing.
    pub fn sync_scroll_activity(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        input: &Interaction,
        now: Instant,
    ) {
        for (path, handle) in &self.scroll_controls {
            if !handle.observes_scroll_interaction() {
                continue;
            }
            let id = handle.identity();
            let Some((node, bar)) =
                scene
                    .at_path(path)
                    .zip(layout.at_path(path))
                    .and_then(|(node, item)| {
                        let bar = item.scroll_control_geometry(node)?;
                        (node.geometry_handle.as_ref() == Some(handle)
                            && item.enabled
                            && bar.scrollable())
                        .then_some((node, bar))
                    })
            else {
                self.scroll_activity.remove(&id);
                handle.publish_scroll_interaction(false, false, false);
                continue;
            };
            let viewport = node.scroll_target.clone();
            let axis = bar.orientation;
            let offset = bar.scroll.offset;
            let sample = self
                .scroll_activity
                .entry(id)
                .or_insert_with(|| ScrollActivity {
                    viewport: viewport.clone(),
                    axis,
                    offset,
                    until: None,
                });
            if sample.viewport != viewport || sample.axis != axis {
                *sample = ScrollActivity {
                    viewport,
                    axis,
                    offset,
                    until: None,
                };
            } else if sample.offset != offset {
                sample.offset = offset;
                sample.until = Some(now + Duration::from_millis(180));
            } else if sample.until.is_some_and(|until| now >= until) {
                sample.until = None;
            }
            let (pressed, hovered) = input.scroll_bar_interaction(scene, layout, path, axis);
            handle.publish_scroll_interaction(
                pressed || hovered || sample.until.is_some(),
                pressed,
                hovered,
            );
        }
    }

    pub fn scroll_activity_deadline(&self) -> Option<Instant> {
        self.scroll_activity
            .values()
            .filter_map(|sample| sample.until)
            .min()
    }

    pub fn clear(&mut self) {
        self.scroll_controls.clear();
        self.scroll_activity.clear();
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
    handle.publish_scroll_bar_extent(zero, zero);
    for axis in [Orientation::Horizontal, Orientation::Vertical] {
        handle.publish_scroll_range(axis, 0.0, 0.0, 0.0);
    }
}

fn viewport(item: &LayoutNode) -> Rect {
    item.table.as_ref().map_or_else(
        || item.scroll_viewport(Orientation::Vertical),
        |table| table.body,
    )
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
    /// Queue the same refresh event as a completed pull gesture.
    #[doc(hidden)]
    pub fn request_viewport_refresh(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        viewport: &ViewportHandle,
    ) -> bool {
        let mut path = Vec::new();
        if self.refresh_request.is_some()
            || !find_path(scene, viewport, &mut path)
            || self.refresh_viewport(scene, layout, &path) != Some(viewport)
        {
            return false;
        }
        self.refresh_request = Some(viewport.clone());
        true
    }

    /// Resolve movement after layout clamps offsets and replaces virtual rows.
    pub fn take_scroll_events(&mut self, scene: &Node, layout: &LayoutNode) -> Vec<ScrollDelivery> {
        let mut events: Vec<_> = std::mem::take(&mut self.user_scroll_changes)
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
                    event: if scene
                        .at_path(&path)
                        .is_some_and(|node| node.kind == "Flickable")
                    {
                        "flicked"
                    } else {
                        "scrolled"
                    },
                    path,
                })
            })
            .collect();
        if let Some(target) = self.refresh_request.take() {
            let mut path = Vec::new();
            if find_path(scene, &target, &mut path)
                && self.refresh_viewport(scene, layout, &path) == Some(&target)
            {
                events.push(ScrollDelivery {
                    target,
                    path,
                    event: "refresh-requested",
                });
            }
        }
        events
    }

    pub fn clear_scroll_events(&mut self) {
        self.user_scroll_changes.clear();
        self.refresh_request = None;
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
                ScrollAction::By { dx, dy } => {
                    for (axis, delta) in
                        [(Orientation::Horizontal, dx), (Orientation::Vertical, dy)]
                    {
                        changed |= self.scroll_viewport_by(&path, item, axis, delta.0);
                    }
                }
                ScrollAction::StepBar {
                    axis,
                    step_size,
                    direction,
                } => {
                    if let Some(scroll) = item.scroll_on(axis) {
                        let viewport = item.scroll_viewport(axis);
                        let page = match axis {
                            Orientation::Horizontal => viewport.width,
                            Orientation::Vertical => viewport.height,
                        };
                        let step =
                            ScrollBarGeometry::viewport_step(page, scroll.max_offset, step_size);
                        changed |= self.scroll_viewport_by(
                            &path,
                            item,
                            axis,
                            direction.delta(f64::from(step)),
                        );
                    }
                }
                ScrollAction::ToChild {
                    target,
                    horizontal,
                    vertical,
                } => {
                    let viewport_depth = path.len();
                    if !find_scroll_child(node, target, &mut path) || path.len() <= viewport_depth {
                        continue;
                    }
                    let Some(item) = layout.at_path(&path).filter(|item| item.visible) else {
                        continue;
                    };
                    let mode = RevealMode::Child {
                        viewport_depth,
                        horizontal,
                        vertical,
                    };
                    let plan = plan_scroll(
                        (scene, layout),
                        &path,
                        item.bounds,
                        mode,
                        |path, item, axis, layout_offset| {
                            self.current_viewport_offset(path, item, axis)
                                .unwrap_or(layout_offset)
                        },
                    );
                    if let Some(plan) = plan {
                        changed |= self.apply_reveal_plan(layout, &path, plan);
                    }
                }
                ScrollAction::ToIndex { index, alignment } => {
                    if let Ok(index) = usize::try_from(index) {
                        changed |= self.position_row(&path, node, item, index, alignment);
                    }
                }
                ScrollAction::ToBusinessKey { key, alignment } => {
                    if let Some(index) = node
                        .row_locator
                        .as_ref()
                        .and_then(|locator| locator.business_position(key))
                    {
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
        let view = viewport(item);
        let cells = ViewCells::new(node, view.width);
        self.position_axis(
            path,
            item,
            Orientation::Vertical,
            (cells.top(index) as f32, cells.height(index)),
            cells.leading_clearance(index, view.height),
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
        let Some((start, width)) = table.column_scroll_extent(column) else {
            return false;
        };
        self.position_axis(
            path,
            item,
            Orientation::Horizontal,
            (start, width),
            0.0,
            alignment,
        )
    }

    fn position_axis(
        &mut self,
        path: &[usize],
        item: &LayoutNode,
        axis: Orientation,
        (start, length): (f32, f32),
        leading: f32,
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
            ScrollLayout::new(current + leading, scroll.max_offset + leading),
            start,
            length,
            page - leading,
            alignment,
        );
        self.set_viewport_offset(path, item, axis, f64::from(offset - leading))
    }
}

/// Content viewports can reveal items that already have a stable layout path.
pub(crate) fn reveals_focus(node: &Node, item: &LayoutNode) -> bool {
    can_reveal_focus(node)
        && [Orientation::Horizontal, Orientation::Vertical]
            .into_iter()
            .any(|axis| {
                item.scroll_on(axis)
                    .is_some_and(|axis| axis.max_offset > 0.0)
            })
}

fn can_reveal_focus(node: &Node) -> bool {
    matches!(node.kind, "ScrollView" | "Flickable")
        && node.boolean("auto-reveal-focus", node.kind == "ScrollView")
}

pub(crate) fn plan_keyboard_avoidance(
    scene: &Node,
    layout: &LayoutNode,
    editor: &[usize],
    mut field: Rect,
    keyboard: Rect,
    previous: Option<(&[usize], f32)>,
    require_visible: bool,
) -> Option<(Vec<usize>, f32)> {
    const GAP: f32 = 8.0;
    let previous = previous.and_then(|(path, offset)| {
        let item = layout.at_path(path)?;
        matches!(scene.at_path(path)?.kind, "ScrollView" | "Flickable").then(|| {
            (
                path,
                item.world_transform()
                    .map_vector(Point { x: 0.0, y: offset })
                    .y,
            )
        })
    });
    let applied_shift = |path: &[usize]| {
        previous
            .filter(|(owner, _)| path.len() > owner.len() && path.starts_with(owner))
            .map_or(0.0, |(_, y)| y)
    };
    // Always compare against the original position so moving the keyboard
    // away restores the layout, even if the shifted editor is already visible.
    field.y -= applied_shift(editor);
    let overlap = field.intersect(keyboard);
    if overlap.width <= 0.0 || overlap.height <= 0.0 {
        return None;
    }
    for depth in (0..editor.len()).rev() {
        let path = &editor[..depth];
        let node = scene.at_path(path)?;
        if is_window_overlay(node.kind) {
            break;
        }
        let item = layout.at_path(path)?;
        if !item.visible || !item.enabled || !can_reveal_focus(node) {
            continue;
        }
        let mut bounds = item.visible_window_bounds(item.scroll_viewport(Orientation::Vertical));
        bounds.y -= applied_shift(path);
        let visible = bounds.intersect(field);
        if require_visible && (visible.width <= 0.0 || visible.height <= 0.0) {
            continue;
        }
        let shifts = [
            keyboard.y - GAP - field.y - field.height,
            keyboard.y + keyboard.height + GAP - field.y,
        ];
        let shift = shifts
            .into_iter()
            .filter(|shift| {
                field.y + shift >= bounds.y
                    && field.y + field.height + shift <= bounds.y + bounds.height
            })
            .min_by(|left, right| left.abs().total_cmp(&right.abs()));
        if let Some(shift) = shift
            && let Some(local) = item.map_delta_from_window(Point { x: 0.0, y: shift })
            && local.x.abs() < 0.01
            && local.y.is_finite()
        {
            return Some((path.to_vec(), local.y));
        }
    }
    None
}

pub(crate) struct ScrollPosition {
    depth: usize,
    axis: Orientation,
    offset: f32,
}

#[derive(Clone, Copy)]
enum RevealMode {
    Focus,
    Child {
        viewport_depth: usize,
        horizontal: ScrollAlignment,
        vertical: ScrollAlignment,
    },
}

fn independent_content(node: &Node) -> bool {
    is_window_overlay(node.kind)
        || node.is_control_part()
        || matches!(node.kind, "Menu" | "Tooltip" | "Shortcut")
}

fn find_scroll_child(node: &Node, target: u64, path: &mut Vec<usize>) -> bool {
    if independent_content(node) {
        return false;
    }
    if node
        .geometry_handle
        .iter()
        .chain(node.instance_geometry_handle.iter())
        .any(|handle| handle.matches_scroll_target(target))
    {
        return true;
    }
    for (index, child) in node.materialized_children() {
        path.push(index);
        if find_scroll_child(child, target, path) {
            return true;
        }
        path.pop();
    }
    false
}

pub(crate) fn plan_focus_scroll(
    scene: &Node,
    layout: &LayoutNode,
    path: &[usize],
    rect: Rect,
) -> Option<Vec<ScrollPosition>> {
    plan_scroll(
        (scene, layout),
        path,
        rect,
        RevealMode::Focus,
        |_, _, _, offset| offset,
    )
}

/// Resolve a complete reveal before changing any bound offsets. Each outer
/// viewport receives the clipped rectangle produced by its inner viewport.
fn plan_scroll(
    (scene, layout): (&Node, &LayoutNode),
    path: &[usize],
    mut rect: Rect,
    mode: RevealMode,
    current_offset: impl Fn(&[usize], &LayoutNode, Orientation, f32) -> f32,
) -> Option<Vec<ScrollPosition>> {
    let focus = matches!(mode, RevealMode::Focus);
    let scope = match mode {
        RevealMode::Focus => 0,
        RevealMode::Child { viewport_depth, .. } => viewport_depth,
    };
    let mut ancestors = Vec::with_capacity(path.len() + 1);
    let (mut node, mut item) = (scene, layout);
    ancestors.push((node, item));
    for &index in path {
        node = node.child_at(index)?;
        item = item.child_at(index)?;
        ancestors.push((node, item));
    }
    let mut plan = Vec::new();
    for depth in (scope + 1..ancestors.len()).rev() {
        let (child, item) = ancestors[depth];
        if is_window_overlay(child.kind) {
            let clipped = item.clip_rect(rect);
            return (clipped.width > 0.0 && clipped.height > 0.0).then_some(plan);
        }
        rect = item.local_transform().map_rect(rect);
        let (parent, item) = ancestors[depth - 1];
        if !item.visible || focus && !item.enabled || parent.is_control_part() {
            return None;
        }
        let content_viewport = matches!(parent.kind, "ScrollView" | "Flickable");
        if if focus {
            reveals_focus(parent, item)
        } else {
            content_viewport
        } {
            for axis in [Orientation::Horizontal, Orientation::Vertical] {
                let Some(scroll) = item
                    .scroll_on(axis)
                    .filter(|scroll| scroll.max_offset > 0.0)
                else {
                    continue;
                };
                let viewport = item.scroll_viewport(axis);
                let reversed = item.scroll_axis_reversed(axis);
                let before = current_offset(&path[..depth - 1], item, axis, scroll.offset);
                shift_rect(&mut rect, axis, reversed, before - scroll.offset);
                let (start, length, page) = match axis {
                    Orientation::Horizontal => (
                        if reversed {
                            viewport.x + viewport.width - rect.x - rect.width
                        } else {
                            rect.x - viewport.x
                        },
                        rect.width,
                        viewport.width,
                    ),
                    Orientation::Vertical => (rect.y - viewport.y, rect.height, viewport.height),
                };
                if page <= 0.0 {
                    return None;
                }
                let alignment = match mode {
                    RevealMode::Child {
                        horizontal,
                        vertical,
                        ..
                    } if depth - 1 == scope => match axis {
                        Orientation::Horizontal => horizontal,
                        Orientation::Vertical => vertical,
                    },
                    _ => ScrollAlignment::Contain,
                };
                let offset = position(
                    ScrollLayout::new(before, scroll.max_offset),
                    before + start,
                    length,
                    page,
                    alignment,
                )
                .clamp(0.0, scroll.max_offset);
                if offset != before {
                    plan.push(ScrollPosition {
                        depth: depth - 1,
                        axis,
                        offset,
                    });
                    shift_rect(&mut rect, axis, reversed, offset - before);
                }
            }
        }
        if content_viewport || item.table.is_some() {
            rect = rect.intersect(item.scroll_viewport(Orientation::Vertical));
        } else if (parent.kind == "Rectangle" || xrack_runtime::is_pane(parent.kind))
            && parent.boolean("clip", false)
        {
            rect = crate::RoundedClip::content_box(parent, item.bounds).clip_rect(rect);
        } else if !item.allows_overflow() {
            rect = rect.intersect(item.bounds);
        }
        if rect.width <= 0.0 || rect.height <= 0.0 {
            return None;
        }
    }
    if focus {
        rect = layout.clip_rect(rect);
    }
    (rect.width > 0.0 && rect.height > 0.0).then_some(plan)
}

fn shift_rect(rect: &mut Rect, axis: Orientation, reversed: bool, delta: f32) {
    let displacement = if reversed { delta } else { -delta };
    match axis {
        Orientation::Horizontal => rect.x += displacement,
        Orientation::Vertical => rect.y += displacement,
    }
}

impl Interaction {
    pub(crate) fn reveal_focus_in_viewports(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        path: &[usize],
        rect: Rect,
    ) -> bool {
        let Some(plan) = plan_focus_scroll(scene, layout, path, rect) else {
            return false;
        };
        self.apply_reveal_plan(layout, path, plan)
    }

    fn apply_reveal_plan(
        &mut self,
        layout: &LayoutNode,
        path: &[usize],
        plan: Vec<ScrollPosition>,
    ) -> bool {
        let mut changed = false;
        for request in plan {
            let path = &path[..request.depth];
            if let Some(item) = layout.at_path(path) {
                changed |=
                    self.set_viewport_offset(path, item, request.axis, f64::from(request.offset));
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
