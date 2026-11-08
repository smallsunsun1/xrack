use crate::{
    Rect, Size, TextMeasurer, dimension, linear,
    linear::{Axis, AxisInfo, Metrics, Padding},
    participates_in_flow,
};
use xrack_runtime::{CrossAxisAlignment, Node, Orientation};

pub(crate) fn wraps(node: &Node) -> bool {
    node.kind == "FlowLayout" || node.kind == "ToolBar" && node.boolean("wrap", false)
}

struct Item {
    index: usize,
    order: i64,
    metrics: Metrics,
}

struct Flow {
    axis: Axis,
    main: f32,
    cross: f32,
    children: Vec<Rect>,
}

impl Flow {
    fn plan(node: &Node, inner: Rect, text: &mut impl TextMeasurer) -> Self {
        let horizontal = node.orientation() == Orientation::Horizontal;
        let axis = if horizontal {
            Axis::Horizontal
        } else {
            Axis::Vertical
        };
        let available = Size::new(inner.width, inner.height);
        let limit = axis.extent(available);
        let spacing = linear::spacing(node);
        let x_gap = dimension(node, "horizontal-spacing").unwrap_or(spacing);
        let y_gap = dimension(node, "vertical-spacing").unwrap_or(spacing);
        let (main_gap, cross_gap) = if horizontal {
            (x_gap, y_gap)
        } else {
            (y_gap, x_gap)
        };
        let alignment = linear::layout_alignment(node);
        let default_cross = match linear::cross_alignment(node, "cross-axis-alignment") {
            CrossAxisAlignment::Auto => CrossAxisAlignment::Start,
            value => value,
        };
        let mut items = Vec::new();
        for (index, child) in node.children.iter().enumerate() {
            if participates_in_flow(child) {
                let metrics = linear::measure(child, available, text);
                if metrics.x.preferred > 0.0 && metrics.y.preferred > 0.0 {
                    items.push(Item {
                        index,
                        order: child.int("layout-order").unwrap_or_default(),
                        metrics,
                    });
                }
            }
        }
        items.sort_by_key(|item| item.order);

        let mut flow = Self {
            axis,
            main: 0.0,
            cross: 0.0,
            children: vec![Rect::new(inner.x, inner.y, 0.0, 0.0); node.children.len()],
        };
        let mut remaining = items.as_slice();
        while let Some(first) = remaining.first() {
            let mut count = 1;
            let mut preferred = first.metrics.along(axis).preferred;
            for item in &remaining[1..] {
                let next = preferred + main_gap + item.metrics.along(axis).preferred;
                if next > limit {
                    break;
                }
                preferred = next;
                count += 1;
            }
            let (line, rest) = remaining.split_at(count);
            remaining = rest;

            let (sizes, mut cursor, gap) = linear::fit_line(
                line.iter().map(|item| item.metrics.along(axis)),
                limit,
                main_gap,
                alignment,
            );
            let cross_infos: Vec<_> = line
                .iter()
                .zip(&sizes)
                .map(|(item, &size)| {
                    let child = &node.children[item.index];
                    if horizontal && size != linear::measurement_width(child, available) {
                        linear::measure_with_width(child, available, Some(size), text).y
                    } else {
                        item.metrics.along(axis.other())
                    }
                })
                .collect();
            let line_cross = cross_infos
                .iter()
                .map(|info| info.preferred)
                .fold(0.0_f32, f32::max);
            for ((item, size), cross) in line.iter().zip(sizes).zip(cross_infos) {
                let child = &node.children[item.index];
                let alignment = match linear::cross_alignment(child, "cross-axis-self-alignment") {
                    CrossAxisAlignment::Auto => default_cross,
                    value => value,
                };
                let (offset, extent) = linear::cross_geometry(cross, alignment, line_cross);
                flow.children[item.index] =
                    axis.rect(inner, cursor, flow.cross + offset, size, extent);
                cursor += size + gap;
            }
            flow.main = flow.main.max((cursor - gap).min(limit));
            flow.cross += line_cross;
            if !remaining.is_empty() {
                flow.cross += cross_gap;
            }
        }
        flow
    }
}

pub(crate) fn measure(node: &Node, available: Size, text: &mut impl TextMeasurer) -> Metrics {
    let padding = Padding::read(node);
    let inner = padding.inner_rect(Rect::new(0.0, 0.0, available.width, available.height));
    let flow = Flow::plan(node, inner, text);
    let padding = padding.size();
    let main = AxisInfo::flexible(flow.main + flow.axis.extent(padding));
    let cross = flow.cross + flow.axis.other().extent(padding);
    let cross = AxisInfo {
        min: 0.0,
        preferred: cross,
        max: cross,
        stretch: 0.0,
    };
    match flow.axis {
        Axis::Horizontal => Metrics { x: main, y: cross },
        Axis::Vertical => Metrics { x: cross, y: main },
    }
}

pub(crate) fn children(node: &Node, bounds: Rect, text: &mut impl TextMeasurer) -> Vec<Rect> {
    let inner = Padding::read(node).inner_rect(bounds);
    Flow::plan(node, inner, text).children
}
