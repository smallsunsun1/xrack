//! Logical-pixel layout shared by software rendering and input hit testing.
mod button;
mod calendar;
mod choice;
mod clip;
mod delay_button;
mod drag;
mod edit;
mod file_drop;
mod flow;
mod focus_observers;
mod focus_scope;
mod geometry;
mod grid;
mod group_box;
mod history;
mod image;
mod input;
mod keyboard;
mod label;
mod linear;
mod list_anchor;
mod menu;
mod modal;
mod page;
mod page_indicator;
mod password;
mod popup;
mod range;
mod scroll_bar;
mod scroll_gesture;
mod search;
mod selection;
mod shortcut;
mod spin;
mod split;
mod stack_transition;
mod styled_text;
mod swipe_delegate;
mod table;
mod text_area;
mod tooltip;
mod transform;
mod tree;
mod tumbler;
mod view_cells;
mod viewport;
mod visual_text;
mod window_chrome;
pub use button::ButtonContent;
pub use calendar::{CalendarDay, CalendarGeometry};
pub use choice::{ChoiceKey, ChoiceMenu, ComboCompletion, ComboGeometry};
use clip::ClipChain;
pub use clip::{RoundedClip, corner_radii, normalize_radii};
pub use drag::{DragDispatch, DragUpdate};
pub use edit::{EditAction, TextEdit, TextPosition};
pub use file_drop::{FileDragInput, FileDropScope};
pub use focus_observers::{FocusDelivery, FocusObservers};
pub use geometry::{Point, Rect, Size};
pub use group_box::group_header;
pub use history::EditSource;
pub use image::{ImagePlacement, ImagePresentation, ImageRegion, fitted_rect, tile_extent};
pub use input::{
    ActivationKey, ControlInput, Interaction, NavigationKey, RangeInput, SliderKey, SplitResize,
    TablePasteDestination, TableSelectionKey, Target, TextLinkTarget,
};
pub use keyboard::KeyRoute;
pub use label::{LabelLayout, LabelLine, LabelPlan, PlannedLabelLine, uses_label_layout};
pub use menu::{MenuEntry, MenuKey, MenuPanel, MenuRow};
pub use modal::{dialog_title_height, drawer_grip};
pub use page_indicator::{PageIndicatorGeometry, PageMarker};
pub use password::PasswordMask;
pub use range::{DialGeometry, SliderGeometry};
pub use scroll_bar::ScrollBarGeometry;
pub use scroll_gesture::{ScrollGestureEnd, ScrollGestureKind};
pub use search::SearchGeometry;
pub use selection::TAB_LABEL_SIZE;
pub use shortcut::{MenuShortcut, ShortcutKey, ShortcutModifiers, ShortcutOutcome};
pub use spin::SpinGeometry;
pub use split::{SplitGeometry, SplitHandle};
pub use stack_transition::{StackSlide, StackTransitions};
use std::{collections::BTreeMap, rc::Rc, sync::Arc};
pub use styled_text::{StyledFragment, StyledTextLayout};
pub use swipe_delegate::SwipeDelegateGeometry;
pub use table::TableGeometry;
pub use text_area::{TextAreaLayout, TextAreaMotion};
pub use tooltip::{TooltipPopup, layout_tooltip};
pub use transform::Affine;
use transform::TransformSpace;
pub use tree::TreeRowGeometry;
pub use tumbler::{TumblerGeometry, TumblerRow, TumblerScroll};
use view_cells::ViewCells;
pub use viewport::{LayoutObservers, ScrollDelivery, ViewportObservers};
pub use visual_text::{VisualGrapheme, VisualTextLine};
pub use window_chrome::WindowResizeEdge;
use xrack_runtime::{
    Component, ListMetrics, Node, Orientation, RowKey, SelectionTarget, group_index,
    is_modal_overlay, is_window_overlay,
};

#[cfg(test)]
mod tests;

pub const DEFAULT_FONT_SIZE: f32 = 16.0;

pub trait TextMeasurer {
    fn measure(&mut self, text: &str, font_size: f32, max_width: f32) -> Size;

    fn begin_window(&mut self, _scene: &Node) {}

    fn default_font_size(&self) -> Option<f32> {
        None
    }

    fn font_size(&self, node: &Node) -> f32 {
        node.length("font-size")
            .or_else(|| self.default_font_size())
            .unwrap_or(DEFAULT_FONT_SIZE)
            .clamp(1.0, 512.0)
    }

    fn measure_node(&mut self, node: &Node, text: &str, max_width: f32) -> Size {
        self.measure(text, self.font_size(node), max_width)
    }

    fn layout_styled_text(&mut self, node: &Node, available: Size) -> Option<StyledTextLayout> {
        let document = node.styled_text("text")?;
        let size = self.font_size(node);
        Some(StyledTextLayout::new(
            node,
            document,
            available,
            size,
            |text, _| self.measure_node(node, text, f32::MAX).width,
        ))
    }

    fn measure_label(&mut self, node: &Node, max_width: f32) -> Size {
        if let Some(plan) = self.layout_styled_text(node, Size::new(max_width, f32::INFINITY)) {
            return plan.size;
        }
        LabelLayout::with_font_size(
            node,
            Size::new(max_width, f32::INFINITY),
            self.font_size(node),
            |value| self.measure_node(node, value, f32::MAX).width,
        )
        .size
    }
}

#[derive(Clone, Debug, PartialEq)]
struct TransformPlacement {
    local: Affine,
    parent_clip: Rect,
    parent_space: Option<Arc<TransformSpace>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayoutNode {
    pub bounds: Rect,
    /// Rectangular part of the ancestor clip.
    pub clip: Rect,
    pub visible: bool,
    pub enabled: bool,
    pub scroll: Option<ScrollLayout>,
    horizontal_scroll: Option<ScrollLayout>,
    viewport_handle: Option<xrack_runtime::ViewportHandle>,
    mouse_drag_pan: bool,
    pub table: Option<Box<TableGeometry>>,
    pub split: Option<Box<SplitGeometry>>,
    /// Header hit box of a tab, independent of the page visibility.
    pub tab_header: Option<Rect>,
    pub tumbler: Option<Box<TumblerGeometry>>,
    pub swipe_delegate: Option<Box<SwipeDelegateGeometry>>,
    pub styled_text: Option<Box<StyledTextLayout>>,
    font_size: f32,
    overlays: Option<Box<modal::OverlayIndex>>,
    paint_order: Option<Box<[usize]>>,
    overflow: bool,
    child_shape: Option<RoundedClip>,
    ancestor_shapes: Option<Arc<ClipChain>>,
    space: Option<Arc<TransformSpace>>,
    transform: Option<Box<TransformPlacement>>,
    table_pane: Option<Box<(Rect, Affine)>>,
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

    pub(crate) fn revealing(self, top: f32, height: f32, viewport: f32) -> Self {
        let offset = if top < self.offset {
            top
        } else {
            self.offset.max(top + height - viewport)
        };
        Self::new(offset, self.max_offset)
    }
}

impl LayoutNode {
    pub fn allows_overflow(&self) -> bool {
        self.overflow
    }

    pub fn child_clip_shape(&self) -> Option<RoundedClip> {
        self.child_shape
    }

    pub fn local_transform(&self) -> Affine {
        self.transform
            .as_deref()
            .map_or(Affine::IDENTITY, |transform| transform.local)
    }

    /// An exact table pane clip, expressed in the parent space before this item's transform.
    pub fn table_pane_clip(&self) -> Option<(Rect, Affine)> {
        self.table_pane.as_deref().copied()
    }

    pub fn world_transform(&self) -> Affine {
        self.space
            .as_deref()
            .map_or(Affine::IDENTITY, |space| space.world)
    }

    fn inverse_transform(&self) -> Option<Affine> {
        self.space
            .as_deref()
            .map_or(Some(Affine::IDENTITY), |space| space.inverse)
    }

    /// Undo the visual transform, returning pre-transform layout coordinates.
    pub fn map_from_window(&self, point: Point) -> Option<Point> {
        Some(self.inverse_transform()?.map_point(point))
    }

    pub fn map_delta_from_window(&self, delta: Point) -> Option<Point> {
        Some(self.inverse_transform()?.map_vector(delta))
    }

    pub fn map_to_window(&self, point: Point) -> Point {
        self.world_transform().map_point(point)
    }

    pub fn window_bounds(&self, rect: Rect) -> Rect {
        self.world_transform().map_rect(rect)
    }

    /// Rectangular projection of the clipping region in layout coordinates.
    pub fn clip_rect(&self, rect: Rect) -> Rect {
        let rect = self.clip.intersect(rect);
        let Some(inverse) = self.inverse_transform() else {
            return Rect::default();
        };
        self.ancestor_shapes.as_deref().map_or(rect, |chain| {
            chain.clip_rect(rect, self.world_transform(), inverse)
        })
    }

    pub fn visible_window_bounds(&self, rect: Rect) -> Rect {
        let rect = self.window_bounds(self.clip_rect(rect));
        // The local projection can overestimate after a rotation; bound it
        // once more in window space for anchors and native accessibility.
        self.ancestor_shapes.as_deref().map_or(rect, |chain| {
            chain.clip_rect(rect, Affine::IDENTITY, Affine::IDENTITY)
        })
    }

    pub fn contains_clip_point(&self, point: Point) -> bool {
        self.map_from_window(point)
            .is_some_and(|local| self.clip.contains(local))
            && self
                .ancestor_shapes
                .as_deref()
                .is_none_or(|chain| chain.contains(point))
    }

    pub(crate) fn contains_subtree_point(&self, point: Point) -> bool {
        self.visible
            && self.contains_clip_point(point)
            && (self.overflow
                || self
                    .map_from_window(point)
                    .is_some_and(|local| self.bounds.contains(local)))
    }

    /// Back to front. Scene indices stay in declaration order for routing.
    pub fn child_paint_order(&self) -> impl DoubleEndedIterator<Item = usize> + '_ {
        (0..self.children.len()).map(|index| {
            self.paint_order
                .as_deref()
                .map_or(index, |order| order[index])
        })
    }

    pub fn children_in_paint_order<'a>(
        &'a self,
        node: &'a Node,
    ) -> impl DoubleEndedIterator<Item = (usize, &'a Node, &'a LayoutNode)> + 'a {
        self.child_paint_order().filter_map(move |index| {
            Some((index, node.children.get(index)?, &self.children[index]))
        })
    }

    pub fn text_link_regions(&self) -> impl Iterator<Item = (usize, Rect)> + '_ {
        self.styled_text
            .iter()
            .flat_map(|plan| &plan.fragments)
            .filter_map(|fragment| {
                let link = fragment.link()?;
                let rect = fragment.bounds;
                let visible = self.clip_rect(
                    Rect::new(
                        self.bounds.x + rect.x,
                        self.bounds.y + rect.y,
                        rect.width,
                        rect.height,
                    )
                    .intersect(self.bounds),
                );
                (self.visible && visible.width > 0.0 && visible.height > 0.0)
                    .then_some((link, visible))
            })
    }

    pub fn group_header(&self, node: &Node) -> Rect {
        group_box::header_with_font_size(node, self.bounds, self.font_size)
    }

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
            .map_or(&[], |overlays| overlays.modal.as_slice())
    }

    /// Paths of all open overlays, in declaration order.
    pub fn overlay_paths(&self) -> &[Vec<usize>] {
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

fn sibling_paint_order(children: &[Node]) -> Option<Box<[usize]>> {
    // Already ordered (including the default of all zeroes) needs no storage.
    if !children.windows(2).any(|pair| pair[0].z() > pair[1].z()) {
        return None;
    }
    let mut order: Vec<_> = (0..children.len()).collect();
    order.sort_by(|&left, &right| children[left].z().total_cmp(&children[right].z()));
    Some(order.into_boxed_slice())
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
    !is_window_overlay(node.kind)
        && !matches!(node.kind, "Menu" | "Tooltip" | "Shortcut")
        && node.is_visible()
}

pub fn is_layout(kind: &str) -> bool {
    matches!(
        kind,
        "GridLayout"
            | "FlowLayout"
            | "FlexboxLayout"
            | "ToolBar"
            | "Row"
            | "RadioGroup"
            | "SplitView"
    ) || linear::Axis::for_layout(kind).is_some()
}

/// Child indices in visual reading order. Paths keep their original scene indices.
pub fn child_reading_order(node: &Node) -> impl Iterator<Item = usize> {
    let (header, footer) = page::slots(node);
    let columns = (node.kind == "TableRow")
        .then(|| node.table_column_order.as_slice(node.children.len()))
        .flatten();
    let (ordered, len) = if columns.is_some() {
        (Vec::new(), 0)
    } else if matches!(node.kind, "DialogButtonBox" | "FlexboxLayout") {
        let mut ordered = (0..node.children.len()).collect::<Vec<_>>();
        ordered.sort_by_key(|&index| linear::item_order(node, &node.children[index]));
        (ordered, 0)
    } else {
        (Vec::new(), node.children.len())
    };
    ordered
        .into_iter()
        .chain(columns.into_iter().flatten().copied())
        .chain(header)
        .chain((0..len).filter(move |&index| Some(index) != header && Some(index) != footer))
        .chain(footer)
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
    text.begin_window(scene);
    let bounds = Rect::new(0.0, 0.0, viewport.width.max(0.0), viewport.height.max(0.0));
    let mut layout = LayoutContext {
        text,
        offsets,
        input,
        path: Vec::new(),
        row_key: None,
        anchor: None,
        resolved_list_scroll: None,
        ancestor_shapes: None,
        space: None,
        viewport: bounds,
    }
    .place(scene, bounds, bounds, true, true, None);
    modal::index_overlays(scene, &mut layout);
    layout
}

/// Resolve virtual rows after laying out the viewport containers. Row contents
/// have no effect on the indexed row heights or scroll extent.
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
            row_key: None,
            anchor: None,
            resolved_list_scroll: None,
            ancestor_shapes: None,
            space: None,
            viewport,
        }
        .populate_lists(component, Rc::make_mut(&mut scene), &mut layout);
        modal::index_overlays(&scene, &mut layout);
    }
    (scene, layout)
}

struct LayoutContext<'a, T> {
    text: &'a mut T,
    offsets: &'a BTreeMap<Vec<usize>, f32>,
    input: Option<&'a Interaction>,
    path: Vec<usize>,
    row_key: Option<RowKey>,
    anchor: Option<Rect>,
    resolved_list_scroll: Option<ScrollLayout>,
    ancestor_shapes: Option<Arc<ClipChain>>,
    space: Option<Arc<TransformSpace>>,
    viewport: Rect,
}

impl<T: TextMeasurer> LayoutContext<'_, T> {
    fn requested_scroll(&self, node: &Node, orientation: Orientation) -> f32 {
        self.scroll_request(node, orientation).unwrap_or_default()
    }

    fn scroll_request(&self, node: &Node, orientation: Orientation) -> Option<f32> {
        if let Some(offset) = node
            .viewport_handle
            .as_ref()
            .and_then(|handle| handle.bound_offset(orientation))
        {
            return Some(offset);
        }
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
            let viewport = layout
                .table
                .as_ref()
                .map_or(layout.bounds, |table| table.body);
            let clipped = layout.clip_rect(viewport);
            let range = if let Some(scroll) = layout.scroll
                && clipped.width > 0.0
                && clipped.height > 0.0
            {
                let top = f64::from(scroll.offset) + f64::from(clipped.y - viewport.y);
                let bottom = top + f64::from(clipped.height);
                ViewCells::new(node, viewport.width).visible(top, bottom, list.row_count)
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
                    if delegate.kind == "TableRow" {
                        delegate.table_column_order = node.table_column_order.clone();
                    }
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
                    if (node.kind == "GridView"
                        || tree_selection.is_some()
                        || selection.is_some() && grid.is_none())
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
            let (incoming_clip, incoming_shapes, incoming_space) =
                if let Some(placement) = &layout.transform {
                    (
                        placement.parent_clip,
                        layout
                            .ancestor_shapes
                            .as_ref()
                            .and_then(|shape| shape.parent.clone()),
                        placement.parent_space.clone(),
                    )
                } else {
                    (
                        layout.clip,
                        layout.ancestor_shapes.clone(),
                        layout.space.clone(),
                    )
                };
            let shapes = std::mem::replace(&mut self.ancestor_shapes, incoming_shapes);
            let space = std::mem::replace(&mut self.space, incoming_space);
            // Materializing row contents leaves the resolved scroll geometry intact.
            self.resolved_list_scroll = layout.scroll;
            *layout = self.place(
                node,
                layout.bounds,
                incoming_clip,
                layout.visible,
                layout.enabled,
                None,
            );
            self.ancestor_shapes = shapes;
            self.space = space;
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
        row_children: Option<Vec<grid::CellPlacement>>,
    ) -> LayoutNode {
        let outer_key = self.row_key;
        let outer_anchor = self.anchor;
        let outer_shapes = self.ancestor_shapes.clone();
        let outer_space = self.space.clone();
        self.row_key = node.row_key.or(outer_key);
        let layout = self.place_node(
            node,
            bounds,
            clip,
            parent_visible,
            parent_enabled,
            row_children,
        );
        self.row_key = outer_key;
        self.anchor = outer_anchor;
        self.ancestor_shapes = outer_shapes;
        self.space = outer_space;
        layout
    }

    fn place_node(
        &mut self,
        node: &Node,
        bounds: Rect,
        clip: Rect,
        parent_visible: bool,
        parent_enabled: bool,
        row_children: Option<Vec<grid::CellPlacement>>,
    ) -> LayoutNode {
        let resolved_list_scroll = self.resolved_list_scroll.take();
        let font_size = self.text.font_size(node);
        let enabled = parent_enabled && node.enables_children();
        let mut visible = parent_visible
            && node.is_visible()
            && (node.kind != "PopupWindow" || enabled && self.anchor.is_some())
            && xrack_runtime::indicator_state(node) != Some(xrack_runtime::IndicatorState::Hidden);
        let (bounds, clip) = if node.kind == "Shortcut" {
            (Rect::new(bounds.x, bounds.y, 0.0, 0.0), clip)
        } else if visible
            && node.kind == "PopupWindow"
            && let Some(anchor) = self.anchor
        {
            (
                popup::bounds(node, anchor, self.viewport, self.text),
                self.viewport,
            )
        } else if visible && is_modal_overlay(node.kind) {
            (modal::bounds(node, self.viewport, self.text), self.viewport)
        } else {
            (bounds, clip)
        };
        if is_window_overlay(node.kind) {
            self.ancestor_shapes = None;
            self.space = None;
        }
        let local = Affine::for_node(node, bounds);
        let mut clip = clip;
        let transform = if local != Affine::IDENTITY {
            let parent_space = self.space.clone();
            let parent_clip = clip;
            self.ancestor_shapes = Some(Arc::new(ClipChain {
                shape: RoundedClip {
                    bounds: clip,
                    radii: [0.0; 4],
                },
                parent: self.ancestor_shapes.clone(),
                space: parent_space.clone(),
            }));
            let world = local.then(
                parent_space
                    .as_deref()
                    .map_or(Affine::IDENTITY, |space| space.world),
            );
            self.space = Some(Arc::new(TransformSpace {
                world,
                inverse: world.inverse(),
            }));
            clip = local
                .inverse()
                .map_or(Rect::default(), |inverse| inverse.map_rect(clip));
            Some(Box::new(TransformPlacement {
                local,
                parent_clip,
                parent_space,
            }))
        } else {
            None
        };
        let space = self.space.clone();
        let world = space
            .as_deref()
            .map_or(Affine::IDENTITY, |space| space.world);
        let inverse = space
            .as_deref()
            .map_or(Some(Affine::IDENTITY), |space| space.inverse);
        visible &= inverse.is_some();
        let ancestor_shapes = self.ancestor_shapes.clone();
        let clipped = world.map_rect(bounds.intersect(clip));
        let clipped = ancestor_shapes.as_deref().map_or(clipped, |chain| {
            chain.clip_rect(clipped, Affine::IDENTITY, Affine::IDENTITY)
        });
        if node.kind != "Shortcut" {
            self.anchor =
                (visible && clipped.width > 0.0 && clipped.height > 0.0).then_some(clipped);
        }
        if matches!(node.kind, "Menu" | "Tooltip" | "Shortcut") {
            // Menus and tooltips lay out their children in the overlay presenter.
            return LayoutNode {
                bounds,
                clip,
                visible: visible && node.kind != "Tooltip",
                enabled,
                scroll: None,
                horizontal_scroll: None,
                viewport_handle: None,
                mouse_drag_pan: false,
                table: None,
                split: None,
                tab_header: None,
                tumbler: None,
                swipe_delegate: None,
                styled_text: None,
                font_size,
                overlays: None,
                paint_order: None,
                overflow: false,
                child_shape: None,
                ancestor_shapes,
                space,
                transform,
                table_pane: None,
                children: Vec::new(),
            };
        }
        let table = if visible && node.table_header.is_some() {
            TableGeometry::with_offset(
                node,
                bounds,
                clip,
                self.requested_scroll(node, Orientation::Horizontal),
            )
            .map(Box::new)
        } else {
            None
        };
        let overflow = node.kind == "Rectangle" && !node.boolean("clip", true);
        let clipping = (node.kind == "Rectangle" && node.boolean("clip", false))
            .then(|| RoundedClip::content_box(node, bounds));
        let child_shape = clipping.filter(|shape| shape.radii.iter().any(|radius| *radius > 0.0));
        if let Some(shape) = child_shape {
            self.ancestor_shapes = Some(Arc::new(ClipChain {
                shape,
                parent: ancestor_shapes.clone(),
                space: space.clone(),
            }));
        }
        let viewport = table.as_ref().map_or(bounds, |table| table.body);
        let child_clip = if overflow {
            clip
        } else {
            clip.intersect(clipping.map_or(viewport, |shape| shape.bounds))
        };
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
                viewport_handle: None,
                mouse_drag_pan: false,
                table: None,
                split: None,
                tab_header: None,
                tumbler: None,
                swipe_delegate: None,
                styled_text: None,
                font_size,
                overlays: None,
                paint_order: None,
                overflow: false,
                child_shape: None,
                ancestor_shapes,
                space,
                transform,
                table_pane: None,
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
        let page =
            (visible && node.kind == "Page").then(|| page::Regions::new(node, bounds, self.text));
        let swipe_delegate = (visible && node.kind == "SwipeDelegate").then(|| {
            let mut geometry = SwipeDelegateGeometry::new(node, bounds, self.text);
            if enabled
                && let Some(offset) = self.input.and_then(|input| {
                    input.swipe_preview_offset(&self.path, node, &geometry, self.row_key)
                })
            {
                geometry.set_preview(offset);
            }
            Box::new(geometry)
        });
        let placements = if !visible {
            (0..node.children.len())
                .map(|_| grid::Placement::plain(Rect::new(bounds.x, bounds.y, 0.0, 0.0)))
                .collect()
        } else if let Some(rects) = split_rects {
            rects.into_iter().map(grid::Placement::plain).collect()
        } else if let Some(list) = node.list {
            let cells = ViewCells::new(node, viewport.width);
            let axis = resolved_list_scroll
                .unwrap_or_else(|| self.list_scroll(node, &cells, viewport.height, list.row_count));
            scroll = Some(axis);
            let panes = table.as_ref().and_then(|table| {
                table.frozen_boundary()?;
                let pane = |bounds| {
                    Arc::new(table::PaneClip {
                        bounds,
                        space: space.clone(),
                    })
                };
                Some((pane(table.frozen_body()), pane(table.scrollable_body())))
            });
            node.children
                .iter()
                .enumerate()
                .map(|(slot, _)| {
                    let bounds = cells.bounds(list.first_row + slot, viewport, axis.offset);
                    grid::Placement {
                        bounds,
                        clip: None,
                        row_children: table.as_ref().map(|table| {
                            table
                                .columns
                                .iter()
                                .enumerate()
                                .map(|(index, column)| grid::CellPlacement {
                                    bounds: Rect::new(
                                        column.x,
                                        bounds.y,
                                        column.width,
                                        bounds.height,
                                    ),
                                    clip: panes.as_ref().map(|(frozen, scrolling)| {
                                        if table.is_frozen(index) {
                                            frozen.clone()
                                        } else {
                                            scrolling.clone()
                                        }
                                    }),
                                })
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
                    grid::Placement::plain(bounds)
                })
                .collect()
        } else if node.kind == "ListRow" {
            let content = TreeRowGeometry::new(node, bounds).map_or(bounds, |tree| tree.content);
            node.children
                .iter()
                .map(|_| grid::Placement {
                    bounds: content,
                    clip: None,
                    row_children: row_children.take(),
                })
                .collect()
        } else if node.kind == "GridLayout" {
            grid::children(node, bounds, self.text)
        } else if node.kind == "TabBar" {
            let orientation = node.orientation();
            let requested = self.scroll_request(node, orientation);
            let reveal = requested.is_none()
                || self
                    .input
                    .is_some_and(|input| input.scroll_selection_changed(node, &self.path, 1));
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
            rects.into_iter().map(grid::Placement::plain).collect()
        } else if node.kind == "ScrollView" {
            let (rects, x, y) = self.scroll_view_children(node, bounds);
            horizontal_scroll = Some(x);
            scroll = Some(y);
            rects.into_iter().map(grid::Placement::plain).collect()
        } else if let Some(placements) = row_children {
            placements
                .into_iter()
                .map(|cell| grid::Placement {
                    bounds: cell.bounds,
                    row_children: None,
                    clip: cell.clip,
                })
                .collect()
        } else {
            let mut content = bounds;
            let title = (selection::radio_title_height(node, font_size)
                + modal::title_height(node, font_size)
                + group_box::title_height(node, font_size))
            .min(bounds.height);
            content.y += title;
            content.height -= title;
            let rects = if let Some(geometry) = &swipe_delegate {
                node.children
                    .iter()
                    .map(|child| geometry.region(child.kind).bounds)
                    .collect()
            } else if let Some(page) = &page {
                node.children
                    .iter()
                    .map(|child| {
                        page.band(child)
                            .unwrap_or_else(|| self.absolute_child(child, page.body))
                    })
                    .collect()
            } else if flow::uses_flow(node) {
                flow::children(node, content, self.text)
            } else if let Some(axis) = linear::Axis::for_node(node) {
                linear::children(node, axis, content, self.text)
            } else {
                self.absolute_children(node, content)
            };
            rects.into_iter().map(grid::Placement::plain).collect()
        };
        let children: Vec<_> = node
            .children
            .iter()
            .zip(placements)
            .enumerate()
            .map(|(index, (child, placement))| {
                self.path.push(index);
                let is_handle = child.kind == "SplitHandle";
                let clip = page.as_ref().map_or(child_clip, |page| {
                    child_clip.intersect(page.band(child).unwrap_or(page.body))
                });
                let region = swipe_delegate
                    .as_ref()
                    .map(|geometry| geometry.region(child.kind));
                let clip = region
                    .as_ref()
                    .map_or(clip, |region| clip.intersect(region.clip));
                let outer_shapes = placement
                    .clip
                    .as_ref()
                    .map(|_| self.ancestor_shapes.clone());
                let pane = placement.clip.as_ref().map(|pane| {
                    let table_world = pane
                        .space
                        .as_ref()
                        .map_or(Affine::IDENTITY, |space| space.world);
                    let to_parent = self
                        .space
                        .as_ref()
                        .and_then(|space| space.inverse)
                        .map_or(table_world, |inverse| table_world.then(inverse));
                    if to_parent != Affine::IDENTITY {
                        self.ancestor_shapes = Some(Arc::new(ClipChain {
                            shape: RoundedClip {
                                bounds: pane.bounds,
                                radii: [0.0; 4],
                            },
                            parent: self.ancestor_shapes.clone(),
                            space: pane.space.clone(),
                        }));
                    }
                    (pane.bounds, to_parent)
                });
                let clip = pane.map_or(clip, |(bounds, to_parent)| {
                    clip.intersect(to_parent.map_rect(bounds))
                });
                let mut child = self.place(
                    child,
                    placement.bounds,
                    clip,
                    visible
                        && region
                            .as_ref()
                            .is_none_or(|_| clip.width > 0.0 && clip.height > 0.0),
                    enabled
                        && node.group_content_enabled()
                        && region.is_none_or(|region| region.enabled),
                    placement.row_children,
                );
                if let Some(shapes) = outer_shapes {
                    self.ancestor_shapes = shapes;
                }
                if let Some((bounds, to_parent)) = pane
                    && (to_parent != Affine::IDENTITY
                        || child.local_transform() != Affine::IDENTITY)
                {
                    child.table_pane = Some(Box::new((bounds, to_parent)));
                }
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
        let tumbler = (visible && node.kind == "Tumbler").then(|| {
            let wheel = xrack_runtime::Tumbler::from_node(node);
            let offset = self
                .input
                .map_or(0.0, |input| input.tumbler_offset(&wheel, &self.path));
            Box::new(TumblerGeometry::new(&wheel, bounds, offset))
        });
        LayoutNode {
            bounds,
            clip,
            visible,
            enabled,
            scroll,
            horizontal_scroll,
            viewport_handle: node.viewport_handle.clone(),
            mouse_drag_pan: node.boolean("mouse-drag-pan-enabled", false),
            table,
            split,
            tab_header: None,
            tumbler,
            swipe_delegate,
            styled_text: if visible && node.kind == "StyledText" {
                self.text
                    .layout_styled_text(node, Size::new(bounds.width, bounds.height))
                    .map(Box::new)
            } else {
                None
            },
            font_size,
            overlays: None,
            paint_order: sibling_paint_order(&node.children[..children.len()]),
            overflow,
            child_shape,
            ancestor_shapes,
            space,
            transform,
            table_pane: None,
            children,
        }
    }

    fn list_scroll(
        &self,
        node: &Node,
        cells: &ViewCells,
        height: f32,
        rows: usize,
    ) -> ScrollLayout {
        let requested = self.scroll_request(node, Orientation::Vertical);
        let axis = ScrollLayout::new(requested.unwrap_or_default(), cells.extent(rows) - height);
        if node
            .viewport_handle
            .as_ref()
            .is_some_and(|handle| handle.has_bound_offset(Orientation::Vertical))
        {
            return axis;
        }
        let anchor = self
            .input
            .and_then(|input| input.scroll_anchors.get(&self.path))
            .filter(|_| node.boolean("preserve-scroll-anchor", false));
        if node.boolean(
            "follow-current",
            matches!(node.kind, "StandardListView" | "GridView"),
        ) && let Some(index) = input::scroll_selection(node)
            && anchor.map_or_else(
                || {
                    requested.is_none()
                        || self.input.is_some_and(|input| {
                            input.scroll_selection_changed(node, &self.path, cells.columns)
                        })
                },
                |anchor| anchor.selection_changed(node),
            )
        {
            return axis.revealing(cells.top(index) as f32, cells.height(index), height);
        }
        anchor
            .and_then(|anchor| anchor.restore(node, cells, requested.unwrap_or_default()))
            .map_or(axis, |offset| ScrollLayout::new(offset, axis.max_offset))
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
            self.requested_scroll(node, Orientation::Horizontal),
            extent.width - viewport.width,
        );
        let vertical = ScrollLayout::new(
            self.requested_scroll(node, Orientation::Vertical),
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
        node.children
            .iter()
            .map(|child| self.absolute_child(child, content))
            .collect()
    }

    fn absolute_child(&mut self, child: &Node, content: Rect) -> Rect {
        if !participates_in_flow(child) {
            return Rect::new(content.x, content.y, 0.0, 0.0);
        }
        let available = Size::new(content.width, content.height);
        let metrics = linear::measure(child, available, self.text);
        let width = absolute_width(child, metrics.x, content.width);
        let height = absolute_height(child, metrics.y, content.height);
        Rect::new(
            content.x + child.length("x").unwrap_or(0.0),
            content.y + child.length("y").unwrap_or(0.0),
            width,
            height,
        )
    }
}

fn absolute_width(node: &Node, info: linear::AxisInfo, available: f32) -> f32 {
    if xrack_runtime::is_button(node.kind)
        || matches!(node.kind, "Image" | "AnimatedImage")
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
