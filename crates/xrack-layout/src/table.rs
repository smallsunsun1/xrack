use crate::{
    LayoutNode, Point, Rect, ScrollLayout, TransformSpace,
    linear::{AxisInfo, fit_axis},
    place_in_direction,
};
use std::sync::Arc;
use xrack_runtime::{
    LayoutDirection, Node, TableColumnOrder, TableColumnWindow, resizable_table_column,
};

/// Shared by the row layout, header painting, and header hit testing.
#[derive(Clone, Debug, PartialEq)]
pub struct TableGeometry {
    pub header: Rect,
    pub body: Rect,
    pub row_header: Rect,
    pub columns: Vec<Rect>,
    pub horizontal: ScrollLayout,
    direction: LayoutDirection,
    order: TableColumnOrder,
    hidden_positions: Vec<usize>,
    frozen: Vec<bool>,
    frozen_count: usize,
    frozen_end: usize,
    frozen_width: f32,
}

const MAX_COLUMN_WIDTH: f32 = 1_000_000.0;
const GRIP_RADIUS: f32 = 5.0;
const MIN_SCROLLABLE_WIDTH: f32 = 32.0;

pub(crate) struct PaneClip {
    pub bounds: Rect,
    pub space: Option<Arc<TransformSpace>>,
}

pub(crate) fn column_limits(column: &Node, viewport_width: f32) -> (f32, f32) {
    let default_min = column
        .relative_length("width", viewport_width)
        .map_or(24.0, |width| width.clamp(0.0, 24.0));
    let min = column
        .relative_length("min-width", viewport_width)
        .unwrap_or(default_min)
        .max(0.0);
    let max = column
        .relative_length("max-width", viewport_width)
        .unwrap_or(MAX_COLUMN_WIDTH)
        .max(min);
    (min, max)
}

pub(crate) fn list_viewport(node: &Node, bounds: Rect) -> Rect {
    let header = if node.table_header.is_some() {
        node.length("header-height")
            .unwrap_or(34.0)
            .clamp(16.0, 128.0)
            .min(bounds.height)
    } else {
        0.0
    };
    Rect::new(
        bounds.x,
        bounds.y + header,
        bounds.width,
        bounds.height - header,
    )
}

impl TableGeometry {
    pub fn new(node: &Node, bounds: Rect) -> Option<Self> {
        Self::with_offset(
            node,
            bounds,
            bounds,
            0.0,
            node.layout_direction().unwrap_or_default(),
        )
    }

    pub(crate) fn with_offset(
        node: &Node,
        bounds: Rect,
        clip: Rect,
        requested: f32,
        direction: LayoutDirection,
    ) -> Option<Self> {
        let header = node.table_header.as_ref()?;
        let rows = list_viewport(node, bounds);
        let gutter = node
            .relative_length("row-header-width", bounds.width)
            .filter(|width| width.is_finite())
            .unwrap_or_default()
            .clamp(0.0, (bounds.width - MIN_SCROLLABLE_WIDTH).max(0.0));
        let body = place_in_direction(
            Rect::new(rows.x + gutter, rows.y, rows.width - gutter, rows.height),
            rows,
            direction,
        );
        let row_header = place_in_direction(
            Rect::new(rows.x, rows.y, gutter, rows.height),
            rows,
            direction,
        );
        let header_bounds = Rect::new(body.x, bounds.y, body.width, body.y - bounds.y);
        let tracks = header.columns.iter().map(|column| {
            if !column.is_visible() {
                return AxisInfo::fixed(0.0);
            }
            let (min, max) = column_limits(column, body.width);
            if let Some(width) = column.relative_length("width", body.width) {
                return AxisInfo::fixed(width.clamp(min, max));
            }
            let preferred = column
                .relative_length("preferred-width", body.width)
                .unwrap_or(min)
                .clamp(min, max);
            let stretch = column.float("horizontal-stretch").unwrap_or(1.0).max(0.0);
            AxisInfo {
                min,
                preferred,
                max: if stretch == 0.0 { preferred } else { max },
                stretch,
            }
        });
        let mut widths = fit_axis(tracks, body.width);
        for (index, (width, override_width)) in
            widths.iter_mut().zip(&header.column_widths).enumerate()
        {
            if let Some(override_width) = override_width
                && header.columns[index].is_visible()
            {
                let (min, max) = column_limits(&header.columns[index], body.width);
                *width = (*override_width as f32).clamp(min, max);
            }
        }
        let total_width = widths.iter().sum::<f32>();
        let horizontal = ScrollLayout::new(requested, total_width - body.width);
        let order = node.table_column_order.clone();
        let requested_frozen =
            usize::try_from(node.int("frozen-columns").unwrap_or(0)).unwrap_or(0);
        let visible = place_in_direction(body.intersect(clip), body, direction);
        let visible_right = (visible.x + visible.width - body.x).clamp(0.0, body.width);
        let limit = if total_width > visible_right {
            (visible_right - MIN_SCROLLABLE_WIDTH).max(0.0)
        } else {
            visible_right
        };
        let mut frozen_width = 0.0;
        let mut frozen_count = 0;
        let mut frozen_end = 0;
        let mut frozen = Vec::new();
        for (visual, column) in order
            .iter(widths.len())
            .enumerate()
            .filter(|&(_, column)| widths[column] > 0.0)
            .take(requested_frozen)
        {
            let end = frozen_width + widths[column];
            if end > limit {
                break;
            }
            if frozen.is_empty() {
                frozen.resize(widths.len(), false);
            }
            frozen[column] = true;
            frozen_width = end;
            frozen_count += 1;
            frozen_end = visual + 1;
        }
        let mut x = body.x;
        let mut columns = vec![Rect::default(); widths.len()];
        let mut hidden_positions = Vec::new();
        for (visual, index) in order.iter(widths.len()).enumerate() {
            if visual == frozen_end {
                x -= horizontal.offset;
            }
            let width = widths[index];
            if width == 0.0 {
                hidden_positions.push(visual);
            }
            columns[index] = place_in_direction(
                Rect::new(x, bounds.y, width, header_bounds.height),
                body,
                direction,
            );
            x += width;
        }
        Some(Self {
            header: header_bounds,
            body,
            row_header,
            columns,
            horizontal,
            direction,
            order,
            hidden_positions,
            frozen,
            frozen_count,
            frozen_end,
            frozen_width,
        })
    }

    pub(crate) fn rows_area(&self) -> Rect {
        Rect::new(
            self.row_header.x.min(self.body.x),
            self.body.y,
            self.row_header.width + self.body.width,
            self.body.height,
        )
    }

    pub(crate) fn column_window(&self, clip: Rect) -> TableColumnWindow {
        let clip = self.body.intersect(clip);
        TableColumnWindow::new(self.columns.iter().enumerate().map(|(index, column)| {
            let cell = Rect::new(column.x, self.body.y, column.width, self.body.height);
            let visible = cell.intersect(self.column_clip(index, clip));
            visible.width > 0.0 && visible.height > 0.0
        }))
    }

    pub fn row_header_cell(&self, row: Rect) -> Rect {
        Rect::new(self.row_header.x, row.y, self.row_header.width, row.height)
    }

    pub fn visual_columns(&self) -> impl DoubleEndedIterator<Item = usize> + ExactSizeIterator {
        self.order.iter(self.columns.len())
    }

    pub fn visual_index(&self, column: usize) -> Option<usize> {
        self.order.visual_index(self.columns.len(), column)
    }

    pub fn is_column_visible(&self, column: usize) -> bool {
        self.columns
            .get(column)
            .is_some_and(|bounds| bounds.width > 0.0)
    }

    pub fn visible_columns(&self) -> impl DoubleEndedIterator<Item = usize> {
        self.visual_columns()
            .filter(|&column| self.is_column_visible(column))
    }

    pub fn visible_column_count(&self) -> usize {
        self.columns.len() - self.hidden_positions.len()
    }

    pub fn visible_column_index(&self, column: usize) -> Option<usize> {
        if !self.is_column_visible(column) {
            return None;
        }
        let visual = self.visual_index(column)?;
        Some(
            visual
                - self
                    .hidden_positions
                    .partition_point(|&hidden| hidden < visual),
        )
    }

    pub fn is_frozen(&self, column: usize) -> bool {
        self.frozen.get(column).copied().unwrap_or(false)
    }

    pub fn mirrored(&self) -> bool {
        self.direction == LayoutDirection::RightToLeft
    }

    pub(crate) fn logical_delta(&self, delta: f32) -> f32 {
        if self.mirrored() { -delta } else { delta }
    }

    fn mirror_x(&self, x: f32) -> f32 {
        if self.mirrored() {
            self.body.x + self.body.width - (x - self.body.x)
        } else {
            x
        }
    }

    fn mirror_rect(&self, rect: Rect) -> Rect {
        place_in_direction(rect, self.body, self.direction)
    }

    /// Position row or header content from the logical leading edge of its cell.
    #[doc(hidden)]
    pub fn content_bounds(&self, cell: Rect, start: f32, width: f32) -> Rect {
        place_in_direction(
            Rect::new(cell.x + start, cell.y, width, cell.height),
            cell,
            self.direction,
        )
    }

    fn scrollable_band(&self, band: Rect) -> Rect {
        let width = if self.frozen_count == self.visible_column_count() {
            0.0
        } else {
            (band.width - self.frozen_width).max(0.0)
        };
        self.content_bounds(band, self.frozen_width, width)
    }

    pub fn scrollable_header(&self) -> Rect {
        self.scrollable_band(self.header)
    }

    pub fn scrollable_body(&self) -> Rect {
        self.scrollable_band(self.body)
    }

    pub(crate) fn frozen_body(&self) -> Rect {
        self.content_bounds(self.body, 0.0, self.frozen_width)
    }

    /// Clip a logical column to the pane it currently belongs to.
    pub fn column_clip(&self, column: usize, band: Rect) -> Rect {
        if self.frozen_width == 0.0 {
            return band;
        }
        let pane = if self.is_frozen(column) {
            self.frozen_body()
        } else {
            self.scrollable_body()
        };
        Rect::new(pane.x, band.y, pane.width, band.height).intersect(band)
    }

    pub fn frozen_boundary(&self) -> Option<f32> {
        (self.frozen_width > 0.0 && self.frozen_count < self.visible_column_count())
            .then(|| self.mirror_x(self.header.x + self.frozen_width))
    }

    pub(crate) fn column_scroll_extent(&self, column: usize) -> Option<(f32, f32)> {
        let bounds = self
            .columns
            .get(column)
            .filter(|bounds| bounds.width > 0.0 && !self.is_frozen(column))?;
        let viewport = self.mirror_rect(self.scrollable_body());
        let bounds = self.mirror_rect(*bounds);
        Some((bounds.x - viewport.x + self.horizontal.offset, bounds.width))
    }

    pub(crate) fn reveal_column(&self, column: usize) -> f32 {
        let Some((start, width)) = self.column_scroll_extent(column) else {
            return self.horizontal.offset;
        };
        let available = self.scrollable_body().width;
        self.horizontal
            .revealing(start, width.min(available), available)
            .offset
    }

    pub(crate) fn column_drop(&self, from: usize, point: Point) -> Option<(usize, f32)> {
        if !self.header.contains(point) {
            return None;
        }
        let x = self.mirror_x(point.x);
        let frozen = self.frozen_width > 0.0 && x < self.header.x + self.frozen_width;
        let (start, end, pane) = if frozen {
            (
                0,
                self.frozen_end,
                Rect::new(
                    self.header.x,
                    self.header.y,
                    self.frozen_width,
                    self.header.height,
                ),
            )
        } else {
            (
                self.frozen_end,
                self.columns.len(),
                self.mirror_rect(self.scrollable_header()),
            )
        };
        let mut insertion = None;
        for (visual, column) in self
            .visual_columns()
            .enumerate()
            .skip(start)
            .take(end - start)
        {
            if !self.is_column_visible(column) {
                continue;
            }
            let bounds = self.mirror_rect(self.columns[column]);
            let visible = if self.frozen_width == 0.0 {
                bounds
            } else {
                bounds.intersect(pane)
            };
            if bounds.x >= pane.x + pane.width
                || visible.width > 0.0 && x < visible.x + visible.width * 0.5
            {
                insertion = Some((visual, bounds.x));
                break;
            }
            insertion = Some((visual + 1, bounds.x + bounds.width));
        }
        let (slot, marker) = insertion?;
        let to = slot - usize::from(slot > from);
        if from == to {
            return None;
        }
        Some((to, self.mirror_x(marker.clamp(pane.x, pane.x + pane.width))))
    }

    pub(crate) fn column_at(&self, point: Point) -> Option<usize> {
        self.visible_columns().find(|&column| {
            self.columns[column]
                .intersect(self.column_clip(column, self.header))
                .contains(point)
        })
    }

    pub(crate) fn body_column_at(&self, point: Point) -> Option<usize> {
        if !self.body.contains(point) {
            return None;
        }
        self.column_at(Point {
            x: point.x,
            y: self.header.y,
        })
    }

    pub(crate) fn resize_at(&self, node: &Node, clip: Rect, point: Point) -> Option<usize> {
        let visible = clip.intersect(self.header);
        if !visible.contains(point) || !node.boolean("resizable-columns", false) {
            return None;
        }
        self.columns
            .iter()
            .enumerate()
            .filter_map(|(index, bounds)| {
                let column = resizable_table_column(node, index)?;
                let (min, max) = column_limits(column, self.header.width);
                let bounds = self.mirror_rect(*bounds);
                let edge = bounds.x + bounds.width;
                let distance = (self.mirror_x(point.x) - edge).abs();
                let pane = self.mirror_rect(self.column_clip(index, visible));
                (max > min
                    && bounds.width > 0.0
                    && edge > pane.x
                    && edge <= pane.x + pane.width
                    && distance <= GRIP_RADIUS)
                    .then_some((index, distance))
            })
            .min_by(|(_, a), (_, b)| a.total_cmp(b))
            .map(|(index, _)| index)
    }

    pub(crate) fn row_resize_at(
        &self,
        node: &Node,
        layout: &LayoutNode,
        point: Point,
    ) -> Option<usize> {
        let visible = self.rows_area().intersect(layout.clip);
        if !visible.contains(point) || !node.boolean("resizable-rows", false) {
            return None;
        }
        node.children
            .iter()
            .zip(&layout.children)
            .enumerate()
            .filter_map(|(slot, (row, item))| {
                row.row_key?;
                let end = item.bounds.y + item.bounds.height;
                let distance = (end - point.y).abs();
                (item.visible
                    && item.bounds.height > 0.0
                    && end > visible.y
                    && end <= visible.y + visible.height
                    && distance <= GRIP_RADIUS)
                    .then_some((slot, distance))
            })
            .min_by(|(_, a), (_, b)| a.total_cmp(b))
            .map(|(slot, _)| slot)
    }
}
