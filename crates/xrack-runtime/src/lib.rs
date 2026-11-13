//! Renderer-independent scene snapshots produced by generated components.

/// Compatibility level expected by the Rust code emitted by xrackc.
#[doc(hidden)]
pub const CODEGEN_ABI_VERSION: u32 = 2;

mod animation;
mod arithmetic;
mod button_group;
mod calendar;
mod callback;
mod child_window;
mod color_picker;
mod combo;
mod controls;
mod delegate;
mod double_spin;
mod drag;
mod event_loop;
#[doc(hidden)]
pub use event_loop::EventLoopRegistration;
pub use event_loop::{EventLoopError, WeakComponent, invoke_from_event_loop};
mod focus;
mod font_picker;
mod geometry;
mod hover;
mod image_animation;
mod image_asset;
mod keyboard;
mod list;
mod localization;
mod model;
mod native_dialog;
mod picker;
mod pointer;
mod property;
mod range;
mod record;
pub use record::{RecordList, RecordRowValidator, RecordValue};
mod repeat;
mod row_heights;
mod scale_rotate;
mod scene_plan;
mod split;
mod stack;
mod styled_text;
mod swipe_gesture;
mod table_clipboard;
mod table_history;
mod table_rows;
mod table_selection;
mod text_input;
mod time_picker;
mod timer;
mod tree;
mod tumbler;
mod viewport;
mod window_status;
pub use animation::{Animation, Easing, SceneTransition, StateTiming};
#[doc(hidden)]
pub use arithmetic::{int_divide, int_remainder, try_int_divide, try_int_remainder};
#[doc(hidden)]
pub use button_group::{
    ButtonGroup, ButtonGroupMembership, ButtonGroupSelection, RadioMembership, bind_button_group,
    bind_check_group, bind_dynamic_button_group, prepare_radio_selection, radio_value,
    summarize_checks, try_prepare_radio_selection, try_summarize_checks,
};
pub use calendar::{CalendarAction, CalendarState, Date, is_calendar_control};
#[doc(hidden)]
pub use calendar::{
    CalendarCell, CalendarCells, CalendarUpdate, DateBounds, DatePopupState, apply_calendar_action,
    calendar_input_is_current, try_apply_calendar_action, try_calendar_input_is_current,
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
    ComboEditor, ComboInput, ComboItems, ComboModel, ComboSelection, ComboSubmission, ComboUpdate,
    combo_model_is_current, combo_submission, combo_value, prepare_combo_input,
    try_prepare_combo_input,
};
pub use controls::{
    ActionKey, ControlAction, DialogCommand, DialogRole, PopupPlacement, SpinAction, SpinRange,
    StandardButtonKind, SwipeSide, TableColumnOrder, TableHeader, TableSort, button_text,
    combo_editable, delay_millis, dialog_button_action, group_index, group_item, is_button,
    is_modal_overlay, is_pane, is_spin_box, is_window_overlay, page_indicator_count,
    page_indicator_index, resizable_table_column, resolve_swipe_index, search_suggestion,
    search_suggestions, spin_editable, spin_text, spin_value_after, text_input_value,
};
#[doc(hidden)]
pub use controls::{DialogEvent, set_overlay_open, try_resolve_swipe_index, try_set_overlay_open};
#[doc(hidden)]
pub use controls::{
    SPIN_VIEW_PROPERTIES, prepare_spin_view, spin_view_is_current, try_prepare_spin_view,
    try_spin_view_is_current,
};
#[doc(hidden)]
pub use controls::{
    delay_button_transition_is_current, group_selection_item, prepare_action_activation,
    prepare_action_view, prepare_group_box_toggle, prepare_group_selection,
    prepare_index_selection, prepare_row_toggle, prepare_source_selection, resize_table_column,
    table_drag_available, try_delay_button_transition_is_current, try_prepare_action_activation,
    try_prepare_action_view, try_prepare_group_box_toggle, try_prepare_group_selection,
    try_prepare_index_selection, try_prepare_row_toggle, try_prepare_source_selection,
    try_table_drag_available,
};
#[doc(hidden)]
pub use delegate::{ComponentRepeater, RowContext};
pub use double_spin::{
    DecimalSpinRange, SpinDecimal, decimal_separator, decimal_spin_value_after, spin_text_is_valid,
};
pub use drag::{DragAction, DragEvent, is_drag_area};
pub use focus::{
    FocusAction, FocusContext, FocusEvent, FocusHandle, FocusReason, FocusRequest, FocusRequests,
    TextCommand,
};
#[doc(hidden)]
pub use focus::{FocusCommandMethod, is_focus_handler};
pub use font_picker::Font;
#[doc(hidden)]
pub use font_picker::{FontPopupState, is_font_family_choice, set_font_family_choices};
#[doc(hidden)]
pub use geometry::{GeometryHandle, GeometryMetric};
pub use hover::{CursorShape, HoverHandle};
pub use image_animation::{ImageAnimation, ImageAnimationFrame};
#[doc(hidden)]
pub use image_animation::{ImagePlaybackQueue, ImagePlaybackRegistration, ImagePlaybackSlot};
pub use image_asset::{ImageAsset, ImageIdentity, ImagePixels, LoadedAnimation, LoadedImage};
pub use keyboard::{EventResult, KeyEvent, KeyModifiers, KeyPhase, WheelEvent};
#[doc(hidden)]
pub use list::ViewIndex;
pub use list::{ListMetrics, RowKey};
#[doc(hidden)]
pub use list::{
    ListModel, ListRowLocator, ListRowRenderer, RowLifetime, TableCellEditor, TableColumnWindow,
    project_standard_table_rows, standard_list_rows, standard_list_rows_in_view,
    standard_table_rows, standard_table_rows_in_order, standard_table_rows_with_editors_in_order,
    standard_table_rows_with_header, try_project_standard_table_rows,
};
pub use localization::LocaleContext;
#[doc(hidden)]
pub use localization::{LocaleBinding, LocaleId, TranslationCatalog, TranslationValue};
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
pub use pointer::{
    PointerButton, PointerButtons, PointerEvent, PointerEventKind, PointerHandle, TouchPoint,
    TouchPoints,
};
#[doc(hidden)]
pub use property::ComponentLifetime;
pub use property::{ChangeSignal, InputProperty, Property};
pub use range::{
    DialInputMode, IndicatorState, RangeAction, ScrollBarRange, ScrollSnapMode, SliderInterval,
    SliderRange, indicator_state, progress_value,
};
#[doc(hidden)]
pub use range::{
    RANGE_VIEW_PROPERTIES, prepare_range_view, range_view_properties, try_prepare_range_view,
};
#[doc(hidden)]
pub use repeat::{Repeater, repeat_scene, static_repeat_scene};
#[doc(hidden)]
pub use row_heights::{RowHeightOverrides, RowHeightProjection, RowHeights};
pub use scale_rotate::{ScaleRotateHandle, ScaleRotateSample};
#[doc(hidden)]
pub use scene_plan::{
    GridCellPosition, GridCellSlot, GridPlanner, GridTrackInput, LoadedVectorPath, ScenePlanSlice,
    StaticVectorPath, VectorSegment,
};
#[doc(hidden)]
pub use split::{
    SplitHandleInput, SplitResizeHandle, decode_split_sizes, decode_split_state,
    encode_split_sizes, encode_split_state,
};
#[doc(hidden)]
pub use stack::{StackChange, StackEvent, StackNavigationArgument, StackNavigationMethod};
pub use stack::{StackNavigation, StackSnapshot, StackState};
use std::{collections::BTreeMap, rc::Rc, sync::Arc};
pub use styled_text::{StyledLink, StyledRun, StyledText, StyledTextError, TextStyle};
pub use swipe_gesture::{SwipeHandle, SwipeSample};
#[doc(hidden)]
pub use table_clipboard::{
    TablePaste, TablePasteInput, paste_table_text, table_paste_bool, table_paste_check_state,
    table_paste_number, try_table_paste_number,
};
#[doc(hidden)]
pub use table_history::{TableEditHistory, TablePasteHistory, TableRowPatch};
pub use table_rows::{
    TableCellEditTarget, TableEditPosition, TableRowOrder, TableRowPositions, TableRows,
    next_table_edit_cell, next_table_edit_cell_in_view, next_table_edit_cell_in_view_if,
    next_table_edit_cell_while_current, table_cell_edit_target, table_cell_edit_target_if,
    try_history_replay_source, try_next_table_edit_cell, try_next_table_edit_cell_in_view,
    try_next_table_edit_cell_while_current, try_prepare_history_replay, try_table_cell_edit_target,
};
pub use table_selection::{
    RowSelection, SelectionTarget, TableSelection, TableSelectionAction, TableSelectionSnapshot,
};
#[doc(hidden)]
pub use table_selection::{
    TableSelectionChange, TableSelectionCommand, TableSelectionEvent, TableSelectionMethod,
    TableSelectionOptions,
};
pub use table_selection::{
    copy_table_text, copy_table_text_in_order, copy_table_text_in_view, copy_table_text_in_view_if,
    copy_table_text_while_current, try_copy_table_text_in_order, try_copy_table_text_in_view,
    try_copy_table_text_while_current,
};
pub use text_input::InputType;
#[doc(hidden)]
pub use text_input::{
    SearchInput, SearchUpdate, text_submission_is_valid, try_text_submission_is_valid,
};
pub use time_picker::Time;
#[doc(hidden)]
pub use time_picker::{TimePickerOptions, TimePopupState};
#[doc(hidden)]
pub use timer::{
    TimerCommand, TimerConfig, TimerDelivery, TimerQueue, TimerRegistration, TimerSlot, TimerTick,
};
pub use tree::{TreeAction, TreeCommand, TreeItem, TreeSelectionAction, TreeSnapshot, TreeState};
#[doc(hidden)]
pub use tree::{
    TreeCommandMethod, TreeControlChange, TreeControlEvent, TreeExpansionChange,
    TreeSelectionChange,
};
pub use tumbler::{Tumbler, TumblerItems};
pub use viewport::{
    ScrollAction, ScrollAlignment, ScrollContext, ScrollRequest, ScrollRequests, ViewportHandle,
    ViewportMotion,
};
#[doc(hidden)]
pub use viewport::{ScrollCommand, ScrollCommandMethod, SourceScroll};
pub use window_status::{WindowCommand, WindowStatus};
#[doc(hidden)]
pub use window_status::{WindowCommandMethod, WindowLifecycle, WindowMetric, WindowStatusBinding};

/// The backend-independent interface implemented by generated UI components.
pub trait Component {
    fn render(&self) -> Node;

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

    fn control_at(&mut self, _scene: &Node, _path: &[usize], _action: ControlAction) -> bool {
        false
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
            Some(reads) => Ok(!reads.changed() && available()? && !reads.changed()),
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
        if self.follows_row(scene, path) {
            Some(route(input))
        } else {
            Some(property::watch_reads(|reads| {
                route(InputRoute {
                    reads: Some(reads),
                    ..input
                })
            }))
        }
    }

    pub fn deliver_input(
        &self,
        ui: &mut (impl Component + ?Sized),
        scene: &Node,
        path: &[usize],
    ) -> bool {
        self.with_input(scene, path, |input| ui.route_input(scene, path, input))
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ImageFormat {
    Png,
    Svg,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Image {
    #[default]
    Empty,
    Embedded {
        format: ImageFormat,
        bytes: &'static [u8],
        size: [u32; 2],
    },
    Animated(&'static ImageAnimation),
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

impl Image {
    pub fn size(self) -> [u32; 2] {
        match self {
            Self::Empty => [0, 0],
            Self::Animated(animation) => animation.size(),
            Self::Embedded { size, .. } | Self::NineSliced { size, .. } => size,
        }
    }

    pub fn frame_count(self) -> usize {
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
    StringList(Vec<String>),
    /// A live string list whose row identities are shared by its bindings.
    StringModel(VecModel<String>),
    Record(RecordValue),
    RecordList(RecordList),
    ComboModel(ComboModel),
    Length(Length),
    Percent(Percent),
    Color(Color),
    Brush(Brush),
    Image(Image),
    ImageAsset(ImageAsset),
    ImageFit(ImageFit),
    ImageTileMode(ImageTileMode),
    ImageHorizontalAlignment(ImageHorizontalAlignment),
    ImageVerticalAlignment(ImageVerticalAlignment),
    ImageRendering(ImageRendering),
    LayoutAlignment(LayoutAlignment),
    CrossAxisAlignment(CrossAxisAlignment),
    Orientation(Orientation),
    FlickDirection(FlickDirection),
    FlickBoundsBehavior(FlickBoundsBehavior),
    FlickBoundsMovement(FlickBoundsMovement),
    FlexboxLayoutDirection(FlexboxLayoutDirection),
    FlexboxLayoutWrap(FlexboxLayoutWrap),
    DialInputMode(DialInputMode),
    ScrollBarPolicy(ScrollBarPolicy),
    ScrollSnapMode(ScrollSnapMode),
    ScrollAlignment(ScrollAlignment),
    TextWrap(TextWrap),
    TextHorizontalAlignment(TextHorizontalAlignment),
    TextVerticalAlignment(TextVerticalAlignment),
    TextOverflow(TextOverflow),
    InputType(InputType),
    CursorShape(CursorShape),
    PointerEventKind(PointerEventKind),
    PointerButton(PointerButton),
    PointerButtons(PointerButtons),
    ColorScheme(ColorScheme),
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
    Date(Date),
    Time(Time),
    Font(Font),
}

impl Value {
    /// Compare a host image and an image asset using their shared image semantics.
    #[doc(hidden)]
    pub fn eq_with_image_assets(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Image(embedded), Self::ImageAsset(asset))
            | (Self::ImageAsset(asset), Self::Image(embedded)) => {
                ImageAsset::from(*embedded) == *asset
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
value_from!(Length, Length);
value_from!(Percent, Percent);
value_from!(Color, Color);
value_from!(Image, Image);
value_from!(ImageFit, ImageFit);
value_from!(ImageTileMode, ImageTileMode);
value_from!(ImageHorizontalAlignment, ImageHorizontalAlignment);
value_from!(ImageVerticalAlignment, ImageVerticalAlignment);
value_from!(ImageRendering, ImageRendering);
value_from!(LayoutAlignment, LayoutAlignment);
value_from!(CrossAxisAlignment, CrossAxisAlignment);
value_from!(Orientation, Orientation);
value_from!(FlickDirection, FlickDirection);
value_from!(FlickBoundsBehavior, FlickBoundsBehavior);
value_from!(FlickBoundsMovement, FlickBoundsMovement);
value_from!(FlexboxLayoutDirection, FlexboxLayoutDirection);
value_from!(FlexboxLayoutWrap, FlexboxLayoutWrap);
value_from!(DialInputMode, DialInputMode);
value_from!(ScrollBarPolicy, ScrollBarPolicy);
value_from!(ScrollSnapMode, ScrollSnapMode);
value_from!(ScrollAlignment, ScrollAlignment);
value_from!(TextWrap, TextWrap);
value_from!(TextHorizontalAlignment, TextHorizontalAlignment);
value_from!(TextVerticalAlignment, TextVerticalAlignment);
value_from!(TextOverflow, TextOverflow);
value_from!(InputType, InputType);
value_from!(CursorShape, CursorShape);
value_from!(PointerEventKind, PointerEventKind);
value_from!(PointerButton, PointerButton);
value_from!(PointerButtons, PointerButtons);
value_from!(ColorScheme, ColorScheme);
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

#[derive(Clone, Debug)]
pub struct Node(NodeStorage);

#[derive(Clone, Debug)]
#[allow(clippy::large_enum_variant)] // Keep ordinary dynamic nodes inline.
enum NodeStorage {
    Inline(NodeData),
    Shared(Arc<NodeData>),
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
    #[doc(hidden)]
    pub fonts: Option<&'static EmbeddedFonts>,
    #[doc(hidden)]
    pub loaded_fonts: Option<LoadedFonts>,
    pub tumbler_items: Option<TumblerItems>,
    pub children: Vec<Node>,
    #[doc(hidden)]
    pub table_column_window: TableColumnWindow,
    #[doc(hidden)]
    pub timers: Vec<TimerRegistration>,
    #[doc(hidden)]
    pub image_playback: Option<Rc<ImagePlaybackRegistration>>,
    #[doc(hidden)]
    pub native_dialogs: Vec<NativeDialogRegistration>,
    pub animations: BTreeMap<&'static str, Animation>,
    #[doc(hidden)]
    pub state_timings: Option<Box<BTreeMap<&'static str, StateTiming>>>,
    #[doc(hidden)]
    pub focus_handle: Option<FocusHandle>,
    #[doc(hidden)]
    pub scale_rotate_handle: Option<ScaleRotateHandle>,
    #[doc(hidden)]
    pub swipe_handle: Option<SwipeHandle>,
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
    /// Selection snapshot for model-backed views. Retains the generated field name.
    #[doc(hidden)]
    pub standard_list_stamp: Option<(u64, u64)>,
    pub table_header: Option<Box<TableHeader>>,
    #[doc(hidden)]
    pub table_column_order: TableColumnOrder,
    pub table_selection: Option<Box<TableSelectionSnapshot>>,
    #[doc(hidden)]
    pub table_cell_editor: Option<TableCellEditor>,
    pub tree: Option<Arc<TreeSnapshot>>,
    pub stack: Option<Arc<StackSnapshot>>,
    pub action: Option<ActionKey>,
    #[doc(hidden)]
    pub button_group: Option<ButtonGroupMembership>,
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
    pub row_key: Option<RowKey>,
    #[doc(hidden)]
    pub row_scope: u64,
    paint_only: u8,
    static_paint_order: Option<ScenePlanSlice<usize>>,
    precompiled_path: Option<&'static StaticVectorPath>,
    loaded_path: Option<LoadedVectorPath>,
    static_grid: Option<ScenePlanSlice<GridCellSlot>>,
}

impl std::ops::Deref for Node {
    type Target = NodeData;

    fn deref(&self) -> &Self::Target {
        match &self.0 {
            NodeStorage::Inline(data) => data,
            NodeStorage::Shared(data) => data,
        }
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
        macro_rules! compare {
            ($($field:ident),+ $(,)?) => {{
                // An exhaustive pattern makes new scene fields opt in to comparison.
                let Self { properties: _, children: _, paint_only: _, $($field: _,)+ } = self;
                $(self.$field == other.$field)&&+
            }};
        }
        compare!(
            kind,
            table_column_window,
            application_window,
            id,
            owned_id,
            fonts,
            loaded_fonts,
            tumbler_items,
            timers,
            image_playback,
            native_dialogs,
            animations,
            state_timings,
            focus_handle,
            scale_rotate_handle,
            swipe_handle,
            hover_handle,
            pointer_handle,
            scroll_event_handler,
            press_and_hold_handler,
            viewport_handle,
            scroll_target,
            geometry_handle,
            instance_geometry_handle,
            initial_focus,
            key_handlers,
            list,
            row_locator,
            list_row_renderer,
            row_heights,
            standard_list_stamp,
            table_header,
            table_column_order,
            table_selection,
            table_cell_editor,
            tree,
            stack,
            action,
            button_group,
            tree_selection,
            split_weights,
            split_fill_pane,
            split_fill_index,
            split_sizes,
            split_resize_handle,
            row_key,
            row_scope,
            precompiled_path,
            loaded_path,
            static_grid,
            static_paint_order,
        )
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
    if matches!(event, "scrolled" | "flicked") || (kind == "SwipeDelegate" && event == "clicked") {
        return true;
    }
    !matches!(
        event,
        "check-state-changed" | "close-requested" | "pointer-event" | "scroll-event"
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
        Self(NodeStorage::Inline(NodeData {
            kind,
            application_window: false,
            id,
            owned_id: None,
            properties: BTreeMap::new(),
            fonts: None,
            loaded_fonts: None,
            tumbler_items: None,
            children: Vec::new(),
            table_column_window: TableColumnWindow::default(),
            timers: Vec::new(),
            image_playback: None,
            native_dialogs: Vec::new(),
            animations: BTreeMap::new(),
            state_timings: None,
            focus_handle: None,
            scale_rotate_handle: None,
            swipe_handle: None,
            hover_handle: None,
            pointer_handle: None,
            scroll_event_handler: false,
            press_and_hold_handler: false,
            viewport_handle: None,
            scroll_target: None,
            geometry_handle: None,
            instance_geometry_handle: None,
            initial_focus: None,
            key_handlers: 0,
            list: None,
            row_locator: None,
            list_row_renderer: None,
            row_heights: None,
            standard_list_stamp: None,
            table_header: None,
            table_column_order: TableColumnOrder::default(),
            table_selection: None,
            table_cell_editor: None,
            tree: None,
            stack: None,
            action: None,
            button_group: None,
            tree_selection: None,
            split_weights: None,
            split_fill_pane: None,
            split_fill_index: None,
            split_sizes: None,
            split_resize_handle: None,
            row_key: None,
            row_scope: 0,
            paint_only: 0,
            static_paint_order: None,
            precompiled_path: None,
            loaded_path: None,
            static_grid: None,
        }))
    }

    /// Freeze a static subtree so subsequent snapshot clones share its storage.
    #[doc(hidden)]
    pub fn into_shared(self) -> Self {
        match self.0 {
            NodeStorage::Inline(mut data) => {
                data.children = data.children.into_iter().map(Self::into_shared).collect();
                Self(NodeStorage::Shared(Arc::new(data)))
            }
            NodeStorage::Shared(data) => Self(NodeStorage::Shared(data)),
        }
    }

    pub fn shares_storage_with(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
            || matches!((&self.0, &other.0), (NodeStorage::Shared(a), NodeStorage::Shared(b)) if Arc::ptr_eq(a, b))
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
        match &mut self.0 {
            NodeStorage::Inline(data) => data,
            NodeStorage::Shared(data) => Arc::make_mut(data),
        }
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

    fn paint_only_bit(&self, property: &str) -> u8 {
        match (self.kind, property) {
            ("Rectangle", "background") => 1,
            ("Text", "color") => 2,
            ("Text" | "Label", "text") => 4,
            ("Image", "source") => 8,
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
    pub fn same_frame_inputs(&self, other: &Self) -> bool {
        if self.shares_storage_with(other) {
            return true;
        }
        let paint_only = self.paint_only & other.paint_only;
        let ignore_frame = self.kind == "AnimatedImage" && other.kind == "AnimatedImage";
        let ignore_colorize =
            matches!(self.kind, "Image" | "AnimatedImage") && self.kind == other.kind;
        let fixed_visual_box =
            |node: &Self| node.length("width").is_some() && node.length("height").is_some();
        let relevant = |(name, _): &(&&str, &Value)| match **name {
            "text" | "source" if paint_only & self.paint_only_bit(name) != 0 => {
                !fixed_visual_box(self) || !fixed_visual_box(other)
            }
            name if paint_only & self.paint_only_bit(name) != 0 => false,
            "opacity" => false,
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
            "colorize" => !ignore_colorize,
            _ => true,
        };
        self.same_structure_and_state(other)
            && self
                .properties
                .iter()
                .filter(relevant)
                .eq(other.properties.iter().filter(relevant))
            && self.children.len() == other.children.len()
            && self
                .children
                .iter()
                .zip(&other.children)
                .all(|(child, other)| child.same_frame_inputs(other))
    }

    /// Compare a paint layer's content independently of its composited opacity.
    #[doc(hidden)]
    pub fn same_paint_except_opacity(&self, other: &Self) -> bool {
        let without_opacity = |(name, _): &(&&str, &Value)| **name != "opacity";
        self.shares_storage_with(other)
            || self.same_structure_and_state(other)
                && self
                    .properties
                    .iter()
                    .filter(without_opacity)
                    .eq(other.properties.iter().filter(without_opacity))
                && self.children == other.children
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
            _ => self.length(name),
        }
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
        self.number("opacity").unwrap_or(1.0).clamp(0.0, 1.0) as f32
    }

    /// Finite stacking order within an ordinary sibling list.
    pub fn z(&self) -> f64 {
        self.number("z").filter(|&z| z != 0.0).unwrap_or_default()
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

    pub fn selection_target(&self) -> SelectionTarget {
        match self.properties.get("selection-target") {
            Some(Value::SelectionTarget(target)) => *target,
            _ => SelectionTarget::Rows,
        }
    }

    pub fn is_visible(&self) -> bool {
        self.boolean("visible", true)
            && (!is_window_overlay(self.kind) || self.boolean("open", false))
    }

    #[doc(hidden)]
    pub fn handles_wheel(&self) -> bool {
        self.kind == "WheelHandler" || self.kind == "TouchArea" && self.scroll_event_handler
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
            Some(Value::Image(value)) => Some(*value),
            _ => None,
        }
    }

    pub fn image_asset(&self, name: &str) -> Option<ImageAsset> {
        match self.properties.get(name) {
            Some(Value::Image(value)) => Some((*value).into()),
            Some(Value::ImageAsset(value)) => Some(value.clone()),
            _ => None,
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
                    "TabWidget" | "SwipeView" => group_index(node),
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
        )
    }

    #[doc(hidden)]
    pub fn focus_event_target(&self, path: &[usize], event: FocusEvent) -> Option<&Self> {
        let node = if event.is_loss() {
            self.at_path(path)
        } else {
            self.interactive_at_path(path)
        };
        node.filter(|node| node.kind == "FocusScope")
    }

    #[doc(hidden)]
    pub fn key_event_target(&self, path: &[usize], phase: KeyPhase) -> Option<&Self> {
        self.interactive_at_path(path)
            .filter(|node| node.kind == "FocusScope" && node.key_handlers & phase.mask() != 0)
    }

    pub fn group_checkable(&self) -> bool {
        self.kind == "GroupBox" && self.boolean("checkable", false)
    }

    pub fn group_content_enabled(&self) -> bool {
        !self.group_checkable() || self.boolean("checked", true)
    }

    #[doc(hidden)]
    pub fn control_target(&self, path: &[usize], action: ControlAction) -> Option<&Self> {
        if action != ControlAction::Drawer(true) {
            return self.interactive_at_path(path);
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

    /// Identity of the virtual row containing this scene path.
    pub fn row_key_at(&self, path: &[usize]) -> Option<RowKey> {
        let mut node = self;
        let mut key = None;
        for &index in path {
            node = node.child_at(index)?;
            if node.kind == "ListRow" {
                key = node.routing_row_key();
            }
        }
        key
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
