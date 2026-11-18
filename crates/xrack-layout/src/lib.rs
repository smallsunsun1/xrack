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
mod hover_observers;
mod image;
mod input;
mod keyboard;
mod label;
mod linear;
mod list_anchor;
mod menu;
mod modal;
mod overscroll;
mod page;
mod page_indicator;
mod password;
mod path_view;
mod pointer_events;
mod popup;
mod range;
mod row_layout;
mod scale_rotate;
#[doc(hidden)]
pub mod scale_rotate_gestures;
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
mod swipe_gesture;
mod tab_move;
mod table;
mod table_drag_selection;
mod text_area;
mod tooltip;
mod transform;
mod tree;
mod tumbler;
mod view_cells;
mod viewport;
mod visual_text;
mod window_chrome;
pub use button::{ButtonContent, ButtonPlacement};
pub use calendar::{CalendarDay, CalendarGeometry, CalendarWeek};
pub use choice::{ChoiceKey, ChoiceMenu, ComboCompletion, ComboGeometry};
use clip::ClipChain;
pub use clip::{RoundedClip, corner_radii, normalize_radii};
pub use delay_button::ButtonHoldEvent;
pub use drag::{DragDispatch, DragUpdate};
#[doc(hidden)]
pub use edit::selection_drag_axis;
pub use edit::{EditAction, TextEdit, TextEditContext, TextPosition};
pub use file_drop::{FileDragInput, FileDropScope};
pub use focus_observers::{FocusDelivery, FocusObservers};
pub use geometry::{Point, Rect, Size};
pub use group_box::group_header;
pub use history::EditSource;
pub use hover_observers::{HoverDelivery, HoverObservers};
pub use image::{ImagePlacement, ImagePresentation, ImageRegion, fitted_rect, tile_extent};
pub use input::{
    ActivationKey, ControlInput, Interaction, NavigationKey, RangeInput, ScrollBarUpdate,
    SliderKey, SplitResize, TableEditCommit, TableEditTarget, TablePasteDestination,
    TableSelectionKey, Target, TextLinkTarget, WheelAdjustment, WheelRoute, swipe_view_selection,
};
pub use keyboard::KeyRoute;
pub use label::{
    LabelLayout, LabelLine, LabelPlan, PlannedLabelLine, TextAlignment, uses_label_layout,
};
pub use menu::{MenuEntry, MenuKey, MenuPanel, MenuRow};
pub use modal::{dialog_title_height, drawer_grip};
pub use overscroll::OverscrollFrame;
pub use page_indicator::{PageIndicatorGeometry, PageMarker};
pub use password::PasswordMask;
pub use pointer_events::{ButtonClick, PointerDelivery, PointerSequence, pointer_click_follows};
pub use range::{ColorPlaneGeometry, DialGeometry, SliderGeometry, progress_fill_bounds};
#[doc(hidden)]
pub use row_layout::{RowLayoutCache, RowLayoutStats};
pub use scale_rotate::{ScaleRotateTarget, TouchTransform, touch_centroid};
pub use scroll_bar::ScrollBarGeometry;
pub use scroll_gesture::{ScrollGestureEnd, ScrollGestureKind};
pub use search::SearchGeometry;
pub use selection::{TAB_LABEL_SIZE, tab_content_rect};
pub use shortcut::{MenuShortcut, ShortcutKey, ShortcutModifiers, ShortcutOutcome};
pub use spin::SpinGeometry;
pub use split::{SplitGeometry, SplitHandle, SplitHandleRange};
pub use stack_transition::{StackSlide, StackTransitions};
use std::{collections::BTreeMap, rc::Rc, sync::Arc};
pub use styled_text::{StyledFragment, StyledTextLayout};
pub use swipe_delegate::SwipeDelegateGeometry;
pub use swipe_gesture::SwipeDelivery;
pub use table::TableGeometry;
pub use text_area::{
    TextAreaLayout, TextAreaMotion, text_editor_line_bounds, text_editor_viewport,
};
pub use tooltip::{TooltipPopup, layout_tooltip};
pub use transform::Affine;
use transform::TransformSpace;
pub use tree::TreeRowGeometry;
pub use tumbler::{TumblerGeometry, TumblerRow, TumblerScroll};
pub use view_cells::ListSectionHeader;
use view_cells::ViewCells;
pub use viewport::{LayoutObservers, ScrollDelivery, ViewportObservers};
pub use visual_text::{VisualGrapheme, VisualTextLine};
pub use window_chrome::WindowResizeEdge;
use xrack_runtime::{
    Component, LayoutDirection, ListMetrics, Node, Orientation, RowKey, SelectionTarget,
    group_index, is_font_family_choice, is_modal_overlay, is_window_overlay,
    set_font_family_choices,
};

#[cfg(test)]
mod tests;

pub const DEFAULT_FONT_SIZE: f32 = 16.0;

pub trait TextMeasurer {
    fn measure(&mut self, text: &str, font_size: f32, max_width: f32) -> Size;

    fn begin_window(&mut self, _scene: &Node) {}

    /// Font families available for the current window, including embedded fonts.
    fn font_families(&self) -> &[String] {
        &[]
    }

    fn default_font_size(&self) -> Option<f32> {
        None
    }

    fn font_size(&self, node: &Node) -> f32 {
        node.font_size()
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
    pub layout_direction: LayoutDirection,
    direction_override: Option<LayoutDirection>,
    pub(crate) content_bounds: Option<Box<Rect>>,
    pub scroll: Option<ScrollLayout>,
    list_section_headers: Option<Box<[ListSectionHeader]>>,
    horizontal_scroll: Option<ScrollLayout>,
    horizontal_scroll_reversed: bool,
    viewport_handle: Option<xrack_runtime::ViewportHandle>,
    nested_list: Option<xrack_runtime::ListRowRenderer>,
    missing_list_rows: bool,
    missing_descendant_list_rows: bool,
    attached_scroll_bar: Option<Box<scroll_bar::AttachedScrollBar>>,
    mouse_drag_pan: bool,
    middle_mouse_pan: bool,
    pan_axes: [bool; 2],
    elastic: Option<overscroll::ElasticOptions>,
    pub table: Option<Box<TableGeometry>>,
    pub split: Option<Box<SplitGeometry>>,
    /// Header hit box of a tab, independent of the page visibility.
    pub tab_header: Option<Rect>,
    /// Close button bounds within a tab header or standalone tab button.
    pub tab_close: Option<Rect>,
    tab_strip: Option<Rect>,
    pub tumbler: Option<Box<TumblerGeometry>>,
    /// Logical thumb position while a switch gesture is active.
    pub switch_position: Option<f32>,
    pub swipe_delegate: Option<Box<SwipeDelegateGeometry>>,
    #[doc(hidden)]
    pub swipe_preview: Option<Box<LayoutNode>>,
    pub styled_text: Option<Box<StyledTextLayout>>,
    font_size: f32,
    overlays: Option<Box<modal::OverlayIndex>>,
    paint_order: Option<xrack_runtime::ScenePlanSlice<usize>>,
    overflow: bool,
    child_shape: Option<RoundedClip>,
    ancestor_shapes: Option<Arc<ClipChain>>,
    space: Option<Arc<TransformSpace>>,
    transform: Option<Box<TransformPlacement>>,
    table_pane: Option<Box<(Rect, Affine)>>,
    table_column_window: xrack_runtime::TableColumnWindow,
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

/// Window-level registrations visible in a prepared scene.
#[doc(hidden)]
#[derive(Default)]
pub struct WindowEffects<'a> {
    pub button_animations: Vec<&'a xrack_runtime::ButtonPressHandle>,
    pub timers: Vec<&'a xrack_runtime::TimerRegistration>,
    pub dialogs: Vec<&'a xrack_runtime::NativeDialogRegistration>,
    pub media: Vec<&'a xrack_runtime::MediaRegistration>,
    pub playback: Vec<(&'a Rc<xrack_runtime::ImagePlaybackRegistration>, bool)>,
}

impl<'a> WindowEffects<'a> {
    pub fn collect(node: &'a Node, layout: &LayoutNode) -> Self {
        let mut effects = Self::default();
        effects.extend(node, layout);
        effects
    }

    fn extend(&mut self, node: &'a Node, layout: &LayoutNode) {
        if node.expander_collapsed() {
            return;
        }
        if let Some(handle) = node
            .button_press_handle
            .as_ref()
            .filter(|handle| handle.supports_animation())
        {
            self.button_animations.push(handle);
        }
        self.timers.extend(layout.timer_registrations(node));
        if layout.visible && layout.enabled {
            self.dialogs.extend(&node.native_dialogs);
        }
        self.playback.extend(layout.image_playback(node));
        if layout.visible {
            self.media.extend(node.media.as_ref());
        }
        self.media.extend(&node.sound_effects);
        for (child, item) in node.children.iter().zip(&layout.children) {
            self.extend(child, item);
        }
    }
}

impl LayoutNode {
    pub fn list_section_headers(&self) -> &[ListSectionHeader] {
        self.list_section_headers.as_deref().unwrap_or_default()
    }

    /// The nearest explicit direction, including an inherited declaration.
    pub fn layout_direction_override(&self) -> Option<LayoutDirection> {
        self.direction_override
    }

    #[doc(hidden)]
    pub fn has_missing_list_rows(&self) -> bool {
        self.missing_list_rows || self.missing_descendant_list_rows
    }

    #[doc(hidden)]
    pub fn timer_registrations<'a>(
        &self,
        node: &'a Node,
    ) -> &'a [xrack_runtime::TimerRegistration] {
        if self.visible { &node.timers } else { &[] }
    }

    pub fn image_playback<'a>(
        &self,
        node: &'a Node,
    ) -> Option<(&'a Rc<xrack_runtime::ImagePlaybackRegistration>, bool)> {
        let registration = node.image_playback.as_ref()?;
        let clip = self.clip_rect(self.bounds);
        Some((
            registration,
            self.visible && clip.width > 0.0 && clip.height > 0.0,
        ))
    }

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
            let dense = self
                .paint_order
                .as_deref()
                .map_or(index, |order| order[index]);
            self.table_column_window.logical_index(dense)
        })
    }

    pub fn children_in_paint_order<'a>(
        &'a self,
        node: &'a Node,
    ) -> impl DoubleEndedIterator<Item = (usize, &'a Node, &'a LayoutNode)> + 'a {
        self.child_paint_order()
            .filter_map(move |index| Some((index, node.child_at(index)?, self.child_at(index)?)))
    }

    pub(crate) fn children_in_scene_order<'a, 'b>(
        &'b self,
        node: &'a Node,
    ) -> impl Iterator<Item = (usize, &'a Node, &'b LayoutNode)> {
        node.materialized_children()
            .zip(&self.children)
            .map(|((index, child), layout)| (index, child, layout))
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

    /// Reserved title height using the text metrics prepared for this layout.
    pub fn dialog_title_height(&self, node: &Node) -> f32 {
        modal::title_height(node, self.font_size)
    }

    pub fn group_header(&self, node: &Node) -> Rect {
        group_box::header_with_font_size(node, self.bounds, self.font_size)
    }

    pub(crate) fn tab_viewport(&self) -> Rect {
        self.tab_strip.unwrap_or(self.bounds)
    }

    pub(crate) fn tab_item_rect(&self, index: usize) -> Option<Rect> {
        let child = self.child_at(index)?;
        if self.tab_strip.is_some() {
            child.tab_header
        } else {
            child.visible.then_some(child.bounds)
        }
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

    fn accepts_pan(&self, axis: Orientation, device: scroll_gesture::PanDevice) -> bool {
        use scroll_gesture::PanDevice;
        self.pan_axes[usize::from(axis == Orientation::Vertical)]
            && match device {
                PanDevice::Touch => true,
                PanDevice::LeftMouse => self.mouse_drag_pan,
                PanDevice::MiddleMouse => self.middle_mouse_pan,
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

    /// Look up a laid-out child by its logical scene index.
    pub fn child_at(&self, index: usize) -> Option<&Self> {
        self.children
            .get(self.table_column_window.dense_index(index)?)
    }

    pub(crate) fn child_at_mut(&mut self, index: usize) -> Option<&mut Self> {
        let dense = self.table_column_window.dense_index(index)?;
        self.children.get_mut(dense)
    }

    pub fn at_path(&self, path: &[usize]) -> Option<&Self> {
        let mut item = self;
        for &index in path {
            item = item.child_at(index)?;
        }
        Some(item)
    }
}

fn sibling_paint_order(node: &Node, count: usize) -> Option<xrack_runtime::ScenePlanSlice<usize>> {
    use xrack_runtime::ScenePlanSlice;

    if node.has_natural_paint_order() {
        return None;
    }
    if let Some(order) = node.paint_order_plan().filter(|order| order.len() == count) {
        return Some(order.clone());
    }
    let children = &node.children[..count];
    // Already ordered (including the default of all zeroes) needs no storage.
    if !children.windows(2).any(|pair| pair[0].z() > pair[1].z()) {
        return None;
    }
    let mut order: Vec<_> = (0..children.len()).collect();
    order.sort_by(|&left, &right| children[left].z().total_cmp(&children[right].z()));
    Some(ScenePlanSlice::Owned(order))
}

pub fn font_size(node: &Node) -> f32 {
    node.font_size()
        .unwrap_or(DEFAULT_FONT_SIZE)
        .clamp(1.0, 512.0)
}

fn dimension(node: &Node, name: &str) -> Option<f32> {
    node.length(name).map(|n| n.max(0.0))
}

fn default_toggle_indicator_size(node: &Node) -> Size {
    match node.kind {
        "Switch" => Size::new(36.0, 18.0),
        "RadioButton" => Size::new(16.0, 16.0),
        _ => Size::new(18.0, 18.0),
    }
}

fn toggle_indicator_size(node: &Node) -> Option<Size> {
    let indicator = node.control_part("ToggleIndicator")?;
    let default = default_toggle_indicator_size(node);
    Some(Size::new(
        dimension(indicator, "width").unwrap_or(default.width),
        dimension(indicator, "height").unwrap_or(default.height),
    ))
}

/// Bounds shared by the built-in indicator, its replacement and the label.
#[doc(hidden)]
pub struct ToggleGeometry {
    pub indicator: Rect,
    pub label: Rect,
    direction: LayoutDirection,
    thumb_size: Size,
}

impl ToggleGeometry {
    pub fn new(node: &Node, bounds: Rect, direction: LayoutDirection) -> Self {
        let replacement = toggle_indicator_size(node);
        let size = replacement.unwrap_or_else(|| {
            let preferred = default_toggle_indicator_size(node);
            let scale = (bounds.height / preferred.height).clamp(0.0, 1.0);
            Size::new(preferred.width * scale, preferred.height * scale)
        });
        let delegate = node.is_item_delegate();
        let inset = (if delegate { 12.0_f32 } else { 6.0 }).min(bounds.width);
        let width = size.width.min((bounds.width - inset).max(0.0));
        let height = size.height.min(bounds.height);
        let x = if delegate {
            bounds.x + bounds.width - inset - width
        } else {
            bounds.x + inset
        };
        let indicator = Rect::new(x, bounds.y + (bounds.height - height) * 0.5, width, height);
        let (left, right) = if delegate {
            (bounds.x + 12.0, x - 10.0)
        } else if node.kind == "RadioButton" && replacement.is_none() {
            (bounds.x + 28.0, bounds.x + bounds.width)
        } else {
            (x + width + 7.0, bounds.x + bounds.width)
        };
        let label = Rect::new(left, bounds.y, (right - left).max(0.0), bounds.height);
        let thumb = node.control_part("SwitchThumb");
        let diameter = (width.min(height) - 4.0).max(0.0);
        let dimension = |name, available| {
            thumb
                .and_then(|thumb| thumb.relative_length(name, available))
                .unwrap_or(diameter)
                .clamp(0.0, available)
        };
        Self {
            indicator: place_in_direction(indicator, bounds, direction),
            label: place_in_direction(label, bounds, direction),
            direction,
            thumb_size: Size::new(dimension("width", width), dimension("height", height)),
        }
    }

    pub fn switch_travel(&self) -> f32 {
        (self.indicator.width - self.indicator.height.max(self.thumb_size.width)).max(0.0)
    }

    pub fn switch_thumb(&self, checked: bool, preview: Option<f32>) -> Rect {
        let logical = preview.unwrap_or(if checked { 1.0 } else { 0.0 });
        let visual = if self.direction == LayoutDirection::RightToLeft {
            1.0 - logical
        } else {
            logical
        };
        let travel = self.switch_travel();
        let bounds = self.indicator;
        Rect::new(
            bounds.x + (bounds.width - travel - self.thumb_size.width) * 0.5 + travel * visual,
            bounds.y + (bounds.height - self.thumb_size.height) * 0.5,
            self.thumb_size.width,
            self.thumb_size.height,
        )
    }
}

/// Bounds of a supplied replacement indicator.
#[doc(hidden)]
pub fn toggle_indicator_rect(node: &Node, bounds: Rect) -> Option<Rect> {
    node.control_part("ToggleIndicator")?;
    Some(ToggleGeometry::new(node, bounds, node.layout_direction().unwrap_or_default()).indicator)
}

fn place_in_direction(mut rect: Rect, content: Rect, direction: LayoutDirection) -> Rect {
    if direction == LayoutDirection::RightToLeft {
        rect.x = content.x + content.width - (rect.x - content.x) - rect.width;
    }
    rect
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
    let slots @ [menu, header, footer] = page::slots(node);
    let count = node.table_column_window.logical_len(node.children.len());
    let (ordered, len) =
        if node.kind == "TableRow" && node.table_column_order.as_slice(count).is_some() {
            let mut ordered: Vec<_> = node
                .materialized_children()
                .map(|(column, _)| column)
                .collect();
            ordered.sort_by_key(|&column| node.table_column_order.visual_index(count, column));
            (ordered, 0)
        } else if node.kind == "SplitView" {
            let count = node.split_pane_count();
            let ordered = (0..count)
                .flat_map(|slot| [slot, count + slot])
                .filter(|&slot| slot < node.children.len())
                .collect();
            (ordered, 0)
        } else if matches!(node.kind, "DialogButtonBox" | "FlexboxLayout") {
            let mut ordered = (0..node.children.len()).collect::<Vec<_>>();
            ordered.sort_by_key(|&index| linear::item_order(node, &node.children[index]));
            (ordered, 0)
        } else {
            (Vec::new(), node.children.len())
        };
    ordered
        .into_iter()
        .chain(menu)
        .chain(header)
        .chain(
            (0..len)
                .map(move |index| node.table_column_window.logical_index(index))
                .filter(move |&index| !slots.contains(&Some(index))),
        )
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
    let mut layout = layout_with_offsets(scene, viewport, text, offsets, None);
    scroll_bar::attach(scene, &mut layout, text);
    layout
}

pub fn layout_with_input(
    scene: &Node,
    viewport: Size,
    text: &mut impl TextMeasurer,
    input: &Interaction,
) -> LayoutNode {
    let mut layout =
        layout_with_offsets(scene, viewport, text, input.scroll_offsets(), Some(input));
    scroll_bar::attach(scene, &mut layout, text);
    layout
}

fn layout_with_offsets(
    scene: &Node,
    viewport: Size,
    text: &mut impl TextMeasurer,
    offsets: &BTreeMap<Vec<usize>, f32>,
    input: Option<&Interaction>,
) -> LayoutNode {
    text.begin_window(scene);
    layout_current_window(scene, viewport, text, offsets, input)
}

fn layout_current_window(
    scene: &Node,
    viewport: Size,
    text: &mut impl TextMeasurer,
    offsets: &BTreeMap<Vec<usize>, f32>,
    input: Option<&Interaction>,
) -> LayoutNode {
    let bounds = Rect::new(0.0, 0.0, viewport.width.max(0.0), viewport.height.max(0.0));
    let safe_viewport = window_safe_area(scene, bounds);
    let mut layout = LayoutContext {
        text,
        layout_direction: None,
        offsets,
        input,
        path: Vec::new(),
        row_key: None,
        anchor: None,
        resolved_list_scroll: None,
        row_layouts: None,
        ancestor_shapes: None,
        space: None,
        viewport: safe_viewport,
    }
    .place(scene, bounds, bounds, true, true, None);
    modal::index_overlays(scene, &mut layout);
    layout
}

const WINDOW_SAFE_AREA: [&str; 4] = [
    "__safe-area-left",
    "__safe-area-top",
    "__safe-area-right",
    "__safe-area-bottom",
];

/// Record host-provided insets on a window scene in logical pixels.
#[doc(hidden)]
pub fn set_window_safe_area(scene: &mut Node, insets: [f64; 4]) {
    for (edge, value) in WINDOW_SAFE_AREA.into_iter().zip(insets) {
        if value > 0.0 && value.is_finite() {
            scene
                .properties
                .insert(edge, xrack_runtime::Length(value).into());
        } else {
            scene.properties.remove(edge);
        }
    }
}

fn window_safe_area(node: &Node, bounds: Rect) -> Rect {
    if node.kind != "Window" {
        return bounds;
    }
    let [left, top, right, bottom] =
        WINDOW_SAFE_AREA.map(|edge| node.length(edge).unwrap_or(0.0).max(0.0));
    Rect::new(
        bounds.x + left.min(bounds.width),
        bounds.y + top.min(bounds.height),
        (bounds.width - left - right).max(0.0),
        (bounds.height - top - bottom).max(0.0),
    )
}

/// Place replacement artwork in the already resolved popup row.
#[doc(hidden)]
pub fn layout_choice_option(
    node: &Node,
    bounds: Rect,
    direction: LayoutDirection,
    enabled: bool,
    text: &mut impl TextMeasurer,
) -> LayoutNode {
    LayoutContext {
        text,
        layout_direction: Some(direction),
        offsets: &BTreeMap::new(),
        input: None,
        path: Vec::new(),
        row_key: None,
        anchor: None,
        resolved_list_scroll: None,
        row_layouts: None,
        ancestor_shapes: None,
        space: None,
        viewport: bounds,
    }
    .place(node, bounds, bounds, true, enabled, None)
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
    prepare_scene_with_offsets(component, scene, viewport, text, offsets, None, None)
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
        None,
    )
}

#[doc(hidden)]
pub fn prepare_scene_with_row_cache(
    component: &impl Component,
    scene: Rc<Node>,
    viewport: Size,
    text: &mut impl TextMeasurer,
    input: &Interaction,
    cache: &mut RowLayoutCache,
) -> (Rc<Node>, LayoutNode) {
    cache.begin_frame(&scene);
    let result = prepare_scene_with_offsets(
        component,
        scene,
        viewport,
        text,
        input.scroll_offsets(),
        Some(input),
        Some(cache),
    );
    cache.finish_frame();
    result
}

fn prepare_scene_with_offsets(
    component: &impl Component,
    mut scene: Rc<Node>,
    viewport: Size,
    text: &mut impl TextMeasurer,
    offsets: &BTreeMap<Vec<usize>, f32>,
    input: Option<&Interaction>,
    row_layouts: Option<&mut RowLayoutCache>,
) -> (Rc<Node>, LayoutNode) {
    fn has_lists(node: &Node) -> bool {
        node.list.is_some()
            || node.children.iter().any(has_lists)
            || node
                .swipe_preview
                .as_ref()
                .and_then(|preview| preview.page.as_ref())
                .is_some_and(|(_, page)| has_lists(page))
    }

    text.begin_window(&scene);
    let families = text.font_families();
    if !families.is_empty()
        && let Some(populated) = populate_font_choices(&scene, families)
    {
        scene = Rc::new(populated);
    }
    let mut layout = layout_current_window(&scene, viewport, text, offsets, input);
    if has_lists(&scene) {
        let viewport = Rect::new(0.0, 0.0, viewport.width.max(0.0), viewport.height.max(0.0));
        LayoutContext {
            text,
            layout_direction: None,
            offsets,
            input,
            path: Vec::new(),
            row_key: None,
            anchor: None,
            resolved_list_scroll: None,
            row_layouts,
            ancestor_shapes: None,
            space: None,
            viewport,
        }
        .populate_lists(component, Rc::make_mut(&mut scene), &mut layout);
        modal::index_overlays(&scene, &mut layout);
    }
    scroll_bar::attach(&scene, &mut layout, text);
    (scene, layout)
}

fn populate_font_choices(node: &Node, families: &[String]) -> Option<Node> {
    if is_font_family_choice(node) {
        let mut populated = node.clone();
        set_font_family_choices(&mut populated, families);
        return Some(populated);
    }
    let mut populated = None;
    for (index, child) in node.children.iter().enumerate() {
        if let Some(child) = populate_font_choices(child, families) {
            populated.get_or_insert_with(|| node.clone()).children[index] = child;
        }
    }
    populated
}

struct ScrollContent {
    extent: Size,
    children: Vec<Option<(f32, f32, f32, linear::AxisInfo)>>,
}

struct LayoutContext<'a, T> {
    text: &'a mut T,
    layout_direction: Option<LayoutDirection>,
    offsets: &'a BTreeMap<Vec<usize>, f32>,
    input: Option<&'a Interaction>,
    path: Vec<usize>,
    row_key: Option<RowKey>,
    anchor: Option<Rect>,
    resolved_list_scroll: Option<ScrollLayout>,
    row_layouts: Option<&'a mut RowLayoutCache>,
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
        if orientation == Orientation::Vertical && self.tree_root_changed(node) {
            return Some(0.0);
        }
        if self.row_key.is_some()
            && let Some(renderer) = &node.list_row_renderer
        {
            return renderer.offset(orientation);
        }
        let offsets = match orientation {
            Orientation::Horizontal => self.input.map(Interaction::horizontal_scroll_offsets),
            Orientation::Vertical => Some(self.offsets),
        };
        offsets.and_then(|offsets| offsets.get(&self.path)).copied()
    }

    fn tree_root_changed(&self, node: &Node) -> bool {
        if self.row_key.is_some()
            && let Some(renderer) = &node.list_row_renderer
        {
            node.tree
                .as_ref()
                .is_some_and(|tree| renderer.tree_root_changed(tree.root_identity()))
        } else {
            self.input
                .is_some_and(|input| input.tree_root_changed(&self.path, node))
        }
    }

    fn populate_lists(
        &mut self,
        component: &impl Component,
        node: &mut Node,
        layout: &mut LayoutNode,
    ) {
        if node.list.is_none() && node.children.is_empty() {
            return;
        }
        let previous_row = self.row_key;
        let previous_direction = self.layout_direction;
        self.row_key = node.routing_row_key().or(previous_row);
        self.layout_direction = layout.direction_override;
        if let Some(mut list) = node.list
            && !(list.row_count == 0
                && node
                    .children
                    .first()
                    .is_some_and(|child| child.kind == "EmptyState"))
        {
            let viewport = layout
                .table
                .as_ref()
                .map_or(layout.bounds, |table| table.body);
            let clipped = layout.clip_rect(
                layout
                    .table
                    .as_ref()
                    .map_or(viewport, |table| table.rows_area()),
            );
            let clipped = view_cells::list_body_clip(clipped, layout.list_section_headers());
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
            let row_headers = layout
                .table
                .as_ref()
                .is_some_and(|table| table.row_header.width > 0.0);
            let indentation = node.length("indentation").unwrap_or(16.0).clamp(0.0, 64.0);
            let renderer = node.list_row_renderer.as_ref();
            let scope = renderer
                .filter(|_| self.row_key.is_some())
                .map_or(0, xrack_runtime::ListRowRenderer::scope);
            let expects_rows = !range.is_empty();
            let delegates = if let Some(renderer) = renderer {
                let columns = layout
                    .table
                    .as_ref()
                    .map_or_else(xrack_runtime::TableColumnWindow::default, |table| {
                        table.column_window(layout.clip_rect(table.body))
                    });
                renderer.render_in_scene(node, range, &columns)
            } else {
                component.list_rows(&self.path, range)
            };
            xrack_runtime::update_profile::rows(node.element_id(), delegates.len());
            let missing_list_rows = expects_rows && delegates.is_empty();
            node.children = delegates
                .into_iter()
                .enumerate()
                .map(|(slot, mut delegate)| {
                    let families = self.text.font_families();
                    if !families.is_empty()
                        && let Some(populated) = populate_font_choices(&delegate, families)
                    {
                        delegate = populated;
                    }
                    let mut row = Node::new("ListRow", None);
                    row.row_key = delegate.row_key.take();
                    row.row_scope = scope;
                    if delegate.kind == "TableRow" {
                        delegate.table_column_order = node.table_column_order.clone();
                        if row_headers {
                            let title = delegate.text("row-header-text").map_or_else(
                                || (list.first_row + slot + 1).to_string(),
                                str::to_owned,
                            );
                            row.properties.insert("row-header-text", title.into());
                        }
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
                        let columns = delegate.table_column_window.clone();
                        for (index, cell) in delegate.children.iter_mut().enumerate() {
                            let column = columns.logical_index(index);
                            if !cell.is_visible() {
                                continue;
                            }
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
                    if matches!(node.kind, "ListView" | "GridView") && delegate.is_item_delegate() {
                        delegate
                            .properties
                            .entry("highlighted")
                            .or_insert(xrack_runtime::Value::Bool(is_selected));
                        if delegate.kind == "RadioButton" {
                            delegate
                                .properties
                                .insert("checked", xrack_runtime::Value::Bool(current));
                        }
                    }
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
            let pane = layout.table_pane.take();
            *layout = self.place(
                node,
                layout.bounds,
                incoming_clip,
                layout.visible,
                layout.enabled,
                None,
            );
            layout.table_pane = pane;
            layout.missing_list_rows = missing_list_rows;
            self.ancestor_shapes = shapes;
            self.space = space;
        }
        self.populate_section_headers(node, layout);
        let columns = node.table_column_window.clone();
        for (index, (child, item)) in node
            .children
            .iter_mut()
            .zip(&mut layout.children)
            .enumerate()
        {
            self.path.push(columns.logical_index(index));
            self.populate_lists(component, child, item);
            layout.missing_descendant_list_rows |= item.has_missing_list_rows();
            self.path.pop();
        }
        if let Some((index, page)) = node
            .swipe_preview
            .as_mut()
            .and_then(|preview| preview.page.as_mut())
            && let Some(item) = &mut layout.swipe_preview
        {
            self.path.push(*index);
            self.populate_lists(component, page, item);
            self.path.pop();
            scroll_bar::attach(page, item, self.text);
        }
        self.row_key = previous_row;
        self.layout_direction = previous_direction;
    }

    fn populate_section_headers(&mut self, node: &Node, layout: &mut LayoutNode) {
        let Some(renderer) = node
            .list_section_renderer
            .as_ref()
            .filter(|_| layout.visible)
        else {
            return;
        };
        let clip = layout.clip_rect(layout.bounds);
        let Some(headers) = layout.list_section_headers.as_deref_mut() else {
            return;
        };
        let previous_space = std::mem::replace(&mut self.space, layout.space.clone());
        let previous_shapes =
            std::mem::replace(&mut self.ancestor_shapes, layout.ancestor_shapes.clone());
        for header in headers {
            let visible = clip.intersect(header.bounds);
            if visible.width <= 0.0 || visible.height <= 0.0 {
                continue;
            }
            let Some(node) = renderer.render(xrack_runtime::ListSectionInput {
                label: header.label.clone(),
                first_index: header.first_row as i64,
                count: header.row_count as i64,
                pinned: header.pinned,
            }) else {
                continue;
            };
            let item = self.place(&node, header.bounds, visible, true, layout.enabled, None);
            header.content = Some(Box::new((node, item)));
        }
        self.space = previous_space;
        self.ancestor_shapes = previous_shapes;
    }

    fn place(
        &mut self,
        node: &Node,
        bounds: Rect,
        clip: Rect,
        parent_visible: bool,
        parent_enabled: bool,
        row_children: Option<grid::RowPlacements>,
    ) -> LayoutNode {
        let outer_key = self.row_key;
        let outer_direction = self.layout_direction;
        let outer_anchor = self.anchor;
        let outer_shapes = self.ancestor_shapes.clone();
        let outer_space = self.space.clone();
        self.row_key = node.routing_row_key().or(outer_key);
        self.layout_direction = node.layout_direction().or(outer_direction);
        let row_cache = node.kind == "ListRow"
            && row_children.is_none()
            && self.space.is_none()
            && self.ancestor_shapes.is_none()
            && self.row_layouts.is_some();
        let layout = if row_cache && let Some(row) = node.routing_row_key() {
            let slot = row_layout::Slot { row, column: None };
            let inputs = row_layout::RowInputs {
                size: Size::new(bounds.width, bounds.height),
                visible: parent_visible,
                enabled: parent_enabled,
                layout_direction: self.layout_direction,
            };
            self.place_cached_content(node, bounds, clip, inputs, slot)
        } else {
            self.place_node(
                node,
                bounds,
                clip,
                parent_visible,
                parent_enabled,
                row_children,
            )
        };
        self.row_key = outer_key;
        self.layout_direction = outer_direction;
        self.anchor = outer_anchor;
        self.ancestor_shapes = outer_shapes;
        self.space = outer_space;
        layout
    }

    fn place_cached_content(
        &mut self,
        node: &Node,
        bounds: Rect,
        clip: Rect,
        inputs: row_layout::RowInputs,
        slot: row_layout::Slot,
    ) -> LayoutNode {
        if self
            .input
            .is_some_and(|input| input.contains_switch_preview(&self.path))
        {
            return self.place_node(node, bounds, clip, inputs.visible, inputs.enabled, None);
        }
        let depth = if slot.column.is_some() { 3 } else { 1 };
        let path_len = self.path.len().saturating_sub(depth);
        let path = &self.path[..path_len];
        if let Some(mut layout) = self
            .row_layouts
            .as_deref_mut()
            .and_then(|cache| cache.get(path, slot, node, inputs))
        {
            row_layout::place_in_view(&mut layout, bounds, clip);
            return layout;
        }
        if !row_layout::cacheable(node) {
            return self.place_node(node, bounds, clip, inputs.visible, inputs.enabled, None);
        }
        let local = Rect::new(0.0, 0.0, inputs.size.width, inputs.size.height);
        let mut layout = self.place_node(node, local, local, inputs.visible, inputs.enabled, None);
        let path = &self.path[..path_len];
        if let Some(cache) = self.row_layouts.as_deref_mut() {
            cache.insert(path, slot, node, inputs, &layout);
        }
        row_layout::place_in_view(&mut layout, bounds, clip);
        layout
    }

    fn place_node(
        &mut self,
        node: &Node,
        bounds: Rect,
        clip: Rect,
        parent_visible: bool,
        parent_enabled: bool,
        row_children: Option<grid::RowPlacements>,
    ) -> LayoutNode {
        let resolved_list_scroll = self.resolved_list_scroll.take();
        let tree_parts = match &row_children {
            Some(grid::RowPlacements::Tree {
                geometry,
                has_children,
            }) => Some((*geometry, *has_children)),
            _ => None,
        };
        let font_size = self.text.font_size(node);
        let children_enabled = parent_enabled && node.enables_children();
        let enabled =
            children_enabled && (node.kind != "TouchArea" || node.boolean("enabled", true));
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
                layout_direction: self.layout_direction.unwrap_or_default(),
                direction_override: self.layout_direction,
                content_bounds: None,
                scroll: None,
                list_section_headers: None,
                horizontal_scroll: None,
                horizontal_scroll_reversed: false,
                viewport_handle: None,
                nested_list: None,
                missing_list_rows: false,
                missing_descendant_list_rows: false,
                attached_scroll_bar: None,
                mouse_drag_pan: false,
                middle_mouse_pan: false,
                pan_axes: [false; 2],
                elastic: None,
                table: None,
                split: None,
                tab_header: None,
                tab_close: None,
                tab_strip: None,
                tumbler: None,
                switch_position: None,
                swipe_delegate: None,
                swipe_preview: None,
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
                table_column_window: node.table_column_window.clone(),
                children: Vec::new(),
            };
        }
        let table = if visible && node.table_header.is_some() {
            TableGeometry::with_offset(
                node,
                bounds,
                clip,
                self.requested_scroll(node, Orientation::Horizontal),
                self.layout_direction.unwrap_or_default(),
            )
            .map(Box::new)
        } else {
            None
        };
        let pane = xrack_runtime::is_pane(node.kind);
        let overflow =
            (matches!(node.kind, "Rectangle" | "Flickable") || pane) && !node.boolean("clip", true);
        let clipping = ((node.kind == "Rectangle" || pane) && node.boolean("clip", false))
            .then(|| RoundedClip::content_box(node, bounds));
        let child_shape = clipping.filter(|shape| shape.radii.iter().any(|radius| *radius > 0.0));
        if let Some(shape) = child_shape {
            self.ancestor_shapes = Some(Arc::new(ClipChain {
                shape,
                parent: ancestor_shapes.clone(),
                space: space.clone(),
            }));
        }
        let viewport = table.as_ref().map_or(bounds, |table| table.rows_area());
        let child_clip = if overflow {
            clip
        } else {
            clip.intersect(clipping.map_or(viewport, |shape| shape.bounds))
        };
        if matches!(
            node.kind,
            "TabWidget" | "SwipeView" | "StackView" | "StackLayout"
        ) && visible
        {
            let tabs = (node.kind == "TabWidget").then(|| {
                selection::TabGeometry::new(
                    bounds,
                    node.orientation(),
                    self.layout_direction.unwrap_or_default(),
                )
            });
            let stack_layout = node.kind == "StackLayout";
            let page_bounds = if stack_layout {
                linear::Padding::read(node).inner_rect(bounds)
            } else {
                tabs.as_ref().map_or(bounds, |geometry| geometry.content)
            };
            let selected = if node.kind == "StackView" {
                node.stack_page_index()
            } else {
                group_index(node)
            };
            let orientation = node.orientation();
            let direction = self.layout_direction.unwrap_or_default();
            let headers = tabs.as_ref().map(|geometry| {
                let requested = self.scroll_request(node, orientation);
                let reveal = requested.is_none()
                    || self.input.is_some_and(|input| {
                        input.scroll_selection_changed(node, &self.path, 1, direction)
                    });
                selection::tab_bar(node, geometry.bar, self.text, direction, requested, reveal)
            });
            let header_scroll = headers.as_ref().map(|(_, scroll)| *scroll);
            let children = node
                .children
                .iter()
                .enumerate()
                .map(|(index, tab)| {
                    self.path.push(index);
                    let active = selected == Some(index);
                    let bounds = if stack_layout && active && tab.is_visible() {
                        let size = Size::new(page_bounds.width, page_bounds.height);
                        let metrics = linear::measure(tab, size, self.text);
                        place_in_direction(
                            Rect::new(
                                page_bounds.x,
                                page_bounds.y,
                                metrics.x.clamp(size.width),
                                metrics.y.clamp(size.height),
                            ),
                            page_bounds,
                            direction,
                        )
                    } else {
                        page_bounds
                    };
                    let clip = if stack_layout {
                        child_clip.intersect(page_bounds)
                    } else {
                        child_clip
                    };
                    let mut page = self.place(tab, bounds, clip, active, enabled, None);
                    self.path.pop();
                    if let Some((rects, _)) = &headers
                        && tab.is_visible()
                    {
                        page.tab_header = rects.get(index).copied();
                        if xrack_runtime::tab_is_closable(node, tab) {
                            page.tab_close = page
                                .tab_header
                                .and_then(|header| selection::tab_close_rect(header, direction));
                        }
                    }
                    page
                })
                .collect();
            let swipe_preview = node
                .swipe_preview
                .as_ref()
                .and_then(|preview| preview.page.as_ref())
                .map(|(index, page)| {
                    self.path.push(*index);
                    let item = self.place(page, page_bounds, child_clip, true, enabled, None);
                    self.path.pop();
                    Box::new(item)
                });
            return LayoutNode {
                bounds,
                clip,
                visible,
                enabled,
                layout_direction: self.layout_direction.unwrap_or_default(),
                direction_override: self.layout_direction,
                content_bounds: None,
                scroll: header_scroll.filter(|_| orientation == Orientation::Vertical),
                list_section_headers: None,
                horizontal_scroll: header_scroll.filter(|_| orientation == Orientation::Horizontal),
                horizontal_scroll_reversed: false,
                viewport_handle: None,
                nested_list: None,
                missing_list_rows: false,
                missing_descendant_list_rows: false,
                attached_scroll_bar: None,
                mouse_drag_pan: false,
                middle_mouse_pan: tabs.is_some(),
                pan_axes: [tabs.is_some(); 2],
                elastic: None,
                table: None,
                split: None,
                tab_header: None,
                tab_close: None,
                tab_strip: tabs.as_ref().map(|geometry| geometry.bar),
                tumbler: None,
                switch_position: None,
                swipe_delegate: None,
                swipe_preview,
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
                table_column_window: node.table_column_window.clone(),
                children,
            };
        }
        let mut scroll = None;
        let mut horizontal_scroll = None;
        let mut list_section_headers = None;
        let mut scroll_area = None;
        let horizontal_scroll_reversed = matches!(node.kind, "ScrollView" | "Flickable")
            && self.layout_direction == Some(LayoutDirection::RightToLeft);
        let (split, split_rects) = if visible && node.kind == "SplitView" {
            let (geometry, rects) = SplitGeometry::new(
                node,
                bounds,
                self.layout_direction.unwrap_or_default(),
                self.text,
            );
            (Some(Box::new(geometry)), Some(rects))
        } else {
            (None, None)
        };
        let table_cells = node.kind == "TableRow" && row_children.is_some();
        let mut row_children = row_children;
        let content_bounds = window_safe_area(node, bounds);
        let page = (visible && (node.kind == "Page" || node.application_window))
            .then(|| page::Regions::new(node, content_bounds, self.text));
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
        let switch_position = if enabled && visible && node.kind == "Switch" {
            self.input.and_then(|input| {
                input.switch_position(
                    node,
                    bounds,
                    self.layout_direction.unwrap_or_default(),
                    &self.path,
                    self.row_key,
                )
            })
        } else {
            None
        };
        let custom_scroll = (visible && node.has_scroll_parts() && node.scroll_target.is_none())
            .then(|| {
                scroll_bar::standalone_geometry(
                    node,
                    bounds,
                    self.layout_direction.unwrap_or_default(),
                )
            })
            .flatten();
        let placements = if !visible {
            (0..node.children.len())
                .map(|_| grid::Placement::plain(Rect::new(bounds.x, bounds.y, 0.0, 0.0)))
                .collect()
        } else if node.has_scroll_parts() {
            node.children
                .iter()
                .map(|child| {
                    let rect = custom_scroll.map_or_else(
                        || Rect::new(bounds.x, bounds.y, 0.0, 0.0),
                        |geometry| {
                            if child.kind == "ScrollThumb" {
                                geometry.thumb
                            } else {
                                geometry.track
                            }
                        },
                    );
                    grid::Placement::plain(rect)
                })
                .collect()
        } else if let Some(rects) = split_rects {
            rects.into_iter().map(grid::Placement::plain).collect()
        } else if node.kind == "MonthGrid" && node.calendar_cells().is_some() {
            node.children
                .iter()
                .map(|child| {
                    grid::Placement::plain(if child.kind == "CalendarCells" {
                        bounds
                    } else {
                        self.absolute_child(child, bounds)
                    })
                })
                .collect()
        } else if node.has_progress_parts() {
            let fill = progress_fill_bounds(
                bounds,
                self.layout_direction.unwrap_or_default(),
                xrack_runtime::indicator_state(node)
                    .unwrap_or(xrack_runtime::IndicatorState::Hidden),
                None,
            );
            node.children
                .iter()
                .map(|child| {
                    grid::Placement::plain(match child.kind {
                        "ProgressTrack" => bounds,
                        "ProgressFill" => fill,
                        _ => self.absolute_child(child, bounds),
                    })
                })
                .collect()
        } else if node.control_part("SliderHandle").is_some() {
            let geometry = SliderGeometry::with_layout_direction(
                bounds,
                node,
                self.layout_direction.unwrap_or_default(),
            );
            let range = xrack_runtime::SliderRange::from_node(node);
            let fraction = self
                .input
                .and_then(|input| {
                    (!node.boolean("live", true))
                        .then(|| input.slider_preview(&self.path))
                        .flatten()
                })
                .map_or_else(|| range.fraction(), |value| range.preview_fraction(value));
            node.children
                .iter()
                .map(|child| {
                    let rect = if child.kind == "SliderHandle" {
                        geometry.thumb_bounds(0, fraction)
                    } else {
                        self.absolute_child(child, bounds)
                    };
                    grid::Placement::plain(rect)
                })
                .collect()
        } else if node.kind == "ComboBox" && !node.children.is_empty() {
            let direction = self.layout_direction.unwrap_or_default();
            let geometry = ComboGeometry::for_control(node, bounds, direction);
            node.children
                .iter()
                .map(|child| match child.kind {
                    "ComboIndicator" => grid::Placement::plain(geometry.indicator_bounds(child)),
                    "ComboPopup" => grid::Placement::plain(Rect::new(bounds.x, bounds.y, 0.0, 0.0)),
                    "ComboContent" => {
                        let bounds = geometry.label_bounds(direction);
                        grid::Placement {
                            bounds,
                            clip: Some(Arc::new(table::PaneClip {
                                bounds,
                                space: space.clone(),
                            })),
                            row_children: None,
                        }
                    }
                    _ => grid::Placement::plain(self.absolute_child(child, bounds)),
                })
                .collect()
        } else if xrack_runtime::is_spin_box(node.kind) && !node.children.is_empty() {
            let geometry = SpinGeometry::with_layout_direction(
                bounds,
                node,
                self.layout_direction.unwrap_or_default(),
            );
            node.children
                .iter()
                .map(|child| {
                    grid::Placement::plain(match child.kind {
                        "SpinUpIndicator" => geometry.up,
                        "SpinDownIndicator" => geometry.down,
                        _ => self.absolute_child(child, bounds),
                    })
                })
                .collect()
        } else if node.control_part("ToggleIndicator").is_some()
            || node.control_part("SwitchThumb").is_some()
        {
            let geometry =
                ToggleGeometry::new(node, bounds, self.layout_direction.unwrap_or_default());
            let checked = node.boolean("checked", false);
            node.children
                .iter()
                .map(|child| {
                    grid::Placement::plain(match child.kind {
                        "ToggleIndicator" => geometry.indicator,
                        "SwitchThumb" => geometry.switch_thumb(checked, switch_position),
                        _ => self.absolute_child(child, bounds),
                    })
                })
                .collect()
        } else if node.control_part("ButtonContent").is_some() {
            let content = bounds.inset(ButtonContent::new(node).padding);
            node.children
                .iter()
                .map(|child| {
                    grid::Placement::plain(if child.kind == "ButtonContent" {
                        content
                    } else {
                        self.absolute_child(child, bounds)
                    })
                })
                .collect()
        } else if node.kind == "SplitHandle" {
            node.children
                .iter()
                .map(|_| grid::Placement::plain(bounds))
                .collect()
        } else if node.kind == "CalendarCells" {
            (0..node.children.len())
                .map(|slot| {
                    let bounds = calendar::month_cell(
                        bounds,
                        slot,
                        self.layout_direction.unwrap_or_default(),
                    );
                    grid::Placement {
                        bounds,
                        clip: Some(Arc::new(table::PaneClip {
                            bounds,
                            space: space.clone(),
                        })),
                        row_children: None,
                    }
                })
                .collect()
        } else if let Some(list) = node.list {
            let cells = ViewCells::new(node, viewport.width);
            let axis = resolved_list_scroll
                .unwrap_or_else(|| self.list_scroll(node, &cells, viewport.height, list.row_count));
            scroll = Some(axis);
            list_section_headers = cells.headers(viewport, axis.offset, list.row_count);
            let panes = table.as_ref().and_then(|table| {
                if table.row_header.width == 0.0 {
                    table.frozen_boundary()?;
                }
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
                .map(|(slot, row)| {
                    if list.row_count == 0 && row.kind == "EmptyState" {
                        return grid::Placement::plain(viewport);
                    }
                    let bounds = cells.bounds(list.first_row + slot, viewport, axis.offset);
                    let bounds = if node.kind == "GridView" {
                        place_in_direction(
                            bounds,
                            viewport,
                            self.layout_direction.unwrap_or_default(),
                        )
                    } else {
                        bounds
                    };
                    let delegate = if row.kind == "ListRow" {
                        row.children.first()
                    } else {
                        Some(row)
                    };
                    grid::Placement {
                        bounds,
                        clip: None,
                        row_children: table.as_ref().zip(delegate).map(|(table, row)| {
                            grid::RowPlacements::Cells(
                                row.materialized_children()
                                    .filter_map(|(index, _)| {
                                        let column = table.columns.get(index)?;
                                        Some(grid::CellPlacement {
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
                                    })
                                    .collect(),
                            )
                        }),
                    }
                })
                .collect()
        } else if node.kind == "RangeSlider" {
            let geometry = SliderGeometry::with_layout_direction(
                bounds,
                node,
                self.layout_direction.unwrap_or_default(),
            );
            let interval = xrack_runtime::SliderInterval::from_node(node);
            let preview = self
                .input
                .filter(|_| enabled)
                .and_then(|input| input.interval_preview(node, &self.path));
            let fractions = preview.map_or_else(
                || interval.fractions(),
                |(index, fraction)| interval.preview_fractions(index, fraction),
            );
            node.children
                .iter()
                .enumerate()
                .map(|(index, child)| {
                    let index = if index < Node::RANGE_THUMBS {
                        Some(index)
                    } else {
                        Node::RANGE_HANDLE_PARTS
                            .iter()
                            .position(|kind| *kind == child.kind)
                    };
                    let rect = index.map_or(Rect::default(), |index| {
                        geometry.thumb_bounds(index, fractions[index])
                    });
                    grid::Placement::plain(rect)
                })
                .collect()
        } else if node.kind == "ListRow" {
            let direction = self.layout_direction.unwrap_or_default();
            let tree = TreeRowGeometry::with_layout_direction(node, bounds, direction);
            node.children
                .iter()
                .map(|child| {
                    if let Some(geometry) = tree
                        && child.kind == "TreeRow"
                        && child.control_part("TreeDisclosure").is_some()
                    {
                        grid::Placement {
                            bounds,
                            clip: None,
                            row_children: Some(grid::RowPlacements::Tree {
                                geometry,
                                has_children: node.boolean("tree-has-children", false),
                            }),
                        }
                    } else {
                        grid::Placement {
                            bounds: tree.map_or(bounds, |tree| tree.content),
                            clip: None,
                            row_children: row_children.take(),
                        }
                    }
                })
                .collect()
        } else if node.kind == "GridLayout" {
            grid::children(
                node,
                bounds,
                self.layout_direction.unwrap_or_default(),
                self.text,
            )
        } else if node.kind == "TabBar" {
            let orientation = node.orientation();
            let direction = self.layout_direction.unwrap_or_default();
            let requested = self.scroll_request(node, orientation);
            let reveal = requested.is_none()
                || self.input.is_some_and(|input| {
                    input.scroll_selection_changed(node, &self.path, 1, direction)
                });
            let (rects, axis) =
                selection::tab_bar(node, bounds, self.text, direction, requested, reveal);
            match orientation {
                Orientation::Horizontal => horizontal_scroll = Some(axis),
                Orientation::Vertical => scroll = Some(axis),
            }
            rects.into_iter().map(grid::Placement::plain).collect()
        } else if matches!(node.kind, "ScrollView" | "Flickable") {
            let (rects, x, y, area) =
                self.scroll_view_children(node, bounds, enabled, horizontal_scroll_reversed);
            scroll_area = (area != bounds).then_some(area);
            horizontal_scroll = Some(x);
            scroll = Some(y);
            rects.into_iter().map(grid::Placement::plain).collect()
        } else if let Some((tree, _)) = tree_parts {
            node.children
                .iter()
                .map(|child| {
                    grid::Placement::plain(if child.kind == "TreeDisclosure" {
                        tree.indicator
                    } else {
                        self.absolute_child(child, tree.content)
                    })
                })
                .collect()
        } else if let Some(grid::RowPlacements::Cells(placements)) = row_children {
            placements
                .into_iter()
                .map(|cell| grid::Placement {
                    bounds: cell.bounds,
                    row_children: None,
                    clip: cell.clip,
                })
                .collect()
        } else {
            let mut content = if pane || matches!(node.kind, "Loader" | "Window") {
                linear::Padding::read(node).inner_rect(content_bounds)
            } else {
                content_bounds
            };
            let title = (selection::radio_title_height(node, font_size)
                + modal::title_height(node, font_size)
                + group_box::title_height(node, font_size))
            .min(bounds.height);
            content.y += title;
            content.height -= title;
            let rects = if matches!(node.kind, "Loader" | "LoaderCandidate") {
                node.children.iter().map(|_| content).collect()
            } else if let Some(geometry) = &swipe_delegate {
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
            } else if let Some(rects) = modal::picker_children(
                node,
                content,
                self.layout_direction.unwrap_or_default(),
                self.text,
            ) {
                rects
            } else if flow::uses_flow(node) {
                flow::children(
                    node,
                    content,
                    self.layout_direction.unwrap_or_default(),
                    self.text,
                )
            } else if let Some(axis) = linear::Axis::for_node(node) {
                linear::children(
                    node,
                    axis,
                    content,
                    self.layout_direction.unwrap_or_default(),
                    self.text,
                )
            } else {
                self.absolute_children(node, content)
            };
            rects.into_iter().map(grid::Placement::plain).collect()
        };
        let child_clip = scroll_area.map_or(child_clip, |area| child_clip.intersect(area));
        let children: Vec<_> = node
            .materialized_children()
            .filter(|_| !node.expander_collapsed())
            .zip(placements)
            .map(|((index, child), placement)| {
                self.path.push(index);
                let is_handle = child.kind == "SplitHandle";
                let closable = xrack_runtime::tab_is_closable(node, child);
                let clip = page.as_ref().map_or(child_clip, |page| {
                    child_clip.intersect(page.band(child).unwrap_or(page.body))
                });
                let region = swipe_delegate
                    .as_ref()
                    .map(|geometry| geometry.region(child.kind));
                let clip = region
                    .as_ref()
                    .map_or(clip, |region| clip.intersect(region.clip));
                let clip = view_cells::list_body_clip(
                    clip,
                    list_section_headers.as_deref().unwrap_or_default(),
                );
                let (clip, visible) = match tree_parts {
                    Some((tree, has_children)) if child.kind == "TreeDisclosure" => {
                        (clip.intersect(tree.indicator), visible && has_children)
                    }
                    Some((tree, _)) => (clip.intersect(tree.content), visible),
                    None => (clip, visible),
                };
                // The animated fill carries a local clip. Its moving viewport
                // intersection is applied by the renderer.
                let clip = if child.kind == "ProgressFill" {
                    placement.bounds
                } else {
                    clip
                };
                let scroll_part = Node::SCROLL_PARTS.contains(&child.kind);
                let clip = if scroll_part {
                    clip.intersect(placement.bounds)
                } else {
                    clip
                };
                let visible = visible
                    && (!scroll_part || custom_scroll.is_some())
                    && child.kind != "ComboPopup"
                    && !(child.kind == "ComboContent" && xrack_runtime::combo_editable(node));
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
                let visible = visible
                    && region
                        .as_ref()
                        .is_none_or(|_| clip.width > 0.0 && clip.height > 0.0);
                let enabled = children_enabled
                    && node.group_content_enabled()
                    && region.is_none_or(|region| region.enabled);
                let mut child = if table_cells
                    && !child.children.is_empty()
                    && self.row_layouts.is_some()
                    && self.space.is_none()
                    && self.ancestor_shapes.is_none()
                    && child.routing_row_key().is_none()
                    && row_layout::cacheable(child)
                    && let Some(row) = self.row_key
                {
                    let outer_anchor = self.anchor;
                    let outer_direction = self.layout_direction;
                    self.layout_direction = child.layout_direction().or(outer_direction);
                    let inputs = row_layout::RowInputs {
                        size: Size::new(placement.bounds.width, placement.bounds.height),
                        visible,
                        enabled,
                        layout_direction: self.layout_direction,
                    };
                    let slot = row_layout::Slot {
                        row,
                        column: Some(index),
                    };
                    let child =
                        self.place_cached_content(child, placement.bounds, clip, inputs, slot);
                    self.anchor = outer_anchor;
                    self.layout_direction = outer_direction;
                    child
                } else {
                    self.place(
                        child,
                        placement.bounds,
                        clip,
                        visible,
                        enabled,
                        placement.row_children,
                    )
                };
                if child.visible && closable {
                    child.tab_close = selection::tab_close_rect(
                        placement.bounds,
                        self.layout_direction.unwrap_or_default(),
                    );
                }
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
        let tumbler = (visible && matches!(node.kind, "Tumbler" | "PathView"))
            .then(|| {
                let wheel = xrack_runtime::Tumbler::from_node(node);
                let offset = self
                    .input
                    .map_or(0.0, |input| input.tumbler_offset(&wheel, &self.path));
                let mut geometry = if node.kind == "PathView" {
                    TumblerGeometry::along_path(
                        node,
                        &wheel,
                        bounds,
                        offset,
                        self.layout_direction.unwrap_or_default(),
                    )?
                } else {
                    TumblerGeometry::new(&wheel, bounds, offset)
                };
                if let Some(renderer) = &node.combo_option_renderer {
                    let clip = clip.intersect(bounds);
                    for row in &mut geometry.rows {
                        let visible = clip.intersect(row.bounds);
                        if visible.width <= 0.0 || visible.height <= 0.0 {
                            continue;
                        }
                        let mut input = wheel.option_input(
                            row.index,
                            row.label.clone(),
                            f64::from(row.displacement),
                            enabled,
                        );
                        input.progress = f64::from(row.progress);
                        if let Some(content) = renderer.render(input) {
                            let layout =
                                self.place(&content, row.bounds, visible, true, enabled, None);
                            row.content = Some(Box::new((content, layout)));
                        }
                    }
                }
                Some(Box::new(geometry))
            })
            .flatten();
        LayoutNode {
            bounds,
            clip,
            visible,
            enabled,
            layout_direction: self.layout_direction.unwrap_or_default(),
            direction_override: self.layout_direction,
            content_bounds: scroll_area
                .or_else(|| {
                    page.filter(|_| node.application_window)
                        .map(|page| page.body)
                })
                .map(Box::new),
            scroll,
            list_section_headers,
            horizontal_scroll,
            horizontal_scroll_reversed,
            viewport_handle: node.viewport_handle.clone(),
            nested_list: node
                .list_row_renderer
                .clone()
                .filter(|_| self.row_key.is_some()),
            missing_list_rows: false,
            missing_descendant_list_rows: false,
            attached_scroll_bar: None,
            mouse_drag_pan: node.boolean("mouse-drag-pan-enabled", node.kind == "Flickable"),
            middle_mouse_pan: node.kind != "Flickable"
                || node.boolean("mouse-drag-pan-enabled", true),
            pan_axes: if node.kind == "Flickable" {
                let direction = match node.properties.get("flick-direction") {
                    Some(xrack_runtime::Value::FlickDirection(direction)) => *direction,
                    _ => xrack_runtime::FlickDirection::Both,
                };
                [Orientation::Horizontal, Orientation::Vertical]
                    .map(|axis| node.boolean("interactive", true) && direction.allows(axis))
            } else {
                [true; 2]
            },
            elastic: overscroll::ElasticOptions::from_node(node),
            table,
            split,
            tab_header: None,
            tab_close: None,
            tab_strip: None,
            tumbler,
            switch_position,
            swipe_delegate,
            swipe_preview: None,
            styled_text: if visible && node.kind == "StyledText" {
                self.text
                    .layout_styled_text(node, Size::new(bounds.width, bounds.height))
                    .map(|mut plan| {
                        if let Some(direction) = self.layout_direction {
                            plan.align_in_direction(node, bounds.width, direction);
                        }
                        Box::new(plan)
                    })
            } else {
                None
            },
            font_size,
            overlays: None,
            paint_order: sibling_paint_order(node, children.len()),
            overflow,
            child_shape,
            ancestor_shapes,
            space,
            transform,
            table_pane: None,
            table_column_window: node.table_column_window.clone(),
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
        let nested = node
            .list_row_renderer
            .as_ref()
            .filter(|_| self.row_key.is_some());
        let anchor = self
            .input
            .and_then(|input| {
                list_anchor::ListAnchor::find(
                    &input.scroll_anchors,
                    &self.path,
                    nested.map(xrack_runtime::ListRowRenderer::scope),
                )
            })
            .filter(|_| {
                node.boolean("preserve-scroll-anchor", false) && !self.tree_root_changed(node)
            });
        if node.boolean(
            "follow-current",
            matches!(node.kind, "StandardListView" | "GridView"),
        ) && let Some(index) = input::scroll_selection(node)
            && anchor.map_or_else(
                || {
                    requested.is_none()
                        || if let Some(renderer) = nested {
                            renderer.selection_changed(input::scroll_selection(node), cells.columns)
                        } else {
                            self.input.is_some_and(|input| {
                                input.scroll_selection_changed(
                                    node,
                                    &self.path,
                                    cells.columns,
                                    self.layout_direction.unwrap_or_default(),
                                )
                            })
                        }
                },
                |anchor| anchor.selection_changed(node),
            )
        {
            return cells.reveal(axis, index, height);
        }
        anchor
            .and_then(|anchor| anchor.restore(node, cells, requested.unwrap_or_default()))
            .map_or(axis, |offset| ScrollLayout::new(offset, axis.max_offset))
    }

    fn scroll_view_children(
        &mut self,
        node: &Node,
        viewport: Rect,
        enabled: bool,
        horizontal_reversed: bool,
    ) -> (Vec<Rect>, ScrollLayout, ScrollLayout, Rect) {
        let (viewport, ScrollContent { extent, children }) =
            self.fit_scroll_content(node, viewport, horizontal_reversed);
        let horizontal = ScrollLayout::new(
            self.requested_scroll(node, Orientation::Horizontal),
            extent.width - viewport.width,
        );
        let vertical = ScrollLayout::new(
            self.requested_scroll(node, Orientation::Vertical),
            extent.height - viewport.height,
        );
        let elastic_offset = |axis| {
            if !enabled
                || !node.boolean("interactive", true)
                || matches!(node.properties.get("flick-direction"), Some(xrack_runtime::Value::FlickDirection(direction)) if !direction.allows(axis))
                || !overscroll::ElasticOptions::from_node(node)
                    .is_some_and(overscroll::ElasticOptions::moves_contents)
            {
                return 0.0;
            }
            let offset = self
                .input
                .zip(node.viewport_handle.as_ref())
                .map_or(0.0, |(input, handle)| input.visual_overscroll(handle, axis));
            let (scroll, logical_offset) = if axis == Orientation::Horizontal {
                (
                    horizontal,
                    if horizontal_reversed { -offset } else { offset },
                )
            } else {
                (vertical, offset)
            };
            if scroll.max_offset > 0.0
                && ((logical_offset < 0.0 && scroll.offset == 0.0)
                    || (logical_offset > 0.0 && scroll.offset == scroll.max_offset))
            {
                offset
            } else {
                0.0
            }
        };
        let physical_x = if horizontal_reversed {
            horizontal.max_offset - horizontal.offset
        } else {
            horizontal.offset
        };
        let keyboard_shift = self
            .input
            .and_then(Interaction::keyboard_avoidance)
            .filter(|(path, _)| *path == self.path)
            .map_or(0.0, |(_, shift)| shift);
        let origin = Point {
            x: viewport.x - physical_x - elastic_offset(Orientation::Horizontal),
            y: viewport.y - vertical.offset - elastic_offset(Orientation::Vertical)
                + keyboard_shift,
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
        (rects, horizontal, vertical, viewport)
    }

    fn measure_scroll_content(&mut self, node: &Node, viewport: Rect) -> ScrollContent {
        // Keep percentage constraints tied to the viewport or explicit content size,
        // so an overflowing child cannot enlarge its own percentage reference.
        let explicit_width = node.relative_length("content-width", viewport.width);
        let explicit_height = node
            .relative_length("content-height", viewport.height)
            .or_else(|| node.length("viewport-height"));
        let reference = Size::new(
            viewport.width.max(explicit_width.unwrap_or(0.0)),
            viewport.height.max(explicit_height.unwrap_or(0.0)),
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
        if node.kind == "Flickable" {
            extent.width = explicit_width.unwrap_or(extent.width).max(0.0);
            extent.height = explicit_height.unwrap_or(extent.height).max(0.0);
        }
        ScrollContent { extent, children }
    }

    fn fit_scroll_content(
        &mut self,
        node: &Node,
        bounds: Rect,
        mirrored: bool,
    ) -> (Rect, ScrollContent) {
        use xrack_runtime::ScrollBarPolicy;

        if node.kind != "ScrollView" || !node.boolean("reserve-scroll-bar-space", false) {
            return (bounds, self.measure_scroll_content(node, bounds));
        }
        let thickness = scroll_bar::scroll_bar_thickness(bounds);
        if thickness < 1.0 {
            return (bounds, self.measure_scroll_content(node, bounds));
        }
        let policies = [Orientation::Horizontal, Orientation::Vertical]
            .map(|axis| node.scroll_bar_policy(axis));
        let mut fallback = None;
        // Trying the smallest sets first handles width-sensitive content as well
        // as the common case where reserving one bar makes the other necessary.
        for bars in [[false, false], [false, true], [true, false], [true, true]] {
            if !bars
                .iter()
                .zip(policies)
                .all(|(&shown, policy)| match policy {
                    ScrollBarPolicy::AlwaysOn => shown,
                    ScrollBarPolicy::AlwaysOff => !shown,
                    ScrollBarPolicy::AsNeeded => true,
                })
            {
                continue;
            }
            let bottom = if bars[0] { thickness } else { 0.0 };
            let side = if bars[1] { thickness } else { 0.0 };
            let viewport = Rect::new(
                bounds.x + if mirrored { side } else { 0.0 },
                bounds.y,
                bounds.width - side,
                bounds.height - bottom,
            );
            let content = self.measure_scroll_content(node, viewport);
            let overflowing = [
                content.extent.width > viewport.width,
                content.extent.height > viewport.height,
            ];
            if bars
                .into_iter()
                .zip(policies)
                .zip(overflowing)
                .all(|((shown, policy), overflow)| {
                    policy != ScrollBarPolicy::AsNeeded || shown == overflow
                })
            {
                return (viewport, content);
            }
            fallback = Some((viewport, content));
        }
        // If responsive content oscillates between two bar sets, keep the
        // gutters permitted by policy and give the view a stable area.
        #[expect(
            clippy::expect_used,
            reason = "enumeration always includes the allowed combination of forced and optional scrollbars"
        )]
        fallback.expect("scrollbar policies allow a candidate")
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
        || xrack_runtime::is_pane(node.kind)
        || matches!(
            node.kind,
            "Image" | "AnimatedImage" | "ToolSeparator" | "Loader"
        )
        || node.properties.contains_key("preferred-width")
    {
        info.preferred
    } else {
        info.clamp(available)
    }
}

fn absolute_height(node: &Node, info: linear::AxisInfo, available: f32) -> f32 {
    if linear::intrinsic_item(node.kind)
        || matches!(node.kind, "ToolSeparator" | "Loader")
        || xrack_runtime::is_pane(node.kind)
        || node.properties.contains_key("preferred-height")
    {
        info.preferred
    } else {
        info.clamp(available)
    }
}
