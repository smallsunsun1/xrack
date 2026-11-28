//! Renderer-independent scene snapshots produced by generated components.

/// Compatibility level expected by the Rust code emitted by xrackc.
#[doc(hidden)]
pub const CODEGEN_ABI_VERSION: u32 = 69;

mod component;
mod controls;
mod diagnostics;
mod host;
mod input;
mod physics3d;
mod scene;
#[doc(hidden)]
pub mod scene3d;
mod views;

use component::{
    animation, arithmetic, callback, delegate, dynamic_children, localization, localized_number,
    model, property, record, repeat,
};
use controls::common as control_semantics;
use controls::{
    button_group, calendar, color_picker, combo, double_spin, font_picker, picker, range, split,
    stack, time_picker, tumbler,
};
use diagnostics::{inspection, render_stats, ui_log};
use host::{
    child_window, event_loop, external_loader, image_animation, image_asset, loader, media,
    native_dialog, timer, window_status,
};
use input::{
    drag, focus, hover, input_mask, input_pattern, keyboard, pointer, scale_rotate, styled_text,
    swipe_gesture, text_input,
};
use scene::{geometry, scene_plan, scene_tracking, visual_transform};
use views::{
    list, list_keys, list_sections, row_heights, table_clipboard, table_history, table_rows,
    table_selection, tree, viewport,
};

pub use animation::{Animation, Easing, SceneTransition, StateTiming};
#[doc(hidden)]
pub use arithmetic::{
    ceil_to_int, character_count, clamp_float, clamp_int, floor_to_int, format_fixed_float,
    format_fixed_int, max_float, min_float, modulo_int, number_sign, parse_float, parse_int,
    same_float_bits,
};
#[doc(hidden)]
pub use arithmetic::{int_divide, int_remainder, round_to_int, try_int_divide, try_int_remainder};
#[doc(hidden)]
pub use button_group::{
    ButtonGroup, ButtonGroupMembership, ButtonGroupOptions, ButtonGroupSelection, RadioMembership,
    bind_button_group, bind_button_group_options, bind_check_group, bind_dynamic_button_group,
    is_exclusive_push_option, prepare_button_group_option, prepare_radio_selection, radio_value,
    summarize_checks, toggled_button_group_index, toggled_button_group_value,
    try_prepare_button_group_option, try_prepare_radio_selection, try_summarize_checks,
};
pub use calendar::{CalendarAction, CalendarState, Date, is_calendar_control};
#[doc(hidden)]
pub use calendar::{
    CalendarCell, CalendarCells, CalendarHighlight, CalendarHighlightDay, CalendarUpdate,
    DateBounds, DatePopupState, apply_calendar_action, calendar_input_is_current,
    calendar_weekday_mask, try_apply_calendar_action, try_calendar_input_is_current,
};
#[doc(hidden)]
pub use callback::{CallbackAlreadyRunning, CallbackSlot, EffectfulCallback};
#[doc(hidden)]
pub use child_window::{SecondaryWindowCommand, SecondaryWindowRegistration, SecondaryWindowSlot};
#[doc(hidden)]
pub use color_picker::ColorPopupState;
pub use combo::{
    ChoiceIdentity, choice_enabled, choice_identity, combo_count, combo_find_enabled_label,
    combo_find_label, combo_index, combo_label, combo_labels, combo_row_index, combo_row_key,
    combo_text,
};
#[doc(hidden)]
pub use combo::{
    ComboEditor, ComboInput, ComboItems, ComboModel, ComboOptionInput, ComboOptionParameter,
    ComboOptionRenderer, ComboPopupHandle, ComboSelection, ComboStateModel, ComboSubmission,
    ComboUpdate, combo_model_is_current, combo_submission, combo_value, prepare_combo_input,
    prepare_combo_input_with_draft, prepare_combo_input_with_live_draft,
    resolve_combo_after_accept, try_prepare_combo_input, try_prepare_combo_input_with_draft,
    try_prepare_combo_input_with_live_draft, try_resolve_combo_after_accept,
};
pub use control_semantics::{
    ActionKey, ControlAction, DialogCommand, DialogRole, PopupPlacement, SpinAction, SpinRange,
    StandardButtonKind, SwipeSide, TabClose, TabMove, TableColumnOrder, TableHeader, TableSort,
    button_text, combo_editable, delay_millis, dialog_button_action, group_index, group_item,
    is_button, is_modal_overlay, is_pane, is_spin_box, is_window_overlay, page_indicator_count,
    page_indicator_index, resizable_table_column, resolve_swipe_index, search_suggestion,
    search_suggestions, spin_editable, spin_text, spin_value_after, text_input_value,
};
#[doc(hidden)]
pub use control_semantics::{
    DialogEvent, TableHeaderAction, page_indicator_step, prepare_swipe_side,
    resolve_navigation_value, set_overlay_open, try_prepare_swipe_side,
    try_resolve_navigation_value, try_resolve_swipe_index, try_set_overlay_open,
};
#[doc(hidden)]
pub use control_semantics::{PageIdentity, SwipePreview, SwipePreviewHandle};
#[doc(hidden)]
pub use control_semantics::{
    SPIN_VIEW_PROPERTIES, prepare_spin_view, spin_view_is_current, try_prepare_spin_view,
    try_spin_view_is_current,
};
#[doc(hidden)]
pub use control_semantics::{
    delay_button_transition_is_current, group_selection_item, moved_navigation_index,
    prepare_action_activation, prepare_action_view, prepare_expander_change,
    prepare_group_box_toggle, prepare_group_selection, prepare_group_selection_with_model,
    prepare_index_selection, prepare_row_toggle, prepare_row_toggle_matching,
    prepare_source_selection, prepare_tab_close, prepare_tab_move, resize_table_column,
    tab_is_closable, table_drag_available, try_delay_button_transition_is_current,
    try_prepare_action_activation, try_prepare_action_view, try_prepare_expander_change,
    try_prepare_group_box_toggle, try_prepare_group_selection,
    try_prepare_group_selection_with_model, try_prepare_index_selection, try_prepare_row_toggle,
    try_prepare_row_toggle_matching, try_prepare_source_selection, try_prepare_tab_close,
    try_prepare_tab_move, try_table_drag_available,
};
#[doc(hidden)]
pub use delegate::{ComponentRepeater, RowContext};
#[doc(hidden)]
pub use diagnostics::ui_trace;
#[doc(hidden)]
pub use diagnostics::update_profile;
pub use double_spin::{
    DecimalSpinRange, SpinDecimal, decimal_separator, decimal_spin_value_after, spin_text_is_valid,
};
#[doc(hidden)]
pub use drag::DragArguments;
pub use drag::{DragAction, DragEvent, is_drag_area};
#[doc(hidden)]
pub use dynamic_children::DynamicChildren;
#[doc(hidden)]
pub use event_loop::EventLoopRegistration;
pub use event_loop::{EventLoopError, WeakComponent, invoke_from_event_loop};
#[doc(hidden)]
pub use external_loader::{
    ExternalComponent, ExternalLoader, ExternalSource, LoadedExternal, register_external_loader,
};
#[doc(hidden)]
pub use focus::{ButtonAnimationDelivery, ButtonAnimations};
#[doc(hidden)]
pub use focus::{
    ButtonPressHandle, FocusCommandMethod, FocusHandler, QueuedFocusRequest, TextReplacement,
    is_focus_handler,
};
pub use focus::{
    FocusAction, FocusContext, FocusEvent, FocusHandle, FocusReason, FocusRequest, FocusRequests,
    TextCommand, TextCommandAvailability,
};
pub use font_picker::Font;
#[doc(hidden)]
pub use font_picker::{FontPopupState, is_font_family_choice, set_font_family_choices};
#[doc(hidden)]
pub use geometry::{CustomItemUpdater, GeometryHandle, GeometryMetric, SceneTouchRequest};
pub use hover::{CursorShape, HoverHandle};
pub use image_animation::{ImageAnimation, ImageAnimationFrame};
#[doc(hidden)]
pub use image_animation::{ImagePlaybackQueue, ImagePlaybackRegistration, ImagePlaybackSlot};
pub use image_asset::{ImageAsset, ImageIdentity, ImagePixels, LoadedAnimation, LoadedImage};
pub use input_mask::{InputMask, MaskDelete, MaskEdit};
pub use input_pattern::InputValidity;
#[doc(hidden)]
pub use input_pattern::input_pattern_accepts;
#[doc(hidden)]
pub use inspection::{
    InspectionInput, PresentationFeedback, PresentedFrame, RecordedAction, ViewportGpuFrame,
    ViewportGpuPass, ViewportGpuStats, ViewportRenderStats, ViewportRenderWork, ViewportStatistics,
    WindowInspector,
};
pub use keyboard::{EventResult, KeyEvent, KeyModifiers, KeyPhase, WheelEvent};
#[doc(hidden)]
pub use list::ViewIndex;
pub use list::{ListMetrics, RowKey};
#[doc(hidden)]
pub use list::{
    ListModel, ListRowLocator, ListRowRenderer, ListTextSearch, RowLifetime, StandardTableRowCache,
    TableCellEditor, TableColumnWindow, TableField, project_standard_table_rows,
    render_standard_table_rows, standard_list_rows, standard_list_rows_in_view,
    standard_table_rows, standard_table_rows_in_order, standard_table_rows_with_editors_in_order,
    standard_table_rows_with_header, try_project_standard_table_rows, try_render_rows_from_source,
    try_render_standard_table_rows,
};
#[doc(hidden)]
pub use list_keys::ListSelectionKeys;
pub use list_sections::{
    ListSection, ListSectionInput, ListSectionParameter, ListSectionProjection, ListSectionRenderer,
};
pub use loader::LoaderItem;
#[doc(hidden)]
pub use loader::{
    LoaderBuild, LoaderChild, LoaderFrame, LoaderMount, LoaderSlot, dispatch_loader_events,
    finish_loader_frame, loader_candidate_scene,
};
pub use localization::LocaleContext;
#[doc(hidden)]
pub use localization::{LocaleBinding, LocaleId, TranslationCatalog, TranslationValue};
pub use media::{
    MediaCommand, MediaFrame, MediaOptions, MediaQueue, MediaRegistration, MediaSession, MediaSlot,
    MediaState, MediaStatus, open_media_session, register_media_backend, register_sound_backend,
};
#[doc(hidden)]
pub use model::ModelEdit;
pub use model::VecModel;
pub use native_dialog::{
    FileDialogMode, FileDialogOptions, FileDialogSelection, MessageButtons, MessageDialogOptions,
    MessageLevel,
};
#[doc(hidden)]
pub use native_dialog::{
    NativeDialogEvent, NativeDialogOptions, NativeDialogPresentation, NativeDialogQueue,
    NativeDialogRegistration, NativeDialogResult, NativeDialogSlot,
};
#[doc(hidden)]
pub use physics3d::{
    PhysicsBodySpec, PhysicsCollider3D, PhysicsContact3D, PhysicsDebugLines, PhysicsDebugVertex,
    PhysicsHit3D, PhysicsJointSpec, PhysicsQueryFilter3D, PhysicsShape3D, PhysicsShapeSpec,
    PhysicsWorld3D,
};
#[doc(hidden)]
pub use pointer::FIRST_SCENE_TOUCH_FINGER_ID;
pub use pointer::{
    PointerButton, PointerButtons, PointerEvent, PointerEventKind, PointerHandle, TouchPoint,
    TouchPoints,
};
pub use property::ComponentLifetime;
pub use property::{ChangeSignal, InputProperty, Property};
pub use range::{
    DialInputMode, IndicatorState, ProgressRange, RangeAction, ScrollBarRange, ScrollSnapMode,
    SliderInterval, SliderRange, indicator_state, progress_value,
};
#[doc(hidden)]
pub use range::{
    RANGE_VIEW_PROPERTIES, RangePositionHandle, prepare_range_view, range_view_properties,
    try_prepare_range_view,
};
pub use record::{RecordList, RecordRowValidator, RecordValue};
pub use render_stats::RenderStats3D;
#[doc(hidden)]
pub use render_stats::RenderStats3DData;
pub use repeat::{Repeater, repeat_scene, static_repeat_scene};
#[doc(hidden)]
pub use row_heights::{RowHeightOverrides, RowHeightProjection, RowHeights};
pub use scale_rotate::{ScaleRotateHandle, ScaleRotateSample};
pub use scene::custom_item::{
    CustomItem, CustomItemChange, CustomItemContext, CustomItemData, CustomItemDrawing,
    CustomItemEvent, CustomItemId, CustomItemPlacement, CustomItemRect, CustomItemRegistration,
    CustomItemRole, CustomItemUpdate, register_custom_item,
};
#[doc(hidden)]
pub use scene_plan::{
    GridCellPosition, GridCellSlot, GridPlanner, GridTrackInput, LoadedVectorPath, ScenePlanSlice,
    StaticVectorPath, VectorSegment,
};
pub use scene_tracking::{SceneChanges, SceneIndex, SceneNodeId};
pub use scene3d::bounds::SceneBounds3D as Bounds3D;
pub use scene3d::mesh::loader::{
    MeshLoader3D, mesh_loader_deadline, poll_mesh_loaders, register_mesh_importer,
};
pub use scene3d::mesh::nodes::MeshNode3D;
pub use scene3d::rotation::{Quaternion3D, RotationTrack3D};
pub use scene3d::{
    Hit3D, Instance3D, InstanceTable3D, MeshAsset3D, Point3D, ProjectedPoint3D, TextureData2D,
    TextureData3D, Vector3D, WalkResult3D,
};
#[doc(hidden)]
pub use scene3d::{effect as scene_effect, material_shader};
#[doc(hidden)]
pub use split::{
    SplitHandleInput, SplitResizeHandle, SplitState, decode_split_sizes, decode_split_state,
    encode_split_sizes, encode_split_state,
};
#[doc(hidden)]
pub use stack::{StackChange, StackEvent, StackNavigationArgument, StackNavigationMethod};
pub use stack::{StackNavigation, StackSnapshot, StackState};
use std::{cell::OnceCell, collections::BTreeMap, rc::Rc, sync::Arc};
pub use styled_text::{
    MarkdownStream, StyledLink, StyledLinkRewriter, StyledRun, StyledText, StyledTextError,
    TextStyle,
};
pub use swipe_gesture::{SwipeHandle, SwipeSample};
#[doc(hidden)]
pub use table_clipboard::{
    TablePaste, TablePasteInput, paste_table_text, prepare_table_paste, table_paste_bool,
    table_paste_check_state, table_paste_number, try_table_paste_number,
    try_with_table_paste_target, with_table_paste_target,
};
#[doc(hidden)]
pub use table_history::{TableEditHistory, TableHistoryPayload, TablePasteHistory, TableRowPatch};
pub use table_rows::{
    TableCellEditTarget, TableEditPosition, TableRowOrder, TableRowPositions, TableRows,
    next_table_edit_cell, next_table_edit_cell_in_view, next_table_edit_cell_in_view_if,
    next_table_edit_cell_while_current, table_cell_edit_target, table_cell_edit_target_if,
    table_cell_edit_target_matching, try_history_replay_source, try_next_table_edit_cell,
    try_next_table_edit_cell_in_view, try_next_table_edit_cell_while_current,
    try_prepare_history_replay, try_table_cell_edit_target, try_table_cell_edit_target_matching,
};
#[doc(hidden)]
pub use table_selection::{
    ListSelectionCommand, TableSelectionChange, TableSelectionCommand, TableSelectionDrag,
    TableSelectionDragHandle, TableSelectionEvent, TableSelectionMethod, TableSelectionOptions,
};
pub use table_selection::{
    RowSelection, SelectionGesture, SelectionTarget, TableSelection, TableSelectionAction,
    TableSelectionSnapshot,
};
pub use table_selection::{
    copy_table_text, copy_table_text_in_order, copy_table_text_in_view, copy_table_text_in_view_if,
    copy_table_text_while_current, try_copy_table_text_in_order, try_copy_table_text_in_view,
    try_copy_table_text_while_current,
};
pub use text_input::{InputPurpose, InputType, KeyboardCapitalization, ReturnKey};
#[doc(hidden)]
pub use text_input::{
    NumericTextLimits, SearchInput, SearchUpdate, text_input_validity, text_submission_is_valid,
    text_submission_with, text_submission_with_mask, try_text_input_validity,
    try_text_submission_is_valid, try_text_submission_with, try_text_submission_with_mask,
};
pub use time_picker::Time;
#[doc(hidden)]
pub use time_picker::{TimePickerOptions, TimePopupState};
#[doc(hidden)]
pub use timer::{
    FrameTick, TimerCommand, TimerConfig, TimerDelivery, TimerQueue, TimerRegistration, TimerSlot,
    TimerTick,
};
pub use tree::{TreeAction, TreeCommand, TreeItem, TreeSelectionAction, TreeSnapshot, TreeState};
#[doc(hidden)]
pub use tree::{
    TreeCommandMethod, TreeControlChange, TreeControlEvent, TreeExpansionChange,
    TreeSelectionChange, TreeSelectionCommand, TreeSelectionLookup,
};
pub use tumbler::{Tumbler, TumblerItems};
pub use ui_log::LogLevel;
#[doc(hidden)]
pub use ui_log::log_ui;
pub use viewport::{
    ScrollAction, ScrollAlignment, ScrollContext, ScrollRequest, ScrollRequests, ViewportHandle,
    ViewportMotion,
};
#[doc(hidden)]
pub use viewport::{ScrollBarStep, ScrollCommand, ScrollCommandMethod, SourceScroll};
pub use visual_transform::{Point2D, Transform2D, Vector2D};
pub use window_status::{WindowCommand, WindowStatus};
#[doc(hidden)]
pub use window_status::{WindowCommandMethod, WindowLifecycle, WindowMetric, WindowStatusBinding};

/// The backend-independent interface implemented by generated UI components.
pub trait Component {
    fn render(&self) -> Node;

    /// Revision of the source currently applied to this instance.
    #[doc(hidden)]
    fn source_generation(&self) -> u64 {
        0
    }

    #[doc(hidden)]
    fn window_inspector(&self, _scene: &Rc<Node>) -> Option<WindowInspector> {
        None
    }

    /// Supply the language shared by the desktop hosting this component.
    #[doc(hidden)]
    fn set_locale_context(&self, _context: LocaleContext) {}

    /// Return the current scene. Reactive components may reuse a clean snapshot.
    fn render_shared(&self) -> Rc<Node> {
        Rc::new(self.render())
    }

    /// Report invalidation caused by a shared reactive dependency.
    fn scene_dirty(&self) -> bool {
        false
    }

    /// Decide whether the host may close this window.
    fn close_requested(&mut self) -> bool {
        true
    }

    /// Consume a close command emitted by the component.
    fn take_window_close_request(&mut self) -> bool {
        false
    }

    /// Consume presentation commands in the order emitted by the component.
    fn take_window_commands(&mut self) -> Vec<WindowCommand> {
        Vec::new()
    }

    #[doc(hidden)]
    fn has_window_commands(&self) -> bool {
        false
    }

    /// Publish native state after reading it from the window backend.
    fn publish_window_status(&self, _status: WindowStatus) {}

    /// Native child windows currently requested by this component.
    #[doc(hidden)]
    fn secondary_windows(&self) -> Option<Rc<[SecondaryWindowRegistration]>> {
        None
    }

    /// Render the requested model rows of a virtual view in the current base scene.
    fn list_rows(&self, _path: &[usize], _rows: std::ops::Range<usize>) -> Vec<Node> {
        Vec::new()
    }

    #[doc(hidden)]
    fn next_table_edit_cell(
        &self,
        _scene: &Node,
        _path: &[usize],
        _origin: TableEditPosition,
        _columns: &[usize],
        _backward: bool,
    ) -> Option<TableEditPosition> {
        None
    }

    #[doc(hidden)]
    fn copy_table_at(&self, _scene: &Node, _path: &[usize], _columns: &[usize]) -> Option<String> {
        None
    }

    #[doc(hidden)]
    fn route_table_query(
        &self,
        scene: &Node,
        path: &[usize],
        input: InputRoute<'_>,
    ) -> Option<TableQueryResult> {
        let RoutedEvent::QueryTable(query) = *input.event() else {
            return None;
        };
        match query {
            TableQuery::Copy(columns) => self
                .copy_table_at(scene, path, columns)
                .map(TableQueryResult::Text),
            TableQuery::NextEdit(query) => self
                .next_table_edit_cell(scene, path, query.origin, query.columns, query.backward)
                .map(TableQueryResult::Position),
        }
    }

    #[doc(hidden)]
    fn prepare_table_paste(
        &self,
        _scene: &Node,
        _path: &[usize],
        _row: usize,
        _columns: Vec<usize>,
    ) -> Option<TablePaste> {
        None
    }

    #[doc(hidden)]
    fn table_paste_stamp(&self, _scene: &Node, _path: &[usize]) -> Option<(u64, u64)> {
        None
    }

    #[doc(hidden)]
    fn replay_table_paste_at(&self, _scene: &Node, _path: &[usize], _redo: bool) -> bool {
        false
    }

    #[doc(hidden)]
    fn replay_table_edit_at(&self, scene: &Node, path: &[usize], redo: bool) -> bool {
        self.replay_table_paste_at(scene, path, redo)
    }

    fn dispatch(&mut self, element_id: &str, event: &str) -> bool;

    /// Commit the complete value of an editable text item after input validation.
    fn edit_at(&mut self, _scene: &Node, _path: &[usize], _text: String) -> bool {
        false
    }

    /// Commit a TableCell or TreeCell draft only if its model row and original value still match.
    fn commit_table_cell(
        &mut self,
        _scene: &Node,
        _path: &[usize],
        _original: &str,
        _text: String,
    ) -> bool {
        false
    }

    /// Apply a range control action through the same path used by pointer and keyboard input.
    fn adjust_range_at(&mut self, _scene: &Node, _path: &[usize], _action: RangeAction) -> bool {
        false
    }

    fn control_at(&mut self, scene: &Node, path: &[usize], action: ControlAction) -> bool {
        action == ControlAction::ValidateTextSubmission
            && scene
                .interactive_at_path(path)
                .is_some_and(Node::accepts_text_submission)
    }

    #[doc(hidden)]
    fn drag_event_at(&mut self, _scene: &Node, _path: &[usize], _event: &DragEvent) -> bool {
        false
    }

    #[doc(hidden)]
    fn key_event_at(
        &mut self,
        _scene: &Node,
        _path: &[usize],
        _phase: KeyPhase,
        _event: &KeyEvent,
    ) -> EventResult {
        EventResult::Reject
    }

    #[doc(hidden)]
    fn focus_event_at(&mut self, _scene: &Node, _path: &[usize], _event: FocusEvent) -> bool {
        false
    }

    #[doc(hidden)]
    fn wheel_event_at(
        &mut self,
        _scene: &Node,
        _path: &[usize],
        _event: &WheelEvent,
    ) -> EventResult {
        EventResult::Reject
    }

    #[doc(hidden)]
    fn pointer_event_at(&mut self, _scene: &Node, _path: &[usize], _event: &PointerEvent) -> bool {
        false
    }

    #[doc(hidden)]
    fn route_input(&mut self, scene: &Node, path: &[usize], input: InputRoute<'_>) -> bool {
        input.event().apply(self, scene, path)
    }

    fn resize_split_at(
        &mut self,
        _scene: &Node,
        _path: &[usize],
        _weights: &[(usize, f32)],
    ) -> bool {
        false
    }

    fn resize_split_sizes_at(
        &mut self,
        _scene: &Node,
        _path: &[usize],
        _sizes: &[(usize, f32)],
    ) -> bool {
        false
    }

    /// Dispatch to the node hit by the backend. IDs may repeat in component instances.
    fn dispatch_at(&mut self, scene: &Node, path: &[usize], event: &str) -> bool {
        scene
            .at_path(path)
            .and_then(Node::element_id)
            .is_some_and(|id| self.dispatch(id, event))
    }
}

impl<T: Component + ?Sized> Component for Box<T> {
    fn route_input(&mut self, scene: &Node, path: &[usize], input: InputRoute<'_>) -> bool {
        (**self).route_input(scene, path, input)
    }

    fn render(&self) -> Node {
        (**self).render()
    }
    fn source_generation(&self) -> u64 {
        (**self).source_generation()
    }
    fn window_inspector(&self, scene: &Rc<Node>) -> Option<WindowInspector> {
        (**self).window_inspector(scene)
    }
    fn set_locale_context(&self, context: LocaleContext) {
        (**self).set_locale_context(context);
    }
    fn focus_event_at(&mut self, scene: &Node, path: &[usize], event: FocusEvent) -> bool {
        (**self).focus_event_at(scene, path, event)
    }
    fn drag_event_at(&mut self, scene: &Node, path: &[usize], event: &DragEvent) -> bool {
        (**self).drag_event_at(scene, path, event)
    }
    fn key_event_at(
        &mut self,
        scene: &Node,
        path: &[usize],
        phase: KeyPhase,
        event: &KeyEvent,
    ) -> EventResult {
        (**self).key_event_at(scene, path, phase, event)
    }
    fn render_shared(&self) -> Rc<Node> {
        (**self).render_shared()
    }
    fn scene_dirty(&self) -> bool {
        (**self).scene_dirty()
    }
    fn close_requested(&mut self) -> bool {
        (**self).close_requested()
    }
    fn take_window_close_request(&mut self) -> bool {
        (**self).take_window_close_request()
    }
    fn take_window_commands(&mut self) -> Vec<WindowCommand> {
        (**self).take_window_commands()
    }
    fn has_window_commands(&self) -> bool {
        (**self).has_window_commands()
    }
    fn publish_window_status(&self, status: WindowStatus) {
        (**self).publish_window_status(status);
    }
    fn secondary_windows(&self) -> Option<Rc<[SecondaryWindowRegistration]>> {
        (**self).secondary_windows()
    }

    fn list_rows(&self, path: &[usize], rows: std::ops::Range<usize>) -> Vec<Node> {
        (**self).list_rows(path, rows)
    }

    fn next_table_edit_cell(
        &self,
        scene: &Node,
        path: &[usize],
        origin: TableEditPosition,
        columns: &[usize],
        backward: bool,
    ) -> Option<TableEditPosition> {
        (**self).next_table_edit_cell(scene, path, origin, columns, backward)
    }

    fn copy_table_at(&self, scene: &Node, path: &[usize], columns: &[usize]) -> Option<String> {
        (**self).copy_table_at(scene, path, columns)
    }

    fn route_table_query(
        &self,
        scene: &Node,
        path: &[usize],
        input: InputRoute<'_>,
    ) -> Option<TableQueryResult> {
        (**self).route_table_query(scene, path, input)
    }

    fn prepare_table_paste(
        &self,
        scene: &Node,
        path: &[usize],
        row: usize,
        columns: Vec<usize>,
    ) -> Option<TablePaste> {
        (**self).prepare_table_paste(scene, path, row, columns)
    }

    fn table_paste_stamp(&self, scene: &Node, path: &[usize]) -> Option<(u64, u64)> {
        (**self).table_paste_stamp(scene, path)
    }

    fn replay_table_paste_at(&self, scene: &Node, path: &[usize], redo: bool) -> bool {
        (**self).replay_table_paste_at(scene, path, redo)
    }

    fn replay_table_edit_at(&self, scene: &Node, path: &[usize], redo: bool) -> bool {
        (**self).replay_table_edit_at(scene, path, redo)
    }

    fn dispatch(&mut self, element_id: &str, event: &str) -> bool {
        (**self).dispatch(element_id, event)
    }

    fn edit_at(&mut self, scene: &Node, path: &[usize], text: String) -> bool {
        (**self).edit_at(scene, path, text)
    }

    fn commit_table_cell(
        &mut self,
        scene: &Node,
        path: &[usize],
        original: &str,
        text: String,
    ) -> bool {
        (**self).commit_table_cell(scene, path, original, text)
    }

    fn adjust_range_at(&mut self, scene: &Node, path: &[usize], action: RangeAction) -> bool {
        (**self).adjust_range_at(scene, path, action)
    }

    fn control_at(&mut self, scene: &Node, path: &[usize], action: ControlAction) -> bool {
        (**self).control_at(scene, path, action)
    }

    fn resize_split_at(&mut self, scene: &Node, path: &[usize], weights: &[(usize, f32)]) -> bool {
        (**self).resize_split_at(scene, path, weights)
    }
    fn resize_split_sizes_at(
        &mut self,
        scene: &Node,
        path: &[usize],
        sizes: &[(usize, f32)],
    ) -> bool {
        (**self).resize_split_sizes_at(scene, path, sizes)
    }

    fn dispatch_at(&mut self, scene: &Node, path: &[usize], event: &str) -> bool {
        (**self).dispatch_at(scene, path, event)
    }

    fn wheel_event_at(&mut self, scene: &Node, path: &[usize], event: &WheelEvent) -> EventResult {
        (**self).wheel_event_at(scene, path, event)
    }

    fn pointer_event_at(&mut self, scene: &Node, path: &[usize], event: &PointerEvent) -> bool {
        (**self).pointer_event_at(scene, path, event)
    }
}

#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct TableEditNavigation<'a> {
    pub origin: TableEditPosition,
    pub columns: &'a [usize],
    pub backward: bool,
}

#[doc(hidden)]
#[derive(Clone, Copy)]
pub enum TableQuery<'a> {
    Copy(&'a [usize]),
    NextEdit(TableEditNavigation<'a>),
}

impl TableQuery<'_> {
    pub fn run(
        self,
        ui: &(impl Component + ?Sized),
        scene: &Node,
        path: &[usize],
    ) -> Option<TableQueryResult> {
        RoutedEvent::QueryTable(self)
            .with_input(scene, path, |input| {
                ui.route_table_query(scene, path, input)
            })
            .flatten()
    }
}

#[doc(hidden)]
pub enum TableQueryResult {
    Text(String),
    Position(TableEditPosition),
}

impl TableQueryResult {
    pub fn text(self) -> Option<String> {
        match self {
            Self::Text(text) => Some(text),
            Self::Position(_) => None,
        }
    }

    pub fn position(self) -> Option<TableEditPosition> {
        match self {
            Self::Position(position) => Some(position),
            Self::Text(_) => None,
        }
    }
}

#[doc(hidden)]
pub enum RoutedEvent<'a> {
    Dispatch(&'a str),
    Edit(&'a str),
    CommitCell { original: &'a str, text: &'a str },
    PasteTable(TablePasteInput<'a>),
    ReplayTable { redo: bool },
    QueryTable(TableQuery<'a>),
    Range(RangeAction),
    Control(ControlAction),
    Drag(&'a DragEvent),
    Key(KeyPhase, &'a KeyEvent),
    Wheel(&'a WheelEvent),
    Pointer(&'a PointerEvent),
    Focus(FocusEvent),
    ResizeSplit(&'a [(usize, f32)]),
    ResizeSplitSizes(&'a [(usize, f32)]),
}

/// Carries one availability check through all component instances on an input path.
#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct InputRoute<'a> {
    event: &'a RoutedEvent<'a>,
    reads: Option<&'a property::ReadWatch>,
    root_visibility_overridden: bool,
}

impl<'a> InputRoute<'a> {
    pub fn event(self) -> &'a RoutedEvent<'a> {
        self.event
    }

    pub fn checks_availability(self) -> bool {
        self.reads.is_some()
    }

    pub fn root_visibility_overridden(self) -> bool {
        self.root_visibility_overridden
    }

    pub fn with_root_visibility_override(mut self, overridden: bool) -> Self {
        self.root_visibility_overridden = overridden;
        self
    }

    pub fn record_root_visibility_override(self, lifetime: &ComponentLifetime) {
        if self.root_visibility_overridden && self.checks_availability() {
            lifetime.record_input_root_override();
        }
    }

    pub fn try_visible<E>(self, visible: impl FnOnce() -> Result<bool, E>) -> Result<bool, E> {
        if self.root_visibility_overridden {
            Ok(true)
        } else {
            visible()
        }
    }

    pub fn visible(self, visible: impl FnOnce() -> bool) -> bool {
        self.root_visibility_overridden || visible()
    }

    pub fn is_current(self) -> bool {
        self.reads.is_none_or(|reads| !reads.changed())
    }

    pub fn can_open_drawer(self, node: &Node, is_target: bool) -> bool {
        is_target
            && node.kind == "Drawer"
            && matches!(
                self.event,
                RoutedEvent::Control(ControlAction::Drawer(true))
            )
    }

    pub fn try_available<E>(self, available: impl FnOnce() -> Result<bool, E>) -> Result<bool, E> {
        match self.reads {
            Some(reads) => Ok(!reads.changed()
                && property::track_input_availability(available)?
                && !reads.changed()),
            None => Ok(true),
        }
    }

    pub fn available(self, available: impl FnOnce() -> bool) -> bool {
        self.try_available(|| Ok::<_, std::convert::Infallible>(available()))
            .unwrap_or_else(|error| match error {})
    }

    /// Only run preparation after availability checks pass and discard its result if input changes.
    pub fn try_prepare<T, E>(
        self,
        available: impl FnOnce() -> Result<bool, E>,
        prepare: impl FnOnce() -> Result<Option<T>, E>,
    ) -> Result<Option<T>, E> {
        if !self.try_available(available)? {
            return Ok(None);
        }
        Ok(prepare()?.filter(|_| self.is_current()))
    }

    pub fn prepare<T>(
        self,
        available: impl FnOnce() -> bool,
        prepare: impl FnOnce() -> Option<T>,
    ) -> Option<T> {
        self.try_prepare(
            || Ok::<_, std::convert::Infallible>(available()),
            || Ok(prepare()),
        )
        .unwrap_or_else(|error| match error {})
    }
}

impl RoutedEvent<'_> {
    #[doc(hidden)]
    pub fn activates_item_delegate(&self, has_action: bool) -> bool {
        matches!(
            self,
            Self::Dispatch("clicked" | "toggled") | Self::Control(ControlAction::SetSwitch(_))
        ) || has_action && matches!(self, Self::Control(ControlAction::ActivateAction))
    }

    #[doc(hidden)]
    pub fn checked_after(&self, current: bool) -> Option<bool> {
        match *self {
            Self::Dispatch("clicked") => Some(!current),
            Self::Control(ControlAction::SetSwitch(checked)) if checked != current => Some(checked),
            _ => None,
        }
    }

    fn is_input_cleanup(&self) -> bool {
        match self {
            Self::Focus(event) => event.is_loss(),
            Self::Pointer(event) => event.kind == PointerEventKind::Cancel,
            Self::Drag(event) => event.is_terminal(),
            _ => false,
        }
    }

    /// Cleanup events can follow an existing delegate to a new position.
    pub fn follows_row(&self, scene: &Node, path: &[usize]) -> bool {
        match self {
            Self::Dispatch(event) => scene
                .at_path(path)
                .is_some_and(|node| node.terminal_dispatch(event)),
            _ => self.is_input_cleanup(),
        }
    }

    /// Resolve input against the scene that produced it.
    pub fn input_target<'a>(&self, scene: &'a Node, path: &[usize]) -> Option<&'a Node> {
        let target = || {
            if self.is_input_cleanup() {
                scene.at_path(path)
            } else {
                scene.interactive_at_path(path)
            }
        };
        match self {
            Self::Dispatch(event) => scene.dispatch_target(path, event),
            Self::Control(action) => scene.control_target(path, *action),
            Self::Range(_) => scene.range_target(path),
            Self::CommitCell { original, .. } => scene.table_cell_commit_target(path, original),
            Self::PasteTable(_) => scene
                .interactive_at_path(path)
                .filter(|node| node.kind == "TableView" && node.boolean("pasteable", false)),
            Self::ReplayTable { .. } => {
                scene.table_edit_history_model(path)?;
                scene.at_path(path)
            }
            Self::QueryTable(query) => scene.interactive_at_path(path).filter(|node| {
                node.kind == "TableView"
                    && node.boolean(
                        match query {
                            TableQuery::Copy(_) => "copyable",
                            TableQuery::NextEdit(_) => "edit-on-tab",
                        },
                        false,
                    )
            }),
            Self::ResizeSplit(_) | Self::ResizeSplitSizes(_) => scene
                .interactive_at_path(path)
                .filter(|node| node.kind == "SplitView"),
            Self::Edit(text) => scene
                .interactive_at_path(path)
                .filter(|node| node.accepts_editor_update(text)),
            Self::Focus(event) => scene.focus_event_target(path, *event),
            Self::Key(phase, _) => scene.key_event_target(path, *phase),
            Self::Wheel(_) => scene
                .interactive_at_path(path)
                .filter(|node| node.handles_wheel()),
            Self::Pointer(_) => target().filter(|node| node.pointer_handle.is_some()),
            Self::Drag(event) => target().filter(|node| node.kind == event.target_kind()),
        }
    }

    pub fn with_input<T>(
        &self,
        scene: &Node,
        path: &[usize],
        route: impl FnOnce(InputRoute<'_>) -> T,
    ) -> Option<T> {
        self.input_target(scene, path)?;
        let input = InputRoute {
            event: self,
            reads: None,
            root_visibility_overridden: false,
        };
        Some(property::with_input_path(|| {
            if self.follows_row(scene, path) {
                route(input)
            } else {
                property::watch_reads(|reads| {
                    route(InputRoute {
                        reads: Some(reads),
                        ..input
                    })
                })
            }
        }))
    }

    pub fn deliver_input(
        &self,
        ui: &mut (impl Component + ?Sized),
        scene: &Node,
        path: &[usize],
    ) -> bool {
        let original_path = path;
        self.with_input(scene, path, |input| {
            let mut node = scene;
            let mut path = path;
            while let Some((&index, tail)) = path.split_first() {
                let Some(child) = node.child_at(index) else {
                    return false;
                };
                if let Some(component) = &child.external_component {
                    return component
                        .with_mut(|ui| self.deliver_input(ui, child, tail))
                        .unwrap_or(false);
                }
                node = child;
                path = tail;
            }
            let native = match self {
                Self::Pointer(event) => Some(CustomItemEvent::Pointer(event)),
                Self::Key(phase, event) => Some(CustomItemEvent::Key(*phase, event)),
                Self::Wheel(event) => Some(CustomItemEvent::Wheel(event)),
                Self::Focus(event) => Some(CustomItemEvent::Focus(*event)),
                Self::Dispatch("clicked") => Some(CustomItemEvent::Activate),
                _ => None,
            };
            let native_handled =
                native.is_some_and(|event| node.custom_item_event(event) == EventResult::Accept);
            if native_handled && matches!(self, Self::Key(..) | Self::Wheel(_)) {
                return true;
            }
            ui.route_input(scene, original_path, input) || native_handled
        })
        .unwrap_or(false)
    }

    pub fn apply(&self, ui: &mut (impl Component + ?Sized), scene: &Node, path: &[usize]) -> bool {
        match self {
            Self::Dispatch(event) => ui.dispatch_at(scene, path, event),
            Self::Edit(text) => ui.edit_at(scene, path, (*text).into()),
            Self::CommitCell { original, text } => {
                ui.commit_table_cell(scene, path, original, (*text).into())
            }
            Self::PasteTable(_) => false,
            Self::ReplayTable { redo } => ui.replay_table_edit_at(scene, path, *redo),
            Self::QueryTable(_) => false,
            Self::Range(action) => ui.adjust_range_at(scene, path, *action),
            Self::Control(action) => ui.control_at(scene, path, *action),
            Self::Drag(event) => ui.drag_event_at(scene, path, event),
            Self::Key(phase, event) => {
                ui.key_event_at(scene, path, *phase, event) == EventResult::Accept
            }
            Self::Wheel(event) => ui.wheel_event_at(scene, path, event) == EventResult::Accept,
            Self::Pointer(event) => ui.pointer_event_at(scene, path, event),
            Self::Focus(event) => ui.focus_event_at(scene, path, *event),
            Self::ResizeSplit(weights) => ui.resize_split_at(scene, path, weights),
            Self::ResizeSplitSizes(sizes) => ui.resize_split_sizes_at(scene, path, sizes),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Length(pub f64);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Percent(pub f64);

#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq)]
pub struct Color(pub u32);

impl Color {
    pub(crate) fn normalized_rgba(self) -> [f64; 4] {
        self.0.to_be_bytes().map(|byte| f64::from(byte) / 255.0)
    }

    /// Determine the hue sector of a chromatic RGB sample.
    pub(crate) fn hue_sector([red, green, blue]: [f64; 3], high: f64, chroma: f64) -> f64 {
        let sector = if high == red {
            (green - blue) / chroma
        } else if high == green {
            (blue - red) / chroma + 2.0
        } else {
            (red - green) / chroma + 4.0
        };
        sector.rem_euclid(6.0)
    }

    pub(crate) fn chromatic_rgb(sector: f64, chroma: f64) -> [f64; 3] {
        let middle = chroma * (1.0 - (sector.rem_euclid(2.0) - 1.0).abs());
        match sector as u8 {
            0 => [chroma, middle, 0.0],
            1 => [middle, chroma, 0.0],
            2 => [0.0, chroma, middle],
            3 => [0.0, middle, chroma],
            4 => [middle, 0.0, chroma],
            _ => [chroma, 0.0, middle],
        }
    }

    #[doc(hidden)]
    pub fn from_rgba(red: i64, green: i64, blue: i64, alpha: f64) -> Self {
        let channel = |value: i64| value.clamp(0, 255) as u32;
        let rgb = (channel(red) << 24) | (channel(green) << 16) | (channel(blue) << 8);
        Self(rgb).with_alpha(alpha)
    }

    #[doc(hidden)]
    pub fn with_alpha(self, alpha: f64) -> Self {
        let alpha = (alpha.clamp(0.0, 1.0) * 255.0).round() as u32;
        Self((self.0 & 0xffff_ff00) | alpha)
    }

    #[doc(hidden)]
    pub fn mix(self, other: Self, amount: f64) -> Self {
        let amount = if amount.is_nan() {
            0.0
        } else {
            amount.clamp(0.0, 1.0)
        };
        crate::animation::mix_color(self, other, amount)
    }

    /// Decode the RGB and RGBA spellings accepted by the XR dialect.
    #[doc(hidden)]
    pub fn parse_hex_literal(spelling: &str) -> Option<Self> {
        let digits = spelling.strip_prefix('#')?;
        if !matches!(digits.len(), 3 | 4 | 6 | 8)
            || !digits.bytes().all(|digit| digit.is_ascii_hexdigit())
        {
            return None;
        }
        let value = u32::from_str_radix(digits, 16).ok()?;
        let rgba = match digits.len() {
            3 | 4 => {
                let expanded = (0..digits.len()).rev().fold(0, |rgba, index| {
                    let digit = (value >> (index * 4)) & 0xf;
                    (rgba << 8) | (digit * 17)
                });
                if digits.len() == 3 {
                    (expanded << 8) | 0xff
                } else {
                    expanded
                }
            }
            6 => (value << 8) | 0xff,
            _ => value,
        };
        Some(Self(rgba))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorScheme {
    #[default]
    System,
    Light,
    Dark,
}

/// Display transform for scene-linear HDR radiance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum SceneToneMap {
    None = 0,
    Linear = 1,
    #[default]
    Reinhard = 2,
    Aces = 3,
    HejlDawson = 4,
    Filmic = 5,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ImageFormat {
    Png,
    Svg,
}

#[derive(Clone, Debug, Default)]
pub enum Image {
    #[default]
    Empty,
    Embedded {
        format: ImageFormat,
        bytes: &'static [u8],
        size: [u32; 2],
    },
    Animated(&'static ImageAnimation),
    Loaded(LoadedImage),
    NineSliced {
        format: ImageFormat,
        bytes: &'static [u8],
        size: [u32; 2],
        edges: NineSlice,
    },
}

/// Source edge widths in pixels, clockwise from the top.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NineSlice {
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
    pub left: u32,
}

impl NineSlice {
    pub fn fits(self, [width, height]: [u32; 2]) -> bool {
        self.left.saturating_add(self.right) < width
            && self.top.saturating_add(self.bottom) < height
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("nine-slice edges must leave a nonempty center in the source image")]
pub struct InvalidNineSlice;

impl PartialEq for Image {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Empty, Self::Empty) => true,
            (
                Self::Embedded {
                    format: a,
                    bytes: b,
                    size: c,
                },
                Self::Embedded {
                    format: x,
                    bytes: y,
                    size: z,
                },
            ) => (a, b, c) == (x, y, z),
            (
                Self::NineSliced {
                    format: a,
                    bytes: b,
                    size: c,
                    edges: d,
                },
                Self::NineSliced {
                    format: w,
                    bytes: x,
                    size: y,
                    edges: z,
                },
            ) => (a, b, c, d) == (w, x, y, z),
            (Self::Animated(left), Self::Animated(right)) => left == right,
            (Self::Loaded(left), Self::Loaded(right)) => left == right,
            (Self::Loaded(_), _) | (_, Self::Loaded(_)) => {
                ImageAsset::from(self.clone()) == ImageAsset::from(other.clone())
            }
            _ => false,
        }
    }
}

impl Eq for Image {}

impl Image {
    pub fn size(&self) -> [u32; 2] {
        match self {
            Self::Loaded(image) => image.size(),
            Self::Empty => [0, 0],
            Self::Animated(animation) => animation.size(),
            Self::Embedded { size, .. } | Self::NineSliced { size, .. } => *size,
        }
    }

    pub fn frame_count(&self) -> usize {
        match self {
            Self::Empty => 0,
            Self::Animated(animation) => animation.frame_count(),
            _ => 1,
        }
    }

    pub fn frame(self, index: usize) -> Self {
        match self {
            Self::Animated(animation) => animation.frame(index),
            _ => self,
        }
    }

    pub fn with_nine_slice(self, edges: NineSlice) -> Result<Self, InvalidNineSlice> {
        match self {
            Self::Embedded {
                format,
                bytes,
                size,
            }
            | Self::NineSliced {
                format,
                bytes,
                size,
                ..
            } if edges.fits(size) => Ok(Self::NineSliced {
                format,
                bytes,
                size,
                edges,
            }),
            Self::Loaded(image) => image.with_nine_slice(edges).map(Self::Loaded),
            _ => Err(InvalidNineSlice),
        }
    }
}

/// Immutable font data shared by scenes from the same generated UI library.
pub struct EmbeddedFonts(&'static [&'static [u8]]);

impl EmbeddedFonts {
    pub const fn new(fonts: &'static [&'static [u8]]) -> Self {
        Self(fonts)
    }

    pub fn sources(&self) -> &'static [&'static [u8]] {
        self.0
    }
}

impl PartialEq for EmbeddedFonts {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl std::fmt::Debug for EmbeddedFonts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmbeddedFonts")
            .field("count", &self.0.len())
            .finish()
    }
}

/// Owned font data shared by the roots of an interpreted UI.
#[derive(Clone)]
pub struct LoadedFonts(Arc<Vec<Arc<Vec<u8>>>>);

impl LoadedFonts {
    pub fn new(fonts: Vec<Vec<u8>>) -> Self {
        Self(Arc::new(fonts.into_iter().map(Arc::new).collect()))
    }

    pub fn sources(&self) -> &[Arc<Vec<u8>>] {
        &self.0
    }
}

impl std::fmt::Debug for LoadedFonts {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        output
            .debug_struct("LoadedFonts")
            .field("count", &self.0.len())
            .finish()
    }
}

impl PartialEq for LoadedFonts {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageFit {
    Fill,
    #[default]
    Contain,
    Cover,
    Preserve,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageTileMode {
    #[default]
    Stretch,
    Repeat,
    Round,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageHorizontalAlignment {
    Left,
    #[default]
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageVerticalAlignment {
    Top,
    #[default]
    Center,
    Bottom,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageRendering {
    #[default]
    Smooth,
    Pixelated,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonDisplay {
    IconOnly,
    TextOnly,
    #[default]
    TextBesideIcon,
    TextUnderIcon,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LayoutAlignment {
    #[default]
    Stretch,
    Center,
    Start,
    End,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CrossAxisAlignment {
    #[default]
    Auto,
    Stretch,
    Start,
    End,
    Center,
    Baseline,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Orientation {
    #[default]
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FlickDirection {
    #[default]
    Both,
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FlickBoundsBehavior {
    #[default]
    StopAtBounds,
    DragOverBounds,
    OvershootBounds,
    DragAndOvershootBounds,
}

impl FlickBoundsBehavior {
    pub fn allows_overshoot(self, flick: bool) -> bool {
        matches!(
            (self, flick),
            (Self::DragAndOvershootBounds, _)
                | (Self::DragOverBounds, false)
                | (Self::OvershootBounds, true)
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FlickBoundsMovement {
    #[default]
    FollowBoundsBehavior,
    StopAtBounds,
}

impl FlickDirection {
    pub fn allows(self, axis: Orientation) -> bool {
        matches!(
            (self, axis),
            (Self::Both, _)
                | (Self::Horizontal, Orientation::Horizontal)
                | (Self::Vertical, Orientation::Vertical)
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LayoutDirection {
    #[default]
    LeftToRight,
    RightToLeft,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FlexboxLayoutDirection {
    #[default]
    Row,
    RowReverse,
    Column,
    ColumnReverse,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FlexboxLayoutWrap {
    #[default]
    Wrap,
    NoWrap,
    WrapReverse,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DrawerEdge {
    #[default]
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScrollBarPolicy {
    #[default]
    AsNeeded,
    AlwaysOn,
    AlwaysOff,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextWrap {
    #[default]
    WordWrap,
    NoWrap,
    CharWrap,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MouseSelectionMode {
    #[default]
    Characters,
    Words,
    Caret,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextHorizontalAlignment {
    #[default]
    Start,
    End,
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextVerticalAlignment {
    #[default]
    Top,
    Center,
    Bottom,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextOverflow {
    #[default]
    Clip,
    Elide,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CheckState {
    #[default]
    Unchecked,
    PartiallyChecked,
    Checked,
}

impl From<bool> for CheckState {
    fn from(checked: bool) -> Self {
        if checked {
            Self::Checked
        } else {
            Self::Unchecked
        }
    }
}

impl CheckState {
    pub fn next(self, tristate: bool) -> Self {
        match self {
            Self::Checked => Self::Unchecked,
            Self::Unchecked if tristate => Self::PartiallyChecked,
            _ => Self::Checked,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradientStop {
    pub color: Color,
    /// Position along the gradient in the range 0.0 to 1.0.
    pub offset: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Brush {
    Solid(Color),
    Linear {
        angle_degrees: f64,
        stops: Arc<[GradientStop]>,
    },
    Radial {
        stops: Arc<[GradientStop]>,
    },
}

impl Default for Brush {
    fn default() -> Self {
        Self::Solid(Color::default())
    }
}

/// Priority of accessibility announcements for changes to a text label.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AccessibleLive {
    #[default]
    Off,
    Polite,
    Assertive,
}

/// A string fragment consumed by generated concatenation code.
#[doc(hidden)]
pub enum TextFragment<'a> {
    Static(&'a str),
    Owned(String),
    Shared(Rc<String>),
}

impl AsRef<str> for TextFragment<'_> {
    fn as_ref(&self) -> &str {
        match self {
            Self::Static(value) => value,
            Self::Owned(value) => value,
            Self::Shared(value) => value.as_str(),
        }
    }
}

/// Join already evaluated fragments using a single allocation for the result.
#[doc(hidden)]
pub fn concat_text<T: AsRef<str>>(parts: impl AsRef<[T]>) -> String {
    let parts = parts.as_ref();
    let mut result = String::with_capacity(parts.iter().map(|part| part.as_ref().len()).sum());
    for part in parts {
        result.push_str(part.as_ref());
    }
    result
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    EventResult(EventResult),
    FocusReason(FocusReason),
    KeyEvent(KeyEvent),
    WheelEvent(WheelEvent),
    PointerEvent(PointerEvent),
    TouchPoint(TouchPoint),
    TouchPoints(TouchPoints),
    KeyModifiers(KeyModifiers),
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    StyledText(StyledText),
    IntList(Vec<i64>),
    /// A live integer list whose bindings share the same row identities.
    IntModel(VecModel<i64>),
    DateList(Vec<Date>),
    DateModel(VecModel<Date>),
    StringList(Vec<String>),
    /// A live string list whose row identities are shared by its bindings.
    StringModel(VecModel<String>),
    Record(RecordValue),
    RecordList(RecordList),
    ComboModel(ComboModel),
    /// Prepared object data for a SceneView3D node.
    #[doc(hidden)]
    SceneObjects(Arc<scene3d::SceneModel>),
    #[doc(hidden)]
    SceneLights(Arc<scene3d::SceneLightModel>),
    SceneEnvironment(Option<Arc<scene3d::EnvironmentProbe>>),
    SceneReflections(Arc<[scene3d::ReflectionProbe]>),
    #[doc(hidden)]
    PhysicsDebugLines(PhysicsDebugLines),
    SceneFogVolumes(Arc<[scene3d::FogVolume3D]>),
    #[doc(hidden)]
    SceneEffects(Arc<scene3d::effect::Chain>),
    SceneCaptures(Arc<scene3d::effect::Captures>),
    #[doc(hidden)]
    SceneParticleEmitters(Arc<[scene3d::ParticleEmitter]>),
    #[doc(hidden)]
    SceneParticleInstanceModels(Arc<[scene3d::ParticleInstanceModel]>),
    #[doc(hidden)]
    SceneParticleAffectors(Arc<[scene3d::ParticleAffector]>),
    Length(Length),
    Point2D(Point2D),
    Point3D(Point3D),
    Vector3D(Vector3D),
    Quaternion3D(Quaternion3D),
    RotationTrack3D(RotationTrack3D),
    Hit3D(Hit3D),
    Hit3DList(VecModel<Hit3D>),
    PhysicsContact3D(PhysicsContact3D),
    PhysicsContact3DList(VecModel<PhysicsContact3D>),
    PhysicsHit3D(PhysicsHit3D),
    PhysicsHit3DList(VecModel<PhysicsHit3D>),
    ProjectedPoint3D(ProjectedPoint3D),
    RenderStats3D(RenderStats3D),
    Instance3D(Instance3D),
    InstanceTable3D(InstanceTable3D),
    MeshAsset3D(MeshAsset3D),
    TextureData2D(TextureData2D),
    TextureData3D(TextureData3D),
    MeshNode3D(MeshNode3D),
    Bounds3D(Bounds3D),
    WalkResult3D(WalkResult3D),
    PhysicsWorld3D(PhysicsWorld3D),
    Vector2D(Vector2D),
    Transform2D(Transform2D),
    Percent(Percent),
    Color(Color),
    Brush(Brush),
    Image(Image),
    ImageAsset(ImageAsset),
    LogLevel(LogLevel),
    ImageFit(ImageFit),
    ImageTileMode(ImageTileMode),
    ImageHorizontalAlignment(ImageHorizontalAlignment),
    ImageVerticalAlignment(ImageVerticalAlignment),
    ImageRendering(ImageRendering),
    ButtonDisplay(ButtonDisplay),
    LayoutAlignment(LayoutAlignment),
    CrossAxisAlignment(CrossAxisAlignment),
    Orientation(Orientation),
    FlickDirection(FlickDirection),
    FlickBoundsBehavior(FlickBoundsBehavior),
    FlickBoundsMovement(FlickBoundsMovement),
    LayoutDirection(LayoutDirection),
    FlexboxLayoutDirection(FlexboxLayoutDirection),
    FlexboxLayoutWrap(FlexboxLayoutWrap),
    DialInputMode(DialInputMode),
    ScrollBarPolicy(ScrollBarPolicy),
    ScrollSnapMode(ScrollSnapMode),
    ScrollAlignment(ScrollAlignment),
    TextWrap(TextWrap),
    MouseSelectionMode(MouseSelectionMode),
    InputValidity(InputValidity),
    TextHorizontalAlignment(TextHorizontalAlignment),
    TextVerticalAlignment(TextVerticalAlignment),
    TextOverflow(TextOverflow),
    InputType(InputType),
    InputPurpose(InputPurpose),
    KeyboardCapitalization(KeyboardCapitalization),
    ReturnKey(ReturnKey),
    AccessibleLive(AccessibleLive),
    CursorShape(CursorShape),
    PointerEventKind(PointerEventKind),
    PointerButton(PointerButton),
    PointerButtons(PointerButtons),
    ColorScheme(ColorScheme),
    SceneToneMap(SceneToneMap),
    FileDialogMode(FileDialogMode),
    MessageLevel(MessageLevel),
    MessageButtons(MessageButtons),
    CheckState(CheckState),
    DialogRole(DialogRole),
    StandardButtonKind(StandardButtonKind),
    SwipeSide(SwipeSide),
    DragAction(DragAction),
    DrawerEdge(DrawerEdge),
    PopupPlacement(PopupPlacement),
    SelectionTarget(SelectionTarget),
    SelectionGesture(SelectionGesture),
    Date(Date),
    Time(Time),
    Font(Font),
}

impl Value {
    /// Compare scalar values when deciding whether to invalidate a property.
    #[doc(hidden)]
    pub fn same_property_value(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Float(left), Self::Float(right)) => same_float_bits(left, right),
            (Self::Length(left), Self::Length(right)) => same_float_bits(&left.0, &right.0),
            (Self::Percent(left), Self::Percent(right)) => same_float_bits(&left.0, &right.0),
            _ => self.eq_with_image_assets(other),
        }
    }

    /// Compare a host image and an image asset using their shared image semantics.
    #[doc(hidden)]
    pub fn eq_with_image_assets(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Image(embedded), Self::ImageAsset(asset))
            | (Self::ImageAsset(asset), Self::Image(embedded)) => {
                ImageAsset::from(embedded.clone()) == *asset
            }
            _ => self == other,
        }
    }
}

macro_rules! value_from {
    ($ty:ty, $variant:ident) => {
        impl From<$ty> for Value {
            fn from(value: $ty) -> Self {
                Self::$variant(value)
            }
        }
    };
}
value_from!(i64, Int);
value_from!(f64, Float);
value_from!(bool, Bool);
value_from!(String, String);
value_from!(StyledText, StyledText);
value_from!(Vec<String>, StringList);

impl From<VecModel<String>> for Value {
    fn from(model: VecModel<String>) -> Self {
        Self::StringList(model.into_snapshot())
    }
}
impl From<VecModel<i64>> for Value {
    fn from(model: VecModel<i64>) -> Self {
        Self::IntList(model.into_snapshot())
    }
}

impl From<VecModel<Date>> for Value {
    fn from(model: VecModel<Date>) -> Self {
        Self::DateList(model.into_snapshot())
    }
}

value_from!(Length, Length);
value_from!(Point2D, Point2D);
value_from!(Point3D, Point3D);
value_from!(Vector3D, Vector3D);
value_from!(Quaternion3D, Quaternion3D);
value_from!(RotationTrack3D, RotationTrack3D);
value_from!(Hit3D, Hit3D);
value_from!(VecModel<Hit3D>, Hit3DList);
value_from!(PhysicsContact3D, PhysicsContact3D);
value_from!(VecModel<PhysicsContact3D>, PhysicsContact3DList);
value_from!(PhysicsHit3D, PhysicsHit3D);
value_from!(VecModel<PhysicsHit3D>, PhysicsHit3DList);
value_from!(ProjectedPoint3D, ProjectedPoint3D);
value_from!(RenderStats3D, RenderStats3D);
value_from!(Instance3D, Instance3D);
value_from!(InstanceTable3D, InstanceTable3D);
value_from!(MeshAsset3D, MeshAsset3D);
value_from!(TextureData2D, TextureData2D);
value_from!(TextureData3D, TextureData3D);
value_from!(MeshNode3D, MeshNode3D);
value_from!(Bounds3D, Bounds3D);
value_from!(WalkResult3D, WalkResult3D);
value_from!(PhysicsWorld3D, PhysicsWorld3D);
value_from!(Arc<scene3d::SceneModel>, SceneObjects);
value_from!(Vector2D, Vector2D);
value_from!(Transform2D, Transform2D);
value_from!(Percent, Percent);
value_from!(Color, Color);
value_from!(Image, Image);
value_from!(LogLevel, LogLevel);
value_from!(ImageFit, ImageFit);
value_from!(ImageTileMode, ImageTileMode);
value_from!(ImageHorizontalAlignment, ImageHorizontalAlignment);
value_from!(ImageVerticalAlignment, ImageVerticalAlignment);
value_from!(ImageRendering, ImageRendering);
value_from!(ButtonDisplay, ButtonDisplay);
value_from!(LayoutAlignment, LayoutAlignment);
value_from!(CrossAxisAlignment, CrossAxisAlignment);
value_from!(Orientation, Orientation);
value_from!(FlickDirection, FlickDirection);
value_from!(FlickBoundsBehavior, FlickBoundsBehavior);
value_from!(FlickBoundsMovement, FlickBoundsMovement);
value_from!(LayoutDirection, LayoutDirection);
value_from!(FlexboxLayoutDirection, FlexboxLayoutDirection);
value_from!(FlexboxLayoutWrap, FlexboxLayoutWrap);
value_from!(DialInputMode, DialInputMode);
value_from!(ScrollBarPolicy, ScrollBarPolicy);
value_from!(ScrollSnapMode, ScrollSnapMode);
value_from!(ScrollAlignment, ScrollAlignment);
value_from!(TextWrap, TextWrap);
value_from!(MouseSelectionMode, MouseSelectionMode);
value_from!(InputValidity, InputValidity);
value_from!(TextHorizontalAlignment, TextHorizontalAlignment);
value_from!(TextVerticalAlignment, TextVerticalAlignment);
value_from!(TextOverflow, TextOverflow);
value_from!(InputType, InputType);
value_from!(InputPurpose, InputPurpose);
value_from!(KeyboardCapitalization, KeyboardCapitalization);
value_from!(ReturnKey, ReturnKey);
value_from!(AccessibleLive, AccessibleLive);
value_from!(CursorShape, CursorShape);
value_from!(PointerEventKind, PointerEventKind);
value_from!(PointerButton, PointerButton);
value_from!(PointerButtons, PointerButtons);
value_from!(ColorScheme, ColorScheme);
value_from!(SceneToneMap, SceneToneMap);
value_from!(FileDialogMode, FileDialogMode);
value_from!(MessageLevel, MessageLevel);
value_from!(MessageButtons, MessageButtons);
value_from!(CheckState, CheckState);
value_from!(DialogRole, DialogRole);
value_from!(StandardButtonKind, StandardButtonKind);
value_from!(SwipeSide, SwipeSide);
value_from!(DragAction, DragAction);
value_from!(DrawerEdge, DrawerEdge);
value_from!(PopupPlacement, PopupPlacement);
value_from!(SelectionTarget, SelectionTarget);
value_from!(SelectionGesture, SelectionGesture);
value_from!(Date, Date);
value_from!(Time, Time);
value_from!(Font, Font);

impl From<Brush> for Value {
    fn from(value: Brush) -> Self {
        match value {
            Brush::Solid(color) => Self::Color(color),
            gradient => Self::Brush(gradient),
        }
    }
}

/// A UI scene node with copy-on-write snapshot storage.
#[derive(Clone, Debug)]
pub struct Node(Rc<NodeStorage>);

#[derive(Clone, Debug)]
struct NodeStorage {
    data: NodeData,
    frozen: bool,
    scene_features: OnceCell<u16>,
    stacking_order: OnceCell<f64>,
    visibility: OnceCell<bool>,
    opacity: OnceCell<f32>,
    clipped_layout_group: OnceCell<bool>,
    height_independent_metrics: OnceCell<bool>,
}

/// A storage identity for caches that should not keep a scene node alive.
#[doc(hidden)]
pub struct SharedNodeIdentity(std::rc::Weak<NodeStorage>);

impl PartialEq for SharedNodeIdentity {
    fn eq(&self, other: &Self) -> bool {
        self.0.ptr_eq(&other.0)
    }
}

impl Eq for SharedNodeIdentity {}

impl std::hash::Hash for SharedNodeIdentity {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::hash::Hash::hash(&self.0.as_ptr(), state);
    }
}

impl SharedNodeIdentity {
    pub fn matches(&self, node: &Node) -> bool {
        std::ptr::eq(self.0.as_ptr(), Rc::as_ptr(&node.0))
    }
}

/// Structural features used by layout's scene-wide passes. Visibility and
/// interaction state are resolved by the pass itself.
#[doc(hidden)]
#[derive(Clone, Copy)]
pub enum SceneFeature {
    VirtualList = 1,
    ListPreparation = 2,
    FontFamilyChoice = 4,
    WindowOverlay = 8,
    AttachedScrollBar = 16,
    ScrollEndpoint = 32,
    SceneTextureSource = 64,
    SceneViewport = 128,
    ScenePreload = 256,
    SceneImageTexture = 512,
    WindowEffects = 2048,
    GeometryObservation = 4096,
    PageTransition = 8192,
    CustomItem = 16384,
    ScaleRotateGesture = 1024,
}

/// Scene data shared by clones of a node. Mutating a node detaches its data.
#[doc(hidden)]
#[derive(Clone, Debug, PartialEq)]
pub struct NodeData {
    pub kind: &'static str,
    #[doc(hidden)]
    pub application_window: bool,
    pub id: Option<&'static str>,
    owned_id: Option<Arc<str>>,
    pub properties: BTreeMap<&'static str, Value>,
    pub children: Vec<Node>,
    #[doc(hidden)]
    pub media_frame: Option<MediaFrame>,
    pub row_key: Option<RowKey>,
    #[doc(hidden)]
    pub row_scope: u64,
    paint_only: u8,
    static_paint_order: Option<ScenePlanSlice<usize>>,
    precompiled_path: Option<&'static StaticVectorPath>,
    loaded_path: Option<LoadedVectorPath>,
    static_grid: Option<ScenePlanSlice<GridCellSlot>>,
    metadata: Rc<NodeMetadata>,
}

/// Optional control and view state shared by copy-on-write scene snapshots.
#[doc(hidden)]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NodeMetadata {
    #[doc(hidden)]
    pub loader_mount: Option<LoaderMount>,
    #[doc(hidden)]
    pub loader_frame: Option<LoaderFrame>,
    #[doc(hidden)]
    pub external_component: Option<ExternalComponent>,
    #[doc(hidden)]
    pub fonts: Option<&'static EmbeddedFonts>,
    #[doc(hidden)]
    pub loaded_fonts: Option<LoadedFonts>,
    pub tumbler_items: Option<TumblerItems>,
    #[doc(hidden)]
    pub table_column_window: TableColumnWindow,
    #[doc(hidden)]
    pub timers: Vec<TimerRegistration>,
    #[doc(hidden)]
    pub image_playback: Option<Rc<ImagePlaybackRegistration>>,
    #[doc(hidden)]
    pub media: Option<MediaRegistration>,
    #[doc(hidden)]
    pub sound_effects: Vec<MediaRegistration>,
    #[doc(hidden)]
    pub native_dialogs: Vec<NativeDialogRegistration>,
    pub animations: BTreeMap<&'static str, Animation>,
    #[doc(hidden)]
    pub state_timings: Option<Box<BTreeMap<&'static str, StateTiming>>>,
    #[doc(hidden)]
    pub focus_handle: Option<FocusHandle>,
    #[doc(hidden)]
    pub button_press_handle: Option<ButtonPressHandle>,
    #[doc(hidden)]
    pub range_position_handle: Option<RangePositionHandle>,
    #[doc(hidden)]
    pub combo_popup_handle: Option<ComboPopupHandle>,
    #[doc(hidden)]
    pub scale_rotate_handle: Option<ScaleRotateHandle>,
    #[doc(hidden)]
    pub swipe_handle: Option<SwipeHandle>,
    #[doc(hidden)]
    pub swipe_preview: Option<Box<SwipePreview>>,
    #[doc(hidden)]
    pub hover_handle: Option<HoverHandle>,
    #[doc(hidden)]
    pub pointer_handle: Option<PointerHandle>,
    #[doc(hidden)]
    pub scroll_event_handler: bool,
    #[doc(hidden)]
    pub press_and_hold_handler: bool,
    #[doc(hidden)]
    pub viewport_handle: Option<ViewportHandle>,
    #[doc(hidden)]
    pub scroll_target: Option<ViewportHandle>,
    #[doc(hidden)]
    pub geometry_handle: Option<GeometryHandle>,
    #[doc(hidden)]
    pub instance_geometry_handle: Option<GeometryHandle>,
    #[doc(hidden)]
    pub initial_focus: Option<FocusHandle>,
    #[doc(hidden)]
    pub key_handlers: u8,
    pub list: Option<ListMetrics>,
    #[doc(hidden)]
    pub row_locator: Option<ListRowLocator>,
    #[doc(hidden)]
    pub list_row_renderer: Option<ListRowRenderer>,
    #[doc(hidden)]
    pub row_heights: Option<Arc<RowHeights>>,
    #[doc(hidden)]
    pub list_sections: Option<Arc<[ListSection]>>,
    #[doc(hidden)]
    pub list_section_renderer: Option<ListSectionRenderer>,
    #[doc(hidden)]
    pub combo_option_renderer: Option<ComboOptionRenderer>,
    #[doc(hidden)]
    pub list_text_search: Option<ListTextSearch>,
    /// Selection snapshot for model-backed views. Retains the generated field name.
    #[doc(hidden)]
    pub standard_list_stamp: Option<(u64, u64)>,
    pub table_header: Option<Box<TableHeader>>,
    #[doc(hidden)]
    pub table_column_order: TableColumnOrder,
    pub table_selection: Option<Box<TableSelectionSnapshot>>,
    #[doc(hidden)]
    pub table_selection_drag: Option<TableSelectionDragHandle>,
    table_selection_parts: Option<Rc<[Option<Node>; 2]>>,
    #[doc(hidden)]
    pub table_cell_editor: Option<TableCellEditor>,
    pub tree: Option<Arc<TreeSnapshot>>,
    pub stack: Option<Arc<StackSnapshot>>,
    pub action: Option<ActionKey>,
    #[doc(hidden)]
    pub button_group: Option<ButtonGroupMembership>,
    pub(crate) radio_option_blocks: Option<Rc<Vec<control_semantics::RadioOptionBlock>>>,
    /// Selected TreeView business keys and model row identities, including folded descendants.
    pub tree_selection: Option<Arc<BTreeMap<i64, RowKey>>>,
    pub split_weights: Option<Arc<[Option<f32>]>>,
    #[doc(hidden)]
    pub split_fill_pane: Option<&'static str>,
    #[doc(hidden)]
    pub split_fill_index: Option<usize>,
    #[doc(hidden)]
    pub split_sizes: Option<Arc<[Option<f32>]>>,
    #[doc(hidden)]
    pub split_resize_handle: Option<SplitResizeHandle>,
}

thread_local! {
    static DEFAULT_NODE_METADATA: Rc<NodeMetadata> = Rc::new(NodeMetadata::default());
}

impl std::ops::Deref for NodeData {
    type Target = NodeMetadata;

    fn deref(&self) -> &Self::Target {
        &self.metadata
    }
}

impl std::ops::DerefMut for NodeData {
    fn deref_mut(&mut self) -> &mut Self::Target {
        Rc::make_mut(&mut self.metadata)
    }
}

impl std::ops::Deref for Node {
    type Target = NodeData;

    fn deref(&self) -> &Self::Target {
        &self.0.data
    }
}

impl std::ops::DerefMut for Node {
    fn deref_mut(&mut self) -> &mut Self::Target {
        let data = self.data_mut();
        // Public edits can change grid tracks, visibility or child order.
        data.static_grid = None;
        data.static_paint_order = None;
        data
    }
}

impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl NodeData {
    fn same_structure_and_state(&self, other: &Self) -> bool {
        // Keep this exhaustive so new scene fields opt in to comparison.
        let Self {
            metadata: _,
            application_window: _,
            children: _,
            id: _,
            kind: _,
            loaded_path: _,
            media_frame: _,
            owned_id: _,
            paint_only: _,
            precompiled_path: _,
            properties: _,
            row_key: _,
            row_scope: _,
            static_grid: _,
            static_paint_order: _,
        } = self;
        self.kind == other.kind
            && self.application_window == other.application_window
            && self.id == other.id
            && self.owned_id == other.owned_id
            && self.row_key == other.row_key
            && self.row_scope == other.row_scope
            && self.precompiled_path == other.precompiled_path
            && self.loaded_path == other.loaded_path
            && self.static_grid == other.static_grid
            && self.static_paint_order == other.static_paint_order
            && (Rc::ptr_eq(&self.metadata, &other.metadata) || self.metadata == other.metadata)
    }
}

pub fn is_text_input(kind: &str) -> bool {
    matches!(kind, "TextInput" | "LineEdit" | "SearchField") || is_multiline_text_input(kind)
}

pub fn editable_table_cell(node: &Node) -> bool {
    matches!(node.kind, "TableCell" | "TreeCell")
        && node.boolean("editable", false)
        && !node.boolean("read-only", false)
}

pub fn is_multiline_text_input(kind: &str) -> bool {
    matches!(kind, "TextEdit" | "TextArea")
}

pub fn is_toggle(kind: &str) -> bool {
    matches!(kind, "CheckBox" | "Switch")
}

/// Whether a handler can be invoked through dispatch without an event payload.
#[doc(hidden)]
pub fn supports_untyped_dispatch(kind: &str, event: &str) -> bool {
    if kind == "CustomItem" {
        return matches!(
            event,
            "clicked" | "moved" | "double-clicked" | "press-and-hold"
        );
    }
    if matches!(event, "scrolled" | "flicked" | "refresh-requested")
        || (kind == "SwipeDelegate" && event == "clicked")
    {
        return true;
    }
    !matches!(
        event,
        "check-state-changed"
            | "close-requested"
            | "editing-finished"
            | "pointer-event"
            | "scroll-event"
    ) && !is_multiline_text_input(kind)
        && !matches!(
            kind,
            "Slider"
                | "WheelHandler"
                | "Dial"
                | "ScrollBar"
                | "RangeSlider"
                | "SpinBox"
                | "DoubleSpinBox"
                | "SearchField"
                | "ComboBox"
                | "RadioGroup"
                | "ButtonGroup"
                | "TabWidget"
                | "TabBar"
                | "SwipeView"
                | "SwipeDelegate"
                | "StackView"
                | "StackPage"
                | "PageIndicator"
                | "Tumbler"
                | "PathView"
                | "Calendar"
                | "MonthGrid"
                | "TimePicker"
                | "TimePickerPopup"
                | "DatePickerPopup"
                | "ColorPickerPopup"
                | "FontPickerPopup"
                | "ListView"
                | "GridView"
                | "StandardListView"
                | "TableView"
                | "StandardTableView"
                | "SplitView"
                | "TreeView"
                | "Dialog"
                | "Drawer"
                | "PopupWindow"
                | "MenuItem"
                | "Shortcut"
                | "DelayButton"
                | "FocusScope"
                | "StyledText"
                | "AnimatedImage"
                | "DragArea"
                | "DropArea"
                | "GroupBox"
        )
}

/// Window placement and rendering preference for a 3D viewport.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SceneViewRenderMode {
    #[default]
    Offscreen,
    Inline,
    Underlay,
    Overlay,
}

impl SceneViewRenderMode {
    pub const fn window_layer(self) -> i8 {
        match self {
            Self::Underlay => -1,
            Self::Offscreen | Self::Inline => 0,
            Self::Overlay => 1,
        }
    }
}

impl Node {
    #[doc(hidden)]
    pub fn table_edit_history_enabled(&self) -> bool {
        self.boolean("undoable", false) || self.boolean("pasteable", false)
    }

    #[doc(hidden)]
    pub fn table_edit_history_model(&self, path: &[usize]) -> Option<u64> {
        let table = self.interactive_at_path(path)?;
        if !table.table_edit_history_enabled() {
            return None;
        }
        table.standard_list_stamp.map(|(model, _)| model)
    }

    pub fn calendar_cells(&self) -> Option<&[Node]> {
        self.children
            .last()
            .filter(|child| child.kind == "CalendarCells")
            .map(|child| child.children.as_slice())
    }

    pub fn is_item_delegate(&self) -> bool {
        self.boolean("__item-delegate", false)
    }

    #[doc(hidden)]
    pub fn enables_children(&self) -> bool {
        !self.input_requires_enabled(false) || self.boolean("enabled", true)
    }

    #[doc(hidden)]
    pub fn input_requires_enabled(&self, is_target: bool) -> bool {
        is_target
            || !(is_drag_area(self.kind)
                || matches!(
                    self.kind,
                    "WindowMoveArea"
                        | "ScaleRotateGestureHandler"
                        | "SwipeGestureHandler"
                        | "WheelHandler"
                        | "HoverHandler"
                        | "TouchArea"
                ))
    }
    pub fn check_state(&self) -> CheckState {
        match self.properties.get("check-state") {
            Some(Value::CheckState(state)) if self.kind == "CheckBox" => *state,
            _ => self.boolean("checked", false).into(),
        }
    }

    pub fn date(&self, name: &str) -> Option<Date> {
        match self.properties.get(name) {
            Some(Value::Date(date)) => Some(*date),
            _ => None,
        }
    }

    pub fn time(&self, name: &str) -> Option<Time> {
        match self.properties.get(name) {
            Some(Value::Time(time)) => Some(*time),
            _ => None,
        }
    }

    pub fn new(kind: &'static str, id: Option<&'static str>) -> Self {
        let data = NodeData {
            kind,
            application_window: false,
            id,
            owned_id: None,
            properties: BTreeMap::new(),
            children: Vec::new(),
            media_frame: None,
            row_key: None,
            row_scope: 0,
            paint_only: 0,
            static_paint_order: None,
            precompiled_path: None,
            loaded_path: None,
            static_grid: None,
            metadata: DEFAULT_NODE_METADATA
                .try_with(Clone::clone)
                .unwrap_or_default(),
        };
        Self(Rc::new(NodeStorage {
            data,
            frozen: false,
            scene_features: OnceCell::new(),
            stacking_order: OnceCell::new(),
            visibility: OnceCell::new(),
            opacity: OnceCell::new(),
            clipped_layout_group: OnceCell::new(),
            height_independent_metrics: OnceCell::new(),
        }))
    }

    /// Mark a subtree for scene caches that retain explicitly frozen nodes.
    #[doc(hidden)]
    pub fn into_shared(mut self) -> Self {
        self.freeze();
        self
    }

    fn freeze(&mut self) {
        if !self.0.frozen {
            let storage = Rc::make_mut(&mut self.0);
            for child in &mut storage.data.children {
                child.freeze();
            }
            storage.frozen = true;
        }
    }

    #[inline]
    pub fn shares_storage_with(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    /// Whether this node is the sole owner of its copy-on-write storage.
    #[doc(hidden)]
    pub fn owns_storage(&self) -> bool {
        Rc::strong_count(&self.0) == 1
    }

    /// Whether the subtree was explicitly frozen for scene caches.
    #[doc(hidden)]
    pub fn has_shared_storage(&self) -> bool {
        self.0.frozen
    }

    #[doc(hidden)]
    pub fn shared_identity(&self) -> Option<SharedNodeIdentity> {
        self.0.frozen.then(|| self.snapshot_identity())
    }

    /// Track this copy-on-write snapshot without keeping its data alive.
    #[doc(hidden)]
    pub fn snapshot_identity(&self) -> SharedNodeIdentity {
        SharedNodeIdentity(Rc::downgrade(&self.0))
    }

    /// Whether the intrinsic metrics of this snapshot are independent of its
    /// enclosing height. Layout can use a previously measured height for
    /// these nodes when the width constraints match.
    #[doc(hidden)]
    pub fn intrinsic_height_independent(&self) -> bool {
        *self.0.height_independent_metrics.get_or_init(|| {
            if self.properties.iter().any(|(&name, value)| {
                matches!(value, Value::Percent(_))
                    && !matches!(
                        name,
                        "width" | "min-width" | "preferred-width" | "max-width" | "x"
                    )
            }) {
                return false;
            }
            // These measurement branches don't consult child sizes or the
            // enclosing height. Containers propagate height to their children.
            if matches!(
                self.kind,
                "Rectangle"
                    | "Path"
                    | "TouchArea"
                    | "Label"
                    | "Text"
                    | "StyledText"
                    | "Image"
                    | "AnimatedImage"
                    | "Empty"
            ) || (self.length("width").is_some() && self.length("height").is_some())
            {
                return true;
            }
            matches!(
                self.kind,
                "HorizontalLayout" | "VerticalLayout" | "Pane" | "Button" | "ButtonContent"
            ) && self.children.iter().all(Self::intrinsic_height_independent)
        })
    }

    #[doc(hidden)]
    pub fn contains_scene_feature(&self, feature: SceneFeature) -> bool {
        self.scene_features() & feature as u16 != 0
    }

    /// Dynamic branches can be cheap to visit directly. Avoid
    /// scanning them twice when an existing traversal is already descending.
    #[doc(hidden)]
    pub fn shared_subtree_excludes(&self, feature: SceneFeature) -> bool {
        self.has_shared_storage() && !self.contains_scene_feature(feature)
    }

    fn scene_features(&self) -> u16 {
        // A clone retains the same snapshot even for non-frozen nodes.
        // data_mut clears these bits on each edited path.
        *self
            .0
            .scene_features
            .get_or_init(|| self.collect_scene_features())
    }

    fn collect_scene_features(&self) -> u16 {
        use SceneFeature::*;

        let mut features = 0;
        if self.scene_texture_source().is_some() {
            features |= SceneTextureSource as u16;
        }
        if self.image_texture_input().is_some() {
            features |= SceneImageTexture as u16;
        }
        if self.scale_rotate_handle.is_some() {
            features |= ScaleRotateGesture as u16;
        }
        if self.kind == "SceneView3D" {
            features |= SceneViewport as u16;
        }
        if self.kind == "CustomItem" {
            features |= CustomItem as u16;
        }
        if self.properties.contains_key("preload-model") {
            features |= ScenePreload as u16;
        }
        if self.list.is_some() {
            features |= VirtualList as u16 | ListPreparation as u16;
        }
        if let Some((_, page)) = self
            .swipe_preview
            .as_ref()
            .and_then(|preview| preview.page.as_ref())
        {
            // Only virtual-list preparation visits the separate swipe scene.
            features |= ListPreparation as u16 | (page.scene_features() & VirtualList as u16);
        }
        if is_font_family_choice(self) {
            features |= FontFamilyChoice as u16;
        }
        if is_window_overlay(self.kind) {
            features |= WindowOverlay as u16;
        }
        if self.scroll_target.is_some() {
            features |= AttachedScrollBar as u16 | ScrollEndpoint as u16;
        }
        if self.viewport_handle.is_some() {
            features |= ScrollEndpoint as u16;
        }
        if self.button_press_handle.is_some()
            || !self.timers.is_empty()
            || !self.native_dialogs.is_empty()
            || self.image_playback.is_some()
            || self.media.is_some()
            || !self.sound_effects.is_empty()
        {
            features |= WindowEffects as u16;
        }
        if self.viewport_handle.is_some()
            || self.range_position_handle.is_some()
            || self.geometry_handle.is_some()
            || self.instance_geometry_handle.is_some()
        {
            features |= GeometryObservation as u16;
        }
        if matches!(self.kind, "StackView" | "SwipeView") {
            features |= PageTransition as u16;
        }
        self.children.iter().fold(features, |features, child| {
            features | child.scene_features()
        })
    }

    #[doc(hidden)]
    pub fn set_precompiled_path(&mut self, path: &'static StaticVectorPath) {
        self.precompiled_path = Some(path);
    }

    #[doc(hidden)]
    pub fn precompiled_path(&self, commands: &str) -> Option<&'static StaticVectorPath> {
        self.precompiled_path
            .filter(|path| path.commands == commands)
    }
    #[doc(hidden)]
    pub fn set_loaded_vector_path(&mut self, path: LoadedVectorPath) {
        self.loaded_path = Some(path);
    }

    #[doc(hidden)]
    pub fn vector_path_geometry(&self, commands: &str) -> Option<([f32; 4], &[VectorSegment])> {
        self.precompiled_path(commands)
            .map(|path| (path.bounds, path.segments))
            .or_else(|| self.loaded_path.as_ref()?.geometry(commands))
    }

    /// Mark a generated Rectangle's background as independent of layout and input.

    fn data_mut(&mut self) -> &mut NodeData {
        let node = Rc::make_mut(&mut self.0);
        node.scene_features.take();
        node.stacking_order.take();
        node.visibility.take();
        node.opacity.take();
        node.clipped_layout_group.take();
        node.height_independent_metrics.take();
        &mut node.data
    }

    #[doc(hidden)]
    pub fn set_static_grid(&mut self, cells: &'static [GridCellSlot]) {
        self.data_mut().static_grid = Some(ScenePlanSlice::Static(cells));
    }

    #[doc(hidden)]
    pub fn set_shared_grid(&mut self, cells: Arc<[GridCellSlot]>) {
        self.data_mut().static_grid = Some(ScenePlanSlice::Shared(cells));
    }

    #[doc(hidden)]
    pub fn static_grid(&self) -> Option<&[GridCellSlot]> {
        self.static_grid.as_deref()
    }

    #[doc(hidden)]
    pub fn set_static_paint_order(&mut self, order: &'static [usize]) {
        self.data_mut().static_paint_order =
            (!order.is_empty()).then_some(ScenePlanSlice::Static(order));
    }

    #[doc(hidden)]
    pub fn set_shared_paint_order(&mut self, order: Arc<[usize]>) {
        self.data_mut().static_paint_order = Some(ScenePlanSlice::Shared(order));
    }

    #[doc(hidden)]
    pub fn set_natural_paint_order(&mut self) {
        self.data_mut().static_paint_order = Some(ScenePlanSlice::Static(&[]));
    }

    #[doc(hidden)]
    pub fn has_natural_paint_order(&self) -> bool {
        self.static_paint_order
            .as_deref()
            .is_some_and(<[usize]>::is_empty)
    }

    #[doc(hidden)]
    pub fn static_paint_order(&self) -> Option<&[usize]> {
        self.paint_order_plan().map(std::ops::Deref::deref)
    }

    #[doc(hidden)]
    pub fn paint_order_plan(&self) -> Option<&ScenePlanSlice<usize>> {
        self.static_paint_order
            .as_ref()
            .filter(|order| !order.is_empty())
    }

    #[doc(hidden)]
    pub fn set_loaded_fonts(&mut self, fonts: Option<LoadedFonts>) {
        self.data_mut().loaded_fonts = fonts;
    }

    #[doc(hidden)]
    pub fn set_embedded_fonts(&mut self, fonts: &'static EmbeddedFonts) {
        self.data_mut().fonts = Some(fonts);
    }

    #[doc(hidden)]
    pub fn set_row_key(&mut self, key: Option<RowKey>) {
        if self.row_key != key {
            self.data_mut().row_key = key;
        }
    }

    #[doc(hidden)]
    pub fn observe_custom_item_updates(&mut self) {
        if let Some(handle) = &self.geometry_handle {
            let [paint, layout] = handle.custom_item_revisions();
            self.properties
                .insert("__custom-paint-revision", Value::Int(paint));
            self.properties
                .insert("__custom-layout-revision", Value::Int(layout));
        }
    }

    fn paint_only_bit(&self, property: &str) -> u8 {
        match (self.kind, property) {
            ("Rectangle", "background") => 1,
            ("Text" | "Label" | "StyledText", "color") => 2,
            ("Text" | "Label", "text") => 4,
            ("Image", "source") => 8,
            ("StyledText", "link-color") => 64,
            (
                "SceneView3D",
                "model"
                | "preload-model"
                | "lod-fade-width"
                | "render-mode"
                | "render-format"
                | "render-overrides"
                | "texture-source"
                | "texture-mipmaps"
                | "lights"
                | "ambient-color"
                | "ambient-intensity"
                | "light-yaw"
                | "light-elevation"
                | "scene-background"
                | "scene-grid"
                | "scene-grid-axes"
                | "scene-grid-color"
                | "scene-grid-height"
                | "scene-grid-spacing"
                | "render-stats-revision"
                | "shadows"
                | "imported-lights"
                | "shadow-map-size"
                | "shadow-map-precision"
                | "scissor-rect"
                | "depth-prepass"
                | "debug-wireframe"
                | "shadow-debug-cascades"
                | "shadow-debug-light-volumes"
                | "shadow-debug-casters"
                | "shadow-debug-receivers"
                | "shadow-debug-intersections"
                | "shadow-freeze-camera"
                | "oit-method"
                | "material-debug"
                | "shadow-bias"
                | "shadow-cascades"
                | "shadow-distance"
                | "shadow-cascade-blend"
                | "shadow-split-balance"
                | "camera"
                | "camera-yaw"
                | "camera-pitch"
                | "camera-distance"
                | "camera-fit"
                | "camera-fit-margin"
                | "selected-object"
                | "environment"
                | "environment-sky"
                | "skybox-source"
                | "skybox-sky"
                | "skybox-yaw"
                | "skybox-pitch"
                | "skybox-roll"
                | "skybox-intensity"
                | "skybox-blur"
                | "environment-intensity"
                | "environment-yaw"
                | "environment-pitch"
                | "environment-roll"
                | "environment-horizon"
                | "environment-skybox"
                | "frustum-culling"
                | "multisampling"
                | "supersampling"
                | "progressive-aa"
                | "reflection-faces-per-frame"
                | "reflection-filter-passes-per-frame"
                | "reflection-filter-texels-per-frame"
                | "bake-denoise-passes"
                | "bake-indirect-factor"
                | "temporal-aa"
                | "temporal-aa-mode"
                | "temporal-aa-strength"
                | "screen-space-reflections"
                | "ssr-strength"
                | "ssr-distance"
                | "ssr-thickness"
                | "ssr-steps"
                | "ssr-roughness-cutoff"
                | "ssr-scale"
                | "screen-space-global-illumination"
                | "ssgi-strength"
                | "ssgi-occlusion"
                | "ssgi-radius"
                | "ssgi-thickness"
                | "ssgi-scale"
                | "ssgi-rays"
                | "ssgi-steps"
                | "specular-aa"
                | "order-independent-transparency"
                | "ambient-occlusion"
                | "scene-hdr"
                | "bloom"
                | "bloom-strength"
                | "bloom-threshold"
                | "bloom-levels"
                | "depth-of-field"
                | "dof-blur-radius"
                | "dof-focus-distance"
                | "dof-focus-range"
                | "ao-strength"
                | "ao-distance"
                | "ao-bias"
                | "particle-emitters"
                | "particle-affectors"
                | "particle-time"
                | "fog-volumes"
                | "fog-volume-scattering"
                | "fog-volume-anisotropy"
                | "fog-volume-samples"
                | "fog-volume-density-samples"
                | "fog-color"
                | "fog-near"
                | "fog-far"
                | "fog-density"
                | "fog-depth-enabled"
                | "fog-height-enabled"
                | "fog-transmit-enabled"
                | "fog-depth-curve"
                | "fog-height-start"
                | "fog-height-end"
                | "fog-height-curve"
                | "fog-transmit-curve"
                | "scene-exposure"
                | "scene-tone-map"
                | "scene-white-point"
                | "scene-brightness"
                | "scene-contrast"
                | "scene-saturation"
                | "scene-lut"
                | "scene-lut-size"
                | "scene-lut-strength"
                | "scene-vignette-strength"
                | "scene-vignette-radius"
                | "scene-vignette-color"
                | "scene-sharpness"
                | "scene-dithering"
                | "fxaa"
                | "fxaa-edge-threshold"
                | "fxaa-min-threshold"
                | "fxaa-subpixel"
                | "fxaa-strength"
                | "fxaa-search-steps"
                | "lens-flare"
                | "lens-flare-strength"
                | "lens-flare-threshold"
                | "lens-flare-ghost-count"
                | "lens-flare-ghost-dispersal"
                | "lens-flare-halo-width"
                | "lens-flare-halo-strength"
                | "lens-flare-distortion"
                | "lens-flare-rotation"
                | "lens-flare-aspect-corrected"
                | "lens-flare-blur-levels"
                | "lens-flare-color-map"
                | "lens-flare-dirt-map"
                | "lens-flare-starburst-map"
                | "scene-effect"
                | "scene-effect-strength"
                | "scene-effect-depth"
                | "scene-effect-history-revision"
                | "scene-effect-animate"
                | "scene-effects"
                | "scene-color-capture"
                | "scene-depth-capture",
            ) => 64,
            (
                _,
                "accessible-name"
                | "accessible-description"
                | "accessible-heading-level"
                | "accessible-live",
            ) => 128,
            ("Image", "texture-input") => 16,
            (kind, "flat") if is_button(kind) || kind == "StandardButton" => 16,
            ("Button" | "ToolButton" | "RoundButton" | "DelayButton", "primary") => 32,
            _ => 0,
        }
    }

    #[doc(hidden)]
    pub fn mark_paint_only(&mut self, property: &str) {
        let bit = self.paint_only_bit(property);
        self.data_mut().paint_only |= bit;
    }

    #[doc(hidden)]
    pub fn mark_paint_only_background(&mut self) {
        self.mark_paint_only("background");
    }

    /// Compare frame inputs, allowing paint-only values to differ.
    #[doc(hidden)]
    #[inline]
    pub fn same_frame_inputs(&self, other: &Self) -> bool {
        self.shares_storage_with(other) || self.compare_distinct_layout_inputs(other, false, false)
    }

    /// Compare a subtree after its parent has resolved the root's position.
    /// Child positions and all layout-affecting inputs remain significant.
    #[doc(hidden)]
    pub fn same_local_layout_inputs(&self, other: &Self) -> bool {
        self.shares_storage_with(other) || self.compare_distinct_layout_inputs(other, true, false)
    }

    #[doc(hidden)]
    pub fn same_local_layout_inputs_at_node(&self, other: &Self) -> bool {
        self.shares_storage_with(other) || self.same_own_layout_inputs(other, true, false, false)
    }

    #[doc(hidden)]
    pub fn same_measurement_inputs_at_node(&self, other: &Self) -> bool {
        self.shares_storage_with(other) || self.same_own_layout_inputs(other, false, true, false)
    }

    #[doc(hidden)]
    pub fn same_measurement_inputs_as_child(&self, other: &Self, parent: &Self) -> bool {
        self.shares_storage_with(other)
            || self.same_own_layout_inputs(other, false, true, parent.fixed_text_children())
    }

    fn compare_distinct_layout_inputs(
        &self,
        other: &Self,
        local: bool,
        fixed_text_child: bool,
    ) -> bool {
        if !self.same_own_layout_inputs(other, local, false, fixed_text_child)
            || self.children.len() != other.children.len()
        {
            return false;
        }
        if self.children.is_empty() {
            return true;
        }
        let fixed_text_children = self.fixed_text_children();
        let window = matches!(self.kind, "Window" | "ApplicationWindow");
        self.children
            .iter()
            .zip(&other.children)
            .all(|(child, other)| {
                child.shares_storage_with(other)
                    || ((!window
                        || child.scene_view_render_mode().window_layer()
                            == other.scene_view_render_mode().window_layer())
                        && child.compare_distinct_layout_inputs(other, false, fixed_text_children))
            })
    }

    fn fixed_text_children(&self) -> bool {
        // These parents measure children with the standard fixed-size rules.
        // Keep specialized controls on their existing invalidation paths.
        matches!(
            self.kind,
            "Window"
                | "ApplicationWindow"
                | "Rectangle"
                | "Pane"
                | "ControlFrame"
                | "HorizontalLayout"
                | "VerticalLayout"
                | "HorizontalBox"
                | "VerticalBox"
                | "GridLayout"
                | "FlexboxLayout"
                | "FlowLayout"
                | "Row"
        )
    }

    fn same_own_layout_inputs(
        &self,
        other: &Self,
        local: bool,
        ignore_transform: bool,
        fixed_text_child: bool,
    ) -> bool {
        let paint_only = self.paint_only & other.paint_only;
        let ignore_frame = self.kind == "AnimatedImage" && other.kind == "AnimatedImage";
        let is_image = matches!(self.kind, "Image" | "AnimatedImage") && self.kind == other.kind;
        // The source contributes intrinsic dimensions. Explicit sizes
        // override both axes, and the remaining layout properties are still
        // compared below, including any source clip.
        let fixed_box = |node: &Self| {
            node.relative_length("width", 0.0).is_some()
                && node.relative_length("height", 0.0).is_some()
        };
        let ignore_image_source = is_image
            && (fixed_box(self) && fixed_box(other)
                || self.image_size("source") == other.image_size("source"));
        let absolute_box =
            |node: &Self| node.length("width").is_some() && node.length("height").is_some();
        let fixed_plain_text = |node: &Self| {
            node.children.is_empty()
                && matches!(node.properties.get("text"), None | Some(Value::String(_)))
                && fixed_box(node)
        };
        let ignore_text = || {
            (paint_only & self.paint_only_bit("text") != 0
                && absolute_box(self)
                && absolute_box(other))
                || (fixed_text_child
                    && matches!(self.kind, "Text" | "Label")
                    && fixed_plain_text(self)
                    && fixed_plain_text(other))
        };
        let custom_paint_only = self.kind == "CustomItem"
            && other.kind == self.kind
            && self.text("implementation") == other.text("implementation")
            && ["data", "revision"]
                .into_iter()
                .any(|name| self.properties.get(name) != other.properties.get(name))
            && self.custom_item().is_some_and(|item| {
                item.update_effects(self.custom_item_data(), other.custom_item_data())
                    == CustomItemUpdate::Paint
            });
        let relevant = |name: &str| match name {
            "data" | "revision" if custom_paint_only => false,
            "__custom-paint-revision" if self.kind == "CustomItem" => false,
            "x" | "y" if local => false,
            name if ignore_transform && (name == "transform" || name.starts_with("transform-")) => {
                false
            }
            "source" if is_image => !ignore_image_source,
            "text" => !ignore_text(),
            "render-mode" if self.kind == "SceneView3D" => false,
            "texture-source"
                if self.boolean("texture-only", false) || other.boolean("texture-only", false) =>
            {
                true
            }
            name if paint_only != 0 && paint_only & self.paint_only_bit(name) != 0 => false,
            "opacity" => false,
            "selection-color" | "selected-text-color" => false,
            "drop-shadow-blur"
            | "drop-shadow-color"
            | "drop-shadow-offset-x"
            | "drop-shadow-offset-y"
            | "drop-shadow-spread"
            | "inner-shadow-blur"
            | "inner-shadow-color"
            | "inner-shadow-offset-x"
            | "inner-shadow-offset-y"
            | "inner-shadow-spread" => self.kind != "Rectangle",
            "current-frame" => !ignore_frame,
            "colorize" => !is_image,
            _ => true,
        };
        if !self.same_structure_and_state(other) {
            return false;
        }
        let mut previous = self.properties.iter();
        let mut current = other.properties.iter();
        loop {
            match (previous.next(), current.next()) {
                (Some((old_name, old_value)), Some((new_name, new_value)))
                    if old_name == new_name =>
                {
                    let changed = match (old_value, new_value) {
                        (Value::Length(Length(before)), Value::Length(Length(after)))
                        | (Value::Percent(Percent(before)), Value::Percent(Percent(after))) => {
                            // Move-only updates usually preserve the geometry of
                            // the cached subtree. Check these scalars before
                            // classifying their property names.
                            before != after && relevant(old_name)
                        }
                        _ => relevant(old_name) && old_value != new_value,
                    };
                    if changed {
                        return false;
                    }
                }
                (None, None) => return true,
                (old, new) => {
                    // The compared prefix is already equivalent. Properties
                    // can be absent on one side if layout ignores them.
                    let keep = |(name, _): &(&&str, &Value)| relevant(name);
                    return old
                        .into_iter()
                        .chain(previous)
                        .filter(keep)
                        .eq(new.into_iter().chain(current).filter(keep));
                }
            }
        }
    }

    /// Compare this node's paint inputs. Descendants are tracked separately.
    #[doc(hidden)]
    pub fn same_own_paint_inputs(&self, other: &Self) -> bool {
        self.shares_storage_with(other)
            || self.same_structure_and_state(other)
                && self.media_frame == other.media_frame
                && self.properties == other.properties
    }

    /// Compare this node's paint inputs after the renderer has resolved its position.
    #[doc(hidden)]
    pub fn same_own_paint_except_position(&self, other: &Self) -> bool {
        self.same_own_paint_where(other, |name| !matches!(name, "x" | "y"))
    }

    #[doc(hidden)]
    pub fn same_own_paint_except_background(&self, other: &Self) -> bool {
        self.same_own_paint_where(other, |name| name != "background")
    }

    fn same_own_paint_where(&self, other: &Self, keep: impl Fn(&str) -> bool) -> bool {
        let paint_property = |(name, _): &(&&str, &Value)| keep(name);
        self.shares_storage_with(other)
            || self.same_structure_and_state(other)
                && self.media_frame == other.media_frame
                && self
                    .properties
                    .iter()
                    .filter(paint_property)
                    .eq(other.properties.iter().filter(paint_property))
    }

    /// Compare a paint layer's content independently of its composited opacity.
    #[doc(hidden)]
    pub fn same_paint_except_opacity(&self, other: &Self) -> bool {
        self.same_layer_content(other, false)
    }

    /// The painter must separately verify that changes to the root position
    /// preserve the layout relative to the cached texture.
    #[doc(hidden)]
    pub fn same_paint_except_layer_composition(&self, other: &Self) -> bool {
        self.same_layer_content(other, true)
    }

    fn same_layer_content(&self, other: &Self, moving: bool) -> bool {
        let content_property = |(name, _): &(&&str, &Value)| {
            **name != "opacity" && !(moving && matches!(**name, "x" | "y"))
        };
        self.shares_storage_with(other)
            || self.same_structure_and_state(other)
                && self.media_frame == other.media_frame
                && self
                    .properties
                    .iter()
                    .filter(content_property)
                    .eq(other.properties.iter().filter(content_property))
                && self.children.len() == other.children.len()
                && self
                    .children
                    .iter()
                    .zip(&other.children)
                    .all(|(child, other)| child.shares_storage_with(other) || child == other)
    }
    /// An empty slot keeps the generated event paths stable across conditional renders.
    pub fn hidden() -> Self {
        let mut node = Self::new("Empty", None);
        node.properties.insert("visible", Value::Bool(false));
        node
    }
    /// The source ID of a named element, including elements loaded at runtime.
    pub fn element_id(&self) -> Option<&str> {
        self.id.or(self.owned_id.as_deref())
    }

    #[doc(hidden)]
    pub fn shared_element_id(&self) -> Option<Arc<str>> {
        self.owned_id.clone()
    }

    #[doc(hidden)]
    pub fn element_id_owned(&self) -> Option<std::borrow::Cow<'static, str>> {
        use std::borrow::Cow;
        self.id
            .map(Cow::Borrowed)
            .or_else(|| self.owned_id.as_ref().map(|id| Cow::Owned(id.to_string())))
    }

    #[doc(hidden)]
    pub fn set_owned_id(&mut self, id: Option<Arc<str>>) {
        let data = self.data_mut();
        data.id = None;
        data.owned_id = id;
    }

    #[doc(hidden)]
    pub(crate) fn set_external_component(&mut self, component: ExternalComponent) {
        self.data_mut().external_component = Some(component);
    }

    #[doc(hidden)]
    pub fn set_loader_frame(&mut self, frame: LoaderFrame) {
        self.data_mut().loader_frame = Some(frame);
    }

    #[doc(hidden)]
    pub fn set_loader_mount(&mut self, mount: LoaderMount) {
        self.data_mut().loader_mount = Some(mount);
    }

    #[doc(hidden)]
    pub fn set_static_id(&mut self, id: Option<&'static str>) {
        let data = self.data_mut();
        data.id = id;
        data.owned_id = None;
    }

    #[doc(hidden)]
    pub fn set_instance_geometry_handle(&mut self, handle: GeometryHandle) {
        self.data_mut().instance_geometry_handle = Some(handle);
    }

    /// Set the instance's placement in its parent without invalidating the
    /// plans for the component's own children.
    #[doc(hidden)]
    pub fn set_instance_layout_property(&mut self, name: &'static str, value: Value) {
        self.data_mut().properties.insert(name, value);
    }

    pub fn find(&self, id: &str) -> Option<&Node> {
        if self.element_id() == Some(id) {
            Some(self)
        } else {
            self.children.iter().find_map(|n| n.find(id))
        }
    }

    pub fn length(&self, name: &str) -> Option<f32> {
        match self.properties.get(name) {
            Some(Value::Length(Length(value))) if value.is_finite() => {
                Some(value.clamp(-1_000_000.0, 1_000_000.0) as f32)
            }
            _ => None,
        }
    }

    pub fn relative_length(&self, name: &str, reference: f32) -> Option<f32> {
        match self.properties.get(name) {
            Some(Value::Percent(Percent(factor))) if factor.is_finite() => {
                Some((factor * f64::from(reference)).clamp(-1_000_000.0, 1_000_000.0) as f32)
            }
            Some(Value::Length(Length(value))) if value.is_finite() => {
                Some(value.clamp(-1_000_000.0, 1_000_000.0) as f32)
            }
            _ => None,
        }
    }

    #[doc(hidden)]
    pub fn can_pull_to_refresh(&self) -> bool {
        self.boolean("pull-to-refresh", false) && !self.boolean("refreshing", false)
    }

    #[doc(hidden)]
    pub fn scene_texture_source(&self) -> Option<&str> {
        if matches!(self.kind, "Rectangle" | "SceneView3D") {
            self.text("texture-source").filter(|name| !name.is_empty())
        } else {
            None
        }
    }

    #[doc(hidden)]
    pub fn image_texture_input(&self) -> Option<&str> {
        if self.kind == "Image" {
            self.text("texture-input").filter(|name| !name.is_empty())
        } else {
            None
        }
    }

    /// Keep this named source in its texture instead of the 2D window.
    #[doc(hidden)]
    pub fn scene_texture_only(&self) -> bool {
        matches!(self.kind, "Rectangle" | "SceneView3D")
            && self.boolean("texture-only", false)
            && self.scene_texture_source().is_some()
    }

    pub fn boolean(&self, name: &str, default: bool) -> bool {
        match self.properties.get(name) {
            Some(Value::Bool(value)) => *value,
            _ => default,
        }
    }

    pub fn text(&self, name: &str) -> Option<&str> {
        match self.properties.get(name) {
            Some(Value::String(value)) => Some(value),
            Some(Value::StyledText(value)) => Some(value.text()),
            _ => None,
        }
    }

    pub fn font(&self, name: &str) -> Option<&Font> {
        match self.properties.get(name) {
            Some(Value::Font(font)) => Some(font),
            _ => None,
        }
    }

    pub fn font_size(&self) -> Option<f32> {
        self.length("font-size")
            .or_else(|| self.font("font").and_then(Font::finite_size))
    }

    #[doc(hidden)]
    pub fn selectable_styled_text(&self) -> bool {
        self.kind == "StyledText" && self.boolean("selectable", false)
    }

    #[doc(hidden)]
    pub fn text_read_only(&self) -> bool {
        self.kind == "StyledText" || self.boolean("read-only", false)
    }

    pub fn styled_text(&self, name: &str) -> Option<&StyledText> {
        match self.properties.get(name) {
            Some(Value::StyledText(value)) => Some(value),
            _ => None,
        }
    }

    /// Resolve a link only while the displayed document matches the live binding.
    #[doc(hidden)]
    pub fn try_prepare_text_link<E>(
        &self,
        index: usize,
        input: InputRoute<'_>,
        live: impl FnOnce() -> Result<StyledText, E>,
    ) -> Result<Option<&str>, E> {
        let Some(document) = self.styled_text("text") else {
            return Ok(None);
        };
        let Some(link) = document.links().get(index) else {
            return Ok(None);
        };
        input.try_prepare(
            || Ok(true),
            || Ok((live()? == *document).then_some(link.href.as_ref())),
        )
    }

    #[doc(hidden)]
    pub fn prepare_text_link(
        &self,
        index: usize,
        input: InputRoute<'_>,
        live: impl FnOnce() -> StyledText,
    ) -> Option<&str> {
        self.try_prepare_text_link(index, input, || Ok::<_, std::convert::Infallible>(live()))
            .unwrap_or_else(|error| match error {})
    }

    pub fn strings(&self, name: &str) -> &[String] {
        match self.properties.get(name) {
            Some(Value::StringList(values)) => values,
            _ => &[],
        }
    }

    pub fn float(&self, name: &str) -> Option<f32> {
        self.number(name)
            .map(|value| value.clamp(-1_000_000.0, 1_000_000.0) as f32)
    }

    pub fn number(&self, name: &str) -> Option<f64> {
        match self.properties.get(name) {
            Some(Value::Float(value)) if value.is_finite() => Some(*value),
            _ => None,
        }
    }

    /// Visual opacity has no effect on layout, focus or hit testing.
    pub fn opacity(&self) -> f32 {
        *self
            .0
            .opacity
            .get_or_init(|| self.number("opacity").unwrap_or(1.0).clamp(0.0, 1.0) as f32)
    }

    /// A layout group without a retained layer that needs tracking offscreen.
    #[doc(hidden)]
    pub fn can_skip_clipped_layout_group(&self) -> bool {
        *self.0.clipped_layout_group.get_or_init(|| {
            matches!(
                self.kind,
                "HorizontalLayout" | "VerticalLayout" | "GridLayout"
            ) && self.opacity() == 1.0
                && !self.boolean("cache-rendering-hint", false)
        })
    }

    /// Finite stacking order within an ordinary sibling list.
    pub fn z(&self) -> f64 {
        *self
            .0
            .stacking_order
            .get_or_init(|| self.number("z").filter(|&z| z != 0.0).unwrap_or_default())
    }

    #[doc(hidden)]
    pub fn scene_view_render_mode(&self) -> SceneViewRenderMode {
        if self.kind != "SceneView3D" {
            return SceneViewRenderMode::Offscreen;
        }
        match self.text("render-mode") {
            Some("inline") => SceneViewRenderMode::Inline,
            Some("underlay") => SceneViewRenderMode::Underlay,
            Some("overlay") => SceneViewRenderMode::Overlay,
            _ => SceneViewRenderMode::Offscreen,
        }
    }

    #[doc(hidden)]
    pub fn scene_internal_passes_disabled(&self) -> bool {
        self.kind == "SceneView3D"
            && self.text("render-overrides") == Some("disable-internal-passes")
    }

    pub fn layout_direction(&self) -> Option<LayoutDirection> {
        match self.properties.get("layout-direction") {
            Some(Value::LayoutDirection(direction)) => Some(*direction),
            _ => None,
        }
    }

    pub fn orientation(&self) -> Orientation {
        match self.properties.get("orientation") {
            Some(Value::Orientation(value)) => *value,
            _ if matches!(self.kind, "ScrollBar" | "ScrollIndicator" | "ToolSeparator") => {
                Orientation::Vertical
            }
            _ => Orientation::Horizontal,
        }
    }

    pub fn dial_input_mode(&self) -> DialInputMode {
        match self.properties.get("input-mode") {
            Some(Value::DialInputMode(value)) => *value,
            _ => DialInputMode::Circular,
        }
    }

    pub fn scroll_bar_policy(&self, orientation: Orientation) -> ScrollBarPolicy {
        let property = match orientation {
            Orientation::Horizontal => "horizontal-scroll-bar-policy",
            Orientation::Vertical => "vertical-scroll-bar-policy",
        };
        match self.properties.get(property) {
            Some(Value::ScrollBarPolicy(policy)) => *policy,
            _ => ScrollBarPolicy::AsNeeded,
        }
    }

    #[doc(hidden)]
    pub fn mouse_selection_mode(&self) -> MouseSelectionMode {
        match self.properties.get("mouse-selection-mode") {
            Some(Value::MouseSelectionMode(mode)) => *mode,
            _ => MouseSelectionMode::default(),
        }
    }

    pub fn text_wrap(&self) -> TextWrap {
        match self.properties.get("wrap") {
            Some(Value::TextWrap(wrap)) => *wrap,
            _ => TextWrap::default(),
        }
    }

    pub fn is_password(&self) -> bool {
        self.input_type() == InputType::Password
    }

    pub fn password_masked(&self) -> bool {
        self.is_password() && !self.boolean("password-visible", false)
    }

    pub fn dialog_role(&self) -> DialogRole {
        match self.properties.get("role") {
            Some(Value::DialogRole(role)) => *role,
            _ => DialogRole::default(),
        }
    }

    pub fn standard_button_kind(&self) -> Option<StandardButtonKind> {
        match (self.kind, self.properties.get("kind")) {
            ("Button", Some(Value::StandardButtonKind(kind))) => Some(*kind),
            _ => None,
        }
    }

    pub fn swipe_side(&self) -> SwipeSide {
        match self.properties.get("open-side") {
            Some(Value::SwipeSide(side)) => *side,
            _ => SwipeSide::Closed,
        }
    }

    #[doc(hidden)]
    pub fn accepts_swipe_side(&self, current: SwipeSide, side: SwipeSide) -> bool {
        self.swipe_side() == current
            && current != side
            && (side == SwipeSide::Closed || self.swipe_panel(side).is_some())
    }

    pub fn swipe_panel(&self, side: SwipeSide) -> Option<&Node> {
        let kind = side.panel()?;
        self.children
            .iter()
            .find(|child| child.kind == kind && child.is_visible())
    }

    pub fn drawer_edge(&self) -> DrawerEdge {
        match self.properties.get("edge") {
            Some(Value::DrawerEdge(edge)) => *edge,
            _ => DrawerEdge::Left,
        }
    }

    pub fn selection_gesture(&self) -> SelectionGesture {
        match self.properties.get("selection-gesture") {
            Some(Value::SelectionGesture(gesture)) => *gesture,
            _ => SelectionGesture::Auto,
        }
    }

    pub fn selection_hold_interval(&self) -> u64 {
        self.int("selection-hold-interval")
            .unwrap_or(500)
            .clamp(100, 5000) as u64
    }

    pub fn selection_target(&self) -> SelectionTarget {
        match self.properties.get("selection-target") {
            Some(Value::SelectionTarget(target)) => *target,
            _ => SelectionTarget::Rows,
        }
    }

    pub fn is_visible(&self) -> bool {
        *self.0.visibility.get_or_init(|| {
            self.boolean("visible", true)
                && (!is_window_overlay(self.kind) || self.boolean("open", false))
        })
    }

    #[doc(hidden)]
    pub fn handles_wheel(&self) -> bool {
        self.kind == "WheelHandler"
            || matches!(self.kind, "TouchArea" | "CustomItem") && self.scroll_event_handler
    }

    #[doc(hidden)]
    pub fn accepts_pointer_button(&self, button: PointerButton) -> bool {
        match self.properties.get("accepted-buttons") {
            Some(Value::PointerButtons(buttons)) => buttons.contains(button),
            _ => true,
        }
    }

    pub fn cursor_shape(&self) -> CursorShape {
        match self.properties.get("cursor-shape") {
            Some(Value::CursorShape(shape)) => *shape,
            _ => CursorShape::Auto,
        }
    }

    pub fn is_modal(&self) -> bool {
        is_modal_overlay(self.kind) || self.kind == "PopupWindow" && self.boolean("modal", false)
    }

    pub fn stack_page_index(&self) -> Option<usize> {
        self.stack_route_index(self.stack.as_ref()?.current()?)
    }

    #[doc(hidden)]
    pub fn stack_route_index(&self, page: &str) -> Option<usize> {
        self.children
            .iter()
            .position(|node| node.text("__stack-route").or_else(|| node.element_id()) == Some(page))
    }

    pub fn int(&self, name: &str) -> Option<i64> {
        match self.properties.get(name) {
            Some(Value::Int(value)) => Some(*value),
            _ => None,
        }
    }

    pub fn image(&self, name: &str) -> Option<Image> {
        match self.properties.get(name) {
            Some(Value::Image(value)) => Some(value.clone()),
            _ => None,
        }
    }

    pub fn image_asset(&self, name: &str) -> Option<ImageAsset> {
        match self.properties.get(name) {
            Some(Value::Image(value)) => Some(value.clone().into()),
            Some(Value::ImageAsset(value)) => Some(value.clone()),
            _ => None,
        }
    }

    #[doc(hidden)]
    pub fn image_size(&self, name: &str) -> [u32; 2] {
        match self.properties.get(name) {
            Some(Value::Image(image)) => image.size(),
            Some(Value::ImageAsset(asset)) => asset.size(),
            _ => [0, 0],
        }
    }

    pub fn image_fit(&self, name: &str) -> Option<ImageFit> {
        match self.properties.get(name) {
            Some(Value::ImageFit(value)) => Some(*value),
            _ => None,
        }
    }

    pub fn image_tile_mode(&self, name: &str) -> ImageTileMode {
        match self.properties.get(name) {
            Some(Value::ImageTileMode(value)) => *value,
            _ => ImageTileMode::default(),
        }
    }

    pub fn color(&self, name: &str) -> Option<Color> {
        match self.properties.get(name) {
            Some(Value::Color(value) | Value::Brush(Brush::Solid(value))) => Some(*value),
            _ => None,
        }
    }

    pub fn color_scheme(&self) -> Option<ColorScheme> {
        match self.properties.get("color-scheme") {
            Some(Value::ColorScheme(scheme)) => Some(*scheme),
            _ => None,
        }
    }

    /// Look up a child by the logical index used in scene paths.
    pub fn child_at(&self, index: usize) -> Option<&Self> {
        self.children
            .get(self.table_column_window.dense_index(index)?)
    }

    #[doc(hidden)]
    pub fn child_at_mut(&mut self, index: usize) -> Option<&mut Self> {
        let index = self.table_column_window.dense_index(index)?;
        self.children.get_mut(index)
    }

    /// Iterate stored children with their logical indices.
    pub fn materialized_children(
        &self,
    ) -> impl DoubleEndedIterator<Item = (usize, &Self)> + ExactSizeIterator {
        self.children
            .iter()
            .enumerate()
            .map(|(index, child)| (self.table_column_window.logical_index(index), child))
    }

    #[doc(hidden)]
    pub fn set_table_selection_part(&mut self, index: usize, part: Node) {
        let parts = self
            .data_mut()
            .table_selection_parts
            .get_or_insert_with(Default::default);
        if let Some(slot) = Rc::make_mut(parts).get_mut(index) {
            *slot = Some(part);
        }
    }

    #[doc(hidden)]
    pub fn table_selection_part(&self, index: usize) -> Option<&Node> {
        self.table_selection_parts.as_ref()?.get(index)?.as_ref()
    }

    #[doc(hidden)]
    pub fn control_part(&self, kind: &str) -> Option<&Self> {
        self.children.iter().find(|child| child.kind == kind)
    }

    #[doc(hidden)]
    pub fn combo_content(&self) -> Option<&Self> {
        if combo_editable(self) {
            return None;
        }
        self.control_part("ComboContent")
            .filter(|part| part.is_visible())
    }

    #[doc(hidden)]
    pub fn is_control_part(&self) -> bool {
        matches!(
            self.kind,
            "ButtonContent"
                | "SliderHandle"
                | "RangeLowerHandle"
                | "RangeUpperHandle"
                | "ProgressTrack"
                | "ProgressFill"
                | "ScrollTrack"
                | "ScrollThumb"
                | "ToggleIndicator"
                | "SwitchThumb"
                | "SpinUpIndicator"
                | "SpinDownIndicator"
                | "ComboContent"
                | "ComboIndicator"
                | "ComboPopup"
                | "TreeDisclosure"
                | "SelectionStartHandle"
                | "SelectionEndHandle"
        )
    }

    #[doc(hidden)]
    pub const PROGRESS_PARTS: [&str; 2] = ["ProgressTrack", "ProgressFill"];

    #[doc(hidden)]
    pub const SCROLL_PARTS: [&str; 2] = ["ScrollTrack", "ScrollThumb"];

    #[doc(hidden)]
    pub fn has_scroll_parts(&self) -> bool {
        matches!(self.kind, "ScrollBar" | "ScrollIndicator")
            && self
                .children
                .iter()
                .any(|child| Self::SCROLL_PARTS.contains(&child.kind))
    }

    #[doc(hidden)]
    pub fn has_progress_parts(&self) -> bool {
        self.kind == "ProgressIndicator"
            && self
                .children
                .iter()
                .any(|child| Self::PROGRESS_PARTS.contains(&child.kind))
    }

    pub fn at_path(&self, path: &[usize]) -> Option<&Self> {
        let mut node = self;
        for &index in path {
            node = node.child_at(index)?;
        }
        Some(node)
    }

    /// Check the state that controls which descendants may receive input.
    #[doc(hidden)]
    pub fn try_input_scope_available<E>(
        &self,
        child: Option<&Self>,
        open: impl FnOnce() -> Result<bool, E>,
        checkable: impl FnOnce() -> Result<bool, E>,
        checked: impl FnOnce() -> Result<bool, E>,
        swipe_side: impl FnOnce() -> Result<SwipeSide, E>,
    ) -> Result<bool, E> {
        if is_window_overlay(self.kind) && !open()? {
            return Ok(false);
        }
        let Some(child) = child else {
            return Ok(true);
        };
        if self.kind == "GroupBox" && checkable()? && !checked()? {
            return Ok(false);
        }
        if self.kind == "SwipeDelegate" && matches!(child.kind, "SwipeLeft" | "SwipeRight") {
            return Ok(swipe_side()?.panel() == Some(child.kind));
        }
        Ok(true)
    }

    #[doc(hidden)]
    pub fn input_scope_available(
        &self,
        child: Option<&Self>,
        open: impl FnOnce() -> bool,
        checkable: impl FnOnce() -> bool,
        checked: impl FnOnce() -> bool,
        swipe_side: impl FnOnce() -> SwipeSide,
    ) -> bool {
        self.try_input_scope_available(
            child,
            || Ok::<_, std::convert::Infallible>(open()),
            || Ok(checkable()),
            || Ok(checked()),
            || Ok(swipe_side()),
        )
        .unwrap_or_else(|error| match error {})
    }

    fn scene_input_scope_available(&self, child: Option<&Self>) -> bool {
        if child.is_some() && self.expander_collapsed() {
            return false;
        }
        self.input_scope_available(
            child,
            || self.boolean("open", false),
            || self.group_checkable(),
            || self.boolean("checked", true),
            || self.swipe_side(),
        )
    }

    pub fn interactive_at_path(&self, path: &[usize]) -> Option<&Self> {
        let mut node = self;
        for (depth, index) in path.iter().enumerate() {
            if !node.boolean("visible", true) || !node.enables_children() {
                return None;
            }
            let child = node.child_at(*index)?;
            if !node.scene_input_scope_available(Some(child)) {
                return None;
            }
            let has_page_content = depth + 1 < path.len() || node.kind != "TabWidget";
            if has_page_content {
                let active = match node.kind {
                    "TabWidget" | "SwipeView" | "StackLayout" => group_index(node),
                    "StackView" => node.stack_page_index(),
                    _ => Some(*index),
                };
                if active != Some(*index) {
                    return None;
                }
            }
            node = child;
        }
        (node.boolean("visible", true)
            && node.boolean("enabled", true)
            && node.scene_input_scope_available(None))
        .then_some(node)
    }

    /// Resolve the editable cell that displayed the draft's original text.
    #[doc(hidden)]
    pub fn table_cell_commit_target(&self, path: &[usize], original: &str) -> Option<&Self> {
        self.interactive_at_path(path).filter(|node| {
            editable_table_cell(node) && node.text("text").unwrap_or_default() == original
        })
    }

    /// Resolve a scene target for an event without an explicit payload.
    #[doc(hidden)]
    pub fn dispatch_target(&self, path: &[usize], event: &str) -> Option<&Self> {
        let node = self.at_path(path)?;
        let target = if node.terminal_dispatch(event) {
            node
        } else {
            self.interactive_at_path(path)?
        };
        (event != "accepted" || !is_text_input(target.kind) || target.accepts_text_submission())
            .then_some(target)
    }

    fn terminal_dispatch(&self, event: &str) -> bool {
        matches!(
            (self.kind, event),
            ("ScaleRotateGestureHandler", "ended" | "cancelled")
                | ("SwipeGestureHandler", "swiped" | "cancelled")
                | ("HoverHandler", "exited")
                | ("Loader", "loaded")
                | ("Button" | "ToolButton" | "RoundButton", "canceled")
        )
    }

    #[doc(hidden)]
    pub fn focus_event_target(&self, path: &[usize], event: FocusEvent) -> Option<&Self> {
        let node = if event.is_loss() {
            self.at_path(path)
        } else {
            self.interactive_at_path(path)
        };
        node.filter(|node| {
            matches!(node.kind, "FocusScope" | "CustomItem")
                || matches!(event, FocusEvent::Lost(_))
                    && (is_text_input(node.kind) || node.kind == "ComboBox")
                    && node
                        .focus_handle
                        .as_ref()
                        .is_some_and(FocusHandle::has_editor_changes)
        })
    }

    #[doc(hidden)]
    pub fn handles_key_phase(&self, phase: KeyPhase) -> bool {
        matches!(self.kind, "FocusScope" | "CustomItem") && self.key_handlers & phase.mask() != 0
    }

    #[doc(hidden)]
    pub fn key_event_target(&self, path: &[usize], phase: KeyPhase) -> Option<&Self> {
        self.interactive_at_path(path)
            .filter(|node| node.handles_key_phase(phase))
    }

    pub fn group_checkable(&self) -> bool {
        self.kind == "GroupBox" && self.boolean("checkable", false)
    }

    pub fn expander_collapsed(&self) -> bool {
        self.kind == "Expander" && !self.boolean("expanded", false)
    }

    pub fn group_content_enabled(&self) -> bool {
        !self.expander_collapsed() && (!self.group_checkable() || self.boolean("checked", true))
    }

    #[doc(hidden)]
    pub fn control_target(&self, path: &[usize], action: ControlAction) -> Option<&Self> {
        if action != ControlAction::Drawer(true) {
            return self.interactive_at_path(path).filter(|node| {
                !matches!(action, ControlAction::SetSwitch(_)) || node.kind == "Switch"
            });
        }
        let node = match path.split_last() {
            Some((&index, parent)) => {
                let parent = self.interactive_at_path(parent)?;
                if !parent.group_content_enabled() {
                    return None;
                }
                parent.child_at(index)?
            }
            None => self,
        };
        (node.kind == "Drawer" && node.boolean("visible", true) && node.boolean("enabled", true))
            .then_some(node)
    }

    /// Identity of the materialized row in its view, for routing and focus.
    pub fn routing_row_key(&self) -> Option<RowKey> {
        self.row_key.map(|key| key.in_view(self.row_scope))
    }

    /// Identity of this mounted component.
    pub fn dynamic_mount_key(&self) -> Option<RowKey> {
        self.external_component.as_ref()?;
        self.loader_mount.as_ref().map(|mount| mount.key)
    }

    /// Identity of the virtual row or mounted component containing this scene path.
    pub fn row_key_at(&self, path: &[usize]) -> Option<RowKey> {
        let mut node = self;
        let mut key = None;
        let mut mount_key = self.loader_mount.as_ref().map(|mount| mount.key);
        for &index in path {
            node = node.child_at(index)?;
            if let Some(mount) = &node.loader_mount {
                mount_key = Some(mount.key);
            }
            if node.kind == "ListRow"
                || matches!(node.kind, "TabButton" | "Tab" | "StackItem" | "SwipePage")
                    && node.row_key.is_some()
            {
                key = node.routing_row_key();
            }
        }
        key.or(mount_key)
    }

    /// Find the displayed index of a source row identity. The source index is a lookup hint.
    pub fn displayed_row_for_key(&self, key: RowKey, source_index: usize) -> Option<usize> {
        if let Some(tree) = &self.tree {
            tree.position_of_source_key(key, source_index)
        } else {
            self.row_locator
                .as_ref()?
                .position_of_source_key(key, source_index)
        }
    }
}

#[cfg(test)]
mod node_storage_tests {
    use super::*;

    #[test]
    fn clipped_group_classification_tracks_snapshot_edits() {
        let original = Node::new("VerticalLayout", None).into_shared();
        assert!(original.can_skip_clipped_layout_group());

        let mut edited = original.clone();
        edited
            .properties
            .insert("cache-rendering-hint", true.into());
        assert!(!edited.can_skip_clipped_layout_group());
        edited.properties.remove("cache-rendering-hint");
        assert!(edited.can_skip_clipped_layout_group());

        edited.properties.insert("opacity", 0.5.into());
        assert!(!edited.can_skip_clipped_layout_group());
        edited.properties.remove("opacity");
        assert!(edited.can_skip_clipped_layout_group());
        edited.kind = "Rectangle";
        assert!(!edited.can_skip_clipped_layout_group());
        assert!(original.can_skip_clipped_layout_group());
    }

    #[test]
    fn cached_visibility_and_opacity_follow_snapshot_edits() {
        let original = Node::new("Rectangle", None).into_shared();
        assert!(original.is_visible());
        assert_eq!(original.opacity(), 1.0);

        let mut edited = original.clone();
        edited.properties.insert("visible", false.into());
        edited.properties.insert("opacity", 0.25.into());
        assert!(!edited.is_visible());
        assert_eq!(edited.opacity(), 0.25);
        assert!(original.is_visible());
        assert_eq!(original.opacity(), 1.0);

        edited.properties.insert("visible", true.into());
        assert!(edited.is_visible());
        edited.kind = "PopupWindow";
        assert!(!edited.is_visible());
        edited.properties.insert("open", true.into());
        assert!(edited.is_visible());
    }

    #[test]
    fn layout_property_comparison_handles_different_ignored_keys() {
        let mut original = Node::new("Rectangle", None);
        original.properties.insert("width", Length(80.0).into());
        original.properties.insert("height", Length(32.0).into());
        original.properties.insert("opacity", 0.75.into());
        let mut changed = original.clone();
        changed.properties.remove("opacity");
        changed
            .properties
            .insert("drop-shadow-blur", Length(3.0).into());
        assert!(original.same_frame_inputs(&changed));

        changed.properties.insert("x", Length(12.0).into());
        assert!(original.same_local_layout_inputs(&changed));
        assert!(!original.same_frame_inputs(&changed));

        changed.properties.remove("width");
        assert!(!original.same_local_layout_inputs(&changed));
        changed.properties.insert("width", Length(80.0).into());
        changed.properties.insert("height", Length(40.0).into());
        assert!(!original.same_local_layout_inputs(&changed));
    }

    #[test]
    fn fixed_plain_text_uses_parent_layout_rules_for_invalidation() {
        for kind in ["Text", "Label"] {
            let mut parent = Node::new("Rectangle", None);
            let mut text = Node::new(kind, None);
            text.properties.insert("text", "Old".to_owned().into());
            text.properties.insert("width", Percent(0.5).into());
            text.properties.insert("height", Length(24.0).into());
            parent.children.push(text);

            let mut changed = parent.clone();
            changed.children[0]
                .properties
                .insert("text", "New text needs more room".to_owned().into());
            assert!(parent.same_frame_inputs(&changed));
            assert!(!parent.children[0].same_own_paint_inputs(&changed.children[0]));
            assert!(!parent.children[0].same_frame_inputs(&changed.children[0]));
            let mut index = SceneIndex::default();
            index.synchronize(&parent, 0);
            let changes = index.synchronize(&changed, 0);
            assert_eq!(changes.measure, 0);
            assert_eq!(changes.paint, 1);

            let mut resized = changed.clone();
            resized.children[0]
                .properties
                .insert("width", Length(48.0).into());
            assert!(!changed.same_frame_inputs(&resized));

            for scene in [&mut parent, &mut changed] {
                scene.kind = "TabWidget";
            }
            assert!(!parent.same_frame_inputs(&changed));

            for scene in [&mut parent, &mut changed] {
                scene.kind = "Rectangle";
                scene.children[0].properties.remove("height");
            }
            assert!(!parent.same_frame_inputs(&changed));
        }
    }

    #[test]
    fn image_source_updates_use_sizes_for_layout_and_pixels_for_paint() {
        let image = |bytes: &'static [u8], size| Image::Embedded {
            format: ImageFormat::Png,
            bytes,
            size,
        };
        for kind in ["Image", "AnimatedImage"] {
            let mut previous = Node::new(kind, None);
            previous
                .properties
                .insert("source", image(b"first", [16, 24]).into());
            let mut current = previous.clone();
            current.properties.insert(
                "source",
                Value::ImageAsset(ImageAsset::from(image(b"second", [16, 24]))),
            );
            assert!(previous.same_frame_inputs(&current));
            assert!(!previous.same_own_paint_inputs(&current));

            let mut cropped = current.clone();
            cropped.properties.insert("source-clip-width", 8_i64.into());
            assert!(!previous.same_frame_inputs(&cropped));

            current
                .properties
                .insert("source", image(b"larger", [32, 24]).into());
            assert!(!previous.same_frame_inputs(&current));
            for node in [&mut previous, &mut current] {
                node.properties.insert("width", Length(48.0).into());
            }
            assert!(!previous.same_frame_inputs(&current));
            for node in [&mut previous, &mut current] {
                node.properties
                    .insert("height", Value::Percent(Percent(0.5)));
            }
            assert!(previous.same_frame_inputs(&current));
            assert!(!previous.same_own_paint_inputs(&current));
        }
    }

    #[test]
    fn stacking_order_follows_edits_to_a_shared_snapshot() {
        let mut node = Node::new("Rectangle", None);
        assert_eq!(node.z(), 0.0);
        node.properties.insert("z", 2.0.into());
        assert_eq!(node.z(), 2.0);

        let original = node.clone();
        node.properties.insert("z", (-1.0).into());
        assert_eq!(node.z(), -1.0);
        assert_eq!(original.z(), 2.0);

        for value in [-0.0, f64::NAN, f64::INFINITY] {
            node.properties.insert("z", value.into());
            assert_eq!(node.z().to_bits(), 0.0_f64.to_bits());
        }
        node.properties.remove("z");
        assert_eq!(node.z(), 0.0);
    }

    #[test]
    fn snapshot_identity_tracks_unfrozen_nodes_without_retaining_them() {
        let mut node = Node::new("Flickable", None);
        let identity = node.snapshot_identity();
        assert!(identity.matches(&node));
        assert_eq!(Rc::strong_count(&node.0), 1);
        node.properties.insert("x", Length(16.0).into());
        assert!(!identity.matches(&node));
        assert_eq!(node.length("x"), Some(16.0));
    }

    #[test]
    fn scene_features_follow_edits_to_unfrozen_snapshots() {
        let mut window = Node::new("Window", None);
        let mut panel = Node::new("Rectangle", None);
        panel.children.extend([
            Node::new("ListView", None),
            Node::new("ComboBox", None),
            Node::new("Rectangle", None),
            Node::new("ScrollBar", None),
        ]);
        window.children.extend([panel, Node::new("Text", None)]);
        let features = [
            SceneFeature::VirtualList,
            SceneFeature::FontFamilyChoice,
            SceneFeature::WindowOverlay,
            SceneFeature::AttachedScrollBar,
            SceneFeature::WindowEffects,
            SceneFeature::GeometryObservation,
            SceneFeature::PageTransition,
        ];
        for feature in features {
            assert!(!window.contains_scene_feature(feature));
        }
        assert!(!window.has_shared_storage());
        assert_eq!(window.children[1].0.scene_features.get(), Some(&0));

        let mut edited = window.clone();
        let panel = &mut edited.children[0];
        panel.children[0].list = Some(ListMetrics {
            row_count: 1,
            first_row: 0,
        });
        panel.children[1]
            .properties
            .insert("__font-picker-field", Value::String("family".into()));
        panel.children[2].kind = "Drawer";
        panel.children[3].scroll_target = Some(ViewportHandle::for_flickable(false));
        panel.children[0].button_press_handle = Some(ButtonPressHandle::animated());
        panel.children[3].viewport_handle = Some(ViewportHandle::default());
        panel.children.push(Node::new("SwipeView", None));
        for feature in features {
            assert!(edited.contains_scene_feature(feature));
            assert!(!window.contains_scene_feature(feature));
        }
        assert!(edited.children[1].shares_storage_with(&window.children[1]));

        edited.children.remove(0);
        for feature in features {
            assert!(!edited.contains_scene_feature(feature));
        }
    }

    #[test]
    fn scene_metadata_detaches_only_when_edited() {
        let mut original = Node::new("Rectangle", None);
        original.key_handlers = 1;
        let mut moved = original.clone();
        moved.properties.insert("x", Length(12.0).into());
        assert!(Rc::ptr_eq(&original.metadata, &moved.metadata));
        assert!(original.same_local_layout_inputs(&moved));

        moved.key_handlers = 2;
        assert!(!Rc::ptr_eq(&original.metadata, &moved.metadata));
        assert_eq!(original.key_handlers, 1);
        assert_eq!(moved.key_handlers, 2);
        assert!(!original.same_local_layout_inputs(&moved));
    }

    #[test]
    fn frame_layout_comparison_detects_an_edited_window_layer() {
        let mut window = Node::new("Window", None);
        let mut scene = Node::new("SceneView3D", None);
        scene
            .properties
            .insert("render-mode", "underlay".to_owned().into());
        window.children.push(scene);
        let window = window.into_shared();
        let mut edited = window.clone();
        assert!(window.same_frame_inputs(&edited));

        edited.children[0]
            .properties
            .insert("render-mode", "overlay".to_owned().into());
        assert!(!window.same_frame_inputs(&edited));
    }

    #[test]
    fn scene_snapshot_clones_detach_only_the_edited_path() {
        assert_eq!(std::mem::size_of::<Node>(), std::mem::size_of::<Rc<()>>());

        let mut root = Node::new("Window", None);
        let mut label = Node::new("Text", None);
        label
            .properties
            .insert("text", Value::String("original".into()));
        root.children.extend([label, Node::new("Rectangle", None)]);
        let mut snapshot = root.clone();
        assert!(root.shares_storage_with(&snapshot));
        assert!(!snapshot.has_shared_storage());

        snapshot.children[0]
            .properties
            .insert("text", Value::String("edited".into()));
        assert!(!root.shares_storage_with(&snapshot));
        assert_eq!(root.children[0].text("text"), Some("original"));
        assert_eq!(snapshot.children[0].text("text"), Some("edited"));
        assert!(root.children[1].shares_storage_with(&snapshot.children[1]));

        let mut frozen = root.clone().into_shared();
        assert!(!root.children[0].has_shared_storage());
        assert!(frozen.children[0].has_shared_storage());
        assert!(!frozen.contains_scene_feature(SceneFeature::VirtualList));
        let identity = frozen.shared_identity().unwrap();
        frozen.children[0].list = Some(ListMetrics {
            row_count: 1,
            first_row: 0,
        });
        assert!(!identity.matches(&frozen));
        assert!(frozen.contains_scene_feature(SceneFeature::VirtualList));
        assert!(!root.contains_scene_feature(SceneFeature::VirtualList));
    }
}
