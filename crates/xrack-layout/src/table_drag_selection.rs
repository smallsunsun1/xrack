use crate::{LayoutNode, Point, Rect, TableGeometry, Target, input::ElementId};
use std::{rc::Rc, time::Instant};
use xrack_runtime::{
    Node, Orientation, RowKey, SelectionGesture, SelectionTarget, TableColumnOrder,
    TableSelectionAction, TableSelectionDrag, TableSelectionDragHandle,
};

#[derive(Clone, Debug)]
pub(crate) struct TableDragSelection {
    pub path: Vec<usize>,
    pub start: Point,
    pub claimed: bool,
    pub touch: bool,
    pub scroll_capture: bool,
    pub gesture: SelectionGesture,
    pub hold_interval: u64,
    drag_handle: Option<TableSelectionDragHandle>,
    presentation: Option<Rc<TableSelectionDrag>>,
    pub scroll_wake: Option<Instant>,
    pub parent_row: Option<RowKey>,
    id: Option<ElementId>,
    anchor: (usize, usize),
    stamp: (u64, u64),
    order: TableColumnOrder,
    mirrored: bool,
    target: SelectionTarget,
    additive: bool,
    adjusted_range: Option<[(usize, usize); 2]>,
}

pub(crate) struct TableSelectionHandle {
    pub bounds: Rect,
    pub opposite: (usize, usize),
}

impl TableDragSelection {
    pub fn focus_target(&self) -> Target {
        Target::from_element_id(self.path.clone(), self.id.clone(), true, self.parent_row)
    }

    pub fn new(
        path: Vec<usize>,
        node: &Node,
        anchor: (usize, usize),
        start: Point,
        additive: bool,
        parent_row: Option<RowKey>,
        mirrored: bool,
    ) -> Option<Self> {
        let view = node.table_selection.as_ref()?;
        if !view.multiple() || !node.boolean("drag-select", false) {
            return None;
        }
        Some(Self {
            path,
            start,
            claimed: false,
            touch: false,
            scroll_capture: false,
            gesture: node.selection_gesture(),
            hold_interval: node.selection_hold_interval(),
            drag_handle: node.table_selection_drag.clone(),
            presentation: None,
            scroll_wake: None,
            parent_row,
            id: ElementId::of(node),
            anchor,
            stamp: view.structure_stamp(),
            order: node.table_column_order.clone(),
            mirrored,
            target: view.target(),
            additive,
            adjusted_range: None,
        })
    }

    pub fn adjust_range(&mut self, range: [(usize, usize); 2]) {
        self.adjusted_range = Some(range);
        self.claimed = true;
    }

    pub fn begin_presentation(&mut self) {
        if self.presentation.is_none() {
            self.presentation = self
                .drag_handle
                .as_ref()
                .map(TableSelectionDragHandle::claim);
        }
    }

    pub fn valid(&self, node: &Node, item: &LayoutNode) -> bool {
        self.claimed
            && node.element_id() == self.id.as_ref().map(ElementId::as_str)
            && node.boolean("drag-select", false)
            && (self.adjusted_range.is_some()
                || (self.gesture == node.selection_gesture()
                    && self.hold_interval == node.selection_hold_interval()))
            && node.table_selection_drag == self.drag_handle
            && node.table_column_order == self.order
            && item.scroll_axis_reversed(Orientation::Horizontal) == self.mirrored
            && item.visible
            && item.enabled
            && (self.target == SelectionTarget::Rows
                || item.table.as_ref().is_some_and(|table| {
                    table.is_column_visible(self.anchor.1)
                        && self.adjusted_range.is_none_or(|range| {
                            range
                                .iter()
                                .all(|&(_, column)| table.is_column_visible(column))
                        })
                }))
            && node.table_selection.as_ref().is_some_and(|view| {
                view.multiple()
                    && view.target() == self.target
                    && view.structure_stamp() == self.stamp
                    && self
                        .adjusted_range
                        .is_none_or(|range| view.drag_range() == Some(range))
            })
    }

    pub fn action(
        &self,
        node: &Node,
        item: &LayoutNode,
        point: Point,
    ) -> Option<TableSelectionAction> {
        if self.adjusted_range.is_some() && point == self.start {
            return None;
        }
        Some(TableSelectionAction::DragRange {
            anchor: self.anchor,
            end: self.endpoint(node, item, point)?,
            additive: self.additive,
        })
    }

    pub fn preview_handles(
        &self,
        node: &Node,
        item: &LayoutNode,
        point: Point,
    ) -> Option<[Option<TableSelectionHandle>; 2]> {
        let end = self.endpoint(node, item, point)?;
        handles_for_range(node, item, [self.anchor, end])
    }

    fn endpoint(&self, node: &Node, item: &LayoutNode, point: Point) -> Option<(usize, usize)> {
        let table = item.table.as_ref()?;
        let body = table.body.intersect(item.clip);
        if body.width <= 0.0 || body.height <= 0.0 {
            return None;
        }
        let point = item.map_from_window(point)?;
        let list = node.list?;
        let (slot, _) = visible_rows(node, item, body).min_by(|(_, a), (_, b)| {
            distance(point.y, a.y, a.height).total_cmp(&distance(point.y, b.y, b.height))
        })?;
        let column = table
            .visual_columns()
            .filter_map(|index| visible_column(table, body, index).map(|rect| (index, rect)))
            .min_by(|(_, a), (_, b)| {
                distance(point.x, a.x, a.width).total_cmp(&distance(point.x, b.x, b.width))
            })?
            .0;
        Some((list.first_row + slot, column))
    }

    pub fn preview(&self, node: &Node, item: &LayoutNode, point: Point) -> Vec<Rect> {
        let (Some(table), Some(list), Some(end)) = (
            item.table.as_ref(),
            node.list,
            self.endpoint(node, item, point),
        ) else {
            return Vec::new();
        };
        let body = table.body.intersect(item.clip);
        let band = if self.target == SelectionTarget::Columns {
            body
        } else {
            let rows = span(self.anchor.0, end.0);
            let mut visible = visible_rows(node, item, body)
                .filter_map(|(slot, rect)| rows.contains(&(list.first_row + slot)).then_some(rect));
            let Some(first) = visible.next() else {
                return Vec::new();
            };
            let bottom = visible
                .next_back()
                .map_or(first.y + first.height, |last| last.y + last.height);
            Rect::new(body.x, first.y, body.width, bottom - first.y)
        };
        if self.target == SelectionTarget::Rows {
            return vec![band];
        }

        let Some(first) = table.visual_index(self.anchor.1) else {
            return Vec::new();
        };
        let Some(last) = table.visual_index(end.1) else {
            return Vec::new();
        };
        let mut panes: [Option<Rect>; 2] = [None, None];
        let start = first.min(last);
        let end = first.max(last);
        for column in table.visual_columns().skip(start).take(end - start + 1) {
            let Some(rect) = visible_column(table, band, column) else {
                continue;
            };
            let pane = &mut panes[usize::from(table.is_frozen(column))];
            *pane = Some(pane.map_or(rect, |previous| {
                let left = previous.x.min(rect.x);
                let right = (previous.x + previous.width).max(rect.x + rect.width);
                Rect::new(left, band.y, right - left, band.height)
            }));
        }
        panes.into_iter().flatten().collect()
    }

    pub fn scroll_delta(&self, item: &LayoutNode, point: Point) -> Point {
        let Some((table, local)) = item.table.as_ref().zip(item.map_from_window(point)) else {
            return Point::default();
        };
        let body = table.body.intersect(item.clip);
        if body.width <= 0.0 || body.height <= 0.0 {
            return Point::default();
        }
        let horizontal = table.scrollable_body().intersect(body);
        let x = if self.target == SelectionTarget::Rows
            || table
                .frozen_boundary()
                .is_some_and(|boundary| table.logical_delta(local.x - boundary) < 0.0)
        {
            0.0
        } else {
            edge_delta(local.x, horizontal.x, horizontal.width)
        };
        let y = if self.target == SelectionTarget::Columns {
            0.0
        } else {
            edge_delta(local.y, body.y, body.height)
        };
        Point {
            x: scrollable_delta(item, Orientation::Horizontal, table.logical_delta(x)),
            y: scrollable_delta(item, Orientation::Vertical, y),
        }
    }
}

pub(crate) fn selection_handles(
    node: &Node,
    item: &LayoutNode,
) -> Option<[Option<TableSelectionHandle>; 2]> {
    handles_for_range(node, item, node.table_selection.as_ref()?.drag_range()?)
}

fn handles_for_range(
    node: &Node,
    item: &LayoutNode,
    [(row_a, col_a), (row_b, col_b)]: [(usize, usize); 2],
) -> Option<[Option<TableSelectionHandle>; 2]> {
    let view = node.table_selection.as_ref()?;
    if !view.multiple() || !node.boolean("drag-select", false) || !item.visible || !item.enabled {
        return None;
    }
    let table = item.table.as_ref()?;
    let (left, right) = if table.visual_index(col_a)? <= table.visual_index(col_b)? {
        (col_a, col_b)
    } else {
        (col_b, col_a)
    };
    let first = (row_a.min(row_b), left);
    let last = (row_a.max(row_b), right);
    let body = table.body.intersect(item.clip);
    let handle = |(row, column): (usize, usize), opposite, leading: bool| {
        let part = node.table_selection_part(usize::from(!leading));
        if part.is_some_and(|part| !part.is_visible() || !part.boolean("enabled", true)) {
            return None;
        }
        let (row_bounds, column_bounds, clip) = (
            if view.target() == SelectionTarget::Columns {
                body
            } else {
                let slot = row.checked_sub(node.list?.first_row)?;
                let row = item.children.get(slot)?;
                row.visible.then_some(row.bounds)?
            },
            if view.target() == SelectionTarget::Rows {
                body
            } else {
                *table.columns.get(column)?
            },
            if view.target() == SelectionTarget::Rows {
                body
            } else {
                table.column_clip(column, body)
            },
        );
        let cell = Rect::new(
            column_bounds.x,
            row_bounds.y,
            column_bounds.width,
            row_bounds.height,
        );
        let left_edge = leading != table.mirrored();
        let x = if left_edge {
            cell.x
        } else {
            cell.x + cell.width
        };
        let y = if leading {
            cell.y
        } else {
            cell.y + cell.height
        };
        if x < clip.x || x > clip.x + clip.width || y < clip.y || y > clip.y + clip.height {
            return None;
        }
        let visible = cell.intersect(clip);
        let max_width = visible.width * 0.5;
        let max_height = visible.height * 0.5;
        let size = 20.0_f32.min(max_width).min(max_height);
        let width = part
            .and_then(|part| part.relative_length("width", visible.width))
            .unwrap_or(size)
            .min(max_width);
        let height = part
            .and_then(|part| part.relative_length("height", visible.height))
            .unwrap_or(size)
            .min(max_height);
        (width > 0.0 && height > 0.0).then_some(TableSelectionHandle {
            bounds: Rect::new(
                if left_edge { x } else { x - width },
                if leading { y } else { y - height },
                width,
                height,
            ),
            opposite,
        })
    };
    Some([handle(first, last, true), handle(last, first, false)])
}

fn span(a: usize, b: usize) -> std::ops::RangeInclusive<usize> {
    a.min(b)..=a.max(b)
}

fn visible_rows<'a>(
    node: &'a Node,
    item: &'a LayoutNode,
    body: Rect,
) -> impl DoubleEndedIterator<Item = (usize, Rect)> + use<'a> {
    node.children
        .iter()
        .zip(&item.children)
        .enumerate()
        .filter_map(move |(slot, (row, item))| {
            let rect = item.bounds.intersect(body);
            (row.kind == "ListRow" && item.visible && rect.height > 0.0).then_some((slot, rect))
        })
}

fn visible_column(table: &TableGeometry, band: Rect, column: usize) -> Option<Rect> {
    let bounds = table.columns[column];
    let rect = Rect::new(bounds.x, band.y, bounds.width, band.height)
        .intersect(table.column_clip(column, band));
    (rect.width > 0.0 && rect.height > 0.0).then_some(rect)
}

fn distance(value: f32, start: f32, length: f32) -> f32 {
    (start - value).max(value - start - length).max(0.0)
}

pub(crate) fn edge_delta(value: f32, start: f32, length: f32) -> f32 {
    let edge = (length * 0.25).min(24.0);
    if edge <= 0.0 {
        return 0.0;
    }
    let low = (start + edge - value).clamp(0.0, edge);
    let high = (value - (start + length - edge)).clamp(0.0, edge);
    (high - low) / edge * 22.0
}

fn scrollable_delta(item: &LayoutNode, orientation: Orientation, delta: f32) -> f32 {
    item.scroll_on(orientation).map_or(0.0, |scroll| {
        (scroll.offset + delta).clamp(0.0, scroll.max_offset) - scroll.offset
    })
}
