//! Logical-pixel layout shared by software rendering and input hit testing.
mod engine;
mod input;
#[doc(hidden)]
pub mod item;
mod overlay;
mod text;
mod view;
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
use engine::{anchors, clip, flow, frame, frame_projection, geometry, grid, linear, transform};
pub use file_drop::{FileDragInput, FileDropScope, TextDrop};
pub use focus_observers::{FocusDelivery, FocusObservers};
#[doc(hidden)]
pub use frame::{FrameCache, FramePreparation, PreparedFrame};
pub use geometry::{Point, Rect, Size};
pub use group_box::group_header;
pub use history::EditSource;
pub use hover_observers::{HoverDelivery, HoverObservers};
pub use image::{ImagePlacement, ImagePresentation, ImageRegion, fitted_rect, tile_extent};
#[doc(hidden)]
pub use input::scale_rotate_gestures;
pub use input::{
    ActivationKey, ControlInput, Interaction, NavigationKey, RangeInput, ScrollBarUpdate,
    SliderKey, SplitResize, TableEditCommit, TableEditTarget, TablePasteDestination,
    TableSelectionKey, Target, TextLinkTarget, WheelAdjustment, WheelRoute, swipe_view_selection,
};
use input::{
    drag, edit, file_drop, focus_observers, focus_scope, history, hover_observers, keyboard,
    pointer_events, scale_rotate, scroll_gesture, shortcut, swipe_gesture, tab_move, window_chrome,
};
use item::{
    DeferredLayout, ItemLayout, OverlayLayout, PageLayout, RowLayout, ScrollContainer, button,
    calendar, choice, delay_button, group_box, page, page_indicator, path_view, range, search,
    selection, spin, swipe_delegate, tumbler,
};
pub use keyboard::KeyRoute;
pub use label::{
    LabelLayout, LabelLine, LabelMeasurements, LabelPlan, LabelStyle, PlannedLabelLine,
    TextAlignment, elide_to_width, uses_label_layout,
};
pub use linear::IntrinsicMeasurements;
pub use menu::{MenuEntry, MenuKey, MenuPanel, MenuRow, NativeTextMenu};
pub use modal::{dialog_title_height, drawer_grip};
use overlay::{menu, modal, popup, tooltip};
pub use overscroll::OverscrollFrame;
pub use page_indicator::{PageIndicatorGeometry, PageMarker};
pub use password::PasswordMask;
pub use pointer_events::{ButtonClick, PointerDelivery, PointerSequence, pointer_click_follows};
pub use range::{ColorPlaneGeometry, DialGeometry, SliderGeometry, progress_fill_bounds};
#[doc(hidden)]
pub use row_layout::{RowLayoutCache, RowLayoutStats};
pub use scale_rotate::{ScaleRotateTarget, TouchTransform, touch_centroid};
pub use scene_texture_input::{SceneTouches, scene_texture_source_active};
pub use scroll_bar::ScrollBarGeometry;
pub use scroll_gesture::{ScrollGestureEnd, ScrollGestureKind};
pub use search::SearchGeometry;
pub use selection::{TAB_LABEL_SIZE, tab_content_rect, tab_font_size};
pub use shortcut::{MenuShortcut, ShortcutKey, ShortcutModifiers, ShortcutOutcome};
pub use spin::SpinGeometry;
pub use split::{SplitGeometry, SplitHandle, SplitHandleRange};
pub use stack_transition::{StackSlide, StackTransitions};
use std::{collections::BTreeMap, rc::Rc};
pub use styled_fragments::{StyledFragmentBlock, StyledFragmentSource, StyledFragments};
pub use styled_text::{
    StyledFragment, StyledGlyph, StyledGlyphFont, StyledGlyphRun, StyledSelectionSpans,
    StyledTextLayout, StyledTextLineIndex, StyledTextMeasurements,
};
pub use swipe_delegate::SwipeDelegateGeometry;
pub use swipe_gesture::SwipeDelivery;
pub use table::{TableGeometry, table_heading_font_size};
use text::{label, password, styled_fragments, styled_text, text_area, visual_text};
#[doc(hidden)]
pub use text_area::is_basic_han_text;
pub use text_area::{
    TextAreaLayout, TextAreaMotion, text_editor_line_bounds, text_editor_viewport,
};
pub use tooltip::{TooltipPopup, TouchTooltip, layout_tooltip, tooltip_text};
use transform::TransformSpace;
pub use transform::{LayoutSpace, Transform2D, WindowSpace};
pub use tree::TreeRowGeometry;
pub use tumbler::{TumblerGeometry, TumblerRow, TumblerScroll};
use view::{
    image, list_anchor, overscroll, row_layout, scene_texture_input, scroll_bar, split,
    stack_transition, table, table_drag_selection, tree, view_cells, viewport,
};
pub use view_cells::ListSectionHeader;
use view_cells::ViewCells;
pub use viewport::{LayoutObservers, ScrollDelivery, ViewportObservers};
pub use visual_text::{VisualGrapheme, VisualTextLine};
pub use window_chrome::WindowResizeEdge;
use xrack_runtime::{
    Component, LayoutDirection, ListMetrics, Node, Orientation, RowKey, SelectionTarget,
    group_index, is_font_family_choice, is_window_overlay, set_font_family_choices,
};

#[cfg(test)]
mod tests;

pub const DEFAULT_FONT_SIZE: f32 = 16.0;

pub trait TextMeasurer {
    fn measure(&mut self, text: &str, font_size: f32, max_width: f32) -> Size;

    fn begin_window(&mut self, _scene: &Node) {}

    /// Identity of the current font metrics, for reusing immutable row layouts.
    #[doc(hidden)]
    fn typography_revision(&self) -> Option<Rc<()>> {
        None
    }

    #[doc(hidden)]
    fn intrinsic_measurements(&mut self) -> Option<&mut IntrinsicMeasurements> {
        None
    }

    /// Reuse measurements in the current frame. Implementations must clear
    /// these measurements when the active font metrics change.
    #[doc(hidden)]
    fn frame_intrinsic_measurements(&mut self) -> Option<&mut IntrinsicMeasurements> {
        None
    }

    /// Font families available for the current window, including embedded fonts.
    fn font_families(&self) -> &[String] {
        &[]
    }

    fn default_font_size(&self) -> Option<f32> {
        None
    }

    /// Scale applied to text by the host's accessibility settings.
    #[doc(hidden)]
    fn text_scale(&self) -> f32 {
        1.0
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

    #[doc(hidden)]
    fn measure_node_at_size(
        &mut self,
        _node: &Node,
        text: &str,
        size: f32,
        max_width: f32,
    ) -> Size {
        self.measure(text, size, max_width)
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

    #[doc(hidden)]
    fn shared_styled_text_layout(
        &mut self,
        node: &Node,
        available: Size,
    ) -> Option<Rc<StyledTextLayout>> {
        self.layout_styled_text(node, available).map(Rc::new)
    }

    // Font resolution can register a new fallback. Only defer a node whose
    // layout cannot change the font set used by subsequent nodes.
    #[doc(hidden)]
    fn can_defer_styled_text(&self, _node: &Node) -> bool {
        false
    }

    /// Distance from the top of a glyph line to its baseline.
    fn font_ascent(&mut self, _node: &Node, size: f32) -> f32 {
        size * 0.8
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
    local: Transform2D<LayoutSpace, LayoutSpace>,
    parent_clip: Rect,
    parent_space: Option<Rc<TransformSpace>>,
}

#[derive(Clone, Debug, Default)]
struct LayoutDetails {
    texture_only: bool,
    baseline_offset: f32,
    content_bounds: Option<Rect>,
    list_section_headers: Option<Box<[ListSectionHeader]>>,
    horizontal_scroll: Option<ScrollLayout>,
    horizontal_scroll_reversed: bool,
    nested_list: Option<xrack_runtime::ListRowRenderer>,
    attached_scroll_bar: Option<Box<scroll_bar::AttachedScrollBar>>,
    elastic: Option<overscroll::ElasticOptions>,
    tab_strip: Option<Rect>,
    modal_title: Option<Box<label::TitleLayout>>,
    group_title: Option<Box<label::TitleLayout>>,
    overlays: Option<Box<modal::OverlayIndex>>,
    paint_order: Option<xrack_runtime::ScenePlanSlice<usize>>,
    child_shape: Option<RoundedClip>,
    viewport_handle: Option<xrack_runtime::ViewportHandle>,
    transform: Option<Box<TransformPlacement>>,
    table_pane: Option<(Rect, Transform2D<LayoutSpace, LayoutSpace>)>,
    window_child_changes: Option<Rc<frame::WindowChildChanges>>,
}

impl PartialEq for LayoutDetails {
    fn eq(&self, other: &Self) -> bool {
        self.same_placement(other, None)
    }
}

impl LayoutDetails {
    fn same_placement(&self, other: &Self, clip_offset: Option<Point>) -> bool {
        macro_rules! compare {
            ($($field:ident),+ $(,)?) => {{
                let Self { child_shape, window_child_changes: _, $($field,)+ } = self;
                let shapes_match = match (*child_shape, other.child_shape, clip_offset) {
                    (Some(before), Some(after), Some(offset)) => before.translates_to(after, offset),
                    (before, after, _) => before == after,
                };
                shapes_match $(&& *$field == other.$field)+
            }};
        }
        compare!(
            texture_only,
            baseline_offset,
            content_bounds,
            list_section_headers,
            horizontal_scroll,
            horizontal_scroll_reversed,
            nested_list,
            attached_scroll_bar,
            elastic,
            tab_strip,
            modal_title,
            group_title,
            overlays,
            paint_order,
            viewport_handle,
            transform,
            table_pane,
        )
    }

    fn into_shared(self) -> Option<Rc<Self>> {
        (self != Self::default()).then(|| Rc::new(self))
    }
}

#[derive(Clone, Debug)]
pub struct LayoutNode {
    pub bounds: Rect,
    /// Rectangular part of the ancestor clip.
    pub clip: Rect,
    pub visible: bool,
    pub enabled: bool,
    pub layout_direction: LayoutDirection,
    direction_override: Option<LayoutDirection>,
    details: Option<Rc<LayoutDetails>>,
    pub scroll: Option<ScrollLayout>,
    missing_list_rows: bool,
    missing_descendant_list_rows: bool,
    mouse_drag_pan: bool,
    middle_mouse_pan: bool,
    pan_axes: [bool; 2],
    pull_to_refresh: bool,
    pub table: Option<Box<TableGeometry>>,
    pub split: Option<Box<SplitGeometry>>,
    /// Header hit box of a tab, independent of the page visibility.
    pub tab_header: Option<Rect>,
    /// Close button bounds within a tab header or standalone tab button.
    pub tab_close: Option<Rect>,
    pub tumbler: Option<Box<TumblerGeometry>>,
    /// Logical thumb position while a switch gesture is active.
    pub switch_position: Option<f32>,
    pub swipe_delegate: Option<Box<SwipeDelegateGeometry>>,
    #[doc(hidden)]
    pub swipe_preview: Option<Box<LayoutNode>>,
    pub styled_text: Option<Rc<StyledTextLayout>>,
    font_size: f32,
    overflow: bool,
    ancestor_shapes: Option<Rc<ClipChain>>,
    space: Option<Rc<TransformSpace>>,
    table_column_window: xrack_runtime::TableColumnWindow,
    child_storage: row_layout::ChildStorage,
}

pub use row_layout::LayoutNodeChildren;

impl std::ops::Deref for LayoutNode {
    type Target = LayoutNodeChildren;

    fn deref(&self) -> &Self::Target {
        self.child_storage.children()
    }
}

impl std::ops::DerefMut for LayoutNode {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.child_storage.children_mut()
    }
}

impl PartialEq for LayoutNode {
    fn eq(&self, other: &Self) -> bool {
        self.bounds == other.bounds
            && self.clip == other.clip
            && self.same_placement_state(other, None)
            && self.ancestor_shapes == other.ancestor_shapes
            && self.children == other.children
    }
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
        if node.expander_collapsed()
            || !node.contains_scene_feature(xrack_runtime::SceneFeature::WindowEffects)
        {
            return;
        }
        let runtime = node.runtime();
        if let Some(handle) = runtime
            .button_press_handle
            .as_ref()
            .filter(|handle| handle.supports_animation())
        {
            self.button_animations.push(handle);
        }
        self.timers.extend(layout.timer_registrations(node));
        if layout.visible && layout.enabled {
            self.dialogs.extend(&runtime.native_dialogs);
        }
        self.playback.extend(layout.image_playback(node));
        if layout.visible {
            self.media.extend(runtime.media.as_ref());
        }
        self.media.extend(&runtime.sound_effects);
        for (child, item) in node.children.iter().zip(&layout.children) {
            self.extend(child, item);
        }
    }
}

macro_rules! layout_placement_fields {
    ($apply:ident) => {
        $apply! {
            copy [
                visible,
                enabled,
                layout_direction,
                direction_override,
                scroll,
                missing_list_rows,
                missing_descendant_list_rows,
                mouse_drag_pan,
                middle_mouse_pan,
                pan_axes,
                pull_to_refresh,
                tab_header,
                tab_close,
                switch_position,
                font_size,
                overflow,
            ]
            optional [
                details,
                table,
                split,
                tumbler,
                swipe_delegate,
                swipe_preview,
                styled_text,
                space,
            ]
        }
    };
}

impl LayoutNode {
    fn same_plain_scroll_template(&self, other: &Self) -> bool {
        // Plain descendants carry geometry and scalar flags. Compare exact
        // coordinates to preserve signed zero when copying a template.
        macro_rules! has_optional_geometry {
            (copy [$($copy:ident,)*] optional [$($optional:ident,)*]) => {{
                $(self.$optional.is_some())||+
            }};
        }
        let rect_bits = |rect: Rect| [rect.x, rect.y, rect.width, rect.height].map(f32::to_bits);
        !layout_placement_fields!(has_optional_geometry)
            && self.scroll.is_none()
            && self.tab_header.is_none()
            && self.tab_close.is_none()
            && self.switch_position.is_none()
            && self.ancestor_shapes.is_none()
            && other.ancestor_shapes.is_none()
            && self.table_column_window == xrack_runtime::TableColumnWindow::default()
            && self.same_placement_state(other, None)
            && rect_bits(self.bounds) == rect_bits(other.bounds)
            && rect_bits(self.clip) == rect_bits(other.clip)
            && self.font_size.to_bits() == other.font_size.to_bits()
            && self.children.len() == other.children.len()
            && self
                .children
                .iter()
                .zip(&other.children)
                .all(|(left, right)| left.same_plain_scroll_template(right))
    }

    fn same_placement_state(&self, other: &Self, clip_offset: Option<Point>) -> bool {
        macro_rules! same_optional {
            (details) => {
                match (
                    self.details.as_deref(),
                    other.details.as_deref(),
                    clip_offset,
                ) {
                    (Some(before), Some(after), Some(offset))
                        if before.child_shape.is_some() || after.child_shape.is_some() =>
                    {
                        before.same_placement(after, Some(offset))
                    }
                    (Some(details), None, _) | (None, Some(details), _)
                        if details.window_child_changes.is_some() =>
                    {
                        *details == LayoutDetails::default()
                    }
                    _ => self.details == other.details,
                }
            };
            ($field:ident) => {
                self.$field == other.$field
            };
        }
        macro_rules! compare {
            (copy [$($copy:ident,)*] optional [$($optional:ident,)*]) => {{
                let Self {
                    bounds: _, clip: _, child_storage: _, ancestor_shapes: _,
                    table_column_window: _, $($copy: _,)* $($optional: _,)*
                } = self;
                $(self.$copy == other.$copy &&)*
                $(same_optional!($optional) &&)*
                self.table_column_window == other.table_column_window
            }};
        }
        layout_placement_fields!(compare)
    }

    #[inline]
    fn clone_with_placement(
        &self,
        bounds: Rect,
        clip: Rect,
        ancestors: Option<&Rc<ClipChain>>,
        children: Vec<Self>,
    ) -> Self {
        macro_rules! clone_placed {
            (copy [$($copy:ident,)*] optional [$($optional:ident,)*]) => {
                Self {
                    bounds,
                    clip,
                    ancestor_shapes: ancestors.or(self.ancestor_shapes.as_ref()).cloned(),
                    child_storage: children.into(),
                    table_column_window: self.table_column_window.clone(),
                    $($copy: self.$copy,)*
                    $($optional: self.$optional.clone(),)*
                }
            };
        }
        layout_placement_fields!(clone_placed)
    }

    #[inline]
    fn clone_placement_from(
        &mut self,
        template: &Self,
        bounds: Rect,
        clip: Rect,
        ancestors: Option<&Rc<ClipChain>>,
    ) {
        self.bounds = bounds;
        self.clip = clip;
        self.ancestor_shapes = ancestors.or(template.ancestor_shapes.as_ref()).cloned();
        macro_rules! copy_placed {
            (copy [$($copy:ident,)*] optional [$($optional:ident,)*]) => {{
                $(self.$copy = template.$copy;)*
                // Most scroll descendants carry none of the optional
                // geometry. Test these fields together before cloning any.
                if $(self.$optional.is_some() | template.$optional.is_some())|+ {
                    self.clone_optional_placement_from(template);
                }
            }};
        }
        layout_placement_fields!(copy_placed);
        self.table_column_window
            .clone_from(&template.table_column_window);
    }

    #[cold]
    #[inline(never)]
    fn clone_optional_placement_from(&mut self, template: &Self) {
        macro_rules! clone_optional {
            (copy [$($copy:ident,)*] optional [$($optional:ident,)*]) => {{
                $(self.$optional.clone_from(&template.$optional);)*
            }};
        }
        layout_placement_fields!(clone_optional)
    }

    #[inline]
    fn clone_plain_placement_from(
        &mut self,
        template: &Self,
        bounds: Rect,
        clip: Rect,
        ancestors: Option<&Rc<ClipChain>>,
    ) {
        self.bounds = bounds;
        self.clip = clip;
        self.ancestor_shapes = ancestors.cloned();
        // Shared templates have verified these fields. Recycled rows can
        // still carry geometry from an earlier placement.
        macro_rules! copy_value {
            (scroll) => {
                self.scroll = None
            };
            (tab_header) => {
                self.tab_header = None
            };
            (tab_close) => {
                self.tab_close = None
            };
            (switch_position) => {
                self.switch_position = None
            };
            ($field:ident) => {
                self.$field = template.$field
            };
        }
        macro_rules! copy_placed {
            (copy [$($copy:ident,)*] optional [$($optional:ident,)*]) => {{
                $(copy_value!($copy);)*
                if $(self.$optional.is_some())|+ {
                    self.clone_optional_placement_from(template);
                }
            }};
        }
        layout_placement_fields!(copy_placed);
        self.table_column_window = Default::default();
    }

    /// Compare a retained layout with a new placement in one traversal.
    /// The caller compares bounds and rectangular clipping for each node.
    #[doc(hidden)]
    pub fn matches_translated(
        &self,
        other: &Self,
        rebind_outer_clip: bool,
        mut geometry_matches: impl FnMut(&Self, &Self) -> bool,
    ) -> bool {
        type ClipPair<'a> = [Option<&'a ClipChain>; 2];

        fn same_clip_pointer(left: Option<&ClipChain>, right: Option<&ClipChain>) -> bool {
            match (left, right) {
                (Some(left), Some(right)) => std::ptr::eq(left, right),
                (None, None) => true,
                _ => false,
            }
        }

        fn clips_match(clips: ClipPair<'_>, outer: ClipPair<'_>, offset: Point) -> bool {
            let reached_outer = [
                same_clip_pointer(clips[0], outer[0]),
                same_clip_pointer(clips[1], outer[1]),
            ];
            if reached_outer[0] || reached_outer[1] {
                return reached_outer == [true; 2];
            }
            let [Some(before), Some(after)] = clips else {
                return false;
            };
            // Clips introduced inside the capture must move rigidly with it.
            before.space.is_none()
                && after.space.is_none()
                && before.shape.translates_to(after.shape, offset)
                && clips_match(
                    [before.parent.as_deref(), after.parent.as_deref()],
                    outer,
                    offset,
                )
        }

        fn compare(
            left: &LayoutNode,
            right: &LayoutNode,
            external_clip: Option<(Point, ClipPair<'_>)>,
            geometry_matches: &mut impl FnMut(&LayoutNode, &LayoutNode) -> bool,
        ) -> bool {
            if !geometry_matches(left, right)
                || !left.same_placement_state(right, external_clip.map(|(offset, _)| offset))
                || left.children.len() != right.children.len()
            {
                return false;
            }
            let children_clip = if let Some((offset, outer)) = external_clip {
                let clips = [
                    left.ancestor_shapes.as_deref(),
                    right.ancestor_shapes.as_deref(),
                ];
                if !clips_match(clips, outer, offset) {
                    return false;
                }
                Some((offset, clips))
            } else {
                if left.ancestor_shapes != right.ancestor_shapes {
                    return false;
                }
                None
            };
            left.children
                .iter()
                .zip(&right.children)
                .all(|(left, right)| compare(left, right, children_clip, geometry_matches))
        }

        // The enclosing clip is applied when compositing the texture.
        // Compare only those clips that were actually drawn into it.
        let external_clip = rebind_outer_clip.then(|| {
            let offset = Point {
                x: other.bounds.x - self.bounds.x,
                y: other.bounds.y - self.bounds.y,
            };
            (
                offset,
                [
                    self.ancestor_shapes.as_deref(),
                    other.ancestor_shapes.as_deref(),
                ],
            )
        });
        compare(self, other, external_clip, &mut geometry_matches)
    }

    fn leaf(
        node: &Node,
        bounds: Rect,
        clip: Rect,
        visible: bool,
        enabled: bool,
        direction: Option<LayoutDirection>,
        font_size: f32,
    ) -> Self {
        Self {
            bounds,
            clip,
            visible,
            enabled,
            layout_direction: direction.unwrap_or_default(),
            direction_override: direction,
            details: None,
            scroll: None,
            missing_list_rows: false,
            missing_descendant_list_rows: false,
            mouse_drag_pan: false,
            middle_mouse_pan: false,
            pan_axes: [false; 2],
            pull_to_refresh: false,
            table: None,
            split: None,
            tab_header: None,
            tab_close: None,
            tumbler: None,
            switch_position: None,
            swipe_delegate: None,
            swipe_preview: None,
            styled_text: None,
            font_size,
            overflow: false,
            ancestor_shapes: None,
            space: None,
            table_column_window: node.table_column_window.clone(),
            child_storage: Default::default(),
        }
    }

    pub(crate) fn is_texture_only(&self) -> bool {
        self.details
            .as_ref()
            .is_some_and(|details| details.texture_only)
    }

    fn details_mut(&mut self) -> &mut LayoutDetails {
        Rc::make_mut(
            self.details
                .get_or_insert_with(|| Rc::new(LayoutDetails::default())),
        )
    }

    /// Return paths and bounds of rectangles changed in this exact frame pair.
    #[doc(hidden)]
    pub fn window_child_changes_from(
        &self,
        before: &Self,
        before_scene: &Node,
        current_scene: &Node,
    ) -> Option<&[(Vec<usize>, Rect)]> {
        self.details
            .as_deref()?
            .window_child_changes
            .as_deref()?
            .between(before, self, before_scene, current_scene)
    }

    fn overlay_index(&self) -> Option<&modal::OverlayIndex> {
        self.details
            .as_deref()
            .and_then(|details| details.overlays.as_deref())
    }

    fn set_overlay_index(&mut self, overlays: Option<Box<modal::OverlayIndex>>) {
        if overlays.is_some() || self.overlay_index().is_some() {
            let details = self.details_mut();
            details.overlays = overlays;
            if *details == LayoutDetails::default() {
                self.details = None;
            }
        }
    }

    fn nested_list_renderer(&self) -> Option<&xrack_runtime::ListRowRenderer> {
        self.details
            .as_deref()
            .and_then(|details| details.nested_list.as_ref())
    }

    fn attached_scroll_bar(&self) -> Option<&scroll_bar::AttachedScrollBar> {
        self.details
            .as_deref()
            .and_then(|details| details.attached_scroll_bar.as_deref())
    }

    fn elastic_options(&self) -> Option<overscroll::ElasticOptions> {
        self.details.as_deref().and_then(|details| details.elastic)
    }

    fn tab_strip(&self) -> Option<Rect> {
        self.details
            .as_deref()
            .and_then(|details| details.tab_strip)
    }

    /// Distance from the item's top edge to its first text baseline.
    pub fn baseline_offset(&self) -> f32 {
        self.details
            .as_deref()
            .map_or(0.0, |details| details.baseline_offset)
    }

    fn content_bounds(&self) -> Option<Rect> {
        self.details
            .as_deref()
            .and_then(|details| details.content_bounds)
    }

    fn modal_title(&self) -> Option<&label::TitleLayout> {
        self.details
            .as_deref()
            .and_then(|details| details.modal_title.as_deref())
    }

    fn group_title_layout(&self) -> Option<&label::TitleLayout> {
        self.details
            .as_deref()
            .and_then(|details| details.group_title.as_deref())
    }

    fn paint_order(&self) -> Option<&xrack_runtime::ScenePlanSlice<usize>> {
        self.details
            .as_deref()
            .and_then(|details| details.paint_order.as_ref())
    }

    pub(crate) fn popup_typography(&self) -> popup::PopupTypography {
        self.overlay_index()
            .map_or_else(popup::PopupTypography::default, |index| {
                index.popup_typography
            })
    }

    pub(crate) fn floating_keyboard(&self) -> Option<Rect> {
        self.overlay_index()
            .and_then(|index| index.floating_keyboard)
    }

    /// Number of columns shared with GridView's virtual cell layout.
    #[doc(hidden)]
    pub fn grid_view_columns(&self, node: &Node) -> usize {
        ViewCells::new(node, self.bounds.width).columns
    }

    pub fn list_section_headers(&self) -> &[ListSectionHeader] {
        self.details
            .as_deref()
            .and_then(|details| details.list_section_headers.as_deref())
            .unwrap_or_default()
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
        self.details
            .as_deref()
            .and_then(|details| details.child_shape)
    }

    fn viewport_handle(&self) -> Option<&xrack_runtime::ViewportHandle> {
        self.details
            .as_deref()
            .and_then(|details| details.viewport_handle.as_ref())
    }

    fn transform_placement(&self) -> Option<&TransformPlacement> {
        self.details
            .as_deref()
            .and_then(|details| details.transform.as_deref())
    }

    fn set_transform_placement(&mut self, transform: Option<Box<TransformPlacement>>) {
        if transform.is_some() || self.transform_placement().is_some() {
            let details = self.details_mut();
            details.transform = transform;
            if *details == LayoutDetails::default() {
                self.details = None;
            }
        }
    }

    pub fn local_transform(&self) -> Transform2D<LayoutSpace, LayoutSpace> {
        self.transform_placement()
            .map_or(Transform2D::IDENTITY, |transform| transform.local)
    }

    /// An exact table pane clip, expressed in the parent space before this item's transform.
    pub fn table_pane_clip(&self) -> Option<(Rect, Transform2D<LayoutSpace, LayoutSpace>)> {
        self.details
            .as_deref()
            .and_then(|details| details.table_pane)
    }

    pub fn world_transform(&self) -> Transform2D<LayoutSpace, WindowSpace> {
        self.space
            .as_deref()
            .map_or(Transform2D::IDENTITY, |space| space.world)
    }

    fn inverse_transform(&self) -> Option<Transform2D<WindowSpace, LayoutSpace>> {
        self.space
            .as_deref()
            .map_or(Some(Transform2D::IDENTITY), |space| space.inverse)
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

    pub(crate) fn layout_to_local(&self, point: Point) -> Point {
        Point {
            x: point.x - self.bounds.x,
            y: point.y - self.bounds.y,
        }
    }

    pub(crate) fn local_to_layout(&self, point: Point) -> Point {
        Point {
            x: point.x + self.bounds.x,
            y: point.y + self.bounds.y,
        }
    }

    pub(crate) fn window_to_local(&self, point: Point) -> Option<Point> {
        let local = self.layout_to_local(self.map_from_window(point)?);
        (local.x.is_finite() && local.y.is_finite()).then_some(local)
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

    #[doc(hidden)]
    pub fn has_ancestor_clip_chain(&self) -> bool {
        self.ancestor_shapes.is_some()
    }

    /// Rebind a shared clip applied outside a retained subtree raster.
    /// Clips introduced inside that subtree still participate in equality.
    #[doc(hidden)]
    pub fn rebind_shared_ancestor_clip(&mut self, next: &Self) {
        if self.ancestor_shapes == next.ancestor_shapes {
            return;
        }
        fn shares_clip(node: &LayoutNode, ancestor: Option<&Rc<ClipChain>>) -> bool {
            match (node.ancestor_shapes.as_ref(), ancestor) {
                (Some(node), Some(ancestor)) => Rc::ptr_eq(node, ancestor),
                (None, None) => true,
                _ => false,
            }
        }
        fn rebind(
            node: &mut LayoutNode,
            next: &LayoutNode,
            previous: Option<&Rc<ClipChain>>,
            incoming: Option<&Rc<ClipChain>>,
        ) {
            if !shares_clip(node, previous) || !shares_clip(next, incoming) {
                return;
            }
            node.ancestor_shapes = incoming.cloned();
            for (child, next) in node.children.iter_mut().zip(&next.children) {
                rebind(child, next, previous, incoming);
            }
        }
        let previous = self.ancestor_shapes.clone();
        rebind(self, next, previous.as_ref(), next.ancestor_shapes.as_ref());
    }

    pub fn visible_window_bounds(&self, rect: Rect) -> Rect {
        let rect = self.window_bounds(self.clip_rect(rect));
        // The local projection can overestimate after a rotation; bound it
        // once more in window space for anchors and native accessibility.
        self.ancestor_shapes
            .as_deref()
            .map_or(rect, |chain| chain.clip_window_rect(rect))
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
        self.child_paint_range(0..self.children.len())
    }

    fn child_paint_range(
        &self,
        range: std::ops::Range<usize>,
    ) -> impl DoubleEndedIterator<Item = usize> + '_ {
        let paint_order = self.paint_order();
        range.map(move |index| {
            let dense = paint_order.map_or(index, |order| order[index]);
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

    #[doc(hidden)]
    pub fn children_in_paint_order_trimming_edges<'a>(
        &'a self,
        node: &'a Node,
        trim: bool,
        discard: impl Fn(&Node, &Self) -> bool,
    ) -> impl DoubleEndedIterator<Item = (usize, &'a Node, &'a Self)> + 'a {
        let mut range = 0..self.children.len();
        if trim
            && node.children.len() == self.children.len()
            && self.table_column_window == xrack_runtime::TableColumnWindow::default()
            && node.table_column_window == xrack_runtime::TableColumnWindow::default()
        {
            // Trim the edges of the paint order to keep batch boundaries
            // among retained children, including when z changes their order.
            if let Some(order) = self.paint_order() {
                if order.len() == range.end {
                    let keep = |&dense: &usize| {
                        node.children
                            .get(dense)
                            .zip(self.children.get(dense))
                            .is_some_and(|(child, layout)| !discard(child, layout))
                    };
                    range.start = order.iter().position(keep).unwrap_or(range.end);
                    if let Some(last) = order[range.start..].iter().rposition(keep) {
                        range.end = range.start + last + 1;
                    }
                }
            } else {
                range.start = node
                    .children
                    .iter()
                    .zip(&self.children)
                    .position(|(child, layout)| !discard(child, layout))
                    .unwrap_or(range.end);
                if let Some(last) = node.children[range.start..]
                    .iter()
                    .zip(&self.children[range.start..])
                    .rposition(|(child, layout)| !discard(child, layout))
                {
                    range.end = range.start + last + 1;
                }
            }
        }
        self.child_paint_range(range)
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

    #[doc(hidden)]
    pub fn measure_styled_text(
        &self,
        node: &Node,
        measure: impl FnOnce(Size) -> Option<Rc<StyledTextLayout>>,
    ) -> Option<Rc<StyledTextLayout>> {
        styled_text_at(node, self.bounds, self.direction_override, measure)
    }

    pub fn text_link_regions(&self) -> impl Iterator<Item = (usize, Rect)> + '_ {
        let top = self.clip.y;
        let bottom = top + self.clip.height;
        self.styled_text
            .iter()
            .filter(|_| self.visible)
            .flat_map(move |plan| {
                plan.fragments
                    .placed_in_y_range(Some((top - self.bounds.y, bottom - self.bounds.y)))
            })
            .filter_map(move |(fragment, offset)| {
                let link = fragment.link()?;
                let rect = fragment.bounds;
                let y = self.bounds.y + (rect.y + offset);
                // Reject rows outside the local viewport before projecting
                // this fragment through its transformed ancestor clips.
                if y > bottom || y + rect.height < top {
                    return None;
                }
                let rect = Rect::new(self.bounds.x + rect.x, y, rect.width, rect.height)
                    .intersect(self.bounds);
                let visible = self.clip_rect(rect);
                (visible.width > 0.0 && visible.height > 0.0).then_some((link, visible))
            })
    }

    /// Reserved title height using the text metrics prepared for this layout.
    pub fn dialog_title_height(&self, _node: &Node) -> f32 {
        self.modal_title().map_or(0.0, |title| title.height)
    }

    #[doc(hidden)]
    pub fn dialog_title_lines(&self) -> &[label::PlannedLabelLine] {
        self.modal_title()
            .map_or(&[], |title| title.lines.as_slice())
    }

    #[doc(hidden)]
    pub fn dialog_title_font_size(&self) -> Option<f32> {
        self.modal_title().map(|title| title.font_size)
    }

    pub fn group_header(&self, node: &Node) -> Rect {
        if let Some(title) = self.group_title_layout() {
            group_box::header_bounds(node, self.bounds, title.height)
        } else {
            group_box::header_with_font_size(node, self.bounds, self.font_size)
        }
    }

    #[doc(hidden)]
    pub fn group_title(&self) -> Option<(f32, &[PlannedLabelLine])> {
        self.group_title_layout()
            .map(|title| (title.font_size, title.lines.as_slice()))
    }

    pub(crate) fn tab_viewport(&self) -> Rect {
        self.tab_strip().unwrap_or(self.bounds)
    }

    pub(crate) fn tab_item_rect(&self, index: usize) -> Option<Rect> {
        let child = self.child_at(index)?;
        if self.tab_strip().is_some() {
            child.tab_header
        } else {
            child.visible.then_some(child.bounds)
        }
    }

    pub fn scroll_on(&self, orientation: Orientation) -> Option<ScrollLayout> {
        match orientation {
            Orientation::Horizontal => {
                self.table
                    .as_ref()
                    .map(|table| table.horizontal)
                    .or_else(|| {
                        self.details
                            .as_deref()
                            .and_then(|details| details.horizontal_scroll)
                    })
            }
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
        self.overlay_index()
            .map_or(&[], |overlays| overlays.modal.as_slice())
    }

    /// Paths of all open overlays, in declaration order.
    pub fn overlay_paths(&self) -> &[Vec<usize>] {
        self.overlay_index()
            .map_or(&[], |overlays| overlays.open.as_slice())
    }

    pub(crate) fn drawer_paths(&self) -> &[Vec<usize>] {
        self.overlay_index()
            .map_or(&[], |overlays| overlays.drawers.as_slice())
    }

    /// A closed drawer can reserve the window's leading edge for its own gesture.
    #[doc(hidden)]
    pub fn has_leading_edge_drawer(&self, scene: &Node) -> bool {
        let leading = match self.layout_direction {
            LayoutDirection::LeftToRight => xrack_runtime::DrawerEdge::Left,
            LayoutDirection::RightToLeft => xrack_runtime::DrawerEdge::Right,
        };
        let viewport = window_safe_area(scene, self.bounds);
        self.drawer_paths().iter().any(|path| {
            scene
                .control_target(path, xrack_runtime::ControlAction::Drawer(true))
                .is_some_and(|node| {
                    !node.boolean("open", false)
                        && node.drawer_edge() == leading
                        && modal::drawer_margin(node, modal::drawer_bounds(node, viewport)) > 0.0
                })
        })
    }

    /// Take the owned children, resolving any deferred coordinates.
    pub fn into_children(self) -> Vec<Self> {
        self.child_storage.into_children()
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
    let window = matches!(node.kind, "Window" | "ApplicationWindow");
    let stacking = |child: &Node| {
        let layer = if window {
            child.scene_view_render_mode().window_layer()
        } else {
            0
        };
        (layer, child.z())
    };
    // Already ordered (including the default of all zeroes) needs no storage.
    if children.iter().map(stacking).is_sorted() {
        return None;
    }
    let mut order: Vec<_> = (0..children.len()).collect();
    order.sort_by(|&left, &right| {
        let (left_layer, left_z) = stacking(&children[left]);
        let (right_layer, right_z) = stacking(&children[right]);
        left_layer
            .cmp(&right_layer)
            .then_with(|| left_z.total_cmp(&right_z))
    });
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
    match item::paint(node) {
        item::ItemPaint::Toggle(item::ToggleStyle::Switch) => Size::new(36.0, 18.0),
        item::ItemPaint::Toggle(item::ToggleStyle::Radio) => Size::new(16.0, 16.0),
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
        && !node.scene_texture_only()
        && node.is_visible()
}

pub fn is_layout(kind: &str) -> bool {
    matches!(
        item::item(kind).layout(),
        ItemLayout::Grid
            | ItemLayout::Row
            | ItemLayout::LineOrFlow(_)
            | ItemLayout::Overlay(_)
            | ItemLayout::Split
    )
}

/// Child indices in visual reading order. Paths keep their original scene indices.
pub fn child_reading_order(node: &Node) -> impl Iterator<Item = usize> {
    let slots @ [menu, header, footer] = page::slots(node);
    let count = node.table_column_window.logical_len(node.children.len());
    let (ordered, len) = item::item(node.kind)
        .reading_order(node, count)
        .map_or_else(|| (Vec::new(), node.children.len()), |order| (order, 0));
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
    layout_current_window(scene, viewport, text, offsets, input, None)
}

fn layout_current_window(
    scene: &Node,
    viewport: Size,
    text: &mut impl TextMeasurer,
    offsets: &BTreeMap<Vec<usize>, f32>,
    input: Option<&Interaction>,
    row_layouts: Option<&mut RowLayoutCache>,
) -> LayoutNode {
    if let Some(cache) = text.intrinsic_measurements() {
        cache.begin_frame();
    }
    let bounds = Rect::new(0.0, 0.0, viewport.width.max(0.0), viewport.height.max(0.0));
    let safe_viewport = window_safe_area(scene, bounds);
    let mut layout = LayoutContext {
        defer_styled_text: false,
        text,
        layout_direction: None,
        offsets,
        input,
        path: Vec::new(),
        row_key: None,
        anchor: None,
        resolved_list_scroll: None,
        resolved_table: None,
        cache_table_sizes: true,
        row_layouts,
        ancestor_shapes: None,
        space: None,
        viewport: safe_viewport,
    }
    .place(scene, bounds, bounds, true, true, None);
    modal::index_overlays(
        scene,
        &mut layout,
        popup::PopupTypography::from_text(text),
        input.and_then(Interaction::floating_keyboard),
    );
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

/// Supply the host's mirrored layout default to a prepared window scene.
#[doc(hidden)]
pub fn apply_system_layout_direction(mut scene: Rc<Node>, direction: LayoutDirection) -> Rc<Node> {
    if direction == LayoutDirection::RightToLeft && scene.layout_direction().is_none() {
        Rc::make_mut(&mut scene).properties.insert(
            "layout-direction",
            xrack_runtime::Value::LayoutDirection(direction),
        );
    }
    scene
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
        defer_styled_text: false,
        text,
        layout_direction: Some(direction),
        offsets: &BTreeMap::new(),
        input: None,
        path: Vec::new(),
        row_key: None,
        anchor: None,
        resolved_list_scroll: None,
        resolved_table: None,
        cache_table_sizes: false,
        row_layouts: None,
        ancestor_shapes: None,
        space: None,
        viewport: bounds,
    }
    .place(node, bounds, bounds, true, enabled, None)
}

/// Prepare only the chrome of a closed drawer for an opening gesture.
#[doc(hidden)]
pub fn layout_drawer_opening(
    node: &Node,
    viewport: Rect,
    reveal: f32,
    text: &mut impl TextMeasurer,
) -> (Node, LayoutNode, Point) {
    let mut shell = node.clone();
    shell.properties.insert("open", true.into());
    shell.children.clear();
    let item = layout_choice_option(
        &shell,
        viewport,
        node.layout_direction().unwrap_or_default(),
        true,
        text,
    );
    let shown = modal::drawer_revealed_bounds(node.drawer_edge(), item.bounds, reveal);
    let displacement = Point {
        x: shown.x - item.bounds.x,
        y: shown.y - item.bounds.y,
    };
    (shell, item, displacement)
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
    mut row_layouts: Option<&mut RowLayoutCache>,
) -> (Rc<Node>, LayoutNode) {
    text.begin_window(&scene);
    if let Some(cache) = row_layouts.as_deref_mut() {
        cache.begin_frame(&scene, text.typography_revision());
    }
    let families = text.font_families();
    if !families.is_empty()
        && let Some(populated) = populate_font_choices(&scene, families)
    {
        scene = Rc::new(populated);
    }
    // Resolve viewport geometry first. Virtual views use its visible range
    // to request rows and columns from the component before publication.
    let mut layout = layout_current_window(
        &scene,
        viewport,
        text,
        offsets,
        input,
        row_layouts.as_deref_mut(),
    );
    if scene.contains_scene_feature(xrack_runtime::SceneFeature::VirtualList) {
        let viewport = Rect::new(0.0, 0.0, viewport.width.max(0.0), viewport.height.max(0.0));
        LayoutContext {
            defer_styled_text: false,
            text,
            layout_direction: None,
            offsets,
            input,
            path: Vec::new(),
            row_key: None,
            anchor: None,
            resolved_list_scroll: None,
            resolved_table: None,
            cache_table_sizes: false,
            row_layouts,
            ancestor_shapes: None,
            space: None,
            viewport,
        }
        .materialize_visible_rows(component, Rc::make_mut(&mut scene), &mut layout);
        modal::index_overlays(
            &scene,
            &mut layout,
            popup::PopupTypography::from_text(text),
            input.and_then(Interaction::floating_keyboard),
        );
    }
    scroll_bar::attach(&scene, &mut layout, text);
    (scene, layout)
}

fn visible_view_rows(node: &Node, layout: &LayoutNode, row_count: usize) -> std::ops::Range<usize> {
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
    if let Some(scroll) = layout.scroll
        && clipped.width > 0.0
        && clipped.height > 0.0
    {
        let top = f64::from(scroll.offset) + f64::from(clipped.y - viewport.y);
        let bottom = top + f64::from(clipped.height);
        ViewCells::new(node, viewport.width).visible(top, bottom, row_count)
    } else {
        0..0
    }
}

fn populate_font_choices(node: &Node, families: &[String]) -> Option<Node> {
    if !node.contains_scene_feature(xrack_runtime::SceneFeature::FontFamilyChoice) {
        return None;
    }
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
    children: Vec<Option<ScrollChildMeasurement>>,
}

#[derive(Clone, Copy)]
struct ScrollChildMeasurement {
    x: f32,
    y: f32,
    width: f32,
    height: ScrollChildHeight,
}

#[derive(Clone, Copy)]
enum ScrollChildHeight {
    Preferred(f32),
    Available(linear::AxisInfo),
}

impl ScrollChildMeasurement {
    fn bounds(self, content_height: f32) -> Rect {
        let height = match self.height {
            ScrollChildHeight::Preferred(height) => height,
            ScrollChildHeight::Available(info) => {
                info.clamp((content_height - self.y.max(0.0)).max(0.0))
            }
        };
        Rect::new(self.x, self.y, self.width, height)
    }

    fn preferred_height(self) -> f32 {
        match self.height {
            ScrollChildHeight::Preferred(height) => height,
            ScrollChildHeight::Available(info) => info.preferred,
        }
    }
}

struct ScrollPlacement {
    local: Rc<[Rect]>,
    origin: Point,
}

impl ScrollPlacement {
    fn rects(&self) -> impl ExactSizeIterator<Item = Rect> + Clone + '_ {
        self.local.iter().map(|&rect| Rect {
            x: rect.x + self.origin.x,
            y: rect.y + self.origin.y,
            ..rect
        })
    }
}

fn unclipped_content() -> Rect {
    Rect::new(
        -f32::MAX / 4.0,
        -f32::MAX / 4.0,
        f32::MAX / 2.0,
        f32::MAX / 2.0,
    )
}

fn styled_text_at(
    node: &Node,
    bounds: Rect,
    direction: Option<LayoutDirection>,
    measure: impl FnOnce(Size) -> Option<Rc<StyledTextLayout>>,
) -> Option<Rc<StyledTextLayout>> {
    let mut plan = measure(Size::new(bounds.width, bounds.height))?;
    if let Some(direction) = direction {
        Rc::make_mut(&mut plan).align_in_direction(node, bounds.width, direction);
    }
    Some(plan)
}

fn outside_clip(bounds: Rect, clip: Rect) -> bool {
    let visible = bounds.intersect(clip);
    visible.width <= 0.0 || visible.height <= 0.0
}

struct LayoutContext<'a, T> {
    defer_styled_text: bool,
    text: &'a mut T,
    layout_direction: Option<LayoutDirection>,
    offsets: &'a BTreeMap<Vec<usize>, f32>,
    input: Option<&'a Interaction>,
    path: Vec<usize>,
    row_key: Option<RowKey>,
    anchor: Option<Rect>,
    resolved_list_scroll: Option<ScrollLayout>,
    resolved_table: Option<Box<TableGeometry>>,
    cache_table_sizes: bool,
    row_layouts: Option<&'a mut RowLayoutCache>,
    ancestor_shapes: Option<Rc<ClipChain>>,
    space: Option<Rc<TransformSpace>>,
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

    fn materialize_visible_rows(
        &mut self,
        component: &impl Component,
        node: &mut Node,
        layout: &mut LayoutNode,
    ) {
        if (node.list.is_none() && node.children.is_empty())
            || !node.contains_scene_feature(xrack_runtime::SceneFeature::ListPreparation)
        {
            return;
        }
        let previous_row = self.row_key;
        let previous_direction = self.layout_direction;
        self.row_key = node.routing_row_key().or(previous_row);
        self.layout_direction = layout.direction_override;
        let mut prepared_table = None;
        if let Some(mut list) = node.list
            && !(list.row_count == 0
                && node
                    .children
                    .first()
                    .is_some_and(|child| child.kind == "EmptyState"))
        {
            let range = visible_view_rows(node, layout, list.row_count);
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
            let renderer = node.runtime().list_row_renderer.as_ref();
            let scope = renderer
                .filter(|_| self.row_key.is_some())
                .map_or(0, xrack_runtime::ListRowRenderer::scope);
            let expects_rows = !range.is_empty();
            // Pass geometry's visible rows and columns across the component
            // boundary, then place the returned delegates in the same frame.
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
            let grid = selection.filter(|view| view.target() != SelectionTarget::Rows);
            // The wrapper for a table row carries its routing key, optional
            // header, and row selection. Cell selection edits the delegate.
            let mut prepared_rows = if node.kind == "TableView"
                && grid.is_none()
                && tree.is_none()
                && tree_selection.is_none()
            {
                self.row_layouts.as_deref_mut().and_then(|cache| {
                    cache.prepared_table_rows(
                        &self.path,
                        &node.table_column_order,
                        scope,
                        row_headers,
                        list.first_row,
                    )
                })
            } else {
                None
            };
            let families = self.text.font_families();
            let children = delegates
                .into_iter()
                .enumerate()
                .map(|(slot, mut delegate)| {
                    if !families.is_empty()
                        && let Some(populated) = populate_font_choices(&delegate, families)
                    {
                        delegate = populated;
                    }
                    let index = list.first_row + slot;
                    let current = selected == Some(index as i64);
                    let tree_item = tree.and_then(|tree| tree.rows().get(index));
                    let is_selected = if let Some(keys) = tree_selection {
                        tree_item.is_some_and(|item| {
                            delegate.row_key == Some(item.row)
                                && keys.get(&item.key) == Some(&item.row)
                        })
                    } else {
                        selection.map_or(current, |view| {
                            delegate
                                .row_key
                                .is_some_and(|key| view.keys().contains(&key))
                        })
                    };
                    let mark_current = current
                        && (node.kind == "GridView"
                            || tree_selection.is_some()
                            || selection.is_some() && grid.is_none());
                    let flags = [is_selected, mark_current];
                    let source = if let Some(cache) = &mut prepared_rows
                        && delegate.kind == "TableRow"
                        && delegate.row_key.is_some()
                    {
                        if let Some(row) = cache.take(index, &delegate, flags) {
                            return row;
                        }
                        (!delegate.owns_storage()).then(|| delegate.snapshot_identity())
                    } else {
                        None
                    };
                    let mut row = Node::new("ListRow", None);
                    row.row_key = delegate.row_key.take();
                    row.row_scope = scope;
                    if delegate.kind == "TableRow" {
                        delegate.table_column_order = node.table_column_order.clone();
                        if row_headers {
                            let title = delegate
                                .text("row-header-text")
                                .map_or_else(|| (index + 1).to_string(), str::to_owned);
                            row.properties.insert("row-header-text", title.into());
                        }
                    }
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
                    if mark_current {
                        row.properties
                            .insert("current", xrack_runtime::Value::Bool(true));
                    }
                    row.children.push(delegate);
                    if let Some(cache) = &mut prepared_rows {
                        cache.push(source, flags, &row);
                    }
                    row
                })
                .collect();
            if node.table_header.is_some()
                && node.swipe_preview.is_none()
                && let Some(cache) = self.row_layouts.as_deref_mut()
            {
                prepared_table = cache.reuse_prepared_table(&self.path, node);
            }
            node.children = children;
            node.list = Some(list);
            let (incoming_clip, incoming_shapes, incoming_space) =
                if let Some(placement) = layout.transform_placement() {
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
            self.resolved_table = layout.table.take();
            let pane = layout.table_pane_clip();
            *layout = self.place(
                node,
                layout.bounds,
                incoming_clip,
                layout.visible,
                layout.enabled,
                None,
            );
            self.resolved_table = None;
            if let Some(pane) = pane {
                layout.details_mut().table_pane = Some(pane);
            }
            layout.missing_list_rows = missing_list_rows;
            self.ancestor_shapes = shapes;
            self.space = space;
        }
        self.populate_section_headers(node, layout);
        let columns = node.table_column_window.clone();
        for (index, (child, item)) in node
            .children
            .iter_mut()
            .zip(&mut layout.child_storage.children_mut().children)
            .enumerate()
        {
            self.path.push(columns.logical_index(index));
            self.materialize_visible_rows(component, child, item);
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
            self.materialize_visible_rows(component, page, item);
            self.path.pop();
            scroll_bar::attach(page, item, self.text);
        }
        if let Some(slot) = prepared_table
            && let Some(cache) = self.row_layouts.as_deref_mut()
        {
            cache.remember_prepared_table(&self.path, slot, node);
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
        let Some(headers) = layout
            .details
            .as_mut()
            .filter(|details| details.list_section_headers.is_some())
            .and_then(|details| Rc::make_mut(details).list_section_headers.as_deref_mut())
        else {
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
        if node.scene_texture_only() {
            let size = |axis| {
                node.length(axis)
                    .filter(|value| value.is_finite() && *value > 0.0)
                    .unwrap_or(0.0)
            };
            let canvas = Rect::new(0.0, 0.0, size("width"), size("height"));
            let outside = self.space.take();
            let previous_direction = self.layout_direction;
            self.layout_direction = node.layout_direction().or(previous_direction);
            let mut layout = self.place_without_ancestor_clip(
                node,
                canvas,
                canvas,
                parent_visible,
                parent_enabled,
            );
            layout.details_mut().texture_only = true;
            self.observe_item_baseline(node, &mut layout);
            self.space = outside;
            self.layout_direction = previous_direction;
            return layout;
        }
        // Text with a row key can reuse its layout through the row cache below.
        if row_children.is_none()
            && (matches!(
                node.kind,
                "Rectangle" | "Image" | "AnimatedImage" | "TableCell" | "TreeCell"
            ) || node.row_key.is_none() && matches!(node.kind, "Label" | "Text" | "StyledText"))
            && self.can_place_leaf(node)
        {
            let mut leaf = self.place_leaf(node, bounds, clip, parent_visible, parent_enabled);
            if leaf.visible && node.kind == "StyledText" && !self.defer_styled_text {
                leaf.styled_text = styled_text_at(node, bounds, leaf.direction_override, |size| {
                    self.text.shared_styled_text_layout(node, size)
                });
            }
            self.observe_item_baseline(node, &mut leaf);
            return leaf;
        }
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
            && self.row_layouts.is_some();
        let mut layout = if row_cache && let Some(row) = node.routing_row_key() {
            let slot = row_layout::Slot { row, column: None };
            let inputs = row_layout::RowInputs {
                size: Size::new(bounds.width, bounds.height),
                visible: parent_visible,
                enabled: parent_enabled,
                layout_direction: self.layout_direction,
            };
            self.place_cached_content(node, bounds, clip, inputs, slot)
        } else if row_children.is_none()
            && self.row_layouts.is_some()
            && self.space.is_none()
            && let Some(kind) = row_layout::LocalSubtree::for_node(node)
        {
            self.place_local_subtree(node, bounds, clip, parent_visible, parent_enabled, kind)
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
        self.observe_item_baseline(node, &mut layout);
        self.row_key = outer_key;
        self.layout_direction = outer_direction;
        self.anchor = outer_anchor;
        self.ancestor_shapes = outer_shapes;
        self.space = outer_space;
        layout
    }

    fn observe_item_baseline(&mut self, node: &Node, layout: &mut LayoutNode) {
        if layout.visible
            && (node.geometry_handle.is_some() || node.instance_geometry_handle.is_some())
        {
            let size = Size::new(layout.bounds.width, layout.bounds.height);
            let baseline = item::baseline(node, size, self.text);
            if baseline != layout.baseline_offset() {
                layout.details_mut().baseline_offset = baseline;
            }
        }
    }

    fn is_absolute_leaf(&self, node: &Node) -> bool {
        matches!(node.kind, "Rectangle" | "Image" | "AnimatedImage")
            && !node.scene_texture_only()
            && self.can_place_leaf(node)
    }

    fn can_place_leaf(&self, node: &Node) -> bool {
        node.children.is_empty()
            && node.list.is_none()
            && node.table_header.is_none()
            && !node.application_window
            && self.resolved_list_scroll.is_none()
            && self.resolved_table.is_none()
    }

    fn place_absolute_leaf(
        &mut self,
        node: &Node,
        content: Rect,
        clip: Rect,
        enabled: bool,
    ) -> LayoutNode {
        let inputs = row_layout::AbsoluteLeafInputs {
            content,
            clip,
            enabled,
            direction: self.layout_direction,
        };
        let slot = if self.space.is_none()
            && self.ancestor_shapes.is_none()
            && node.list_row_renderer.is_none()
        {
            self.row_layouts
                .as_deref_mut()
                .and_then(RowLayoutCache::absolute_leaf_slot)
        } else {
            None
        };
        if let Some(slot) = slot
            && let Some(layout) = self
                .row_layouts
                .as_deref_mut()
                .and_then(|cache| cache.absolute_leaf(slot, node, inputs))
        {
            return layout;
        }
        let bounds = self.absolute_child(node, content);
        let layout = self.place_leaf(node, bounds, clip, true, enabled);
        if let Some(slot) = slot
            && let Some(cache) = self.row_layouts.as_deref_mut()
        {
            cache.store_absolute_leaf(slot, node, inputs, &layout);
        }
        layout
    }

    fn place_leaf(
        &self,
        node: &Node,
        bounds: Rect,
        clip: Rect,
        parent_visible: bool,
        parent_enabled: bool,
    ) -> LayoutNode {
        let mut space = self.space.clone();
        let mut ancestor_shapes = self.ancestor_shapes.clone();
        let (clip, transform) =
            frame_projection::apply(node, bounds, clip, &mut space, &mut ancestor_shapes);
        let visible = parent_visible
            && node.is_visible()
            && space.as_ref().is_none_or(|space| space.inverse.is_some());
        let clipping = if node.kind == "Rectangle" {
            node.properties.get("clip")
        } else {
            None
        };
        let child_shape = matches!(clipping, Some(xrack_runtime::Value::Bool(true)))
            .then(|| RoundedClip::content_box(node, bounds))
            .filter(|shape| shape.radii.iter().any(|radius| *radius > 0.0));
        let nested_list = node
            .list_row_renderer
            .as_ref()
            .filter(|_| node.routing_row_key().or(self.row_key).is_some());
        let details = (child_shape.is_some()
            || nested_list.is_some()
            || node.viewport_handle.is_some()
            || transform.is_some())
        .then(|| {
            Rc::new(LayoutDetails {
                child_shape,
                nested_list: nested_list.cloned(),
                viewport_handle: node.viewport_handle.clone(),
                transform,
                ..LayoutDetails::default()
            })
        });
        LayoutNode {
            details,
            mouse_drag_pan: node.boolean("mouse-drag-pan-enabled", false),
            middle_mouse_pan: true,
            pan_axes: [true; 2],
            pull_to_refresh: node.boolean("pull-to-refresh", false),
            overflow: matches!(clipping, Some(xrack_runtime::Value::Bool(false))),
            ancestor_shapes,
            space,
            ..LayoutNode::leaf(
                node,
                bounds,
                clip,
                visible,
                parent_enabled && node.enables_children(),
                node.layout_direction().or(self.layout_direction),
                self.text.font_size(node),
            )
        }
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
        if let Some((layout, reuse)) = self
            .row_layouts
            .as_deref_mut()
            .and_then(|cache| cache.get(&self.path, path_len, slot, node, inputs))
        {
            return row_layout::clone_in_view_reusing(
                layout,
                bounds,
                clip,
                self.ancestor_shapes.as_ref(),
                reuse,
            );
        }
        if !row_layout::cacheable(node) {
            return self.place_node(node, bounds, clip, inputs.visible, inputs.enabled, None);
        }
        let local = Rect::new(0.0, 0.0, inputs.size.width, inputs.size.height);
        let mut layout =
            self.place_without_ancestor_clip(node, local, local, inputs.visible, inputs.enabled);
        let path = &self.path[..path_len];
        if let Some(cache) = self.row_layouts.as_deref_mut() {
            cache.insert(path, slot, node, inputs, &layout);
        }
        row_layout::place_in_view(&mut layout, bounds, clip, self.ancestor_shapes.as_ref());
        layout
    }

    fn scroll_geometry(
        &mut self,
        view: &Node,
        path_len: usize,
    ) -> Option<(&mut row_layout::ScrollGeometry, Option<&Rc<ClipChain>>)> {
        if self.row_key.is_some() || self.space.is_some() {
            return None;
        }
        self.row_layouts
            .as_deref_mut()?
            .scroll_geometry(&self.path[..path_len], view)
            .map(|geometry| (geometry, self.ancestor_shapes.as_ref()))
    }

    fn child_paint_order(
        &mut self,
        node: &Node,
        count: usize,
        cached_scroll_children: bool,
    ) -> Option<xrack_runtime::ScenePlanSlice<usize>> {
        if cached_scroll_children
            && count >= 64
            && count == node.children.len()
            && !node.has_natural_paint_order()
            && node.paint_order_plan().is_none()
            && let Some((geometry, _)) = self.scroll_geometry(node, self.path.len())
        {
            return geometry.paint_order(node);
        }
        sibling_paint_order(node, count)
    }

    fn place_local_subtree(
        &mut self,
        node: &Node,
        bounds: Rect,
        clip: Rect,
        visible: bool,
        enabled: bool,
        kind: row_layout::LocalSubtree,
    ) -> LayoutNode {
        let inputs = row_layout::RowInputs {
            size: Size::new(bounds.width, bounds.height),
            visible,
            enabled,
            layout_direction: self.layout_direction,
        };
        if let Some((local, reuse)) = self
            .row_layouts
            .as_deref_mut()
            .and_then(|cache| cache.local_layout(&self.path, node, inputs, kind))
        {
            return row_layout::clone_in_view_reusing(
                local,
                bounds,
                clip,
                self.ancestor_shapes.as_ref(),
                reuse,
            );
        }
        let Some(nodes) = self
            .row_layouts
            .as_ref()
            .and_then(|cache| cache.local_layout_node_count(&self.path, node, kind))
        else {
            return self.place_node(node, bounds, clip, visible, enabled, None);
        };
        let mut local = self.place_without_ancestor_clip(
            node,
            Rect::new(0.0, 0.0, bounds.width, bounds.height),
            unclipped_content(),
            visible,
            enabled,
        );
        if let Some(cache) = self.row_layouts.as_deref_mut() {
            cache.store_local_layout(&self.path, node, inputs, &local, nodes);
        }
        row_layout::place_in_view(&mut local, bounds, clip, self.ancestor_shapes.as_ref());
        local
    }

    fn place_without_ancestor_clip(
        &mut self,
        node: &Node,
        bounds: Rect,
        clip: Rect,
        visible: bool,
        enabled: bool,
    ) -> LayoutNode {
        // A local snapshot doesn't carry viewport coordinates. Projection
        // attaches the current ancestor shapes after moving it into place.
        let ancestor_shapes = self.ancestor_shapes.take();
        let layout = self.place_node(node, bounds, clip, visible, enabled, None);
        self.ancestor_shapes = ancestor_shapes;
        layout
    }

    fn place_scroll_child(
        &mut self,
        view: &Node,
        child: &Node,
        bounds: Rect,
        clip: Rect,
        visible: bool,
        enabled: bool,
    ) -> LayoutNode {
        let Some((&index, parent)) = self.path.split_last() else {
            return self.place(child, bounds, clip, visible, enabled, None);
        };
        let Some(dense_index) = view.table_column_window.dense_index(index) else {
            return self.place(child, bounds, clip, visible, enabled, None);
        };
        let path_len = parent.len();
        let inputs = row_layout::RowInputs {
            size: Size::new(bounds.width, bounds.height),
            visible,
            enabled,
            layout_direction: self.layout_direction,
        };
        let text_outside = self
            .row_layouts
            .as_ref()
            .is_some_and(|cache| cache.defer_clipped_text)
            && outside_clip(bounds, clip);
        if self.row_key.is_some() || self.space.is_some() {
            return self.place(child, bounds, clip, visible, enabled, None);
        }
        let Some((geometry, previous)) = self
            .row_layouts
            .as_deref_mut()
            .and_then(|cache| cache.scroll_geometry_with_recycling(&self.path[..path_len], view))
            .filter(|(geometry, _)| geometry.caches_child(dense_index))
        else {
            return self.place(child, bounds, clip, visible, enabled, None);
        };
        if let Some(local) = geometry.child(dense_index, inputs) {
            if local.deferred_text && !text_outside {
                local.resolve_text(self.text, child);
            }
            return local.place_reusing(
                previous,
                &self.path,
                bounds,
                clip,
                self.ancestor_shapes.as_ref(),
            );
        }

        // This capture will retain every descendant. When it borrows from
        // the local layout budget, don't also build separate templates.
        let row_layouts = if geometry.uses_local_budget() {
            self.row_layouts.take()
        } else {
            None
        };
        let local = Rect::new(0.0, 0.0, bounds.width, bounds.height);
        // Apply the viewport clip after translation. Descendants can overflow
        // the content item's own extent when their parents allow it.
        let ancestors = self.ancestor_shapes.take();
        let defer = text_outside
            && visible
            && child.is_visible()
            && child.kind == "StyledText"
            && child.children.is_empty()
            && child.routing_row_key().is_none()
            && !child.selectable_styled_text()
            && self.text.can_defer_styled_text(child);
        self.defer_styled_text = defer;
        let mut layout = self.place(child, local, unclipped_content(), visible, enabled, None);
        self.defer_styled_text = false;
        self.ancestor_shapes = ancestors;
        if let Some(row_layouts) = row_layouts {
            self.row_layouts = Some(row_layouts);
        }
        if let Some((geometry, _)) = self.scroll_geometry(view, path_len) {
            geometry.store_child(dense_index, inputs, layout.clone(), defer);
        }
        row_layout::place_in_view(&mut layout, bounds, clip, self.ancestor_shapes.as_ref());
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
        let resolved_table = self.resolved_table.take();
        let tree_parts = match &row_children {
            Some(grid::RowPlacements::Tree {
                geometry,
                has_children,
            }) => Some((*geometry, *has_children)),
            _ => None,
        };
        let item = item::item(node.kind);
        let item_layout = item.layout();
        let flickable = item_layout == ItemLayout::Scroll(ScrollContainer::Flickable);
        let popup = item_layout == ItemLayout::Overlay(OverlayLayout::Popup);
        let modal = matches!(
            item_layout,
            ItemLayout::Overlay(OverlayLayout::Modal | OverlayLayout::Drawer)
        );
        let shortcut = item_layout == ItemLayout::Deferred(DeferredLayout::Shortcut);
        let font_size = self.text.font_size(node);
        let children_enabled = parent_enabled && node.enables_children();
        let enabled =
            children_enabled && (node.kind != "TouchArea" || node.boolean("enabled", true));
        let mut visible = parent_visible
            && node.is_visible()
            && (!popup || enabled && self.anchor.is_some())
            && xrack_runtime::indicator_state(node) != Some(xrack_runtime::IndicatorState::Hidden);
        let (bounds, clip) = if shortcut {
            (Rect::new(bounds.x, bounds.y, 0.0, 0.0), clip)
        } else if visible
            && popup
            && let Some(anchor) = self.anchor
        {
            (
                popup::bounds(node, anchor, self.viewport, self.text),
                self.viewport,
            )
        } else if visible && modal {
            let panel = modal::bounds(node, self.viewport, self.text);
            let reveal = (item_layout == ItemLayout::Overlay(OverlayLayout::Drawer))
                .then(|| {
                    self.input?.closing_drawer_reveal(
                        node,
                        &self.path,
                        self.row_key,
                        self.viewport,
                        panel,
                    )
                })
                .flatten();
            let panel = reveal.map_or(panel, |reveal| {
                modal::drawer_revealed_bounds(node.drawer_edge(), panel, reveal)
            });
            (panel, self.viewport)
        } else {
            (bounds, clip)
        };
        if matches!(item_layout, ItemLayout::Overlay(_)) {
            self.ancestor_shapes = None;
            self.space = None;
        }
        let (clip, transform) = frame_projection::apply(
            node,
            bounds,
            clip,
            &mut self.space,
            &mut self.ancestor_shapes,
        );
        let space = self.space.clone();
        let world = space
            .as_deref()
            .map_or(Transform2D::IDENTITY, |space| space.world);
        let inverse = space
            .as_deref()
            .map_or(Some(Transform2D::IDENTITY), |space| space.inverse);
        visible &= inverse.is_some();
        let ancestor_shapes = self.ancestor_shapes.clone();
        // Only a child popup can read this anchor. Tab previews and option
        // delegates can place children outside the ordinary scene child list.
        if !shortcut
            && (!node.children.is_empty()
                || node.swipe_preview.is_some()
                || node.combo_option_renderer.is_some())
        {
            self.anchor = None;
            if visible {
                let clipped = world.map_rect(bounds.intersect(clip));
                let clipped = ancestor_shapes
                    .as_deref()
                    .map_or(clipped, |chain| chain.clip_window_rect(clipped));
                self.anchor = (clipped.width > 0.0 && clipped.height > 0.0).then_some(clipped);
            }
        }
        if let ItemLayout::Deferred(kind) = item_layout {
            // Menus and tooltips lay out their children in the overlay presenter.
            return LayoutNode {
                ancestor_shapes,
                space,
                details: transform.map(|transform| {
                    Rc::new(LayoutDetails {
                        transform: Some(transform),
                        ..LayoutDetails::default()
                    })
                }),
                ..LayoutNode::leaf(
                    node,
                    bounds,
                    clip,
                    visible && kind != DeferredLayout::Tooltip,
                    enabled,
                    self.layout_direction,
                    font_size,
                )
            };
        }
        let table = if resolved_table.is_some() {
            resolved_table
        } else if visible && node.table_header.is_some() {
            let preferred_header = if self.text.text_scale() > 1.0 {
                (table_heading_font_size(self.text) * 1.4 + 8.0).max(34.0)
            } else {
                34.0
            };
            let requested = self.requested_scroll(node, Orientation::Horizontal);
            let sizing = if self.cache_table_sizes {
                self.row_layouts
                    .as_deref_mut()
                    .and_then(|cache| cache.table_sizing(&self.path))
            } else {
                None
            };
            TableGeometry::with_offset(
                node,
                bounds,
                clip,
                requested,
                self.layout_direction.unwrap_or_default(),
                table::header_height(node, bounds.height, preferred_header),
                sizing,
            )
            .map(Box::new)
        } else {
            None
        };
        let pane = item_layout == ItemLayout::Pane;
        let overflow =
            (node.kind == "Rectangle" || flickable || pane) && !node.boolean("clip", true);
        let clipping = ((node.kind == "Rectangle" || pane) && node.boolean("clip", false))
            .then(|| RoundedClip::content_box(node, bounds));
        let child_shape = clipping.filter(|shape| shape.radii.iter().any(|radius| *radius > 0.0));
        if let Some(shape) = child_shape {
            self.ancestor_shapes = Some(Rc::new(ClipChain {
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
        if visible && let ItemLayout::Pages(mode) = item_layout {
            let tabs = (mode == PageLayout::Tabs).then(|| {
                selection::TabGeometry::new(
                    node,
                    bounds,
                    self.layout_direction.unwrap_or_default(),
                    self.text,
                )
            });
            let stack_layout = mode == PageLayout::StackLayout;
            let page_bounds = if stack_layout {
                linear::Padding::read(node).inner_rect(bounds)
            } else {
                tabs.as_ref().map_or(bounds, |geometry| geometry.content)
            };
            let selected = if mode == PageLayout::Stack {
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
                        input.scroll_selection_changed(
                            node,
                            &self.path,
                            1,
                            direction,
                            self.text.text_scale(),
                        )
                    });
                selection::tab_bar(node, geometry.bar, self.text, direction, requested, reveal)
            });
            let header_scroll = headers.as_ref().map(|(_, scroll)| *scroll);
            let children: Vec<_> = node
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
                            page.tab_close = page.tab_header.and_then(|header| {
                                selection::tab_close_rect(
                                    header,
                                    direction,
                                    self.text.text_scale() > 1.0,
                                )
                            });
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
                details: LayoutDetails {
                    horizontal_scroll: header_scroll
                        .filter(|_| orientation == Orientation::Horizontal),
                    tab_strip: tabs.as_ref().map(|geometry| geometry.bar),
                    transform,
                    ..LayoutDetails::default()
                }
                .into_shared(),
                scroll: header_scroll.filter(|_| orientation == Orientation::Vertical),
                missing_list_rows: false,
                missing_descendant_list_rows: false,
                mouse_drag_pan: false,
                middle_mouse_pan: tabs.is_some(),
                pan_axes: [tabs.is_some(); 2],
                pull_to_refresh: false,
                table: None,
                split: None,
                tab_header: None,
                tab_close: None,
                tumbler: None,
                switch_position: None,
                swipe_delegate: None,
                swipe_preview,
                styled_text: None,
                font_size,
                overflow: false,
                ancestor_shapes,
                space,
                table_column_window: node.table_column_window.clone(),
                child_storage: children.into(),
            };
        }
        let mut scroll = None;
        let mut horizontal_scroll = None;
        let mut list_section_headers = None;
        let mut scroll_area = None;
        let mut cached_scroll_children = false;
        let mut scroll_rects = None;
        let horizontal_scroll_reversed = matches!(item_layout, ItemLayout::Scroll(_))
            && self.layout_direction == Some(LayoutDirection::RightToLeft);
        let (split, split_rects) = if visible && item_layout == ItemLayout::Split {
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
        let content_bounds = window_safe_area(node, bounds);
        let page = (visible && (item_layout == ItemLayout::Page || node.application_window))
            .then(|| page::Regions::new(node, content_bounds, self.text));
        let swipe_delegate = (visible && item_layout == ItemLayout::SwipeDelegate).then(|| {
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
        let switch_position = if enabled && visible && item_layout == ItemLayout::Switch {
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
        let modal_title = if visible && modal {
            modal::title_layout(
                node,
                Size::new(bounds.width, bounds.height),
                self.layout_direction.unwrap_or_default(),
                self.text,
            )
            .map(Box::new)
        } else {
            None
        };
        let group_title = if visible {
            group_box::title_layout(
                node,
                Size::new(bounds.width, bounds.height),
                self.layout_direction.unwrap_or_default(),
                self.text,
            )
            .map(Box::new)
        } else {
            None
        };
        let mut absolute_content = None;
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
        } else if let Some(placements) = item.arrange(&mut item::ArrangeContext {
            node,
            bounds,
            direction: self.layout_direction.unwrap_or_default(),
            space: &space,
            enabled,
            input: self.input,
            path: &self.path,
            switch_position,
            layout: self.text,
        }) {
            placements
        } else if let Some(list) = node.list {
            let cells = ViewCells::new(node, viewport.width);
            let axis = resolved_list_scroll
                .unwrap_or_else(|| self.list_scroll(node, &cells, viewport.height, list.row_count));
            scroll = Some(axis);
            list_section_headers = cells.headers(viewport, axis.offset, list.row_count);
            item::views::VisibleRows {
                node,
                viewport,
                cells: &cells,
                offset: axis.offset,
                direction: self.layout_direction.unwrap_or_default(),
                grid: item_layout == ItemLayout::Rows(RowLayout::Grid),
                table: table.as_deref(),
                space: &space,
            }
            .arrange(list)
        } else if item_layout == ItemLayout::Rows(RowLayout::Row) {
            item::views::arrange_row(
                node,
                bounds,
                self.layout_direction.unwrap_or_default(),
                row_children,
            )
        } else if item_layout == ItemLayout::TabBar {
            let orientation = node.orientation();
            let direction = self.layout_direction.unwrap_or_default();
            let requested = self.scroll_request(node, orientation);
            let reveal = requested.is_none()
                || self.input.is_some_and(|input| {
                    input.scroll_selection_changed(
                        node,
                        &self.path,
                        1,
                        direction,
                        self.text.text_scale(),
                    )
                });
            let (rects, axis) =
                selection::tab_bar(node, bounds, self.text, direction, requested, reveal);
            match orientation {
                Orientation::Horizontal => horizontal_scroll = Some(axis),
                Orientation::Vertical => scroll = Some(axis),
            }
            rects.into_iter().map(grid::Placement::plain).collect()
        } else if matches!(item_layout, ItemLayout::Scroll(_)) {
            let (rects, x, y, area, cache_ready) =
                self.scroll_view_children(node, bounds, enabled, horizontal_scroll_reversed);
            cached_scroll_children = cache_ready;
            scroll_area = (area != bounds).then_some(area);
            horizontal_scroll = Some(x);
            scroll = Some(y);
            scroll_rects = Some(rects);
            Vec::new()
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
        } else {
            let mut content = if pane
                || matches!(item_layout, ItemLayout::Fill { padded: true })
                || item_layout == ItemLayout::Window
            {
                linear::Padding::read(node).inner_rect(content_bounds)
            } else {
                content_bounds
            };
            let title = (selection::radio_title_height(node, font_size)
                + modal_title.as_ref().map_or(0.0, |title| title.height)
                + group_title.as_ref().map_or_else(
                    || group_box::title_height(node, font_size),
                    |title| title.height,
                ))
            .min(bounds.height);
            content.y += title;
            content.height -= title;
            if matches!(item_layout, ItemLayout::Fill { .. }) {
                node.children
                    .iter()
                    .map(|_| grid::Placement::plain(content))
                    .collect()
            } else if let Some(geometry) = &swipe_delegate {
                node.children
                    .iter()
                    .map(|child| grid::Placement::plain(geometry.region(child.kind).bounds))
                    .collect()
            } else if let Some(page) = &page {
                let mut bounds: Vec<_> = node
                    .children
                    .iter()
                    .map(|child| {
                        page.band(child)
                            .unwrap_or_else(|| self.absolute_child(child, page.body))
                    })
                    .collect();
                anchors::apply(
                    &node.children,
                    page.body,
                    &mut bounds,
                    self.text,
                    |child| page.band(child).is_some(),
                    None,
                );
                bounds.into_iter().map(grid::Placement::plain).collect()
            } else if let Some(rects) = modal::picker_children(
                node,
                content,
                self.layout_direction.unwrap_or_default(),
                self.text,
            ) {
                rects.into_iter().map(grid::Placement::plain).collect()
            } else if let Some(rects) = modal::scrollable_dialog_children(
                node,
                content,
                self.layout_direction.unwrap_or_default(),
                self.text,
            ) {
                rects.into_iter().map(grid::Placement::plain).collect()
            } else if let Some(placements) = item::container::arrange_content(
                item_layout,
                node,
                content,
                self.layout_direction.unwrap_or_default(),
                self.text,
            ) {
                placements
            } else {
                absolute_content = Some(content);
                Vec::new()
            }
        };
        let child_clip = scroll_area.map_or(child_clip, |area| child_clip.intersect(area));
        let mut placements = placements.into_iter();
        let mut scroll_rects = scroll_rects.as_ref().map(ScrollPlacement::rects);
        let content_enabled = children_enabled && node.group_content_enabled();
        let count = if node.expander_collapsed() {
            0
        } else if absolute_content.is_some() {
            node.children.len()
        } else {
            let placed = scroll_rects
                .as_ref()
                .map_or_else(|| placements.len(), ExactSizeIterator::len);
            node.children.len().min(placed)
        };
        let cached_scroll_projection = cached_scroll_children
            && self.row_key.is_none()
            && self.space.is_none()
            && page.is_none()
            && swipe_delegate.is_none()
            && tree_parts.is_none()
            && list_section_headers.is_none();
        // Project cached scroll children together. The cache only contains
        // ordinary child layouts and leaves special children for placement.
        let projected = if cached_scroll_projection
            && let Some(rects) = scroll_rects.as_ref().filter(|rects| rects.len() == count)
            && let Some(cache) = self.row_layouts.as_deref_mut()
        {
            let defer = cache.defer_clipped_text;
            let inputs = row_layout::RowInputs {
                size: Size::default(),
                visible,
                enabled: content_enabled,
                layout_direction: self.layout_direction,
            };
            cache
                .scroll_geometry_for_projection(&self.path, node, count)
                .and_then(|(geometry, reuse)| {
                    geometry.project_children(
                        rects.clone(),
                        child_clip,
                        inputs,
                        self.ancestor_shapes.as_ref(),
                        reuse,
                        |slot, bounds, local| {
                            if !defer || !outside_clip(bounds, child_clip) {
                                local.resolve_text(self.text, &node.children[slot]);
                            }
                        },
                    )
                })
        } else {
            None
        };
        let place_child = |context: &mut Self, index, child: &Node, placement: grid::Placement| {
            context.path.push(index);
            if cached_scroll_projection
                && child.kind == "StyledText"
                && child.children.is_empty()
                && placement.clip.is_none()
            {
                // Rich-text leaves use the viewport clip and inherited state.
                let layout = context.place_scroll_child(
                    node,
                    child,
                    placement.bounds,
                    child_clip,
                    visible,
                    content_enabled,
                );
                context.path.pop();
                return layout;
            }
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
                .map(|_| context.ancestor_shapes.clone());
            let pane = placement.clip.as_ref().map(|pane| {
                let table_world = pane
                    .space
                    .as_ref()
                    .map_or(Transform2D::IDENTITY, |space| space.world);
                let to_parent = context
                    .space
                    .as_deref()
                    .and_then(|space| space.inverse)
                    .map_or_else(
                        || Transform2D::from_coefficients(table_world.coefficients()),
                        |window_to_parent| table_world.then(window_to_parent),
                    );
                if !to_parent.is_identity() {
                    context.ancestor_shapes = Some(Rc::new(ClipChain {
                        shape: RoundedClip {
                            bounds: pane.bounds,
                            radii: [0.0; 4],
                        },
                        parent: context.ancestor_shapes.clone(),
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
            let enabled = content_enabled && region.is_none_or(|region| region.enabled);
            let mut child = if table_cells
                && !child.children.is_empty()
                && context.row_layouts.is_some()
                && context.space.is_none()
                && child.routing_row_key().is_none()
                && row_layout::cacheable(child)
                && let Some(row) = context.row_key
            {
                let outer_anchor = context.anchor;
                let outer_direction = context.layout_direction;
                context.layout_direction = child.layout_direction().or(outer_direction);
                let inputs = row_layout::RowInputs {
                    size: Size::new(placement.bounds.width, placement.bounds.height),
                    visible,
                    enabled,
                    layout_direction: context.layout_direction,
                };
                let slot = row_layout::Slot {
                    row,
                    column: Some(index),
                };
                let child =
                    context.place_cached_content(child, placement.bounds, clip, inputs, slot);
                context.anchor = outer_anchor;
                context.layout_direction = outer_direction;
                child
            } else if cached_scroll_children {
                context.place_scroll_child(node, child, placement.bounds, clip, visible, enabled)
            } else {
                context.place(
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
                    context.layout_direction.unwrap_or_default(),
                    context.text.text_scale() > 1.0,
                );
            }
            if let Some(shapes) = outer_shapes {
                context.ancestor_shapes = shapes;
            }
            if let Some((bounds, to_parent)) = pane
                && (!to_parent.is_identity() || !child.local_transform().is_identity())
            {
                child.details_mut().table_pane = Some((bounds, to_parent));
            }
            if is_handle {
                child.enabled &= split
                    .as_ref()
                    .and_then(|geometry| geometry.handle(index))
                    .is_some_and(SplitHandle::adjustable);
            }
            context.path.pop();
            child
        };
        let projected = match projected {
            Some(row_layout::ScrollProjection::Complete(children)) => Some(children),
            Some(row_layout::ScrollProjection::Partial(missing)) => {
                let placed: Vec<_> = missing
                    .into_iter()
                    .map(|(slot, bounds)| {
                        let index = node.table_column_window.logical_index(slot);
                        let placement = grid::Placement::plain(bounds);
                        (
                            slot,
                            place_child(self, index, &node.children[slot], placement),
                        )
                    })
                    .collect();
                self.row_layouts
                    .as_deref_mut()
                    .and_then(|cache| cache.take_projected_scroll_children(&self.path, count))
                    .map(|mut children| {
                        for (slot, child) in placed {
                            children[slot] = child;
                        }
                        children
                    })
            }
            None => None,
        };
        let children = projected.unwrap_or_else(|| {
            let anchor_rects = absolute_content
                .filter(|_| node.children.iter().take(count).any(anchors::has_anchors))
                .map(|content| anchors::children(node, content, self.text));
            let mut children = Vec::with_capacity(count);
            let placed = node
                .materialized_children()
                .take(count)
                .enumerate()
                .map_while(|(slot, (index, child))| {
                    let placement = if let Some(rects) = &anchor_rects {
                        grid::Placement::plain(rects[slot])
                    } else if let Some(content) = absolute_content {
                        if self.is_absolute_leaf(child) {
                            return Some(self.place_absolute_leaf(
                                child,
                                content,
                                child_clip,
                                content_enabled,
                            ));
                        }
                        grid::Placement::plain(self.absolute_child(child, content))
                    } else if let Some(rects) = scroll_rects.as_mut() {
                        grid::Placement::plain(rects.next()?)
                    } else {
                        placements.next()?
                    };
                    Some(place_child(self, index, child, placement))
                });
            children.extend(placed);
            children
        });
        let tumbler = if visible && let ItemLayout::Wheel { path } = item_layout {
            (|| {
                let wheel = xrack_runtime::Tumbler::from_node(node);
                let offset = self
                    .input
                    .map_or(0.0, |input| input.tumbler_offset(&wheel, &self.path));
                let mut geometry = if path {
                    TumblerGeometry::along_path(
                        node,
                        &wheel,
                        bounds,
                        offset,
                        self.layout_direction.unwrap_or_default(),
                    )?
                } else {
                    TumblerGeometry::new(
                        &wheel,
                        bounds,
                        offset,
                        tumbler::scaled_row_height(node, self.text),
                    )
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
            })()
        } else {
            None
        };
        LayoutNode {
            bounds,
            clip,
            visible,
            enabled,
            layout_direction: self.layout_direction.unwrap_or_default(),
            direction_override: self.layout_direction,
            details: LayoutDetails {
                content_bounds: scroll_area.or_else(|| {
                    page.filter(|_| node.application_window)
                        .map(|page| page.body)
                }),
                list_section_headers,
                horizontal_scroll,
                horizontal_scroll_reversed,
                nested_list: node
                    .list_row_renderer
                    .clone()
                    .filter(|_| self.row_key.is_some()),
                elastic: overscroll::ElasticOptions::from_node(node),
                modal_title,
                group_title,
                paint_order: self.child_paint_order(node, children.len(), cached_scroll_children),
                child_shape,
                viewport_handle: node.viewport_handle.clone(),
                transform,
                ..LayoutDetails::default()
            }
            .into_shared(),
            scroll,
            missing_list_rows: false,
            missing_descendant_list_rows: false,
            mouse_drag_pan: node.boolean("mouse-drag-pan-enabled", flickable),
            middle_mouse_pan: !flickable || node.boolean("mouse-drag-pan-enabled", true),
            pan_axes: if flickable {
                let direction = match node.properties.get("flick-direction") {
                    Some(xrack_runtime::Value::FlickDirection(direction)) => *direction,
                    _ => xrack_runtime::FlickDirection::Both,
                };
                [Orientation::Horizontal, Orientation::Vertical]
                    .map(|axis| node.boolean("interactive", true) && direction.allows(axis))
            } else {
                [true; 2]
            },
            pull_to_refresh: node.boolean("pull-to-refresh", false),
            table,
            split,
            tab_header: None,
            tab_close: None,
            tumbler,
            switch_position,
            swipe_delegate,
            swipe_preview: None,
            styled_text: if visible && node.kind == "StyledText" && !self.defer_styled_text {
                styled_text_at(node, bounds, self.layout_direction, |size| {
                    self.text.shared_styled_text_layout(node, size)
                })
            } else {
                None
            },
            font_size,
            overflow,
            ancestor_shapes,
            space,
            table_column_window: node.table_column_window.clone(),
            child_storage: children.into(),
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
                                    self.text.text_scale(),
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
    ) -> (ScrollPlacement, ScrollLayout, ScrollLayout, Rect, bool) {
        let path_len = self.path.len();
        let size = Size::new(viewport.width, viewport.height);
        let geometry = self.scroll_geometry(node, path_len);
        let cache_ready = geometry.is_some();
        let mut previous = None;
        let measured = geometry.and_then(|(geometry, _)| {
            let last = geometry
                .measured
                .as_ref()
                .filter(|last| last.size == size && last.mirrored == horizontal_reversed)?;
            if last.changed_children.is_empty() {
                Some((last.viewport, last.extent, last.child_rects.clone()))
            } else {
                previous = geometry.measured.take();
                None
            }
        });
        let (mut area, extent, rects) = measured.unwrap_or_else(|| {
            let local = Rect::new(0.0, 0.0, size.width, size.height);
            let (area, ScrollContent { extent, children }) =
                self.fit_scroll_content(node, local, horizontal_reversed, &mut previous);
            let rects: Rc<[Rect]> = children
                .iter()
                .map(|child| child.map_or_else(Rect::default, |child| child.bounds(extent.height)))
                .collect::<Vec<_>>()
                .into();
            if cache_ready && let Some((geometry, _)) = self.scroll_geometry(node, path_len) {
                // Retain the resolved local rectangles. Scrolling only adds
                // the content origin to these positions.
                geometry.measured = Some(row_layout::ScrollMeasurement {
                    size,
                    mirrored: horizontal_reversed,
                    viewport: area,
                    extent,
                    child_rects: rects.clone(),
                    children,
                    changed_children: Vec::new(),
                });
            }
            (area, extent, rects)
        });
        area.x += viewport.x;
        area.y += viewport.y;
        let viewport = area;
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
        (
            ScrollPlacement {
                local: rects,
                origin,
            },
            horizontal,
            vertical,
            viewport,
            cache_ready,
        )
    }

    fn measure_scroll_child(
        &mut self,
        child: &Node,
        reference: Size,
    ) -> Option<ScrollChildMeasurement> {
        if !participates_in_flow(child) {
            return None;
        }
        let x = child.length("x").unwrap_or(0.0);
        let y = child.length("y").unwrap_or(0.0);
        let width = child.relative_length("width", reference.width).map_or_else(
            || {
                let metrics = linear::measure(child, reference, self.text);
                absolute_width(child, metrics.x, (reference.width - x.max(0.0)).max(0.0))
            },
            |width| width.max(0.0),
        );
        let height = child
            .relative_length("height", reference.height)
            .map_or_else(
                || {
                    let height =
                        linear::measure_with_width(child, reference, Some(width), self.text).y;
                    if absolute_height_uses_preferred(child) {
                        ScrollChildHeight::Preferred(height.preferred)
                    } else {
                        ScrollChildHeight::Available(height)
                    }
                },
                |height| ScrollChildHeight::Preferred(height.max(0.0)),
            );
        Some(ScrollChildMeasurement {
            x,
            y,
            width,
            height,
        })
    }

    fn measure_scroll_content(
        &mut self,
        node: &Node,
        viewport: Rect,
        previous: &mut Option<row_layout::ScrollMeasurement>,
    ) -> ScrollContent {
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
        let has_anchors = node.children.iter().any(anchors::has_anchors);
        let mut children: Vec<_> = if let Some(mut last) = previous.take_if(|last| {
            !has_anchors && last.viewport == viewport && last.children.len() == node.children.len()
        }) {
            for index in last.changed_children {
                last.children[index] = self.measure_scroll_child(&node.children[index], reference);
            }
            last.children
        } else {
            node.children
                .iter()
                .map(|child| self.measure_scroll_child(child, reference))
                .collect()
        };
        if has_anchors {
            let content = Rect::new(
                0.0,
                0.0,
                explicit_width.unwrap_or(reference.width).max(0.0),
                explicit_height.unwrap_or(reference.height).max(0.0),
            );
            let mut rects: Vec<_> = children
                .iter()
                .map(|child| {
                    child.map_or_else(Rect::default, |child| child.bounds(reference.height))
                })
                .collect();
            let mut fixed_heights = vec![false; rects.len()];
            anchors::apply(
                &node.children,
                content,
                &mut rects,
                self.text,
                |_| false,
                Some(&mut fixed_heights),
            );
            for ((child, bounds), fixed_height) in children.iter_mut().zip(rects).zip(fixed_heights)
            {
                let Some(child) = child else { continue };
                let before = child.bounds(reference.height);
                child.x = bounds.x;
                child.y = bounds.y;
                child.width = bounds.width;
                if fixed_height || before.height != bounds.height {
                    child.height = ScrollChildHeight::Preferred(bounds.height);
                }
            }
        }
        let mut extent = reference;
        for child in children.iter().flatten() {
            extent.width = extent.width.max(child.x + child.width);
            extent.height = extent.height.max(child.y + child.preferred_height());
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
        previous: &mut Option<row_layout::ScrollMeasurement>,
    ) -> (Rect, ScrollContent) {
        use xrack_runtime::ScrollBarPolicy;

        if node.kind != "ScrollView" || !node.boolean("reserve-scroll-bar-space", false) {
            return (bounds, self.measure_scroll_content(node, bounds, previous));
        }
        let thickness = scroll_bar::scroll_bar_thickness(bounds);
        if thickness < 1.0 {
            return (bounds, self.measure_scroll_content(node, bounds, previous));
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
            let content = self.measure_scroll_content(node, viewport, previous);
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

    fn absolute_child(&mut self, child: &Node, content: Rect) -> Rect {
        absolute_child(self.text, child, content)
    }
}

fn absolute_child(text: &mut impl TextMeasurer, child: &Node, content: Rect) -> Rect {
    if !participates_in_flow(child) {
        return Rect::new(content.x, content.y, 0.0, 0.0);
    }
    let available = Size::new(content.width, content.height);
    let size = linear::fixed_size(child, available)
        .or_else(|| linear::fill_size_with_fixed_cross_axis(child, available, text))
        .unwrap_or_else(|| {
            let metrics = linear::measure(child, available, text);
            Size::new(
                absolute_width(child, metrics.x, content.width),
                absolute_height(child, metrics.y, content.height),
            )
        });
    absolute_child_bounds(child, content, size)
}

fn absolute_child_bounds(child: &Node, content: Rect, size: Size) -> Rect {
    Rect::new(
        content.x + child.length("x").unwrap_or(0.0),
        content.y + child.length("y").unwrap_or(0.0),
        size.width,
        size.height,
    )
}

fn absolute_width(node: &Node, info: linear::AxisInfo, available: f32) -> f32 {
    if item::item(node.kind).size_policy(node).preferred_width
        || node.properties.contains_key("preferred-width")
    {
        info.preferred
    } else {
        info.clamp(available)
    }
}

fn absolute_height_uses_preferred(node: &Node) -> bool {
    item::item(node.kind).size_policy(node).preferred_height
        || node.properties.contains_key("preferred-height")
}

fn absolute_height(node: &Node, info: linear::AxisInfo, available: f32) -> f32 {
    if absolute_height_uses_preferred(node) {
        info.preferred
    } else {
        info.clamp(available)
    }
}
