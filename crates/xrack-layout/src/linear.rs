use crate::{ButtonContent, Rect, Size, TextMeasurer, dimension, flow, participates_in_flow};
use std::collections::{HashMap, VecDeque};
use xrack_runtime::{
    ControlAction, CrossAxisAlignment, DialogCommand, DialogRole, LayoutAlignment, Node,
    Orientation, RowKey, Value, dialog_button_action, is_modal_overlay, is_multiline_text_input,
    is_pane, is_text_input, is_toggle,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Axis {
    Horizontal,
    Vertical,
}

impl Axis {
    pub(crate) fn for_layout(kind: &str) -> Option<Self> {
        match kind {
            "SwipeContent" | "SwipeLeft" | "SwipeRight" => Some(Self::Horizontal),
            "HorizontalLayout" | "HorizontalBox" | "MenuBar" | "TimePicker" | "DialogButtonBox" => {
                Some(Self::Horizontal)
            }
            "VerticalLayout" | "DynamicContainer" | "VerticalBox" | "Dialog" | "Drawer"
            | "GroupBox" | "Expander" | "PageHeader" | "PageFooter" | "PopupWindow" => {
                Some(Self::Vertical)
            }
            _ => None,
        }
    }

    pub(crate) fn for_node(node: &Node) -> Option<Self> {
        if node.kind == "DialogButtonBox" {
            (!flow::uses_flow(node)).then_some(Self::Horizontal)
        } else if node.kind == "ToolBar" {
            (!flow::uses_flow(node)).then(|| match node.orientation() {
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

#[derive(Clone, Copy, PartialEq)]
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

pub(crate) fn measure_replacement_content(
    content: &Node,
    available: Size,
    text: &mut impl TextMeasurer,
) -> Size {
    let mut size = Size::default();
    for child in &content.children {
        if participates_in_flow(child) {
            let child_metrics = measure(child, available, text);
            size.width = size
                .width
                .max(child_metrics.x.preferred + child.length("x").unwrap_or(0.0));
            size.height = size
                .height
                .max(child_metrics.y.preferred + child.length("y").unwrap_or(0.0));
        }
    }
    size
}

pub(crate) fn intrinsic_item(kind: &str) -> bool {
    matches!(
        kind,
        "Text"
            | "Label"
            | "StyledText"
            | "Button"
            | "ToolButton"
            | "RoundButton"
            | "DelayButton"
            | "DialogButton"
            | "Menu"
            | "Image"
            | "AnimatedImage"
            | "Slider"
            | "PageIndicator"
            | "Dial"
            | "RangeSlider"
            | "ProgressIndicator"
            | "BusyIndicator"
            | "Spinner"
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
        let default = match node.kind {
            "ToolBar" => 6.0,
            "GroupBox" | "Expander" => dimension(node, "content-padding").unwrap_or(12.0),
            "PopupWindow" => 12.0,
            "Pane" | "ControlFrame" => 12.0,
            kind if is_modal_overlay(kind) => 16.0,
            _ => 0.0,
        };
        Self::with_default(node, default)
    }

    pub(crate) const EDITOR_DEFAULT: f32 = 6.0;

    pub(crate) fn for_editor(node: &Node) -> Self {
        Self::with_default(node, Self::EDITOR_DEFAULT)
    }

    fn with_default(node: &Node, default: f32) -> Self {
        let uniform = dimension(node, "padding").unwrap_or(default);
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

#[derive(Clone, Copy)]
pub(crate) struct Metrics {
    pub(crate) x: AxisInfo,
    pub(crate) y: AxisInfo,
}

#[derive(Clone, Copy, PartialEq)]
struct Constraints {
    reference: Size,
    width: Option<f32>,
}

struct RowMeasurements {
    scene: Node,
    sizes: Vec<(Constraints, Metrics)>,
    depends_on_height: bool,
}

impl RowMeasurements {
    fn new(node: &Node) -> Self {
        Self {
            scene: node.clone(),
            sizes: Vec::new(),
            depends_on_height: reads_available_height(node),
        }
    }

    fn constraints(&self, mut input: Constraints) -> Constraints {
        if !self.depends_on_height {
            input.reference.height = 0.0;
        }
        input
    }
}

fn reads_available_height(node: &Node) -> bool {
    // The supported row kinds propagate height to their children, but their
    // intrinsic sizing uses width and text metrics. Relative lengths can add
    // a dependency on the height assigned to the enclosing layout.
    node.properties.iter().any(|(&name, value)| {
        matches!(value, Value::Percent(_))
            && !matches!(
                name,
                "width" | "min-width" | "preferred-width" | "max-width" | "x"
            )
    }) || node.children.iter().any(reads_available_height)
}

/// Intrinsic measurements of immutable repeat rows for one font configuration.
#[doc(hidden)]
#[derive(Default)]
pub struct IntrinsicMeasurements {
    rows: HashMap<RowKey, RowMeasurements>,
    order: VecDeque<RowKey>,
}

impl IntrinsicMeasurements {
    fn get(&self, key: RowKey, node: &Node, constraints: Constraints) -> Option<Metrics> {
        let row = self.rows.get(&key)?;
        if !row.scene.shares_storage_with(node) {
            return None;
        }
        let constraints = row.constraints(constraints);
        row.sizes
            .iter()
            .find_map(|&(last, metrics)| (last == constraints).then_some(metrics))
    }

    fn insert(&mut self, key: RowKey, node: &Node, constraints: Constraints, metrics: Metrics) {
        if !self.rows.contains_key(&key) {
            if self.rows.len() == 512
                && let Some(oldest) = self.order.pop_front()
            {
                self.rows.remove(&oldest);
            }
            self.order.push_back(key);
        }
        let row = self
            .rows
            .entry(key)
            .or_insert_with(|| RowMeasurements::new(node));
        if !row.scene.shares_storage_with(node) {
            *row = RowMeasurements::new(node);
        }
        let constraints = row.constraints(constraints);
        if row.sizes.len() == 8 {
            row.sizes.remove(0);
        }
        row.sizes.push((constraints, metrics));
    }
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

fn reflows_at_width(node: &Node) -> bool {
    flow::uses_flow(node)
        || crate::label::reflows_at_width(node)
        || (is_pane(node.kind) || matches!(node.kind, "Loader" | "LoaderCandidate"))
            && node.children.iter().any(reflows_at_width)
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

pub(crate) fn fixed_size(node: &Node, reference: Size) -> Option<Size> {
    let width = node.relative_length("width", reference.width)?.max(0.0);
    let height = node.relative_length("height", reference.height)?.max(0.0);
    Some(Size::new(width, height))
}

pub(crate) fn fill_size_with_fixed_cross_axis(
    node: &Node,
    reference: Size,
    text: &mut impl TextMeasurer,
) -> Option<Size> {
    let axis = match node.kind {
        "HorizontalLayout" => Axis::Horizontal,
        "VerticalLayout" => Axis::Vertical,
        _ => return None,
    };
    let cross = axis.other();
    let fixed = node
        .relative_length(cross.properties()[0], cross.extent(reference))?
        .max(0.0);
    let main_properties = axis.properties();
    if main_properties
        .iter()
        .any(|name| node.properties.contains_key(name))
    {
        return None;
    }

    let available = match axis {
        Axis::Horizontal => Size::new(measurement_width(node, reference), fixed),
        Axis::Vertical => Size::new(fixed, reference.height),
    };
    let padding = Padding::read(node);
    let inner = padding.inner_size(available);
    let (mut minimum, mut maximum, mut count) = (0.0_f64, 0.0_f64, 0usize);
    for child in node
        .children
        .iter()
        .filter(|child| linear_child(node, child))
    {
        if geometry_primitive(child.kind)
            && !child
                .properties
                .keys()
                .any(|name| main_properties.contains(name))
        {
            maximum += f64::INFINITY;
        } else {
            let main = measure(child, inner, text).along(axis);
            minimum += f64::from(main.min);
            maximum += f64::from(main.max);
        }
        count += 1;
    }

    // The explicit cross size overrides intrinsic sizing on that axis.
    // The other axis fills the available extent within its min and max.
    let gaps = f64::from(spacing(node)) * count.saturating_sub(1) as f64;
    let extra = gaps + f64::from(axis.extent(padding.size()));
    let min = (minimum + extra) as f32;
    let max = if layout_alignment(node) == LayoutAlignment::Stretch {
        ((maximum + extra) as f32).max(min)
    } else {
        f32::INFINITY
    };
    let main = axis.extent(reference).clamp(min, max);
    Some(match axis {
        Axis::Horizontal => Size::new(main, fixed),
        Axis::Vertical => Size::new(fixed, main),
    })
}

fn geometry_primitive(kind: &str) -> bool {
    matches!(kind, "Rectangle" | "Path" | "TouchArea")
}

pub(crate) fn measure_with_width(
    node: &Node,
    reference: Size,
    assigned_width: Option<f32>,
    text: &mut impl TextMeasurer,
) -> Metrics {
    let Some(key) = node.row_key.filter(|_| node.has_shared_storage()) else {
        return compute_measurement(node, reference, assigned_width, text);
    };
    let constraints = Constraints {
        reference,
        width: assigned_width,
    };
    let Some(cache) = text.intrinsic_measurements() else {
        return compute_measurement(node, reference, assigned_width, text);
    };
    if let Some(metrics) = cache.get(key, node, constraints) {
        return metrics;
    }
    let metrics = compute_measurement(node, reference, assigned_width, text);
    if crate::row_layout::cacheable_shared_row(node)
        && let Some(cache) = text.intrinsic_measurements()
    {
        cache.insert(key, node, constraints, metrics);
    }
    metrics
}

fn compute_measurement(
    node: &Node,
    reference: Size,
    assigned_width: Option<f32>,
    text: &mut impl TextMeasurer,
) -> Metrics {
    // Explicit sizes replace both axes in with_properties. Children are
    // measured later, if their own placement depends on intrinsic sizes.
    if let Some(size) = fixed_size(node, reference) {
        return Metrics {
            x: AxisInfo::fixed(size.width),
            y: AxisInfo::fixed(size.height),
        };
    }
    if geometry_primitive(node.kind) {
        // These primitives fill allocated space. Their children and paint
        // properties do not contribute an intrinsic size.
        let axis = AxisInfo::flexible(0.0);
        let has_size_properties = node.properties.keys().any(|property| {
            Axis::Horizontal.properties().contains(property)
                || Axis::Vertical.properties().contains(property)
        });
        return if has_size_properties {
            Metrics {
                x: axis.with_properties(node, Axis::Horizontal, reference),
                y: axis.with_properties(node, Axis::Vertical, reference),
            }
        } else {
            Metrics { x: axis, y: axis }
        };
    }
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
    let metrics = if node.kind == "StackLayout" {
        let padding = Padding::read(node);
        let inner = padding.inner_size(available);
        let mut x = AxisInfo::flexible(0.0);
        let mut y = AxisInfo::flexible(0.0);
        for child in node.children.iter().filter(|child| child.is_visible()) {
            let measured = measure(child, inner, text);
            x.min = x.min.max(measured.x.min);
            y.min = y.min.max(measured.y.min);
            x.preferred = x.preferred.max(measured.x.preferred);
            y.preferred = y.preferred.max(measured.y.preferred);
        }
        let padding = padding.size();
        x.min += padding.width;
        y.min += padding.height;
        x.preferred += padding.width;
        y.preferred += padding.height;
        Metrics { x, y }
    } else if matches!(node.kind, "Loader" | "LoaderCandidate") {
        let padding = Padding::read(node);
        let child = node.children.iter().find(|child| child.is_visible());
        let preferred = child.map_or(Size::default(), |child| {
            let metrics = measure(child, padding.inner_size(available), text);
            Size::new(metrics.x.preferred, metrics.y.preferred)
        });
        let padding = padding.size();
        Metrics {
            x: AxisInfo::flexible(preferred.width + padding.width),
            y: AxisInfo::flexible(preferred.height + padding.height),
        }
    } else if let Some(axis) = Axis::for_node(node) {
        let padding = Padding::read(node);
        let mut inner = padding.inner_size(available);
        let font_size = text.font_size(node);
        let group_title = crate::group_box::title_layout(
            node,
            available,
            node.layout_direction().unwrap_or_default(),
            text,
        )
        .map_or_else(
            || crate::group_box::title_height(node, font_size),
            |title| title.height,
        );
        let radio_title = crate::selection::radio_title_height(node, font_size);
        let title_height =
            radio_title + crate::modal::measured_title_height(node, available, text) + group_title;
        inner.height = (inner.height - title_height).max(0.0);
        let [
            mut main_min,
            mut main_preferred,
            mut main_max,
            mut main_stretch,
        ] = [0.0_f64; 4];
        let mut cross = AxisInfo::flexible(0.0);
        cross.stretch = f32::INFINITY;
        let mut count = 0usize;
        let children = node
            .children
            .iter()
            .filter(|child| linear_child(node, child));
        let remeasure = matches!(axis, Axis::Horizontal) && children.clone().any(reflows_at_width);
        let alignment = layout_alignment(node);
        let default_cross = cross_alignment(node, "cross-axis-alignment");
        let legacy = legacy_box(node.kind);
        let mut line_infos = Vec::new();
        for child in children.clone() {
            let metrics = measure(child, inner, text);
            let child_main = metrics.along(axis);
            let child_cross = metrics.along(axis.other());
            if remeasure {
                line_infos.push(main_info(child, axis, alignment, legacy, child_main));
            }
            main_min += f64::from(child_main.min);
            main_preferred += f64::from(child_main.preferred);
            main_max += f64::from(child_main.max);
            main_stretch += f64::from(child_main.stretch);
            cross.min = cross.min.max(child_cross.min);
            cross.preferred = cross.preferred.max(child_cross.preferred);
            let child_alignment = match cross_alignment(child, "cross-axis-self-alignment") {
                CrossAxisAlignment::Auto => default_cross,
                value => value,
            };
            if matches!(
                child_alignment,
                CrossAxisAlignment::Auto | CrossAxisAlignment::Stretch
            ) {
                cross.max = cross.max.min(child_cross.max);
            }
            cross.stretch = cross.stretch.min(child_cross.stretch);
            count += 1;
        }
        let gaps = f64::from(spacing(node)) * count.saturating_sub(1) as f64;
        let mut padding = padding.size();
        padding.height += title_height;
        let main_extra = gaps + f64::from(axis.extent(padding));
        let cross_extra = axis.other().extent(padding);
        let mut main = AxisInfo {
            min: (main_min + main_extra) as f32,
            preferred: (main_preferred + main_extra) as f32,
            max: (main_max + main_extra) as f32,
            stretch: main_stretch as f32,
        };
        cross.min += cross_extra;
        cross.preferred += cross_extra;
        cross.max += cross_extra;
        if group_title > 0.0 {
            let decoration = if node.group_checkable() || node.kind == "Expander" {
                31.0
            } else {
                0.0
            };
            let title_width = text
                .measure_node(
                    node,
                    node.text("title").unwrap_or(""),
                    (inner.width - decoration).max(0.0),
                )
                .width
                + decoration
                + cross_extra;
            cross.preferred = cross.preferred.max(title_width);
            cross.max = cross.max.max(cross.preferred);
        }
        if remeasure {
            // Wrapped items may need more height after their siblings take their widths.
            let (widths, _, _) = fit_padded_line(
                line_infos.into_iter(),
                available.width,
                padding.width,
                spacing(node),
                alignment,
            );
            let wrapped_height = children
                .zip(widths)
                .filter(|(child, _)| reflows_at_width(child))
                .map(|(child, width)| {
                    measure_with_width(child, inner, Some(width), text)
                        .y
                        .preferred
                })
                .fold(0.0_f32, f32::max);
            cross.preferred = cross.preferred.max(wrapped_height + cross_extra);
            cross.max = cross.max.max(wrapped_height + cross_extra);
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
        let mut metrics = match axis {
            Axis::Horizontal => Metrics { x: main, y: cross },
            Axis::Vertical => Metrics { x: cross, y: main },
        };
        if radio_title > 0.0 {
            let title_width = text
                .measure_node(node, node.text("title").unwrap_or(""), inner.width)
                .width
                + padding.width;
            metrics.x.preferred = metrics.x.preferred.max(title_width);
            metrics.x.max = metrics.x.max.max(metrics.x.preferred);
        }
        metrics
    } else if node.kind == "ScrollView" && node.boolean("__picker-viewport", false) {
        let content = node
            .children
            .first()
            .map(|child| measure(child, available, text));
        Metrics {
            x: AxisInfo::flexible(content.as_ref().map_or(0.0, |metrics| metrics.x.preferred)),
            y: AxisInfo::flexible(content.as_ref().map_or(0.0, |metrics| metrics.y.preferred)),
        }
    } else if flow::uses_flow(node) {
        crate::flow::measure(node, available, text)
    } else if node.kind == "GridLayout" {
        crate::grid::measure(node, available, text)
    } else if node.kind == "ToolSeparator" {
        let padding = dimension(node, "padding").unwrap_or(6.0);
        let thickness = dimension(node, "stroke-width").unwrap_or(1.0);
        let long = AxisInfo::flexible(28.0_f32.max(2.0 * padding + 1.0));
        let short = AxisInfo::fixed(12.0_f32.max(thickness));
        match node.orientation() {
            Orientation::Horizontal => Metrics { x: long, y: short },
            Orientation::Vertical => Metrics { x: short, y: long },
        }
    } else if is_pane(node.kind) {
        let padding = Padding::read(node);
        let mut inner = padding.inner_size(available);
        let content_width = dimension(node, "content-width");
        let content_height = dimension(node, "content-height");
        if assigned_width.is_none()
            && relative_width("width").is_none()
            && let Some(width) = content_width
        {
            inner.width = inner.width.min(width);
        }
        let mut natural = Size::default();
        for child in node
            .children
            .iter()
            .filter(|child| participates_in_flow(child))
        {
            let metrics = measure(child, inner, text);
            natural.width = natural
                .width
                .max(child.length("x").unwrap_or(0.0) + metrics.x.preferred);
            natural.height = natural
                .height
                .max(child.length("y").unwrap_or(0.0) + metrics.y.preferred);
        }
        let padding = padding.size();
        Metrics {
            x: AxisInfo::flexible(content_width.unwrap_or(natural.width) + padding.width),
            y: AxisInfo::flexible(content_height.unwrap_or(natural.height) + padding.height),
        }
    } else {
        let natural = match node.kind {
            "SwipeDelegate" => node
                .children
                .iter()
                .find(|child| child.kind == "SwipeContent")
                .map_or(Size::default(), |content| {
                    let metrics = measure(content, available, text);
                    Size::new(metrics.x.preferred, metrics.y.preferred)
                }),
            "Text" | "Label" | "StyledText" if crate::uses_label_layout(node) => {
                text.measure_label(node, available.width)
            }
            "Text" => text.measure_node(node, node.text("text").unwrap_or(""), available.width),
            kind if is_text_input(kind) => {
                let padding = Padding::for_editor(node).size();
                let width = if matches!(kind, "TextInput" | "LineEdit") {
                    168.0
                } else {
                    228.0
                };
                let line = text.font_size(node) * 1.4;
                let height = if is_multiline_text_input(kind) {
                    4.0 * line + padding.height
                } else {
                    (line + padding.height).max(34.0)
                };
                Size::new(width + padding.width, height)
            }
            "Slider" | "RangeSlider" if node.orientation() == Orientation::Vertical => {
                Size::new(28.0, 160.0)
            }
            "Slider" | "RangeSlider" => Size::new(160.0, 28.0),
            "ScrollBar" | "ScrollIndicator" if node.orientation() == Orientation::Vertical => {
                Size::new(12.0, 160.0)
            }
            "ScrollBar" | "ScrollIndicator" => Size::new(160.0, 12.0),
            "Dial" => Size::new(96.0, 96.0),
            "SpinBox" | "DoubleSpinBox" => {
                let editor_width = if node.kind == "SpinBox" { 88.0 } else { 120.0 };
                let stepper = crate::SpinGeometry::preferred_stepper_size(node);
                Size::new(
                    editor_width + stepper.width,
                    (text.font_size(node) * 1.4 + 10.0)
                        .max(34.0)
                        .max(stepper.height),
                )
            }
            "ComboBox" => crate::ComboGeometry::preferred_size(node, available, text),
            "Calendar" | "MonthGrid" | "DayOfWeekRow" | "WeekNumberColumn" => {
                crate::calendar::preferred_size(node, text)
            }
            "Tumbler" => crate::tumbler::preferred_size(node, text),
            "PathView" => Size::new(320.0, 160.0),
            "TabBar" | "TabWidget" => crate::selection::preferred_tab_view_size(node, text),
            "SwipeView" | "StackView" => Size::new(240.0, 136.0),
            "Page" => crate::page::preferred_size(node, available, text),
            "PageIndicator" => crate::PageIndicatorGeometry::preferred_size(node),
            "ListView" | "StandardListView" | "TreeView" => Size::new(180.0, 144.0),
            "GridView" => Size::new(320.0, 200.0),
            "TableView" => Size::new(280.0, 178.0),
            "SplitView" => Size::new(320.0, 180.0),
            "Menu" => Size::new(
                text.measure_node(node, node.text("title").unwrap_or(""), available.width)
                    .width
                    + 24.0,
                (text.font_size(node) * 1.4 + 8.0).max(32.0),
            ),
            "ProgressIndicator" => Size::new(160.0, 10.0),
            "BusyIndicator" | "Spinner" => Size::new(36.0, 36.0),
            "CheckBox" | "Switch" | "RadioButton" => {
                let indicator = crate::toggle_indicator_size(node);
                let decoration = indicator.map_or_else(
                    || if node.kind == "Switch" { 50.0 } else { 30.0 },
                    |size| size.width + if node.is_item_delegate() { 34.0 } else { 13.0 },
                );
                let label = text.measure_node(
                    node,
                    node.text("text").unwrap_or(""),
                    (available.width - decoration).max(0.0),
                );
                Size::new(
                    label.width + decoration,
                    label
                        .height
                        .max(indicator.map_or(26.0, |size| size.height.max(26.0))),
                )
            }
            "Button" | "ToolButton" | "RoundButton" | "DialogButton" | "DelayButton" => {
                let button = ButtonContent::new(node);
                let chrome = 2.0 * button.padding;
                let content = if let Some(content) = node.control_part("ButtonContent") {
                    let available = Size::new(
                        (available.width - chrome).max(0.0),
                        (available.height - chrome).max(0.0),
                    );
                    measure_replacement_content(content, available, text)
                } else {
                    let label = if button.label.is_empty() {
                        Size::default()
                    } else {
                        text.measure_node(
                            node,
                            button.label,
                            button.natural_label_width(available.width - chrome),
                        )
                    };
                    button.natural_size(label)
                };
                let min = match node.kind {
                    "ToolButton" => Size::new(34.0, 34.0),
                    "RoundButton" => Size::new(40.0, 40.0),
                    _ => Size::new(64.0, 40.0),
                };
                Size::new(
                    (content.width + chrome).max(min.width),
                    (content.height + chrome).max(min.height),
                )
            }
            "MediaPlayer" => Size::new(320.0, 180.0),
            "Image" | "AnimatedImage" => {
                let source = node.image_size("source");
                let [width, height] = crate::ImageRegion::for_node(node, source).size();
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
                "Text"
                    | "Label"
                    | "StyledText"
                    | "Button"
                    | "ToolButton"
                    | "RoundButton"
                    | "DialogButton"
                    | "DelayButton"
                    | "Menu"
            ) {
                preferred
            } else {
                0.0
            },
            stretch: if intrinsic_item(node.kind) { 0.0 } else { 1.0 },
            ..AxisInfo::flexible(preferred)
        };
        let mut x = info(natural.width);
        if crate::label::flexible_width(node) {
            x.min = 0.0;
        }
        Metrics {
            x,
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
        _ if node.kind == "DialogButtonBox" => LayoutAlignment::End,
        _ if matches!(
            node.kind,
            "MenuBar" | "FlowLayout" | "FlexboxLayout" | "ToolBar"
        ) =>
        {
            LayoutAlignment::Start
        }
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
    fit_padded_line(infos, available, 0.0, gap, alignment)
}

fn fit_padded_line(
    infos: impl ExactSizeIterator<Item = AxisInfo> + Clone,
    available: f32,
    padding: f32,
    gap: f32,
    alignment: LayoutAlignment,
) -> (Vec<f32>, f32, f32) {
    let count = infos.len() as f64;
    let preferred = infos
        .clone()
        .map(|item| f64::from(item.preferred))
        .sum::<f64>();
    let gaps = f64::from(gap) * (count - 1.0);
    // Measurement returns the natural extent rounded to f32. If that extent
    // fits, keep each preferred size instead of distributing the rounding
    // error to every child and invalidating their layouts.
    let padding = f64::from(padding);
    if available == (preferred + (gaps + padding)) as f32 {
        return (infos.map(|item| item.preferred).collect(), 0.0, gap);
    }
    let available = (f64::from(available) - padding).max(0.0);
    let slack = available - preferred;
    let free = slack - gaps;
    if alignment == LayoutAlignment::Stretch || free <= 0.0 {
        return (fit_axis(infos, (available - gaps) as f32), 0.0, gap);
    }
    let gap = f64::from(gap);
    let (offset, spacing) = match alignment {
        LayoutAlignment::Stretch | LayoutAlignment::Start => (0.0, gap),
        LayoutAlignment::Center => (free * 0.5, gap),
        LayoutAlignment::End => (free, gap),
        LayoutAlignment::SpaceBetween if count > 1.0 => (0.0, slack / (count - 1.0)),
        LayoutAlignment::SpaceBetween => (0.0, gap),
        LayoutAlignment::SpaceAround => (slack / (2.0 * count), slack / count),
        LayoutAlignment::SpaceEvenly => (slack / (count + 1.0), slack / (count + 1.0)),
    };
    (
        infos.map(|item| item.preferred).collect(),
        offset as f32,
        spacing as f32,
    )
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
    direction: xrack_runtime::LayoutDirection,
    text: &mut impl TextMeasurer,
) -> Vec<Rect> {
    let padding = Padding::read(node);
    let inner = padding.inner_rect(bounds);
    let available = Size::new(inner.width, inner.height);
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
            order: item_order(node, child),
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

    let (sizes, cursor, spacing) = fit_padded_line(
        items.iter().map(|item| item.main),
        axis.extent(Size::new(bounds.width, bounds.height)),
        axis.extent(padding.size()),
        gap,
        alignment,
    );
    let mut cursor = f64::from(cursor);
    for (item, size) in items.into_iter().zip(sizes) {
        let child = &node.children[item.index];
        let reflows = reflows_at_width(child) && size != measurement_width(child, available);
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
        let mut rect = crate::place_in_direction(
            axis.rect(inner, cursor as f32, offset, size, extent),
            inner,
            direction,
        );
        if legacy {
            rect.x += child.length("x").unwrap_or(0.0);
            rect.y += child.length("y").unwrap_or(0.0);
        }
        result[item.index] = rect;
        cursor += f64::from(size) + f64::from(spacing);
    }
    result
}

pub(crate) fn spacing(node: &Node) -> f32 {
    dimension(node, "spacing").unwrap_or(match node.kind {
        "ToolBar" => 6.0,
        "TimePicker" | "DialogButtonBox" | "PopupWindow" => 8.0,
        "GroupBox" | "Expander" => 8.0,
        kind if is_modal_overlay(kind) => 8.0,
        _ => 0.0,
    })
}

pub(crate) fn item_order(parent: &Node, child: &Node) -> i64 {
    child.int("layout-order").unwrap_or_else(|| {
        if parent.kind != "DialogButtonBox" {
            return 0;
        }
        match dialog_button_action(child) {
            Some(ControlAction::DialogCommand(DialogCommand::Help)) => 0,
            Some(ControlAction::DialogCommand(DialogCommand::Reset)) => 10,
            Some(ControlAction::DialogCommand(DialogCommand::Discard)) => 30,
            Some(ControlAction::DialogCommand(DialogCommand::Apply)) => 40,
            Some(ControlAction::Dialog(role)) => {
                let accept_first = cfg!(target_os = "windows");
                if (role == DialogRole::Accept) == accept_first {
                    50
                } else {
                    60
                }
            }
            _ => 20,
        }
    })
}

fn linear_child(parent: &Node, child: &Node) -> bool {
    if parent.expander_collapsed() {
        return false;
    }
    participates_in_flow(child)
        || (parent.kind == "MenuBar" && child.kind == "Menu" && child.is_visible())
}
