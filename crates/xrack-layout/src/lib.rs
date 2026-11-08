//! Logical-pixel layout shared by software rendering and input hit testing.
mod button;
mod calendar;
mod choice;
mod edit;
mod flow;
mod geometry;
mod grid;
mod history;
mod input;
mod linear;
mod menu;
mod modal;
mod page_indicator;
mod range;
mod scroll_bar;
mod scroll_gesture;
mod selection;
mod shortcut;
mod spin;
mod split;
mod stack_transition;
mod table;
mod text_area;
mod tooltip;
mod tree;
pub use button::ButtonContent;
pub use calendar::{CalendarDay, CalendarGeometry};
pub use choice::{ChoiceKey, ChoiceMenu, ComboCompletion, ComboGeometry};
pub use edit::{EditAction, TextEdit, TextPosition};
pub use geometry::{Point, Rect, Size};
pub use history::EditSource;
pub use input::{
    ActivationKey, ControlInput, Interaction, NavigationKey, RangeInput, SliderKey, SplitResize,
    TableSelectionKey, Target,
};
pub use menu::{MenuEntry, MenuKey, MenuPanel, MenuRow};
pub use modal::{dialog_title_height, drawer_grip};
pub use page_indicator::{PageIndicatorGeometry, PageMarker};
pub use range::{DialGeometry, SliderGeometry};
pub use scroll_bar::ScrollBarGeometry;
pub use scroll_gesture::{ScrollGestureEnd, ScrollGestureKind};
pub use selection::TAB_LABEL_SIZE;
pub use shortcut::{MenuShortcut, ShortcutKey, ShortcutModifiers};
pub use spin::SpinGeometry;
pub use split::{SplitGeometry, SplitHandle};
pub use stack_transition::{StackSlide, StackTransitions};
use std::{collections::BTreeMap, rc::Rc};
pub use table::TableGeometry;
pub use text_area::{TextAreaLayout, TextAreaMotion};
pub use tooltip::{TooltipPopup, layout_tooltip};
pub use tree::TreeRowGeometry;
use xrack_runtime::{
    Component, ListMetrics, Node, Orientation, SelectionTarget, group_index, is_modal_overlay,
};

#[cfg(test)]
mod tests;

pub const DEFAULT_FONT_SIZE: f32 = 16.0;

pub trait TextMeasurer {
    fn measure(&mut self, text: &str, font_size: f32, max_width: f32) -> Size;
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayoutNode {
    pub bounds: Rect,
    /// The ancestor clip. Children are clipped to this node's bounds as well.
    pub clip: Rect,
    pub visible: bool,
    pub enabled: bool,
    pub scroll: Option<ScrollLayout>,
    horizontal_scroll: Option<ScrollLayout>,
    pub table: Option<Box<TableGeometry>>,
    pub split: Option<Box<SplitGeometry>>,
    /// Header hit box of a tab, independent of the page visibility.
    pub tab_header: Option<Rect>,
    overlays: Option<Box<modal::OverlayIndex>>,
    pub children: Vec<LayoutNode>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollLayout {
    pub offset: f32,
    pub max_offset: f32,
}

impl ScrollLayout {
    pub(crate) fn new(requested: f32, max_offset: f32) -> Self {
        let max_offset = max_offset.max(0.0);
        Self {
            offset: requested.clamp(0.0, max_offset),
            max_offset,
        }
    }
}

impl LayoutNode {
    pub fn scroll_on(&self, orientation: Orientation) -> Option<ScrollLayout> {
        match orientation {
            Orientation::Horizontal => self
                .table
                .as_ref()
                .map(|table| table.horizontal)
                .or(self.horizontal_scroll),
            Orientation::Vertical => self.scroll,
        }
    }

    /// Paths of visible modal overlays, in declaration order.
    pub fn dialog_paths(&self) -> &[Vec<usize>] {
        self.overlays
            .as_ref()
            .map_or(&[], |overlays| overlays.open.as_slice())
    }

    pub(crate) fn drawer_paths(&self) -> &[Vec<usize>] {
        self.overlays
            .as_ref()
            .map_or(&[], |overlays| overlays.drawers.as_slice())
    }

    pub fn at_path(&self, path: &[usize]) -> Option<&Self> {
        let mut item = self;
        for &index in path {
            item = item.children.get(index)?;
        }
        Some(item)
    }
}

pub fn font_size(node: &Node) -> f32 {
    node.length("font-size")
        .unwrap_or(DEFAULT_FONT_SIZE)
        .clamp(1.0, 512.0)
}

fn dimension(node: &Node, name: &str) -> Option<f32> {
    node.length(name).map(|n| n.max(0.0))
}

fn participates_in_flow(node: &Node) -> bool {
    !is_modal_overlay(node.kind) && !matches!(node.kind, "Menu" | "Tooltip") && node.is_visible()
}

pub fn is_layout(kind: &str) -> bool {
    matches!(
        kind,
        "GridLayout" | "FlowLayout" | "ToolBar" | "Row" | "RadioGroup" | "SplitView"
    ) || linear::Axis::for_layout(kind).is_some()
}

/// Root dimensions come from the actual viewport; DSL dimensions size the initial window.
pub fn layout(scene: &Node, viewport: Size, text: &mut impl TextMeasurer) -> LayoutNode {
    layout_with_scroll(scene, viewport, text, &BTreeMap::new())
}

pub fn layout_with_scroll(
    scene: &Node,
    viewport: Size,
    text: &mut impl TextMeasurer,
    offsets: &BTreeMap<Vec<usize>, f32>,
) -> LayoutNode {
    layout_with_offsets(scene, viewport, text, offsets, None)
}

pub fn layout_with_input(
    scene: &Node,
    viewport: Size,
    text: &mut impl TextMeasurer,
    input: &Interaction,
) -> LayoutNode {
    layout_with_offsets(scene, viewport, text, input.scroll_offsets(), Some(input))
}

fn layout_with_offsets(
    scene: &Node,
    viewport: Size,
    text: &mut impl TextMeasurer,
    offsets: &BTreeMap<Vec<usize>, f32>,
    input: Option<&Interaction>,
) -> LayoutNode {
    let bounds = Rect::new(0.0, 0.0, viewport.width.max(0.0), viewport.height.max(0.0));
    let mut layout = LayoutContext {
        text,
        offsets,
        input,
        path: Vec::new(),
        viewport: bounds,
    }
    .place(scene, bounds, bounds, true, true, None);
    modal::index_dialogs(scene, &mut layout);
    layout
}

/// Resolve virtual rows after laying out the viewport containers. Row contents
/// have no effect on the fixed-height list's viewport or scroll extent.
pub fn prepare_scene(
    component: &impl Component,
    scene: Rc<Node>,
    viewport: Size,
    text: &mut impl TextMeasurer,
    offsets: &BTreeMap<Vec<usize>, f32>,
) -> (Rc<Node>, LayoutNode) {
    prepare_scene_with_offsets(component, scene, viewport, text, offsets, None)
}

pub fn prepare_scene_with_input(
    component: &impl Component,
    scene: Rc<Node>,
    viewport: Size,
    text: &mut impl TextMeasurer,
    input: &Interaction,
) -> (Rc<Node>, LayoutNode) {
    prepare_scene_with_offsets(
        component,
        scene,
        viewport,
        text,
        input.scroll_offsets(),
        Some(input),
    )
}

fn prepare_scene_with_offsets(
    component: &impl Component,
    mut scene: Rc<Node>,
    viewport: Size,
    text: &mut impl TextMeasurer,
    offsets: &BTreeMap<Vec<usize>, f32>,
    input: Option<&Interaction>,
) -> (Rc<Node>, LayoutNode) {
    fn has_lists(node: &Node) -> bool {
        node.list.is_some() || node.children.iter().any(has_lists)
    }

    let mut layout = layout_with_offsets(&scene, viewport, text, offsets, input);
    if has_lists(&scene) {
        let viewport = Rect::new(0.0, 0.0, viewport.width.max(0.0), viewport.height.max(0.0));
        LayoutContext {
            text,
            offsets,
            input,
            path: Vec::new(),
            viewport,
        }
        .populate_lists(component, Rc::make_mut(&mut scene), &mut layout);
        modal::index_dialogs(&scene, &mut layout);
    }
    (scene, layout)
}

struct LayoutContext<'a, T> {
    text: &'a mut T,
    offsets: &'a BTreeMap<Vec<usize>, f32>,
    input: Option<&'a Interaction>,
    path: Vec<usize>,
    viewport: Rect,
}

impl<T: TextMeasurer> LayoutContext<'_, T> {
    fn requested_scroll(&self, orientation: Orientation) -> f32 {
        self.scroll_request(orientation).unwrap_or_default()
    }

    fn scroll_request(&self, orientation: Orientation) -> Option<f32> {
        let offsets = match orientation {
            Orientation::Horizontal => self.input.map(Interaction::horizontal_scroll_offsets),
            Orientation::Vertical => Some(self.offsets),
        };
        offsets.and_then(|offsets| offsets.get(&self.path)).copied()
    }

    fn populate_lists(
        &mut self,
        component: &impl Component,
        node: &mut Node,
        layout: &mut LayoutNode,
    ) {
        if let Some(mut list) = node.list {
            let row_height = f64::from(ListMetrics::row_height(node));
            let viewport = layout
                .table
                .as_ref()
                .map_or(layout.bounds, |table| table.body);
            let clipped = viewport.intersect(layout.clip);
            let range = if let Some(scroll) = layout.scroll
                && clipped.width > 0.0
                && clipped.height > 0.0
            {
                let top = f64::from(scroll.offset) + f64::from(clipped.y - viewport.y);
                let bottom = top + f64::from(clipped.height);
                let first = (top / row_height).floor() as usize;
                let end = (bottom / row_height).ceil() as usize;
                first.min(list.row_count)..end.min(list.row_count)
            } else {
                0..0
            };
            list.first_row = range.start;
            let selected = ListMetrics::selection(node);
            let selection = node.table_selection.as_ref();
            let tree = node.tree.as_ref();
            let tree_selection = node.tree_selection.as_ref();
            let indentation = node.length("indentation").unwrap_or(16.0).clamp(0.0, 64.0);
            node.children = component
                .list_rows(&self.path, range)
                .into_iter()
                .enumerate()
                .map(|(slot, mut delegate)| {
                    let mut row = Node::new("ListRow", None);
                    row.row_key = delegate.row_key.take();
                    let tree_item = tree.and_then(|tree| tree.rows().get(list.first_row + slot));
                    if let Some(item) = tree_item {
                        row.properties
                            .insert("tree-depth", (item.depth as i64).into());
                        row.properties
                            .insert("tree-has-children", item.has_children.into());
                        row.properties.insert("tree-expanded", item.expanded.into());
                        row.properties.insert(
                            "tree-indentation",
                            xrack_runtime::Length(f64::from(indentation)).into(),
                        );
                    }
                    let current = selected == Some((list.first_row + slot) as i64);
                    let grid = selection.filter(|view| view.target() != SelectionTarget::Rows);
                    if let (Some(view), Some(key)) = (grid, row.row_key) {
                        for (column, cell) in delegate.children.iter_mut().enumerate() {
                            if view.contains_cell(key, column) {
                                cell.properties.insert("table-selected", true.into());
                            }
                            if current && view.current_column() == Some(column) {
                                cell.properties.insert("table-current", true.into());
                            }
                        }
                    }
                    let is_selected = if let Some(keys) = tree_selection {
                        tree_item.is_some_and(|item| {
                            row.row_key == Some(item.row) && keys.get(&item.key) == Some(&item.row)
                        })
                    } else {
                        selection.map_or(current, |view| {
                            row.row_key.is_some_and(|key| view.keys().contains(&key))
                        })
                    };
                    row.properties
                        .insert("selected", xrack_runtime::Value::Bool(is_selected));
                    if (tree_selection.is_some() || selection.is_some() && grid.is_none())
                        && current
                    {
                        row.properties
                            .insert("current", xrack_runtime::Value::Bool(true));
                    }
                    row.children.push(delegate);
                    row
                })
                .collect();
            node.list = Some(list);
            *layout = self.place(
                node,
                layout.bounds,
                layout.clip,
                layout.visible,
                layout.enabled,
                None,
            );
        } else {
            for (index, (child, item)) in node
                .children
                .iter_mut()
                .zip(&mut layout.children)
                .enumerate()
            {
                self.path.push(index);
                self.populate_lists(component, child, item);
                self.path.pop();
            }
        }
    }

    fn place(
        &mut self,
        node: &Node,
        bounds: Rect,
        clip: Rect,
        parent_visible: bool,
        parent_enabled: bool,
        row_children: Option<Vec<Rect>>,
    ) -> LayoutNode {
        let visible = parent_visible && node.is_visible();
        let (bounds, clip) = if visible && is_modal_overlay(node.kind) {
            (modal::bounds(node, self.viewport, self.text), self.viewport)
        } else {
            (bounds, clip)
        };
        let enabled = parent_enabled && node.boolean("enabled", true);
        if matches!(node.kind, "Menu" | "Tooltip") {
            // Popup entries use window coordinates and are laid out by the menu presenter.
            return LayoutNode {
                bounds,
                clip,
                visible: visible && node.kind != "Tooltip",
                enabled,
                scroll: None,
                horizontal_scroll: None,
                table: None,
                split: None,
                tab_header: None,
                overlays: None,
                children: Vec::new(),
            };
        }
        let table = if visible && node.table_header.is_some() {
            TableGeometry::with_offset(node, bounds, self.requested_scroll(Orientation::Horizontal))
                .map(Box::new)
        } else {
            None
        };
        let viewport = table.as_ref().map_or(bounds, |table| table.body);
        let child_clip = clip.intersect(viewport);
        if matches!(node.kind, "TabWidget" | "SwipeView" | "StackView") && visible {
            let tabs = (node.kind == "TabWidget")
                .then(|| selection::TabGeometry::new(bounds, node.orientation()));
            let page_bounds = tabs.as_ref().map_or(bounds, |geometry| geometry.content);
            let selected = if node.kind == "StackView" {
                node.stack_page_index()
            } else {
                group_index(node)
            };
            let count = node
                .children
                .iter()
                .filter(|tab| tab.boolean("visible", true))
                .count();
            let mut header_index = 0;
            let children = node
                .children
                .iter()
                .enumerate()
                .map(|(index, tab)| {
                    self.path.push(index);
                    let mut page = self.place(
                        tab,
                        page_bounds,
                        child_clip,
                        selected == Some(index),
                        enabled,
                        None,
                    );
                    self.path.pop();
                    if let Some(geometry) = &tabs
                        && tab.boolean("visible", true)
                    {
                        page.tab_header = Some(geometry.header(header_index, count));
                        header_index += 1;
                    }
                    page
                })
                .collect();
            return LayoutNode {
                bounds,
                clip,
                visible,
                enabled,
                scroll: None,
                horizontal_scroll: None,
                table: None,
                split: None,
                tab_header: None,
                overlays: None,
                children,
            };
        }
        let mut scroll = None;
        let mut horizontal_scroll = None;
        let (split, split_rects) = if visible && node.kind == "SplitView" {
            let (geometry, rects) = SplitGeometry::new(node, bounds, self.text);
            (Some(Box::new(geometry)), Some(rects))
        } else {
            (None, None)
        };
        let mut row_children = row_children;
        let placements = if !visible {
            (0..node.children.len())
                .map(|_| grid::Placement {
                    bounds: Rect::new(bounds.x, bounds.y, 0.0, 0.0),
                    row_children: None,
                })
                .collect()
        } else if let Some(rects) = split_rects {
            rects
                .into_iter()
                .map(|bounds| grid::Placement {
                    bounds,
                    row_children: None,
                })
                .collect()
        } else if let Some(list) = node.list {
            let height = ListMetrics::row_height(node);
            let axis = ScrollLayout::new(
                self.requested_scroll(Orientation::Vertical),
                list.row_count as f32 * height - viewport.height,
            );
            scroll = Some(axis);
            node.children
                .iter()
                .enumerate()
                .map(|(slot, _)| {
                    let y = ((list.first_row + slot) as f64 * f64::from(height)
                        - f64::from(axis.offset)) as f32;
                    let y = viewport.y + y;
                    grid::Placement {
                        bounds: Rect::new(viewport.x, y, viewport.width, height),
                        row_children: table.as_ref().map(|table| {
                            table
                                .columns
                                .iter()
                                .map(|column| Rect::new(column.x, y, column.width, height))
                                .collect()
                        }),
                    }
                })
                .collect()
        } else if node.kind == "RangeSlider" {
            let geometry = SliderGeometry::new(bounds, node.orientation());
            let fractions = xrack_runtime::SliderInterval::from_node(node).fractions();
            node.children
                .iter()
                .enumerate()
                .map(|(index, _)| {
                    let bounds = fractions.get(index).map_or(Rect::default(), |&fraction| {
                        let center = geometry.position(fraction);
                        let radius = geometry.radius;
                        Rect::new(
                            center.x - radius,
                            center.y - radius,
                            radius * 2.0,
                            radius * 2.0,
                        )
                    });
                    grid::Placement {
                        bounds,
                        row_children: None,
                    }
                })
                .collect()
        } else if node.kind == "ListRow" {
            let content = TreeRowGeometry::new(node, bounds).map_or(bounds, |tree| tree.content);
            node.children
                .iter()
                .map(|_| grid::Placement {
                    bounds: content,
                    row_children: row_children.take(),
                })
                .collect()
        } else if node.kind == "GridLayout" {
            grid::children(node, bounds, self.text)
        } else if node.kind == "TabBar" {
            let orientation = node.orientation();
            let requested = self.scroll_request(orientation);
            let reveal = requested.is_none()
                || self
                    .input
                    .is_some_and(|input| input.tab_selection_changed(node, &self.path));
            let (rects, axis) = selection::tab_bar(
                node,
                bounds,
                self.text,
                requested.unwrap_or_default(),
                reveal,
            );
            match orientation {
                Orientation::Horizontal => horizontal_scroll = Some(axis),
                Orientation::Vertical => scroll = Some(axis),
            }
            rects
                .into_iter()
                .map(|bounds| grid::Placement {
                    bounds,
                    row_children: None,
                })
                .collect()
        } else if node.kind == "ScrollView" {
            let (rects, x, y) = self.scroll_view_children(node, bounds);
            horizontal_scroll = Some(x);
            scroll = Some(y);
            rects
                .into_iter()
                .map(|bounds| grid::Placement {
                    bounds,
                    row_children: None,
                })
                .collect()
        } else {
            let mut content = bounds;
            let title = (selection::radio_title_height(node) + dialog_title_height(node))
                .min(bounds.height);
            content.y += title;
            content.height -= title;
            let rects = if let Some(placements) = row_children {
                placements
            } else if flow::wraps(node) {
                flow::children(node, content, self.text)
            } else if let Some(axis) = linear::Axis::for_node(node) {
                linear::children(node, axis, content, self.text)
            } else {
                self.absolute_children(node, content)
            };
            rects
                .into_iter()
                .map(|bounds| grid::Placement {
                    bounds,
                    row_children: None,
                })
                .collect()
        };
        let children = node
            .children
            .iter()
            .zip(placements)
            .enumerate()
            .map(|(index, (child, placement))| {
                self.path.push(index);
                let is_handle = child.kind == "SplitHandle";
                let mut child = self.place(
                    child,
                    placement.bounds,
                    child_clip,
                    visible,
                    enabled,
                    placement.row_children,
                );
                if is_handle {
                    child.enabled &= split
                        .as_ref()
                        .and_then(|geometry| geometry.handle(index))
                        .is_some_and(SplitHandle::adjustable);
                }
                self.path.pop();
                child
            })
            .collect();
        LayoutNode {
            bounds,
            clip,
            visible,
            enabled,
            scroll,
            horizontal_scroll,
            table,
            split,
            tab_header: None,
            overlays: None,
            children,
        }
    }

    fn scroll_view_children(
        &mut self,
        node: &Node,
        viewport: Rect,
    ) -> (Vec<Rect>, ScrollLayout, ScrollLayout) {
        // Keep percentage constraints tied to the viewport or explicit content size,
        // so an overflowing child cannot enlarge its own percentage reference.
        let reference = Size::new(
            viewport.width.max(
                node.relative_length("content-width", viewport.width)
                    .unwrap_or(0.0),
            ),
            viewport.height.max(
                node.relative_length("content-height", viewport.height)
                    .or_else(|| node.length("viewport-height"))
                    .unwrap_or(0.0),
            ),
        );
        let mut extent = reference;
        let mut children = Vec::with_capacity(node.children.len());
        for child in &node.children {
            if !participates_in_flow(child) {
                children.push(None);
                continue;
            }
            let x = child.length("x").unwrap_or(0.0);
            let y = child.length("y").unwrap_or(0.0);
            let metrics = linear::measure(child, reference, self.text);
            let width = absolute_width(child, metrics.x, (reference.width - x.max(0.0)).max(0.0));
            let height = linear::measure_with_width(child, reference, Some(width), self.text).y;
            extent.width = extent.width.max(x + width);
            extent.height = extent.height.max(y + height.preferred);
            children.push(Some((x, y, width, height)));
        }
        let horizontal = ScrollLayout::new(
            self.requested_scroll(Orientation::Horizontal),
            extent.width - viewport.width,
        );
        let vertical = ScrollLayout::new(
            self.requested_scroll(Orientation::Vertical),
            extent.height - viewport.height,
        );
        let origin = Point {
            x: viewport.x - horizontal.offset,
            y: viewport.y - vertical.offset,
        };
        let rects = node
            .children
            .iter()
            .zip(children)
            .map(|(child, placement)| {
                let Some((x, y, width, height)) = placement else {
                    return Rect::new(origin.x, origin.y, 0.0, 0.0);
                };
                let height = absolute_height(child, height, (extent.height - y.max(0.0)).max(0.0));
                Rect::new(origin.x + x, origin.y + y, width, height)
            })
            .collect();
        (rects, horizontal, vertical)
    }

    fn absolute_children(&mut self, node: &Node, content: Rect) -> Vec<Rect> {
        let available = Size::new(content.width, content.height);
        node.children
            .iter()
            .map(|child| {
                if !participates_in_flow(child) {
                    return Rect::new(content.x, content.y, 0.0, 0.0);
                }
                let metrics = linear::measure(child, available, self.text);
                let width = absolute_width(child, metrics.x, content.width);
                let height = absolute_height(child, metrics.y, content.height);
                Rect::new(
                    content.x + child.length("x").unwrap_or(0.0),
                    content.y + child.length("y").unwrap_or(0.0),
                    width,
                    height,
                )
            })
            .collect()
    }
}

fn absolute_width(node: &Node, info: linear::AxisInfo, available: f32) -> f32 {
    if xrack_runtime::is_button(node.kind)
        || node.kind == "Image"
        || node.properties.contains_key("preferred-width")
    {
        info.preferred
    } else {
        info.clamp(available)
    }
}

fn absolute_height(node: &Node, info: linear::AxisInfo, available: f32) -> f32 {
    if linear::intrinsic_item(node.kind) || node.properties.contains_key("preferred-height") {
        info.preferred
    } else {
        info.clamp(available)
    }
}
