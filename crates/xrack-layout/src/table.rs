use crate::{
    LayoutNode, Point, Rect, ScrollLayout, TransformSpace,
    linear::{AxisInfo, fit_axis},
};
use std::sync::Arc;
use xrack_runtime::{Node, TableColumnOrder, resizable_table_column};

/// Shared by the row layout, header painting, and header hit testing.
#[derive(Clone, Debug, PartialEq)]
pub struct TableGeometry {
    pub header: Rect,
    pub body: Rect,
    pub columns: Vec<Rect>,
    pub horizontal: ScrollLayout,
    order: TableColumnOrder,
    frozen: Vec<bool>,
    frozen_count: usize,
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
        Self::with_offset(node, bounds, bounds, 0.0)
    }

    pub(crate) fn with_offset(
        node: &Node,
        bounds: Rect,
        clip: Rect,
        requested: f32,
    ) -> Option<Self> {
        let header = node.table_header.as_ref()?;
        let body = list_viewport(node, bounds);
        let header_bounds = Rect::new(bounds.x, bounds.y, bounds.width, body.y - bounds.y);
        let tracks = header.columns.iter().map(|column| {
            let (min, max) = column_limits(column, bounds.width);
            if let Some(width) = column.relative_length("width", bounds.width) {
                return AxisInfo::fixed(width.clamp(min, max));
            }
            let preferred = column
                .relative_length("preferred-width", bounds.width)
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
        let mut widths = fit_axis(tracks, bounds.width);
        for (index, (width, override_width)) in
            widths.iter_mut().zip(&header.column_widths).enumerate()
        {
            if let Some(override_width) = override_width {
                let (min, max) = column_limits(&header.columns[index], bounds.width);
                *width = (*override_width as f32).clamp(min, max);
            }
        }
        let total_width = widths.iter().sum::<f32>();
        let horizontal = ScrollLayout::new(requested, total_width - bounds.width);
        let order = node.table_column_order.clone();
        let requested_frozen = usize::try_from(node.int("frozen-columns").unwrap_or(0))
            .unwrap_or(0)
            .min(widths.len());
        let visible = bounds.intersect(clip);
        let visible_right = (visible.x + visible.width - bounds.x).clamp(0.0, bounds.width);
        let limit = if total_width > visible_right {
            (visible_right - MIN_SCROLLABLE_WIDTH).max(0.0)
        } else {
            visible_right
        };
        let mut frozen_width = 0.0;
        let mut frozen_count = 0;
        let mut frozen = Vec::new();
        for column in order.iter(widths.len()).take(requested_frozen) {
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
        }
        let mut x = bounds.x;
        let mut columns = vec![Rect::default(); widths.len()];
        for (visual, index) in order.iter(widths.len()).enumerate() {
            if visual == frozen_count {
                x -= horizontal.offset;
            }
            let width = widths[index];
            columns[index] = Rect::new(x, bounds.y, width, header_bounds.height);
            x += width;
        }
        Some(Self {
            header: header_bounds,
            body,
            columns,
            horizontal,
            order,
            frozen,
            frozen_count,
            frozen_width,
        })
    }

    pub fn visual_columns(&self) -> impl DoubleEndedIterator<Item = usize> + ExactSizeIterator {
        self.order.iter(self.columns.len())
    }

    pub fn visual_index(&self, column: usize) -> Option<usize> {
        self.order.visual_index(self.columns.len(), column)
    }

    pub fn is_frozen(&self, column: usize) -> bool {
        self.frozen.get(column).copied().unwrap_or(false)
    }

    fn scrollable_band(&self, band: Rect) -> Rect {
        let width = if self.frozen_count == self.columns.len() {
            0.0
        } else {
            (band.width - self.frozen_width).max(0.0)
        };
        Rect::new(band.x + self.frozen_width, band.y, width, band.height)
    }

    pub fn scrollable_header(&self) -> Rect {
        self.scrollable_band(self.header)
    }

    pub fn scrollable_body(&self) -> Rect {
        self.scrollable_band(self.body)
    }

    pub(crate) fn frozen_body(&self) -> Rect {
        Rect::new(
            self.body.x,
            self.body.y,
            self.frozen_width,
            self.body.height,
        )
    }

    /// Clip a logical column to the pane it currently belongs to.
    pub fn column_clip(&self, column: usize, band: Rect) -> Rect {
        if self.frozen_width == 0.0 {
            return band;
        }
        let boundary = self.header.x + self.frozen_width;
        if self.is_frozen(column) {
            let right = (band.x + band.width).min(boundary);
            Rect::new(band.x, band.y, (right - band.x).max(0.0), band.height)
        } else {
            let left = band.x.max(boundary);
            Rect::new(
                left,
                band.y,
                (band.x + band.width - left).max(0.0),
                band.height,
            )
        }
    }

    pub fn frozen_boundary(&self) -> Option<f32> {
        (self.frozen_width > 0.0 && self.frozen_count < self.columns.len())
            .then_some(self.header.x + self.frozen_width)
    }

    pub(crate) fn reveal_column(&self, column: usize) -> f32 {
        if self.is_frozen(column) {
            return self.horizontal.offset;
        }
        let viewport = self.scrollable_body();
        let bounds = self.columns[column];
        let left = bounds.x - viewport.x + self.horizontal.offset;
        self.horizontal
            .revealing(left, bounds.width.min(viewport.width), viewport.width)
            .offset
    }

    pub(crate) fn column_drop(&self, from: usize, point: Point) -> Option<(usize, f32)> {
        if !self.header.contains(point) {
            return None;
        }
        let frozen = self.frozen_width > 0.0 && point.x < self.header.x + self.frozen_width;
        let (start, end, pane) = if frozen {
            (
                0,
                self.frozen_count,
                Rect::new(
                    self.header.x,
                    self.header.y,
                    self.frozen_width,
                    self.header.height,
                ),
            )
        } else {
            (
                self.frozen_count,
                self.columns.len(),
                self.scrollable_header(),
            )
        };
        let slot = self
            .visual_columns()
            .enumerate()
            .skip(start)
            .take(end - start)
            .find_map(|(visual, column)| {
                let bounds = self.columns[column];
                let visible = if self.frozen_width == 0.0 {
                    bounds
                } else {
                    bounds.intersect(pane)
                };
                (bounds.x >= pane.x + pane.width
                    || visible.width > 0.0 && point.x < visible.x + visible.width * 0.5)
                    .then_some(visual)
            })
            .unwrap_or(end);
        let to = slot - usize::from(slot > from);
        if from == to {
            return None;
        }
        let marker = if slot < end {
            let column = self.visual_columns().nth(slot)?;
            self.columns[column].x
        } else {
            let bounds = self.columns[self.visual_columns().nth(end.checked_sub(1)?)?];
            bounds.x + bounds.width
        };
        Some((to, marker.clamp(pane.x, pane.x + pane.width)))
    }

    pub(crate) fn column_at(&self, point: Point) -> Option<usize> {
        self.visual_columns().find(|&column| {
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
                let edge = bounds.x + bounds.width;
                let distance = (point.x - edge).abs();
                let pane = self.column_clip(index, visible);
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
        let visible = self.body.intersect(layout.clip);
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
