use crate::{
    Rect, Size, TextMeasurer, dimension, linear,
    linear::{Axis, AxisInfo, Metrics, Padding},
    participates_in_flow,
};
use xrack_runtime::{
    CrossAxisAlignment, FlexboxLayoutDirection, FlexboxLayoutWrap, LayoutAlignment, Node,
    Orientation, Value,
};

pub(crate) fn uses_flow(node: &Node) -> bool {
    match node.kind {
        "FlowLayout" | "FlexboxLayout" => true,
        "DialogButtonBox" => node.boolean("wrap", true),
        "ToolBar" => node.boolean("wrap", false),
        _ => false,
    }
}

struct Options {
    flex: bool,
    axis: Axis,
    reverse_main: bool,
    wrap: FlexboxLayoutWrap,
    alignment: LayoutAlignment,
    cross_alignment: CrossAxisAlignment,
    line_alignment: LayoutAlignment,
    main_gap: f32,
    cross_gap: f32,
}

impl Options {
    fn new(node: &Node) -> Self {
        let flex = node.kind == "FlexboxLayout";
        let direction = if flex {
            match node.properties.get("flex-direction") {
                Some(Value::FlexboxLayoutDirection(direction)) => *direction,
                _ => FlexboxLayoutDirection::default(),
            }
        } else if node.orientation() == Orientation::Vertical {
            FlexboxLayoutDirection::Column
        } else {
            FlexboxLayoutDirection::Row
        };
        let (axis, reverse_main) = match direction {
            FlexboxLayoutDirection::Row => (Axis::Horizontal, false),
            FlexboxLayoutDirection::RowReverse => (Axis::Horizontal, true),
            FlexboxLayoutDirection::Column => (Axis::Vertical, false),
            FlexboxLayoutDirection::ColumnReverse => (Axis::Vertical, true),
        };
        let wrap = match node.properties.get("flex-wrap") {
            Some(Value::FlexboxLayoutWrap(wrap)) if flex => *wrap,
            _ => FlexboxLayoutWrap::Wrap,
        };
        let spacing = linear::spacing(node);
        let (horizontal, vertical) = if flex {
            ("spacing-horizontal", "spacing-vertical")
        } else {
            ("horizontal-spacing", "vertical-spacing")
        };
        let horizontal = dimension(node, horizontal).unwrap_or(spacing);
        let vertical = dimension(node, vertical).unwrap_or(spacing);
        let (main_gap, cross_gap) = match axis {
            Axis::Horizontal => (horizontal, vertical),
            Axis::Vertical => (vertical, horizontal),
        };
        let cross_alignment = match linear::cross_alignment(node, "cross-axis-alignment") {
            CrossAxisAlignment::Auto if flex => CrossAxisAlignment::Stretch,
            CrossAxisAlignment::Auto => CrossAxisAlignment::Start,
            value => value,
        };
        let line_alignment = if flex {
            match node.properties.get("cross-axis-line-alignment") {
                Some(Value::LayoutAlignment(alignment)) => *alignment,
                _ => LayoutAlignment::Stretch,
            }
        } else {
            LayoutAlignment::Start
        };
        Self {
            flex,
            axis,
            reverse_main,
            wrap,
            alignment: linear::layout_alignment(node),
            cross_alignment,
            line_alignment,
            main_gap,
            cross_gap,
        }
    }

    fn fit_items(&self, items: &[Item], available: f32) -> (Vec<f32>, f32, f32) {
        let tracks = items.iter().map(|item| item.metrics.along(self.axis));
        if !self.flex {
            return linear::fit_line(tracks, available, self.main_gap, self.alignment);
        }
        let occupied = tracks.clone().map(|info| info.preferred).sum::<f32>()
            + self.main_gap * items.len().saturating_sub(1) as f32;
        let shrink = occupied > available;
        let weighted_growth = self.alignment == LayoutAlignment::Stretch
            && !shrink
            && tracks.clone().any(|info| info.stretch > 0.0);
        let tracks = tracks.map(|mut info| {
            if shrink {
                info.stretch = info.preferred;
            } else if weighted_growth && info.stretch == 0.0 {
                info.max = info.preferred;
            }
            info
        });
        linear::fit_line(tracks, available, self.main_gap, self.alignment)
    }

    fn line_len(&self, items: &[Item], limit: f32) -> usize {
        if self.wrap == FlexboxLayoutWrap::NoWrap {
            return items.len();
        }
        let mut count = 1;
        let mut preferred = items[0].metrics.along(self.axis).preferred;
        for item in &items[1..] {
            let next = preferred + self.main_gap + item.metrics.along(self.axis).preferred;
            if next > limit {
                break;
            }
            preferred = next;
            count += 1;
        }
        count
    }
}

struct Item {
    index: usize,
    order: i64,
    metrics: Metrics,
}

struct Line<'a> {
    items: &'a [Item],
    sizes: Vec<f32>,
    cross_infos: Vec<AxisInfo>,
    preferred_cross: f32,
    cursor: f32,
    gap: f32,
}

struct Flow {
    axis: Axis,
    main: f32,
    minimum_main: f32,
    cross: f32,
    children: Vec<Rect>,
}

impl Flow {
    fn plan(node: &Node, inner: Rect, text: &mut impl TextMeasurer) -> Self {
        let options = Options::new(node);
        let axis = options.axis;
        let available = Size::new(inner.width, inner.height);
        let main_limit = axis.extent(available);
        let cross_limit = axis.other().extent(available);
        let mut items = Vec::new();
        for (index, child) in node.children.iter().enumerate() {
            if !participates_in_flow(child) {
                continue;
            }
            let metrics = linear::measure(child, available, text);
            let usable = if options.flex {
                metrics.x.max > 0.0 && metrics.y.max > 0.0
            } else {
                metrics.x.preferred > 0.0 && metrics.y.preferred > 0.0
            };
            if usable {
                items.push(Item {
                    index,
                    order: linear::item_order(node, child),
                    metrics,
                });
            }
        }
        items.sort_by_key(|item| item.order);
        let minima = items.iter().map(|item| item.metrics.along(axis).min);
        let minimum_main = if !options.flex {
            0.0
        } else if options.wrap == FlexboxLayoutWrap::NoWrap {
            minima.sum::<f32>() + options.main_gap * items.len().saturating_sub(1) as f32
        } else {
            minima.fold(0.0, f32::max)
        };
        let mut flow = Self {
            axis,
            main: 0.0,
            minimum_main,
            cross: 0.0,
            children: vec![Rect::new(inner.x, inner.y, 0.0, 0.0); node.children.len()],
        };
        let mut lines = Vec::new();
        let mut remaining = items.as_slice();
        while !remaining.is_empty() {
            let (line, rest) = remaining.split_at(options.line_len(remaining, main_limit));
            remaining = rest;
            let (sizes, cursor, gap) = options.fit_items(line, main_limit);
            let cross_infos: Vec<_> = line
                .iter()
                .zip(&sizes)
                .map(|(item, &size)| {
                    let child = &node.children[item.index];
                    if matches!(axis, Axis::Horizontal)
                        && size != linear::measurement_width(child, available)
                    {
                        linear::measure_with_width(child, available, Some(size), text).y
                    } else {
                        item.metrics.along(axis.other())
                    }
                })
                .collect();
            let preferred_cross = cross_infos
                .iter()
                .map(|info| info.preferred)
                .fold(0.0_f32, f32::max);
            let gaps = line.len().saturating_sub(1) as f32;
            let used = sizes.iter().sum::<f32>()
                + if options.flex {
                    options.main_gap * gaps
                } else {
                    cursor + gap * gaps
                };
            flow.main = flow.main.max(used.min(main_limit));
            flow.cross += preferred_cross;
            lines.push(Line {
                items: line,
                sizes,
                cross_infos,
                preferred_cross,
                cursor,
                gap,
            });
        }
        if lines.is_empty() {
            return flow;
        }
        flow.cross += options.cross_gap * lines.len().saturating_sub(1) as f32;
        let tracks = lines.iter().map(|line| AxisInfo {
            min: line.preferred_cross,
            ..AxisInfo::flexible(line.preferred_cross)
        });
        let (line_sizes, mut line_cursor, gap) = linear::fit_line(
            tracks,
            cross_limit,
            options.cross_gap,
            options.line_alignment,
        );
        for (line, line_size) in lines.into_iter().zip(line_sizes) {
            let mut cursor = line.cursor;
            for ((item, size), cross) in line.items.iter().zip(line.sizes).zip(line.cross_infos) {
                let child = &node.children[item.index];
                let alignment = match linear::cross_alignment(child, "cross-axis-self-alignment") {
                    CrossAxisAlignment::Auto => options.cross_alignment,
                    value => value,
                };
                let (offset, extent) = linear::cross_geometry(cross, alignment, line_size);
                let main = if options.reverse_main {
                    main_limit - cursor - size
                } else {
                    cursor
                };
                let cross = if options.wrap == FlexboxLayoutWrap::WrapReverse {
                    cross_limit - line_cursor - offset - extent
                } else {
                    line_cursor + offset
                };
                flow.children[item.index] = axis.rect(inner, main, cross, size, extent);
                cursor += size + line.gap;
            }
            line_cursor += line_size + gap;
        }
        flow
    }
}

pub(crate) fn measure(node: &Node, available: Size, text: &mut impl TextMeasurer) -> Metrics {
    let padding = Padding::read(node);
    let inner = padding.inner_rect(Rect::new(0.0, 0.0, available.width, available.height));
    let flow = Flow::plan(node, inner, text);
    let padding = padding.size();
    let main_padding = flow.axis.extent(padding);
    let minimum = if node.kind == "FlexboxLayout" {
        flow.minimum_main + main_padding
    } else {
        0.0
    };
    let main = AxisInfo {
        min: minimum,
        ..AxisInfo::flexible((flow.main + main_padding).max(minimum))
    };
    let cross = flow.cross + flow.axis.other().extent(padding);
    let cross = if node.kind == "FlexboxLayout" {
        AxisInfo::flexible(cross)
    } else {
        AxisInfo {
            min: 0.0,
            preferred: cross,
            max: cross,
            stretch: 0.0,
        }
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
