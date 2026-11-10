use crate::Rect;
use std::{ops::Range, sync::Arc};
use xrack_runtime::{ListMetrics, Node, RowHeights};

/// Geometry shared by virtualization, selection reveal, and keyboard navigation.
#[derive(Clone, Debug)]
pub(crate) struct ViewCells {
    pub(crate) columns: usize,
    height: f32,
    width: f32,
    spacing: f32,
    row_heights: Option<Arc<RowHeights>>,
}

impl ViewCells {
    pub(crate) fn new(node: &Node, viewport_width: f32) -> Self {
        if node.kind != "GridView" {
            return Self {
                columns: 1,
                height: ListMetrics::row_height(node),
                width: viewport_width,
                spacing: 0.0,
                row_heights: node.row_heights.clone(),
            };
        }
        let width = node
            .length("cell-width")
            .unwrap_or(144.0)
            .clamp(16.0, 1024.0);
        let height = node
            .length("cell-height")
            .unwrap_or(96.0)
            .clamp(12.0, 512.0);
        let spacing = node.length("spacing").unwrap_or(0.0).clamp(0.0, 128.0);
        let columns = ((viewport_width + spacing) / (width + spacing)).floor() as usize;
        Self {
            columns: columns.max(1),
            height,
            width: width.min(viewport_width),
            spacing,
            row_heights: None,
        }
    }

    fn pitch(&self) -> f64 {
        f64::from(self.height) + f64::from(self.spacing)
    }

    pub(crate) fn top(&self, index: usize) -> f64 {
        self.row_heights.as_ref().map_or_else(
            || (index / self.columns) as f64 * self.pitch(),
            |heights| heights.top(index),
        )
    }

    pub(crate) fn height(&self, index: usize) -> f32 {
        self.row_heights
            .as_ref()
            .map_or(self.height, |heights| heights.height(index))
    }

    pub(crate) fn extent(&self, count: usize) -> f32 {
        count.checked_sub(1).map_or(0.0, |last| {
            (self.top(last) + f64::from(self.height(last))) as f32
        })
    }

    pub(crate) fn bounds(&self, index: usize, viewport: Rect, offset: f32) -> Rect {
        let x = (index % self.columns) as f32 * (self.width + self.spacing);
        let y = (self.top(index) - f64::from(offset)) as f32;
        Rect::new(
            viewport.x + x,
            viewport.y + y,
            self.width,
            self.height(index),
        )
    }

    pub(crate) fn visible(&self, top: f64, bottom: f64, count: usize) -> Range<usize> {
        if self.row_heights.is_some() {
            let first = self.row_at(top, count);
            let end = boundary(count, |index| self.top(index) < bottom);
            return first..end;
        }
        let first = ((top + f64::from(self.spacing)) / self.pitch()).floor() as usize;
        let end = (bottom / self.pitch()).ceil() as usize;
        first.saturating_mul(self.columns).min(count)..end.saturating_mul(self.columns).min(count)
    }

    fn row_at(&self, y: f64, count: usize) -> usize {
        boundary(count, |index| {
            self.top(index) + f64::from(self.height(index)) <= y
        })
    }

    pub(crate) fn page(&self, row: usize, viewport: f32, count: usize, forward: bool) -> usize {
        if self.row_heights.is_some() {
            let height = f64::from(viewport);
            let y = self.top(row) + if forward { height } else { -height };
            let next = self.row_at(y, count);
            return if forward {
                next.max(row.saturating_add(1)).min(count - 1)
            } else {
                next.min(row.saturating_sub(1))
            };
        }
        let step = ((f64::from(viewport) / self.pitch()).floor() as usize)
            .max(1)
            .saturating_mul(self.columns);
        if forward {
            row.saturating_add(step).min(count - 1)
        } else {
            row.saturating_sub(step)
        }
    }
}

fn boundary(count: usize, before: impl Fn(usize) -> bool) -> usize {
    let mut first = 0;
    let mut end = count;
    while first < end {
        let middle = first + (end - first) / 2;
        if before(middle) {
            first = middle + 1;
        } else {
            end = middle;
        }
    }
    first
}
