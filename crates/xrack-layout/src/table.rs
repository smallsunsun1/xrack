use crate::{
    Point, Rect, ScrollLayout,
    linear::{AxisInfo, fit_axis},
};
use xrack_runtime::{Node, resizable_table_column};

/// Shared by the row layout, header painting, and header hit testing.
#[derive(Clone, Debug, PartialEq)]
pub struct TableGeometry {
    pub header: Rect,
    pub body: Rect,
    pub columns: Vec<Rect>,
    pub horizontal: ScrollLayout,
}

const MAX_COLUMN_WIDTH: f32 = 1_000_000.0;
const GRIP_RADIUS: f32 = 5.0;

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
        Self::with_offset(node, bounds, 0.0)
    }

    pub(crate) fn with_offset(node: &Node, bounds: Rect, requested: f32) -> Option<Self> {
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
        let horizontal = ScrollLayout::new(requested, widths.iter().sum::<f32>() - bounds.width);
        let mut x = bounds.x - horizontal.offset;
        let columns = widths
            .into_iter()
            .map(|width| {
                let column = Rect::new(x, bounds.y, width, header_bounds.height);
                x += width;
                column
            })
            .collect();
        Some(Self {
            header: header_bounds,
            body,
            columns,
            horizontal,
        })
    }

    pub(crate) fn column_at(&self, point: Point) -> Option<usize> {
        self.columns
            .iter()
            .position(|column| column.contains(point))
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
                (max > min
                    && bounds.width > 0.0
                    && edge > visible.x
                    && edge <= visible.x + visible.width
                    && distance <= GRIP_RADIUS)
                    .then_some((index, distance))
            })
            .min_by(|(_, a), (_, b)| a.total_cmp(b))
            .map(|(index, _)| index)
    }
}
