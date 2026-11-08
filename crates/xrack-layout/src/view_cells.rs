use crate::Rect;
use std::ops::Range;
use xrack_runtime::{ListMetrics, Node};

/// Geometry shared by virtualization, selection reveal, and keyboard navigation.
pub(crate) struct ViewCells {
    pub(crate) columns: usize,
    pub(crate) height: f32,
    width: f32,
    spacing: f32,
}

impl ViewCells {
    pub(crate) fn new(node: &Node, viewport_width: f32) -> Self {
        if node.kind != "GridView" {
            return Self {
                columns: 1,
                height: ListMetrics::row_height(node),
                width: viewport_width,
                spacing: 0.0,
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
        }
    }

    pub(crate) fn pitch(&self) -> f64 {
        f64::from(self.height) + f64::from(self.spacing)
    }

    pub(crate) fn top(&self, index: usize) -> f64 {
        (index / self.columns) as f64 * self.pitch()
    }

    pub(crate) fn extent(&self, count: usize) -> f32 {
        count
            .checked_sub(1)
            .map_or(0.0, |last| (self.top(last) + f64::from(self.height)) as f32)
    }

    pub(crate) fn bounds(&self, index: usize, viewport: Rect, offset: f32) -> Rect {
        let x = (index % self.columns) as f32 * (self.width + self.spacing);
        let y = (self.top(index) - f64::from(offset)) as f32;
        Rect::new(viewport.x + x, viewport.y + y, self.width, self.height)
    }

    pub(crate) fn visible(&self, top: f64, bottom: f64, count: usize) -> Range<usize> {
        let first = ((top + f64::from(self.spacing)) / self.pitch()).floor() as usize;
        let end = (bottom / self.pitch()).ceil() as usize;
        first.saturating_mul(self.columns).min(count)..end.saturating_mul(self.columns).min(count)
    }
}
