use crate::{
    ButtonContent, Rect, Size, TextMeasurer, dialog_title_height, dimension, flow, font_size,
    participates_in_flow,
};
use xrack_runtime::{
    CrossAxisAlignment, LayoutAlignment, Node, Orientation, Value, is_modal_overlay,
    is_multiline_text_input, is_text_input, is_toggle,
};

#[derive(Clone, Copy)]
pub(crate) enum Axis {
    Horizontal,
    Vertical,
}

impl Axis {
    pub(crate) fn for_layout(kind: &str) -> Option<Self> {
        match kind {
            "HorizontalLayout" | "HorizontalBox" | "MenuBar" => Some(Self::Horizontal),
            "VerticalLayout" | "VerticalBox" | "Dialog" | "Drawer" => Some(Self::Vertical),
            _ => None,
        }
    }

    pub(crate) fn for_node(node: &Node) -> Option<Self> {
        if node.kind == "ToolBar" {
            (!flow::wraps(node)).then(|| match node.orientation() {
                Orientation::Horizontal => Self::Horizontal,
                Orientation::Vertical => Self::Vertical,
            })
        } else if node.kind == "RadioGroup" {
            if !node.properties.contains_key("orientation")
                || node.orientation() == Orientation::Vertical
            {
                Some(Self::Vertical)
            } else {
                Some(Self::Horizontal)
            }
        } else {
            Self::for_layout(node.kind)
        }
    }

    pub(crate) fn other(self) -> Self {
        match self {
            Self::Horizontal => Self::Vertical,
            Self::Vertical => Self::Horizontal,
        }
    }

    pub(crate) fn extent(self, size: Size) -> f32 {
        match self {
            Self::Horizontal => size.width,
            Self::Vertical => size.height,
        }
    }

    pub(crate) fn rect(
        self,
        inner: Rect,
        main: f32,
        cross: f32,
        size: f32,
        cross_size: f32,
    ) -> Rect {
        match self {
            Self::Horizontal => Rect::new(inner.x + main, inner.y + cross, size, cross_size),
            Self::Vertical => Rect::new(inner.x + cross, inner.y + main, cross_size, size),
        }
    }

    fn properties(self) -> [&'static str; 5] {
        match self {
            Self::Horizontal => [
                "width",
                "min-width",
                "preferred-width",
                "max-width",
                "horizontal-stretch",
            ],
            Self::Vertical => [
                "height",
                "min-height",
                "preferred-height",
                "max-height",
                "vertical-stretch",
            ],
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct AxisInfo {
    pub(crate) min: f32,
    pub(crate) preferred: f32,
    pub(crate) max: f32,
    pub(crate) stretch: f32,
}

impl AxisInfo {
    pub(crate) fn fixed(size: f32) -> Self {
        Self {
            min: size,
            preferred: size,
            max: size,
            stretch: 0.0,
        }
    }

    pub(crate) fn flexible(preferred: f32) -> Self {
        Self {
            min: 0.0,
            preferred,
            max: f32::INFINITY,
            stretch: 1.0,
        }
    }

    fn with_properties(self, node: &Node, axis: Axis, available: Size) -> Self {
        let reference = axis.extent(available);
        let dimension = |node: &Node, name| {
            node.relative_length(name, reference)
                .map(|value| value.max(0.0))
        };
        let [size, min_size, preferred_size, max_size, stretch] = axis.properties();
        if let Some(size) = dimension(node, size) {
            return Self::fixed(size);
        }
        let min = dimension(node, min_size).unwrap_or(self.min);
        let max = dimension(node, max_size).unwrap_or(self.max).max(min);
        Self {
            min,
            preferred: dimension(node, preferred_size)
                .unwrap_or(self.preferred)
                .clamp(min, max),
            max,
            stretch: node
                .float(stretch)
                .map(|value| value.max(0.0))
                .unwrap_or(self.stretch),
        }
    }

    pub(crate) fn clamp(self, size: f32) -> f32 {
        size.clamp(self.min, self.max)
    }
}

pub(crate) fn intrinsic_item(kind: &str) -> bool {
    matches!(
        kind,
        "Text"
            | "Button"
            | "ToolButton"
            | "DialogButton"
            | "Menu"
            | "Image"
            | "Slider"
            | "PageIndicator"
            | "Dial"
            | "RangeSlider"
            | "ProgressIndicator"
            | "SpinBox"
            | "DoubleSpinBox"
            | "ComboBox"
    ) || kind == "RadioButton"
        || is_toggle(kind)
        || is_text_input(kind) && !is_multiline_text_input(kind)
}

fn legacy_box(kind: &str) -> bool {
    matches!(kind, "HorizontalBox" | "VerticalBox")
}

pub(crate) struct Padding {
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
}

impl Padding {
    pub(crate) fn read(node: &Node) -> Self {
        let uniform = dimension(node, "padding").unwrap_or(match node.kind {
            "ToolBar" => 6.0,
            kind if is_modal_overlay(kind) => 16.0,
            _ => 0.0,
        });
        let side = |name| dimension(node, name).unwrap_or(uniform);
        Self {
            left: side("padding-left"),
            right: side("padding-right"),
            top: side("padding-top"),
            bottom: side("padding-bottom"),
        }
    }

    pub(crate) fn size(&self) -> Size {
        Size::new(self.left + self.right, self.top + self.bottom)
    }

    pub(crate) fn inner_size(&self, outer: Size) -> Size {
        let padding = self.size();
        Size::new(
            (outer.width - padding.width).max(0.0),
            (outer.height - padding.height).max(0.0),
        )
    }

    pub(crate) fn inner_rect(&self, bounds: Rect) -> Rect {
        let inner = self.inner_size(Size::new(bounds.width, bounds.height));
        Rect::new(
            bounds.x + self.left.min(bounds.width),
            bounds.y + self.top.min(bounds.height),
            inner.width,
            inner.height,
        )
    }
}

pub(crate) struct Metrics {
    pub(crate) x: AxisInfo,
    pub(crate) y: AxisInfo,
}

impl Metrics {
    pub(crate) fn along(&self, axis: Axis) -> AxisInfo {
        match axis {
            Axis::Horizontal => self.x,
            Axis::Vertical => self.y,
        }
    }
}

pub(crate) fn measure(node: &Node, available: Size, text: &mut impl TextMeasurer) -> Metrics {
    measure_with_width(node, available, None, text)
}

pub(crate) fn measurement_width(node: &Node, reference: Size) -> f32 {
    node.relative_length("width", reference.width)
        .map(|value| value.max(0.0))
        .unwrap_or_else(|| {
            reference.width.min(
                node.relative_length("max-width", reference.width)
                    .map_or(f32::INFINITY, |value| value.max(0.0)),
            )
        })
}

pub(crate) fn measure_with_width(
    node: &Node,
    reference: Size,
    assigned_width: Option<f32>,
    text: &mut impl TextMeasurer,
) -> Metrics {
    let relative_width = |name| {
        node.relative_length(name, reference.width)
            .map(|value| value.max(0.0))
    };
    let available = Size::new(
        assigned_width.unwrap_or_else(|| measurement_width(node, reference)),
        node.relative_length("height", reference.height)
            .map(|value| value.max(0.0))
            .unwrap_or(reference.height),
    );
    let metrics = if let Some(axis) = Axis::for_node(node) {
        let padding = Padding::read(node);
        let mut inner = padding.inner_size(available);
        let title_height = crate::selection::radio_title_height(node) + dialog_title_height(node);
        inner.height = (inner.height - title_height).max(0.0);
        let mut main = AxisInfo::fixed(0.0);
        let mut cross = AxisInfo::flexible(0.0);
        cross.stretch = f32::INFINITY;
        let mut count = 0usize;
        let children = node
            .children
            .iter()
            .filter(|child| linear_child(node, child));
        let has_flow = matches!(axis, Axis::Horizontal) && children.clone().any(flow::wraps);
        let alignment = layout_alignment(node);
        let legacy = legacy_box(node.kind);
        let mut line_infos = Vec::new();
        for child in children.clone() {
            let metrics = measure(child, inner, text);
            let child_main = metrics.along(axis);
            let child_cross = metrics.along(axis.other());
            if has_flow {
                line_infos.push(main_info(child, axis, alignment, legacy, child_main));
            }
            main.min += child_main.min;
            main.preferred += child_main.preferred;
            main.max += child_main.max;
            main.stretch += child_main.stretch;
            cross.min = cross.min.max(child_cross.min);
            cross.preferred = cross.preferred.max(child_cross.preferred);
            cross.max = cross.max.min(child_cross.max);
            cross.stretch = cross.stretch.min(child_cross.stretch);
            count += 1;
        }
        let gaps = spacing(node) * count.saturating_sub(1) as f32;
        let mut padding = padding.size();
        padding.height += title_height;
        let main_extra = gaps + axis.extent(padding);
        let cross_extra = axis.other().extent(padding);
        main.min += main_extra;
        main.preferred += main_extra;
        main.max += main_extra;
        cross.min += cross_extra;
        cross.preferred += cross_extra;
        cross.max += cross_extra;
        if has_flow {
            // A flow may wrap more lines after its siblings take their widths.
            let (widths, _, _) = fit_line(
                line_infos.into_iter(),
                inner.width,
                spacing(node),
                alignment,
            );
            let flow_height = children
                .zip(widths)
                .filter(|(child, _)| flow::wraps(child))
                .map(|(child, width)| {
                    measure_with_width(child, inner, Some(width), text)
                        .y
                        .preferred
                })
                .fold(0.0_f32, f32::max);
            cross.preferred = cross.preferred.max(flow_height + cross_extra);
            cross.max = cross.max.max(flow_height + cross_extra);
        }
        if alignment != LayoutAlignment::Stretch {
            main.max = f32::INFINITY;
        }
        if count == 0 {
            cross.stretch = 1.0;
        }
        if node.kind == "MenuBar" {
            cross = AxisInfo::fixed(cross.preferred.max(32.0));
        }
        if node.kind == "ToolBar" {
            cross.max = cross.preferred.max(cross.min);
            cross.stretch = 0.0;
        }
        if legacy {
            main.max = f32::INFINITY;
            cross.max = f32::INFINITY;
            main.stretch = 1.0;
            cross.stretch = 1.0;
        }
        match axis {
            Axis::Horizontal => Metrics { x: main, y: cross },
            Axis::Vertical => Metrics { x: cross, y: main },
        }
    } else if flow::wraps(node) {
        crate::flow::measure(node, available, text)
    } else if node.kind == "GridLayout" {
        crate::grid::measure(node, available, text)
    } else {
        let natural = match node.kind {
            "Text" => text.measure(
                node.text("text").unwrap_or(""),
                font_size(node),
                available.width,
            ),
            "TextInput" | "LineEdit" => Size::new(180.0, (font_size(node) * 1.4 + 12.0).max(34.0)),
            "TextEdit" | "TextArea" => Size::new(240.0, font_size(node) * 5.6 + 12.0),
            "Slider" | "RangeSlider" if node.orientation() == Orientation::Vertical => {
                Size::new(28.0, 160.0)
            }
            "Slider" | "RangeSlider" => Size::new(160.0, 28.0),
            "Dial" => Size::new(96.0, 96.0),
            "SpinBox" => Size::new(116.0, 34.0),
            "DoubleSpinBox" => Size::new(148.0, 34.0),
            "ComboBox" => Size::new(156.0, 34.0),
            "Calendar" => Size::new(294.0, 300.0),
            "TabWidget" | "SwipeView" | "StackView" => Size::new(240.0, 136.0),
            "TabBar" if node.orientation() == Orientation::Vertical => Size::new(132.0, 144.0),
            "TabBar" => Size::new(240.0, 38.0),
            "PageIndicator" => crate::PageIndicatorGeometry::preferred_size(node),
            "ListView" | "TreeView" => Size::new(180.0, 144.0),
            "TableView" => Size::new(280.0, 178.0),
            "SplitView" => Size::new(320.0, 180.0),
            "Menu" => Size::new(
                text.measure(
                    node.text("title").unwrap_or(""),
                    font_size(node),
                    available.width,
                )
                .width
                    + 24.0,
                32.0,
            ),
            "ProgressIndicator" => Size::new(160.0, 10.0),
            "CheckBox" | "Switch" | "RadioButton" => {
                let decoration = if node.kind == "Switch" { 50.0 } else { 30.0 };
                let label = text.measure(
                    node.text("text").unwrap_or(""),
                    font_size(node),
                    (available.width - decoration).max(0.0),
                );
                Size::new(label.width + decoration, label.height.max(26.0))
            }
            "Button" | "ToolButton" | "DialogButton" => {
                let button = ButtonContent::new(node);
                let icon = button.icon.map_or(0.0, |(_, size)| size);
                let chrome = 2.0 * button.padding;
                let decoration = icon + button.gap();
                let label = if button.label.is_empty() {
                    Size::default()
                } else {
                    text.measure(
                        button.label,
                        font_size(node),
                        (available.width - chrome - decoration).max(0.0),
                    )
                };
                let min = if node.kind == "ToolButton" {
                    Size::new(34.0, 34.0)
                } else {
                    Size::new(64.0, 40.0)
                };
                Size::new(
                    (label.width + decoration + chrome).max(min.width),
                    (label.height.max(icon) + chrome).max(min.height),
                )
            }
            "Image" => {
                let [width, height] = node.image("source").unwrap_or_default().size();
                if width == 0 || height == 0 {
                    Size::default()
                } else {
                    let (width, height) = (width as f32, height as f32);
                    let scale = relative_width("width")
                        .map(|size| size / width)
                        .or_else(|| {
                            node.relative_length("height", reference.height)
                                .map(|size| size.max(0.0) / height)
                        })
                        .unwrap_or(1.0);
                    Size::new(width * scale, height * scale)
                }
            }
            _ => Size::default(),
        };
        let info = |preferred| AxisInfo {
            min: if matches!(
                node.kind,
                "Text" | "Button" | "ToolButton" | "DialogButton" | "Menu"
            ) {
                preferred
            } else {
                0.0
            },
            stretch: if intrinsic_item(node.kind) { 0.0 } else { 1.0 },
            ..AxisInfo::flexible(preferred)
        };
        Metrics {
            x: info(natural.width),
            y: info(natural.height),
        }
    };
    Metrics {
        x: metrics.x.with_properties(node, Axis::Horizontal, reference),
        y: metrics.y.with_properties(node, Axis::Vertical, reference),
    }
}

pub(crate) fn layout_alignment(node: &Node) -> LayoutAlignment {
    match node.properties.get("alignment") {
        Some(Value::LayoutAlignment(value)) => *value,
        _ if matches!(node.kind, "MenuBar" | "FlowLayout" | "ToolBar") => LayoutAlignment::Start,
        _ => LayoutAlignment::Stretch,
    }
}

pub(crate) fn cross_alignment(node: &Node, property: &str) -> CrossAxisAlignment {
    match node.properties.get(property) {
        Some(Value::CrossAxisAlignment(value)) => *value,
        _ => CrossAxisAlignment::Auto,
    }
}

pub(crate) fn cross_geometry(
    info: AxisInfo,
    alignment: CrossAxisAlignment,
    available: f32,
) -> (f32, f32) {
    match alignment {
        CrossAxisAlignment::Auto | CrossAxisAlignment::Stretch => (0.0, info.clamp(available)),
        CrossAxisAlignment::Start => (0.0, info.preferred),
        CrossAxisAlignment::Center => ((available - info.preferred) * 0.5, info.preferred),
        CrossAxisAlignment::End => (available - info.preferred, info.preferred),
    }
}

struct Item {
    index: usize,
    order: i64,
    main: AxisInfo,
    cross: AxisInfo,
    cross_alignment: CrossAxisAlignment,
}

fn main_info(
    child: &Node,
    axis: Axis,
    alignment: LayoutAlignment,
    legacy: bool,
    info: AxisInfo,
) -> AxisInfo {
    if legacy
        && alignment == LayoutAlignment::Stretch
        && !axis
            .properties()
            .iter()
            .any(|name| child.properties.contains_key(name))
    {
        if intrinsic_item(child.kind) {
            AxisInfo::fixed(info.preferred)
        } else {
            AxisInfo::flexible(0.0)
        }
    } else {
        info
    }
}

pub(crate) fn fit_line(
    infos: impl ExactSizeIterator<Item = AxisInfo> + Clone,
    available: f32,
    gap: f32,
    alignment: LayoutAlignment,
) -> (Vec<f32>, f32, f32) {
    let count = infos.len() as f32;
    let preferred = infos.clone().map(|item| item.preferred).sum::<f32>();
    let slack = available - preferred;
    let free = slack - gap * (count - 1.0);
    if alignment == LayoutAlignment::Stretch || free <= 0.0 {
        return (fit_axis(infos, available - gap * (count - 1.0)), 0.0, gap);
    }
    let (offset, spacing) = match alignment {
        LayoutAlignment::Stretch | LayoutAlignment::Start => (0.0, gap),
        LayoutAlignment::Center => (free * 0.5, gap),
        LayoutAlignment::End => (free, gap),
        LayoutAlignment::SpaceBetween if count > 1.0 => (0.0, slack / (count - 1.0)),
        LayoutAlignment::SpaceBetween => (0.0, gap),
        LayoutAlignment::SpaceAround => (slack / (2.0 * count), slack / count),
        LayoutAlignment::SpaceEvenly => (slack / (count + 1.0), slack / (count + 1.0)),
    };
    (infos.map(|item| item.preferred).collect(), offset, spacing)
}

pub(crate) fn fit_axis(infos: impl IntoIterator<Item = AxisInfo>, available: f32) -> Vec<f32> {
    let mut tracks: Vec<_> = infos
        .into_iter()
        .map(|info| (info, info.preferred))
        .collect();
    let preferred = tracks.iter().map(|item| f64::from(item.1)).sum::<f64>();
    let grow = preferred < f64::from(available);
    let mut remaining = (f64::from(available) - preferred).abs();
    let capacity = |item: &(AxisInfo, f32)| {
        if grow {
            f64::from(item.0.max) - f64::from(item.1)
        } else {
            f64::from(item.1) - f64::from(item.0.min)
        }
    };
    // Each pass either spends the remaining space or saturates at least one item.
    for _ in 0..=tracks.len() {
        if remaining <= 0.0 {
            break;
        }
        let active = tracks.iter().filter(|item| capacity(item) > 0.0);
        let total_stretch = active
            .clone()
            .map(|item| f64::from(item.0.stretch))
            .sum::<f64>();
        let weight = |item: &(AxisInfo, f32)| {
            if total_stretch > 0.0 {
                f64::from(item.0.stretch)
            } else {
                1.0
            }
        };
        let total_weight = if total_stretch > 0.0 {
            total_stretch
        } else {
            active.clone().count() as f64
        };
        if total_weight == 0.0 {
            break;
        }
        let step = active
            .filter(|item| weight(item) > 0.0)
            .fold(remaining / total_weight, |step, item| {
                step.min(capacity(item) / weight(item))
            });
        let mut spent = 0.0;
        for item in tracks.iter_mut().filter(|item| capacity(item) > 0.0) {
            let delta = (step * weight(item)).min(capacity(item)) as f32;
            let next = item
                .0
                .clamp(if grow { item.1 + delta } else { item.1 - delta });
            spent += f64::from((next - item.1).abs());
            item.1 = next;
        }
        if spent == 0.0 {
            break;
        }
        remaining -= spent;
    }
    tracks.into_iter().map(|(_, size)| size).collect()
}

pub(crate) fn children(
    node: &Node,
    axis: Axis,
    bounds: Rect,
    text: &mut impl TextMeasurer,
) -> Vec<Rect> {
    let inner = Padding::read(node).inner_rect(bounds);
    let available = Size::new(inner.width, inner.height);
    let main_available = axis.extent(available);
    let cross_available = axis.other().extent(available);
    let gap = spacing(node);
    let alignment = layout_alignment(node);
    let default_cross = cross_alignment(node, "cross-axis-alignment");
    let legacy = legacy_box(node.kind);
    let cross_properties = axis.other().properties();

    let mut items = Vec::new();
    for (index, child) in node.children.iter().enumerate() {
        if !linear_child(node, child) {
            continue;
        }
        let metrics = measure(child, available, text);
        let mut main = metrics.along(axis);
        let mut cross = metrics.along(axis.other());
        let cross_alignment = match cross_alignment(child, "cross-axis-self-alignment") {
            CrossAxisAlignment::Auto => default_cross,
            value => value,
        };
        if legacy
            && !cross_properties[..4]
                .iter()
                .any(|name| child.properties.contains_key(name))
        {
            cross = AxisInfo::flexible(cross.preferred);
        }
        if matches!(axis, Axis::Vertical) {
            let (_, width) = cross_geometry(cross, cross_alignment, cross_available);
            if width != available.width {
                main = measure_with_width(child, available, Some(width), text).along(axis);
            }
        }
        items.push(Item {
            index,
            order: child.int("layout-order").unwrap_or(0),
            main: main_info(child, axis, alignment, legacy, main),
            cross,
            cross_alignment,
        });
    }
    items.sort_by_key(|item| item.order);
    let mut result = vec![Rect::new(inner.x, inner.y, 0.0, 0.0); node.children.len()];
    if items.is_empty() {
        return result;
    }

    let (sizes, mut cursor, spacing) = fit_line(
        items.iter().map(|item| item.main),
        main_available,
        gap,
        alignment,
    );
    for (item, size) in items.into_iter().zip(sizes) {
        let child = &node.children[item.index];
        let reflows = flow::wraps(child) && size != measurement_width(child, available);
        let cross = if matches!(axis, Axis::Horizontal)
            && (reflows
                || !matches!(
                    item.cross_alignment,
                    CrossAxisAlignment::Stretch | CrossAxisAlignment::Auto
                )) {
            measure_with_width(child, available, Some(size), text).along(axis.other())
        } else {
            item.cross
        };
        let (offset, extent) = cross_geometry(cross, item.cross_alignment, cross_available);
        let mut rect = axis.rect(inner, cursor, offset, size, extent);
        if legacy {
            rect.x += child.length("x").unwrap_or(0.0);
            rect.y += child.length("y").unwrap_or(0.0);
        }
        result[item.index] = rect;
        cursor += size + spacing;
    }
    result
}

pub(crate) fn spacing(node: &Node) -> f32 {
    dimension(node, "spacing").unwrap_or(match node.kind {
        "ToolBar" => 6.0,
        kind if is_modal_overlay(kind) => 8.0,
        _ => 0.0,
    })
}

fn linear_child(parent: &Node, child: &Node) -> bool {
    participates_in_flow(child)
        || (parent.kind == "MenuBar" && child.kind == "Menu" && child.is_visible())
}
