//! Native desktop host. The event loop sleeps until input, resize, or OS redraw.
mod accessibility;
mod accessibility_text;
mod clipboard;
mod desktop;
#[cfg(target_os = "macos")]
mod file_drop;
mod frame;
mod local_task;
mod messages;
mod native_dialog;
mod queue;
#[cfg(test)]
mod tests;
mod window_owner;

use accessibility::{Projection, Route};
use accesskit::{Action as AccessibleAction, ActionData, ActionRequest, TreeId};
use clipboard::{ClipboardPort, ClipboardValue, EditContext, Reply as ClipboardReply};
pub use desktop::{Desktop, DesktopWindow, run_desktop};
use desktop::{DesktopGraphics, HostedWindow, WindowEvents};
use frame::FrameCache;
use local_task::LocalExecutor;
pub use local_task::{LocalTask, SpawnLocalError, TaskCancelled, spawn_local};
use messages::Inbox;
pub use messages::UiSender;
use std::{
    borrow::Cow,
    cell::RefCell,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};
use window_owner::WindowOwner;
use winit::{
    dpi::{LogicalPosition, LogicalSize},
    event::{
        DeviceId, ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, Touch, TouchPhase,
        WindowEvent,
    },
    event_loop::ActiveEventLoop,
    keyboard::{Key, ModifiersState, NamedKey},
    window::{CursorIcon, ImePurpose, Theme as NativeTheme, Window, WindowId},
};
use xrack_layout::{
    ActivationKey, ChoiceKey, ComboGeometry, ControlInput, DragUpdate, EditAction, EditSource,
    Interaction, LayoutNode, MenuKey, MenuShortcut, NavigationKey, Point, RangeInput, Rect,
    ScrollGestureKind, ShortcutKey, ShortcutModifiers, ShortcutOutcome, Size, SpinGeometry,
    SplitResize, StackTransitions, TableSelectionKey, Target, TextAreaMotion, TumblerScroll,
};
use xrack_renderer_femtovg::Renderer;
use xrack_runtime::{
    ColorScheme, Component, ControlAction, DragAction, NativeDialogQueue, NativeDialogRegistration,
    NativeDialogResult, Node, Orientation, RangeAction, RowSelection, SceneTransition,
    SecondaryWindowRegistration, SpinAction, SpinDecimal, TimerQueue, TimerRegistration,
    combo_editable, decimal_separator, is_multiline_text_input, is_spin_box, spin_editable,
    text_input_value,
};

struct WindowState {
    surface: wgpu::Surface<'static>,
    renderer: Renderer,
    config: wgpu::SurfaceConfiguration,
    accessibility: accesskit_winit::Adapter,
    projection: Projection,
    accessibility_active: bool,
    ime_purpose: ImePurpose,
    #[cfg(target_os = "macos")]
    file_drop: Option<file_drop::FileDropBridge>,
    owner: WindowOwner,
    window: Arc<Window>,
    frame: FrameCache,
}

fn color_scheme(theme: Option<NativeTheme>) -> ColorScheme {
    match theme {
        Some(NativeTheme::Light) => ColorScheme::Light,
        Some(NativeTheme::Dark) => ColorScheme::Dark,
        None => ColorScheme::System,
    }
}

impl WindowState {
    fn new(
        window: Arc<Window>,
        accessibility: accesskit_winit::Adapter,
        graphics: &mut DesktopGraphics,
        owner: Option<&Window>,
    ) -> Result<Self, String> {
        let surface = graphics
            .instance
            .create_surface(window.clone())
            .map_err(|e| e.to_string())?;
        let mut renderer = graphics.renderer(&surface)?;
        renderer.set_system_color_scheme(color_scheme(window.theme()));
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&renderer.adapter, size.width.max(1), size.height.max(1))
            .ok_or("no supported surface configuration")?;
        let caps = surface.get_capabilities(&renderer.adapter);
        // FemtoVG produces display-encoded colors; prefer a non-sRGB attachment,
        // matching the upstream WGPU integration, to avoid a second conversion.
        config.format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(caps.formats[0]);
        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&renderer.device, &config);
        let owner = WindowOwner::new(&window, owner)?;
        Ok(Self {
            surface,
            renderer,
            config,
            accessibility,
            projection: Projection::default(),
            accessibility_active: false,
            ime_purpose: ImePurpose::Normal,
            #[cfg(target_os = "macos")]
            file_drop: None,
            owner,
            window,
            frame: FrameCache::default(),
        })
    }

    fn set_visible(&self, visible: bool) {
        if visible {
            self.owner.attach();
        } else {
            self.owner.detach();
        }
        self.window.set_visible(visible);
    }
    fn viewport(&self) -> Size {
        let size = self
            .window
            .inner_size()
            .to_logical::<f32>(self.window.scale_factor());
        Size::new(size.width, size.height)
    }
    fn resize(&mut self) -> Result<(), String> {
        let size = self.window.inner_size();
        if size.width == 0 || size.height == 0 {
            return Ok(());
        }
        let limit = self.renderer.device.limits().max_texture_dimension_2d;
        if size.width > limit
            || size.height > limit
            || size.width as u64 * size.height as u64 > 64_000_000
        {
            return Err("window is larger than the supported render extent".into());
        }
        self.config.width = size.width;
        self.config.height = size.height;
        self.surface.configure(&self.renderer.device, &self.config);
        Ok(())
    }
}

pub(crate) enum UserEvent {
    PollTasks,
    ApplyMessages,
    Clipboard(ClipboardReply),
    NativeDialogFinished {
        id: u64,
        result: NativeDialogResult,
    },
    #[cfg(target_os = "macos")]
    NativeFileDrag {
        window: WindowId,
        input: xrack_layout::FileDragInput,
    },
    Accessibility(accesskit_winit::Event),
}

enum ClipboardShortcut {
    Copy,
    Cut,
    Paste,
}

#[derive(Clone, Copy)]
enum ScrollContact {
    Touch(DeviceId, u64),
    MousePan(Point),
    MouseControl(Point),
}

struct Application<C, M, R> {
    component: C,
    base_scene: Rc<Node>,
    scene: Rc<Node>,
    transition: SceneTransition,
    transition_start: Instant,
    timers: TimerQueue,
    native_dialogs: NativeDialogQueue,
    stack_transitions: StackTransitions,
    layout: Option<Rc<LayoutNode>>,
    input: Interaction,
    scroll_contact: Option<ScrollContact>,
    modifiers: ModifiersState,
    state: Option<WindowState>,
    error: Option<String>,
    tasks: Rc<LocalExecutor>,
    inbox: Inbox<M>,
    reduce: R,
    clipboard: Option<Rc<RefCell<ClipboardPort>>>,
    pending_clipboard: Option<(u64, EditContext)>,
    events: Option<WindowEvents>,
}

fn text_area_item<'a>(
    scene: &'a Node,
    layout: Option<&LayoutNode>,
    target: &Target,
) -> Option<(&'a Node, Rect)> {
    let node = scene
        .at_path(&target.path)
        .filter(|node| is_multiline_text_input(node.kind))?;
    let item = layout?.at_path(&target.path)?;
    (item.visible && item.enabled).then_some((node, item.bounds))
}

impl<C: Component, M, R: FnMut(&mut C, M)> Application<C, M, R> {
    fn new(component: C, tasks: Rc<LocalExecutor>, inbox: Inbox<M>, reduce: R) -> Self {
        let scene = component.render_shared();
        Self {
            component,
            base_scene: scene.clone(),
            scene,
            transition: SceneTransition::default(),
            transition_start: Instant::now(),
            timers: TimerQueue::default(),
            native_dialogs: NativeDialogQueue::default(),
            stack_transitions: StackTransitions::default(),
            layout: None,
            input: Interaction::default(),
            scroll_contact: None,
            modifiers: ModifiersState::default(),
            state: None,
            error: None,
            tasks,
            inbox,
            reduce,
            clipboard: None,
            pending_clipboard: None,
            events: None,
        }
    }

    fn process_user_event(&mut self, event: UserEvent) {
        let processed = match event {
            UserEvent::PollTasks => self.tasks.poll_ready(),
            UserEvent::ApplyMessages => {
                let messages = self.inbox.take_batch();
                let processed = !messages.is_empty();
                for message in messages {
                    (self.reduce)(&mut self.component, message);
                }
                processed
            }
            UserEvent::Clipboard(reply) => {
                self.finish_clipboard(reply);
                false
            }
            UserEvent::NativeDialogFinished { id, result } => {
                self.native_dialogs.complete(id, result)
            }
            #[cfg(target_os = "macos")]
            UserEvent::NativeFileDrag { window, input } => {
                if self
                    .state
                    .as_ref()
                    .is_some_and(|state| state.window.id() == window)
                    && let Some(layout) = &self.layout
                {
                    let update = self.input.file_drag_event(&self.scene, layout, input);
                    if !update.events.is_empty() {
                        self.dispatch_drag(update);
                        self.refresh_scene();
                    }
                }
                false
            }
            UserEvent::Accessibility(event) => {
                self.accessibility_event(event);
                false
            }
        };
        if processed {
            self.refresh_after_update(false);
        }
    }

    fn update_accessibility(&mut self) {
        let (Some(state), Some(layout)) = (&mut self.state, &self.layout) else {
            return;
        };
        if state.accessibility_active
            && let Some(update) = state.projection.update(
                &self.scene,
                layout,
                &self.input,
                state.window.scale_factor(),
                Some(&state.renderer.text),
            )
        {
            state.accessibility.update_if_active(|| update);
        }
    }

    fn accessibility_event(&mut self, event: accesskit_winit::Event) {
        use accesskit_winit::WindowEvent as AccessEvent;
        let Some(state) = self
            .state
            .as_mut()
            .filter(|state| state.window.id() == event.window_id)
        else {
            return;
        };
        match event.window_event {
            AccessEvent::InitialTreeRequested => {
                state.accessibility_active = true;
                state.projection.clear();
                self.update_accessibility();
            }
            AccessEvent::AccessibilityDeactivated => {
                state.accessibility_active = false;
                state.projection.clear();
            }
            AccessEvent::ActionRequested(request) if request.target_tree == TreeId::ROOT => {
                if let Some(route) = state.projection.route(request.target_node) {
                    self.accessibility_action(route, request);
                }
            }
            AccessEvent::ActionRequested(_) => {}
        }
    }

    fn accessibility_action(&mut self, route: Route, request: ActionRequest) {
        let Some(layout) = &self.layout else {
            return;
        };
        let target = match &route {
            Route::Element { target, .. }
            | Route::Choice { target, .. }
            | Route::ControlPart { target, .. } => target,
            Route::MenuEntry { trigger, entry } => {
                let activate = match request.action {
                    AccessibleAction::Focus => false,
                    AccessibleAction::Click => true,
                    AccessibleAction::Expand if entry.kind == "Menu" => true,
                    _ => return,
                };
                if let Some(action) = self.input.accessibility_menu_entry(
                    &self.scene,
                    layout,
                    trigger,
                    entry,
                    activate,
                ) {
                    self.apply_control(action);
                } else {
                    self.update_editor();
                }
                return;
            }
            Route::MenuPanel { trigger, path } => {
                if request.action == AccessibleAction::Collapse {
                    self.input
                        .accessibility_close_menu(&self.scene, layout, trigger, path);
                    self.update_editor();
                }
                return;
            }
        };
        if self
            .input
            .accessibility_target(&self.scene, layout, &target.path)
            .as_ref()
            != Some(target)
        {
            return;
        }
        let Some(node) = self.scene.at_path(&target.path) else {
            return;
        };
        if self.input.menu_open() && node.kind != "Menu" {
            return;
        }
        match route {
            Route::ControlPart { target, action } => {
                if request.action == AccessibleAction::Click
                    && let Some(item) = layout.at_path(&target.path)
                    && match action {
                        ControlAction::Calendar(action) if node.kind == "Calendar" => {
                            xrack_layout::CalendarGeometry::new(node, item.bounds)
                                .action_bounds(action)
                        }
                        ControlAction::PickTumbler { .. } if node.kind == "Tumbler" => {
                            item.tumbler.as_ref().and_then(|wheel| {
                                wheel
                                    .rows
                                    .iter()
                                    .find(|entry| entry.action() == action)
                                    .map(|entry| entry.bounds.intersect(item.bounds))
                            })
                        }
                        ControlAction::SortColumn(index) if node.kind == "TableView" => node
                            .table_header
                            .as_ref()
                            .and_then(|header| header.next_sort(index))
                            .and(item.table.as_ref())
                            .and_then(|table| {
                                table
                                    .columns
                                    .get(index)
                                    .map(|column| column.intersect(table.header))
                            }),
                        ControlAction::SubmitSearch | ControlAction::ClearSearch
                            if node.kind == "SearchField" =>
                        {
                            xrack_layout::SearchGeometry::new(item.bounds)
                                .actions(node)
                                .find_map(|(rect, candidate)| (candidate == action).then_some(rect))
                        }
                        ControlAction::SwipeDelegate(_) if node.kind == "SwipeDelegate" => item
                            .swipe_delegate
                            .as_deref()
                            .and_then(xrack_layout::SwipeDelegateGeometry::toggle)
                            .and_then(|(rect, candidate)| (candidate == action).then_some(rect)),
                        _ => None,
                    }
                    .is_some_and(|rect| {
                        let rect = rect.intersect(item.clip);
                        rect.width > 0.0 && rect.height > 0.0
                    })
                {
                    self.input.accessibility_focus(target.clone());
                    self.apply_control(ControlInput {
                        path: target.path,
                        action,
                    });
                }
            }
            Route::Choice {
                target,
                index,
                value,
                record,
            } => {
                if request.action == AccessibleAction::Click
                    && self.input.choice_is_open_for(&target.path)
                    && let Some(menu) = self.input.choice_menu(&self.scene, layout)
                    && let Some(offset) = index.checked_sub(menu.first)
                    && menu.rows.get(offset) == Some(&value)
                    && xrack_runtime::choice_identity(node, index)
                        .is_some_and(|current| current.record == record)
                {
                    self.input.dismiss_choice();
                    if node.kind != "SearchField" {
                        self.input.editor = None;
                    }
                    self.apply_control(ControlInput {
                        path: target.path,
                        action: ControlAction::Select(index),
                    });
                }
            }
            Route::Element {
                target,
                kind,
                standard_kind,
            } if kind == node.kind && standard_kind == node.standard_button_kind() => match request
                .action
            {
                AccessibleAction::Focus => {
                    self.input.accessibility_focus(target);
                    self.update_editor();
                }
                AccessibleAction::Expand | AccessibleAction::Collapse
                    if kind == "SwipeDelegate" =>
                {
                    let expand = request.action == AccessibleAction::Expand;
                    if let Some(action) = self.input.swipe_delegate_accessibility(
                        &self.scene,
                        layout,
                        &target,
                        expand,
                    ) {
                        self.input.accessibility_focus(target);
                        self.apply_control(action);
                    }
                }
                AccessibleAction::Click if kind == "DelayButton" => {
                    if let Some(action) = self.input.accessibility_delay_click(
                        &self.scene,
                        layout,
                        target,
                        Instant::now(),
                    ) {
                        self.apply_control(action);
                    } else {
                        self.update_editor();
                    }
                }
                AccessibleAction::Click if accessibility::supports_click(kind) => {
                    let focus = target
                        .path
                        .split_last()
                        .filter(|(_, parent)| {
                            kind == "ListRow"
                                && self
                                    .scene
                                    .at_path(parent)
                                    .is_some_and(|node| node.kind == "GridView")
                        })
                        .and_then(|(_, parent)| {
                            self.input.accessibility_target(&self.scene, layout, parent)
                        })
                        .unwrap_or_else(|| target.clone());
                    self.input.accessibility_focus(focus);
                    if kind == "ComboBox" {
                        if self.input.choice_is_open_for(&target.path) {
                            self.input.dismiss_choice();
                        } else {
                            self.input.open_choice(&self.scene, layout, target);
                        }
                        self.update_editor();
                    } else {
                        self.activate(Some(target));
                    }
                }
                AccessibleAction::SetValue if accessibility::supports_text_value(node) => {
                    if let Some(ActionData::Value(value)) = request.data {
                        self.input.accessibility_focus(target);
                        self.edit_text_from(EditAction::ReplaceAll(&value), EditSource::Atomic);
                    }
                }
                AccessibleAction::SetTextSelection
                    if target.focusable
                        && accessibility_text::supports_ranges(node)
                        && self
                            .input
                            .editor
                            .as_ref()
                            .is_none_or(|edit| edit.preedit.is_empty()) =>
                {
                    let value = text_input_value(node);
                    if let Some(ActionData::SetTextSelection(selection)) = request.data
                        && let Some((anchor, focus)) = self.state.as_ref().and_then(|state| {
                            state
                                .projection
                                .text_selection(request.target_node, selection, value)
                        })
                    {
                        self.pending_clipboard = None;
                        self.input.accessibility_focus(target);
                        self.input.break_text_edit_group();
                        self.input
                            .editor
                            .get_or_insert_with(|| xrack_layout::TextEdit::at_end(value))
                            .set_selection(value, anchor, focus);
                        self.update_editor();
                    }
                }
                AccessibleAction::SetValue
                    if node.kind == "DoubleSpinBox" && spin_editable(node) =>
                {
                    let value = match request.data {
                        Some(ActionData::NumericValue(value)) => SpinDecimal::new(value),
                        Some(ActionData::Value(text)) => {
                            SpinDecimal::parse(&text, decimal_separator(node))
                        }
                        _ => None,
                    };
                    if let Some(value) = value {
                        self.input.accessibility_focus(target.clone());
                        self.apply_control(ControlInput {
                            path: target.path,
                            action: ControlAction::Spin(SpinAction::CommitDecimal(value)),
                        });
                    }
                }
                AccessibleAction::ShowContextMenu
                    if target.focusable
                        && accessibility::supports_context_menu(
                            &self.scene,
                            layout,
                            &self.input,
                            &target.path,
                        ) =>
                {
                    self.input.accessibility_focus(target);
                    self.input.open_context_menu(&self.scene, layout, true);
                    self.update_editor();
                }
                AccessibleAction::Increment | AccessibleAction::Decrement
                    if matches!(kind, "Slider" | "Dial" | "RangeThumb") =>
                {
                    self.adjust_range(RangeInput {
                        path: target.path,
                        action: if request.action == AccessibleAction::Increment {
                            RangeAction::Increase
                        } else {
                            RangeAction::Decrease
                        },
                    });
                }
                AccessibleAction::Increment | AccessibleAction::Decrement if kind == "Tumbler" => {
                    let wheel = xrack_runtime::Tumbler::from_node(node);
                    let delta = if request.action == AccessibleAction::Increment {
                        1
                    } else {
                        -1
                    };
                    if let Some(action) = wheel.step(delta).and_then(|index| wheel.select(index)) {
                        self.apply_control(ControlInput {
                            path: target.path,
                            action,
                        });
                    }
                }
                AccessibleAction::Increment | AccessibleAction::Decrement
                    if is_spin_box(kind) && !node.boolean("read-only", false) =>
                {
                    self.apply_control(ControlInput {
                        path: target.path,
                        action: ControlAction::Spin(
                            if request.action == AccessibleAction::Increment {
                                SpinAction::Increase
                            } else {
                                SpinAction::Decrease
                            },
                        ),
                    });
                }
                AccessibleAction::ScrollUp
                | AccessibleAction::ScrollDown
                | AccessibleAction::ScrollLeft
                | AccessibleAction::ScrollRight
                | AccessibleAction::SetScrollOffset => {
                    let Some(item) = layout.at_path(&target.path) else {
                        return;
                    };
                    let scale = self
                        .state
                        .as_ref()
                        .map_or(1.0, |state| state.window.scale_factor());
                    let offsets = accessibility::requested_scroll(
                        request.action,
                        request.data.as_ref(),
                        node,
                        item,
                        scale,
                    );
                    let mut moved = false;
                    for (axis, offset) in [Orientation::Horizontal, Orientation::Vertical]
                        .into_iter()
                        .zip(offsets)
                    {
                        if let Some(offset) = offset {
                            moved |= self.input.accessibility_scroll_to(
                                layout,
                                &target.path,
                                axis,
                                offset,
                            );
                        }
                    }
                    if moved {
                        self.relayout_with_caret(false);
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn close_async(&self) {
        self.inbox.close();
        self.tasks.close();
    }

    fn fail(&mut self, error: String) {
        if self.error.is_none() {
            self.error = Some(error);
        }
    }
    fn prepare_frame(&mut self, elapsed: Duration) -> bool {
        let Some(state) = &mut self.state else {
            return false;
        };
        let viewport = state.viewport();
        let base = match self.transition.frame(&self.base_scene, elapsed) {
            Cow::Borrowed(_) => self.base_scene.clone(),
            Cow::Owned(frame) => Rc::new(frame),
        };
        let (scene, layout) = state.frame.prepare(base, viewport, &self.input, |base| {
            state
                .renderer
                .prepare_scene(&self.component, base, viewport, &self.input)
        });
        if self
            .layout
            .as_ref()
            .is_some_and(|previous| Rc::ptr_eq(previous, &layout))
        {
            self.scene = scene;
            return false;
        }
        if let Some(previous) = &self.layout {
            self.stack_transitions.update(
                (&self.scene, previous),
                (&scene, &layout),
                Instant::now(),
            );
            self.input.sync_stack_transitions(&self.stack_transitions);
        }
        self.input.reconcile(&scene, &layout);
        let requested = self.requested_drag_action();
        let drag = self.input.reconcile_drag(&scene, &layout, requested);
        let files = self.input.reconcile_file_drag(&scene, &layout);
        self.scene = scene;
        self.layout = Some(layout);
        #[cfg(target_os = "macos")]
        self.install_file_drop();
        self.dispatch_drag(drag);
        self.dispatch_drag(files);
        self.sync_window_effects(Instant::now());
        true
    }

    fn relayout(&mut self) {
        self.relayout_with_caret(true);
    }

    fn relayout_with_caret(&mut self, follow_caret: bool) {
        self.prepare_frame(self.transition_start.elapsed());
        self.layout_changed(follow_caret);
    }

    fn layout_changed(&mut self, follow_caret: bool) {
        if let Some(state) = &self.state {
            state.window.set_cursor(self.pointer_cursor());
        }
        self.refresh_editor(follow_caret);
    }

    fn pointer_cursor(&self) -> CursorIcon {
        if let Some(action) = self.input.drag_action() {
            match action {
                DragAction::None => CursorIcon::NoDrop,
                DragAction::Copy => CursorIcon::Copy,
                DragAction::Move => CursorIcon::Move,
                DragAction::Link => CursorIcon::Alias,
            }
        } else if let Some(orientation) = self.input.split_orientation(&self.scene) {
            match orientation {
                Orientation::Horizontal => CursorIcon::ColResize,
                Orientation::Vertical => CursorIcon::RowResize,
            }
        } else if self.input.table_resize_cursor() {
            CursorIcon::ColResize
        } else if self.input.hovered.is_some() {
            CursorIcon::Pointer
        } else {
            CursorIcon::Default
        }
    }

    fn dismiss_tooltip(&mut self) -> bool {
        let visible = self.input.dismiss_tooltip();
        if visible && let Some(state) = &self.state {
            state.window.request_redraw();
        }
        visible
    }
    fn activate(&mut self, target: Option<Target>) {
        if let Some(target) = &target
            && let Some(node) = self.scene.interactive_at_path(&target.path)
            && node.kind == "DelayButton"
        {
            if node.boolean("checked", false) {
                self.apply_control(ControlInput {
                    path: target.path.clone(),
                    action: ControlAction::DelayButton(false),
                });
            }
            return;
        }
        if let (Some(target), Some(layout)) = (target.as_ref(), self.layout.as_ref())
            && self
                .input
                .open_menu_bar(&self.scene, layout, target.clone())
        {
            self.pending_clipboard = None;
            self.update_editor();
            return;
        }
        if let (Some(target), Some(layout)) = (target.as_ref(), self.layout.as_ref())
            && let Some(action) = self.input.calendar_activate(&self.scene, layout, target)
        {
            self.apply_control(action);
            return;
        }
        if let (Some(target), Some(layout)) = (target.as_ref(), self.layout.as_ref())
            && let Some(action) = self.input.dialog_button(&self.scene, layout, target)
        {
            self.apply_control(action);
            return;
        }
        if let (Some(target), Some(layout)) = (target.as_ref(), self.layout.as_ref())
            && let Some(action) = self.input.tree_pointer_action(&self.scene, layout, target)
        {
            self.apply_control(action);
            return;
        }
        if let (Some(target), Some(layout)) = (target.as_ref(), self.layout.as_ref())
            && let Some(action) = self.input.select_group_option_with_selection(
                &self.scene,
                layout,
                target,
                self.row_selection(false),
            )
        {
            self.apply_control(action);
            return;
        }
        if let (Some(target), Some(layout)) = (target.as_ref(), self.layout.as_ref())
            && self
                .input
                .open_choice_at_pointer(&self.scene, layout, target.clone())
        {
            self.update_editor();
            return;
        }
        if let Some(target) = target
            && self
                .component
                .dispatch_at(&self.scene, &target.path, "clicked")
        {
            self.refresh_scene();
        } else if let Some(state) = &self.state {
            state.window.request_redraw();
        }
    }

    fn dialog_key(&mut self, event: &KeyEvent) -> bool {
        let Some(layout) = &self.layout else {
            return false;
        };
        if event.logical_key != Key::Named(NamedKey::Escape)
            || self
                .input
                .editor
                .as_ref()
                .is_some_and(|editor| !editor.preedit.is_empty())
        {
            return false;
        }
        if event.state == ElementState::Pressed
            && !event.repeat
            && let Some(action) = self.input.dismiss_focused_swipe(&self.scene, layout)
        {
            self.apply_control(action);
            return true;
        }
        let action = self.input.dialog_escape(&self.scene, layout);
        if layout.dialog_paths().is_empty() && action.is_none() {
            return false;
        }
        if event.state == ElementState::Pressed
            && !event.repeat
            && let Some(action) = action
        {
            self.apply_control(action);
        }
        true
    }

    fn refresh_scene(&mut self) {
        self.refresh_after_update(true);
    }

    fn refresh_dependencies(&mut self) {
        if self.component.scene_dirty()
            || self
                .state
                .as_ref()
                .is_some_and(|state| state.frame.is_dirty())
        {
            self.refresh_after_update(false);
        }
    }

    fn refresh_after_update(&mut self, redraw_input: bool) {
        let target = self.component.render_shared();
        if Rc::ptr_eq(&self.base_scene, &target) {
            if self.prepare_frame(self.transition_start.elapsed()) {
                self.layout_changed(true);
            } else if redraw_input {
                self.update_editor();
            }
            return;
        }
        let previous_width = self.base_scene.length("width");
        let previous_height = self.base_scene.length("height");
        let previous = self
            .transition
            .frame(&self.base_scene, self.transition_start.elapsed());
        self.transition = SceneTransition::between(&previous, &target);
        self.transition_start = Instant::now();
        self.base_scene = target;
        if let Some(state) = &self.state {
            state
                .window
                .set_title(self.base_scene.text("title").unwrap_or("XRack"));
            state.set_visible(self.base_scene.boolean("visible", true));
            let mut size = state
                .window
                .inner_size()
                .to_logical::<f32>(state.window.scale_factor());
            let width = self.base_scene.length("width");
            let height = self.base_scene.length("height");
            if width != previous_width || height != previous_height {
                if width != previous_width
                    && let Some(width) = width
                {
                    size.width = width.clamp(1.0, 16384.0);
                }
                if height != previous_height
                    && let Some(height) = height
                {
                    size.height = height.clamp(1.0, 16384.0);
                }
                let _ = state.window.request_inner_size(size);
            }
        }
        self.relayout();
    }

    fn update_editor(&mut self) {
        self.refresh_editor(true);
    }

    fn refresh_editor(&mut self, follow_caret: bool) {
        #[cfg(target_os = "macos")]
        self.sync_file_drop();
        self.input.sync_editor(&self.scene);
        let Some(state) = &mut self.state else {
            return;
        };
        let focused = self.input.focused.clone();
        let field = focused.as_ref().and_then(|target| {
            self.scene
                .at_path(&target.path)
                .filter(|node| self.input.is_text_editor(node, target) || spin_editable(node))
        });
        let purpose = if field.is_some_and(Node::is_password) {
            ImePurpose::Password
        } else {
            ImePurpose::Normal
        };
        if state.ime_purpose != purpose {
            state.window.set_ime_purpose(purpose);
            state.ime_purpose = purpose;
        }
        state.window.set_ime_allowed(
            field.is_some_and(|node| !node.boolean("read-only", false))
                && state.window.has_focus()
                && !self.input.menu_open(),
        );
        if let Some(node) = field
            && let Some(target) = focused.as_ref()
            && let Some(layout) = self
                .layout
                .as_ref()
                .and_then(|layout| layout.at_path(&target.path))
        {
            let bounds = if is_spin_box(node.kind) {
                SpinGeometry::new(layout.bounds).text
            } else if combo_editable(node) {
                ComboGeometry::new(layout.bounds).entry
            } else if node.kind == "SearchField" {
                xrack_layout::SearchGeometry::new(layout.bounds).entry
            } else {
                layout.bounds
            };
            if is_multiline_text_input(node.kind) {
                let (geometry, caret) =
                    state
                        .renderer
                        .text
                        .text_area_caret(node, bounds, self.input.editor.as_ref());
                let mut scroll = geometry.clamp_scroll(self.input.text_scroll(&target.path));
                if let Some(caret) = caret {
                    if follow_caret {
                        scroll = geometry.keep_visible(scroll, caret);
                    }
                    let area = geometry.area;
                    state.window.set_ime_cursor_area(
                        LogicalPosition::new(
                            area.x + (caret.x - scroll.x).clamp(0.0, area.width),
                            area.y
                                + (caret.y - scroll.y)
                                    .clamp(0.0, (area.height - geometry.line_height).max(0.0)),
                        ),
                        LogicalSize::new(1.0, geometry.line_height),
                    );
                }
                self.input.set_text_scroll(target, scroll);
                state.window.request_redraw();
                return;
            }
            let text = self.input.editor_value(node);
            let x = state.renderer.text.single_line_caret_x(
                node,
                bounds,
                &text,
                self.input.editor.as_ref(),
            );
            state.window.set_ime_cursor_area(
                LogicalPosition::new(x, bounds.y),
                LogicalSize::new(1.0, bounds.height.max(1.0)),
            );
        }
        state.window.request_redraw();
    }

    #[cfg(target_os = "macos")]
    fn install_file_drop(&mut self) {
        let Some(state) = self
            .state
            .as_mut()
            .filter(|state| state.file_drop.is_none())
        else {
            return;
        };
        fn accepts_files(node: &Node) -> bool {
            node.kind == "DropArea" && node.boolean("accept-files", false)
                || node.children.iter().any(accepts_files)
        }
        if accepts_files(&self.scene) {
            let events = self.events.as_ref().expect("window events").clone();
            match file_drop::FileDropBridge::new(&state.window, events) {
                Ok(bridge) => state.file_drop = Some(bridge),
                Err(error) => self.fail(error),
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn sync_file_drop(&self) {
        if let Some(bridge) = self
            .state
            .as_ref()
            .and_then(|state| state.file_drop.as_ref())
            && let Some(layout) = &self.layout
        {
            bridge.sync(
                self.scene.clone(),
                layout.clone(),
                self.input.file_drop_scope(layout),
            );
        }
    }

    fn place_text_caret(&mut self, point: Point, selecting: bool) {
        let Some(target) = self.input.focused.as_ref() else {
            return;
        };
        let Some(node) = self
            .scene
            .interactive_at_path(&target.path)
            .filter(|node| self.input.is_text_editor(node, target) || spin_editable(node))
        else {
            return;
        };
        let Some(item) = self
            .layout
            .as_ref()
            .and_then(|layout| layout.at_path(&target.path))
        else {
            return;
        };
        let Some(state) = &self.state else {
            return;
        };
        let value = if is_spin_box(node.kind) || matches!(node.kind, "TableCell" | "TreeCell") {
            Cow::Owned(self.input.editor_value(node).into_owned())
        } else {
            Cow::Borrowed(text_input_value(node))
        };
        let text = value.as_ref();
        let bounds = if is_spin_box(node.kind) {
            SpinGeometry::new(item.bounds).text
        } else if combo_editable(node) {
            ComboGeometry::new(item.bounds).entry
        } else if node.kind == "SearchField" {
            xrack_layout::SearchGeometry::new(item.bounds).entry
        } else {
            item.bounds
        };
        if (is_spin_box(node.kind) || combo_editable(node) || node.kind == "SearchField")
            && !bounds.contains(self.input.pointer_press_origin().unwrap_or(point))
        {
            return;
        }
        let position = if is_multiline_text_input(node.kind) {
            let geometry = state
                .renderer
                .text
                .text_area_layout(node, item.bounds, text);
            let scroll = geometry.clamp_scroll(self.input.text_scroll(&target.path));
            geometry.hit(text, point, scroll, |text| {
                state.renderer.text.advance_node(node, text)
            })
        } else {
            state.renderer.text.single_line_hit_with_text(
                node,
                bounds,
                point,
                self.input.editor.as_ref(),
                text,
            )
        };
        self.input.sync_editor(&self.scene);
        self.pending_clipboard = None;
        self.input.break_text_edit_group();
        if let Some(editor) = &mut self.input.editor {
            editor.place_caret(text, position, selecting);
        }
    }

    fn move_text_caret(&mut self, motion: TextAreaMotion, selecting: bool) {
        let Some(target) = self.input.focused.as_ref() else {
            return;
        };
        let Some((node, bounds)) = text_area_item(&self.scene, self.layout.as_deref(), target)
        else {
            return;
        };
        self.pending_clipboard = None;
        self.input.break_text_edit_group();
        if let Some(state) = &self.state
            && let Some(editor) = &mut self.input.editor
        {
            let text = node.text("text").unwrap_or("");
            let layout = state.renderer.text.text_area_layout(node, bounds, text);
            layout.move_caret(text, editor, motion, selecting, |text| {
                state.renderer.text.advance_node(node, text)
            });
            self.update_editor();
        }
    }

    fn scroll_text_area(&mut self, delta: Point) -> Point {
        let Some(target) = self.input.hovered.clone() else {
            return delta;
        };
        let Some((node, bounds)) = text_area_item(&self.scene, self.layout.as_deref(), &target)
        else {
            return delta;
        };
        if !delta.x.is_finite() || !delta.y.is_finite() {
            return delta;
        }
        let Some(state) = &self.state else {
            return delta;
        };
        let editor = self
            .input
            .editor
            .as_ref()
            .filter(|_| self.input.focused.as_ref() == Some(&target));
        let (geometry, _) = state.renderer.text.text_area_caret(node, bounds, editor);
        let old = geometry.clamp_scroll(self.input.text_scroll(&target.path));
        let scroll = geometry.clamp_scroll(Point {
            x: old.x + delta.x,
            y: old.y + delta.y,
        });
        if scroll == old {
            return delta;
        }
        self.input.set_text_scroll(&target, scroll);
        let remaining =
            |delta: f32, used: f32| (delta - used).clamp(delta.min(0.0), delta.max(0.0));
        Point {
            x: remaining(delta.x, scroll.x - old.x),
            y: remaining(delta.y, scroll.y - old.y),
        }
    }

    fn edit_text(&mut self, action: EditAction<'_>) {
        self.edit_text_from(action, EditSource::Keyboard);
    }

    fn edit_text_from(&mut self, action: EditAction<'_>, source: EditSource) {
        self.pending_clipboard = None;
        let Some(target) = self.input.focused.clone() else {
            return;
        };
        let Some(node) = self
            .scene
            .at_path(&target.path)
            .filter(|node| self.input.is_text_editor(node, &target) || spin_editable(node))
        else {
            return;
        };
        if is_spin_box(node.kind) {
            self.input.edit_spin(node, action);
            self.update_editor();
            return;
        }
        if combo_editable(node) {
            self.input.dismiss_choice();
        }
        if self.input.edit_text(
            &mut self.component,
            &self.scene,
            action,
            source,
            Instant::now(),
        ) {
            self.refresh_scene();
        } else {
            self.update_editor();
        }
    }

    fn request_clipboard(&mut self, shortcut: ClipboardShortcut) {
        self.pending_clipboard = None;
        self.input.break_text_edit_group();
        let Some((operation, context)) = EditContext::request(&self.input, &self.scene, &shortcut)
        else {
            return;
        };
        let (Some(clipboard), Some(events)) = (&self.clipboard, &self.events) else {
            return;
        };
        match clipboard.borrow_mut().request(events, operation) {
            Ok(id) if !matches!(shortcut, ClipboardShortcut::Copy) => {
                self.pending_clipboard = Some((id, context))
            }
            Ok(_) => {}
            Err(error) => eprintln!("XRack clipboard: {error}"),
        }
    }

    fn finish_clipboard(&mut self, reply: ClipboardReply) {
        let pending = if self
            .pending_clipboard
            .as_ref()
            .is_some_and(|(id, _)| *id == reply.id)
        {
            self.pending_clipboard.take()
        } else {
            None
        };
        let result = match reply.result {
            Ok(result) => result,
            Err(error) => {
                eprintln!("XRack clipboard: {error}");
                return;
            }
        };
        let Some((_, context)) = pending else {
            return;
        };
        if !context.is_current(&self.input, &self.scene) {
            return;
        }
        match result {
            ClipboardValue::Text(text) => {
                self.edit_text_from(EditAction::Insert(&text), EditSource::Atomic)
            }
            ClipboardValue::Written => self.edit_text_from(EditAction::Delete, EditSource::Atomic),
        }
    }

    fn text_shortcut(&mut self, key: &str, shift: bool, repeat: bool) -> bool {
        let clipboard = if key.eq_ignore_ascii_case("c") {
            Some(ClipboardShortcut::Copy)
        } else if key.eq_ignore_ascii_case("x") {
            Some(ClipboardShortcut::Cut)
        } else if key.eq_ignore_ascii_case("v") {
            Some(ClipboardShortcut::Paste)
        } else {
            None
        };
        if let Some(shortcut) = clipboard {
            if !repeat {
                self.request_clipboard(shortcut);
            }
            return true;
        }
        if key.eq_ignore_ascii_case("z") || key.eq_ignore_ascii_case("y") {
            self.pending_clipboard = None;
            let redo = shift || key.eq_ignore_ascii_case("y");
            if self
                .input
                .replay_text_edit(&mut self.component, &self.scene, redo)
            {
                self.refresh_scene();
            }
            return true;
        }
        false
    }

    fn text_key(&mut self, event: &KeyEvent) -> bool {
        let Some(target) = self.input.focused.clone() else {
            return false;
        };
        let Some(node) = self
            .scene
            .at_path(&target.path)
            .filter(|node| self.input.is_text_editor(node, &target) || spin_editable(node))
        else {
            return false;
        };
        let spin = is_spin_box(node.kind);
        if event.state != ElementState::Pressed
            || self
                .input
                .editor
                .as_ref()
                .is_some_and(|edit| !edit.preedit.is_empty())
        {
            return true;
        }
        if combo_editable(node)
            && event.logical_key == Key::Named(NamedKey::ArrowRight)
            && self.modifiers.is_empty()
            && !event.repeat
            && self
                .input
                .accept_combo_completion(&mut self.component, &self.scene, Instant::now())
        {
            self.pending_clipboard = None;
            self.refresh_scene();
            return true;
        }
        let shift = self.modifiers.shift_key();
        let shortcut = (self.modifiers.control_key() || self.modifiers.super_key())
            && !self.modifiers.alt_key();
        let multiline = is_multiline_text_input(node.kind);
        if shortcut
            && let Key::Character(key) = &event.logical_key
            && self.text_shortcut(key, shift, event.repeat)
        {
            return true;
        }
        if multiline
            && let Some(motion) = match event.logical_key {
                Key::Named(NamedKey::ArrowUp) => Some(TextAreaMotion::Up),
                Key::Named(NamedKey::ArrowDown) => Some(TextAreaMotion::Down),
                Key::Named(NamedKey::Home) if !shortcut => Some(TextAreaMotion::Home),
                Key::Named(NamedKey::End) if !shortcut => Some(TextAreaMotion::End),
                Key::Named(NamedKey::PageUp) => Some(TextAreaMotion::PageUp),
                Key::Named(NamedKey::PageDown) => Some(TextAreaMotion::PageDown),
                _ => None,
            }
        {
            self.move_text_caret(motion, shift);
            return true;
        }
        let action = match &event.logical_key {
            Key::Named(NamedKey::Backspace) => Some(EditAction::Backspace),
            Key::Named(NamedKey::Delete) => Some(EditAction::Delete),
            Key::Named(NamedKey::ArrowLeft) => Some(EditAction::Left(shift)),
            Key::Named(NamedKey::ArrowRight) => Some(EditAction::Right(shift)),
            Key::Named(NamedKey::Home) => Some(EditAction::Home(shift)),
            Key::Named(NamedKey::End) => Some(EditAction::End(shift)),
            Key::Character(key) if shortcut && key.eq_ignore_ascii_case("a") => {
                Some(EditAction::SelectAll)
            }
            Key::Named(NamedKey::Escape) if spin => {
                self.input.discard_spin_draft();
                self.update_editor();
                None
            }
            Key::Named(NamedKey::Enter) if spin => {
                self.commit_spin();
                self.update_editor();
                None
            }
            Key::Named(NamedKey::Enter) if multiline => Some(EditAction::Insert("\n")),
            Key::Named(NamedKey::Enter) => {
                if !event.repeat
                    && self
                        .scene
                        .at_path(&target.path)
                        .is_some_and(Node::accepts_text_submission)
                {
                    if self
                        .component
                        .dispatch_at(&self.scene, &target.path, "accepted")
                    {
                        self.refresh_scene();
                    } else if let Some(action) = self
                        .layout
                        .as_ref()
                        .and_then(|layout| self.input.dialog_accept(&self.scene, layout))
                    {
                        self.apply_control(action);
                    }
                }
                None
            }
            _ if !shortcut => event.text.as_deref().map(EditAction::Insert),
            _ => None,
        };
        if let Some(action) = action {
            self.edit_text(action);
        }
        true
    }
    fn apply_control(&mut self, input: ControlInput) {
        if matches!(input.action, ControlAction::Spin(_)) {
            self.input.discard_spin_draft();
        }
        let search = self
            .scene
            .at_path(&input.path)
            .is_some_and(|node| node.kind == "SearchField");
        let changed = match input.action {
            ControlAction::Select(index) if search => {
                self.pending_clipboard = None;
                self.input.select_search_suggestion(
                    &mut self.component,
                    &self.scene,
                    index,
                    Instant::now(),
                )
            }
            ControlAction::ClearSearch if search => {
                self.pending_clipboard = None;
                self.input.edit_text(
                    &mut self.component,
                    &self.scene,
                    EditAction::ReplaceAll(""),
                    EditSource::Atomic,
                    Instant::now(),
                )
            }
            action => self.component.control_at(&self.scene, &input.path, action),
        };
        if changed {
            self.refresh_scene();
        } else if self
            .scene
            .at_path(&input.path)
            .is_some_and(|node| node.list.is_some() || node.kind == "TabBar")
        {
            self.relayout();
        } else {
            self.update_editor();
        }
    }

    fn commit_spin(&mut self) {
        if let Some(input) = self.input.commit_spin_draft() {
            self.apply_control(input);
        }
    }

    fn apply_split_resize(&mut self, input: SplitResize) {
        if self
            .component
            .resize_split_at(&self.scene, &input.path, &input.weights)
        {
            self.refresh_scene();
        }
    }

    fn commit_table_edit(&mut self) {
        self.pending_clipboard = None;
        if self
            .input
            .commit_table_edit(&mut self.component, &self.scene)
        {
            self.refresh_scene();
        } else {
            self.update_editor();
        }
    }

    fn table_edit_key(&mut self, event: &KeyEvent) -> bool {
        let active = self
            .input
            .focused
            .as_ref()
            .is_some_and(|target| self.input.table_cell_editing(&target.path));
        if active
            && matches!(
                event.logical_key,
                Key::Named(NamedKey::Escape | NamedKey::Enter)
            )
        {
            if event.state == ElementState::Pressed
                && !event.repeat
                && self
                    .input
                    .editor
                    .as_ref()
                    .is_none_or(|editor| editor.preedit.is_empty())
            {
                if event.logical_key == Key::Named(NamedKey::Enter) {
                    self.commit_table_edit();
                } else {
                    self.pending_clipboard = None;
                    self.input.cancel_table_edit();
                    self.update_editor();
                }
            }
            return true;
        }
        let start = match event.logical_key {
            Key::Named(NamedKey::F2) => true,
            Key::Named(NamedKey::Enter) => self
                .input
                .focused
                .as_ref()
                .and_then(|target| self.scene.at_path(&target.path))
                .is_some_and(|node| matches!(node.kind, "TableView" | "TableCell")),
            _ => false,
        };
        if start
            && event.state == ElementState::Pressed
            && !event.repeat
            && self
                .layout
                .as_ref()
                .is_some_and(|layout| self.input.begin_table_edit(&self.scene, layout))
        {
            self.pending_clipboard = None;
            self.update_editor();
            return true;
        }
        false
    }

    fn spin_key(&mut self, event: &KeyEvent) -> bool {
        let Some(target) = self.input.focused.as_ref() else {
            return false;
        };
        let Some(node) = self
            .scene
            .interactive_at_path(&target.path)
            .filter(|node| is_spin_box(node.kind))
        else {
            return false;
        };
        let action = match event.logical_key {
            Key::Named(NamedKey::ArrowUp) => SpinAction::Increase,
            Key::Named(NamedKey::ArrowDown) => SpinAction::Decrease,
            Key::Named(NamedKey::Home) if !spin_editable(node) => SpinAction::Minimum,
            Key::Named(NamedKey::End) if !spin_editable(node) => SpinAction::Maximum,
            _ => return false,
        };
        if event.state == ElementState::Pressed
            && !node.boolean("read-only", false)
            && !self
                .input
                .editor
                .as_ref()
                .is_some_and(|editor| !editor.preedit.is_empty())
        {
            self.apply_control(ControlInput {
                path: target.path.clone(),
                action: ControlAction::Spin(action),
            });
        }
        true
    }

    fn context_menu_key(&mut self, event: &KeyEvent) -> bool {
        let Some(layout) = &self.layout else {
            return false;
        };
        if event.state == ElementState::Pressed
            && !event.repeat
            && (event.logical_key == Key::Named(NamedKey::ContextMenu)
                || event.logical_key == Key::Named(NamedKey::F10)
                    && self.modifiers == ModifiersState::SHIFT)
            && self.input.open_context_menu(&self.scene, layout, true)
        {
            self.pending_clipboard = None;
            self.update_editor();
            return true;
        }
        false
    }

    fn shortcut_key(&mut self, event: &KeyEvent) -> bool {
        if event.state != ElementState::Pressed {
            return false;
        }
        let Some(layout) = &self.layout else {
            return false;
        };
        // Native desktop backends expose the base key of the current keyboard layout.
        #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
        let key = {
            use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
            event.key_without_modifiers()
        };
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        let key = event.logical_key.clone();
        let key = match key {
            Key::Character(text) => {
                let mut chars = text.chars();
                let Some(ch) = chars.next().filter(|_| chars.next().is_none()) else {
                    self.input.cancel_shortcut_sequence();
                    return false;
                };
                ShortcutKey::Character(ch.to_ascii_uppercase())
            }
            Key::Named(NamedKey::F1) => ShortcutKey::Function(1),
            Key::Named(NamedKey::F2) => ShortcutKey::Function(2),
            Key::Named(NamedKey::F3) => ShortcutKey::Function(3),
            Key::Named(NamedKey::F4) => ShortcutKey::Function(4),
            Key::Named(NamedKey::F5) => ShortcutKey::Function(5),
            Key::Named(NamedKey::F6) => ShortcutKey::Function(6),
            Key::Named(NamedKey::F7) => ShortcutKey::Function(7),
            Key::Named(NamedKey::F8) => ShortcutKey::Function(8),
            Key::Named(NamedKey::F9) => ShortcutKey::Function(9),
            Key::Named(NamedKey::F10) => ShortcutKey::Function(10),
            Key::Named(NamedKey::F11) => ShortcutKey::Function(11),
            Key::Named(NamedKey::F12) => ShortcutKey::Function(12),
            Key::Named(
                NamedKey::Control
                | NamedKey::Alt
                | NamedKey::AltGraph
                | NamedKey::Shift
                | NamedKey::Super
                | NamedKey::Meta
                | NamedKey::Hyper,
            ) => return false,
            _ => {
                self.input.cancel_shortcut_sequence();
                return false;
            }
        };
        let modifiers = ShortcutModifiers {
            ctrl: self.modifiers.control_key(),
            alt: self.modifiers.alt_key(),
            shift: self.modifiers.shift_key(),
            super_key: self.modifiers.super_key(),
        };
        if self.input.editor.is_some() && !self.input.menu_open() {
            let editing_shortcut = (modifiers.ctrl || modifiers.super_key)
                && !modifiers.alt
                && matches!(
                    key,
                    ShortcutKey::Character('A' | 'C' | 'V' | 'X' | 'Y' | 'Z')
                );
            // Some layouts report AltGr as Ctrl+Alt while producing ordinary text.
            let text_entry = modifiers.ctrl
                && modifiers.alt
                && event
                    .text
                    .as_ref()
                    .is_some_and(|text| text.chars().any(|ch| !ch.is_control()));
            if editing_shortcut || text_entry {
                self.input.cancel_shortcut_sequence();
                return false;
            }
        }
        let Some(key) = MenuShortcut::new(key, modifiers) else {
            self.input.cancel_shortcut_sequence();
            return false;
        };
        match self
            .input
            .shortcut_press(&self.scene, layout, key, event.repeat, Instant::now())
        {
            ShortcutOutcome::Unmatched => false,
            ShortcutOutcome::Consumed => true,
            ShortcutOutcome::Activate(action) => {
                self.pending_clipboard = None;
                self.input.close_menu(&self.scene, layout);
                self.apply_control(action);
                true
            }
            ShortcutOutcome::Ambiguous(actions) => {
                let mut handled = false;
                for action in actions {
                    handled |= self
                        .component
                        .control_at(&self.scene, &action.path, action.action);
                }
                if handled {
                    self.refresh_scene();
                }
                true
            }
        }
    }

    fn menu_key(&mut self, event: &KeyEvent) -> bool {
        let Some(layout) = &self.layout else {
            return false;
        };
        let open = self.input.menu_open();
        if event.state != ElementState::Pressed {
            return open;
        }
        let key = match event.logical_key {
            Key::Named(NamedKey::ArrowUp) => MenuKey::Up,
            Key::Named(NamedKey::ArrowDown) => MenuKey::Down,
            Key::Named(NamedKey::ArrowLeft) => MenuKey::Left,
            Key::Named(NamedKey::ArrowRight) => MenuKey::Right,
            Key::Named(NamedKey::Home) => MenuKey::Home,
            Key::Named(NamedKey::End) => MenuKey::End,
            Key::Named(NamedKey::Enter | NamedKey::Space) => MenuKey::Accept,
            Key::Named(NamedKey::Escape) => MenuKey::Escape,
            _ => return open,
        };
        let header = self
            .input
            .focused
            .as_ref()
            .and_then(|target| self.scene.at_path(&target.path))
            .is_some_and(|node| node.kind == "Menu");
        if !open && (!header || matches!(key, MenuKey::Escape | MenuKey::Home | MenuKey::End)) {
            return false;
        }
        self.pending_clipboard = None;
        if let Some(action) = self.input.menu_key(&self.scene, layout, key) {
            self.apply_control(action);
        } else {
            self.update_editor();
        }
        true
    }

    fn choice_key(&mut self, event: &KeyEvent) -> bool {
        let open = self.input.choice_open();
        let focused = self
            .input
            .focused
            .as_ref()
            .and_then(|target| self.scene.at_path(&target.path));
        let search = focused.is_some_and(|node| node.kind == "SearchField");
        let editing = search || focused.is_some_and(combo_editable);
        if event.state != ElementState::Pressed {
            return open;
        }
        if editing
            && self
                .input
                .editor
                .as_ref()
                .is_some_and(|edit| !edit.preedit.is_empty())
        {
            return false;
        }
        if !open
            && event.logical_key == Key::Named(NamedKey::Escape)
            && self.input.dismiss_combo_completion(&self.scene)
        {
            self.update_editor();
            return true;
        }
        if search
            && matches!(
                event.logical_key,
                Key::Named(NamedKey::Home | NamedKey::End)
            )
            && (self.modifiers.shift_key()
                || self.modifiers.control_key()
                || self.modifiers.super_key()
                || self
                    .layout
                    .as_ref()
                    .and_then(|layout| self.input.choice_menu(&self.scene, layout))
                    .is_none_or(|menu| menu.highlighted.is_none()))
        {
            return false;
        }
        let key = match event.logical_key {
            Key::Named(NamedKey::F4) if !event.repeat => ChoiceKey::Open,
            Key::Named(NamedKey::ArrowDown) if self.modifiers.alt_key() => {
                if event.repeat {
                    return open;
                }
                ChoiceKey::Open
            }
            Key::Named(NamedKey::ArrowUp) => ChoiceKey::Up,
            Key::Named(NamedKey::ArrowDown) => ChoiceKey::Down,
            Key::Named(NamedKey::Home) => ChoiceKey::Home,
            Key::Named(NamedKey::End) => ChoiceKey::End,
            Key::Named(NamedKey::PageUp) => ChoiceKey::PageUp,
            Key::Named(NamedKey::PageDown) => ChoiceKey::PageDown,
            Key::Named(NamedKey::Enter) => ChoiceKey::Accept,
            Key::Named(NamedKey::Space) if !editing => ChoiceKey::Accept,
            Key::Named(NamedKey::Escape) => ChoiceKey::Escape,
            _ if open
                && editing
                && (event.text.is_some()
                    || matches!(
                        event.logical_key,
                        Key::Named(
                            NamedKey::Backspace
                                | NamedKey::Delete
                                | NamedKey::ArrowLeft
                                | NamedKey::ArrowRight
                        )
                    )) =>
            {
                if !search {
                    self.input.dismiss_choice();
                    self.update_editor();
                }
                return false;
            }
            _ => return open && !search,
        };
        if let Some(input) = self
            .layout
            .as_ref()
            .and_then(|layout| self.input.choice_key(&self.scene, layout, key))
        {
            if !event.repeat
                || !matches!(
                    input.action,
                    ControlAction::AcceptComboText | ControlAction::SubmitSearch
                )
            {
                self.apply_control(input);
            }
            true
        } else if open || self.input.choice_open() {
            self.update_editor();
            true
        } else {
            false
        }
    }

    fn adjust_range(&mut self, input: RangeInput) {
        if self
            .component
            .adjust_range_at(&self.scene, &input.path, input.action)
        {
            self.refresh_scene();
        } else if let Some(state) = &self.state {
            state.window.request_redraw();
        }
    }

    fn navigation_key(&mut self, event: &KeyEvent) -> bool {
        if event.logical_key == Key::Named(NamedKey::BrowserBack)
            || self.modifiers.alt_key() && event.logical_key == Key::Named(NamedKey::ArrowLeft)
        {
            return event.state == ElementState::Pressed && !event.repeat && self.back_in_stack();
        }
        let key = match event.logical_key {
            Key::Named(NamedKey::ArrowLeft) => NavigationKey::Left,
            Key::Named(NamedKey::ArrowRight) => NavigationKey::Right,
            Key::Named(NamedKey::ArrowUp) => NavigationKey::Up,
            Key::Named(NamedKey::ArrowDown) => NavigationKey::Down,
            Key::Named(NamedKey::Home) => NavigationKey::Home,
            Key::Named(NamedKey::End) => NavigationKey::End,
            Key::Named(NamedKey::PageUp) => NavigationKey::PageUp,
            Key::Named(NamedKey::PageDown) => NavigationKey::PageDown,
            _ => return false,
        };
        if event.state == ElementState::Pressed
            && let Some(input) = self
                .layout
                .as_ref()
                .and_then(|layout| self.input.split_key(&self.scene, layout, key))
        {
            self.apply_split_resize(input);
            return true;
        }
        if let Some(input) = self.layout.as_ref().and_then(|layout| {
            self.input.slider_key(
                &self.scene,
                layout,
                key,
                event.state == ElementState::Pressed,
            )
        }) {
            self.adjust_range(input);
            return true;
        }
        let operation = self.row_selection(true);
        if event.state == ElementState::Pressed
            && let Some(input) = self.layout.as_ref().and_then(|layout| {
                self.input
                    .tree_key_with_selection(&self.scene, layout, key, operation)
                    .or_else(|| {
                        self.input
                            .list_key_with_selection(&self.scene, layout, key, operation)
                    })
                    .or_else(|| self.input.group_key(&self.scene, layout, key))
                    .or_else(|| self.input.page_indicator_key(&self.scene, layout, key))
                    .or_else(|| self.input.calendar_key(&self.scene, layout, key))
                    .or_else(|| self.input.tumbler_key(&self.scene, layout, key))
                    .or_else(|| self.input.swipe_delegate_key(&self.scene, layout, key))
                    .or_else(|| self.input.swipe_key(&self.scene, layout, key))
            })
        {
            self.apply_control(input);
            return true;
        }
        false
    }

    fn back_in_stack(&mut self) -> bool {
        let Some(action) = self
            .layout
            .as_ref()
            .and_then(|layout| self.input.stack_back(&self.scene, layout))
        else {
            return false;
        };
        self.apply_control(action);
        true
    }

    fn tree_toggle_key(&mut self, event: &KeyEvent) -> bool {
        if event.state != ElementState::Pressed
            || event.repeat
            || !matches!(
                event.logical_key,
                Key::Named(NamedKey::Enter | NamedKey::Space)
            )
        {
            return false;
        }
        let Some(action) = self
            .layout
            .as_ref()
            .and_then(|layout| self.input.tree_toggle(&self.scene, layout))
        else {
            return false;
        };
        self.apply_control(action);
        true
    }

    fn row_selection(&self, keyboard: bool) -> RowSelection {
        let additive = self.modifiers.control_key() || self.modifiers.super_key();
        match (self.modifiers.shift_key(), additive) {
            (true, true) => RowSelection::AddRange,
            (true, false) => RowSelection::Extend,
            (false, true) if keyboard => RowSelection::Move,
            (false, true) => RowSelection::Toggle,
            (false, false) => RowSelection::Replace,
        }
    }

    fn selection_key(&mut self, event: &KeyEvent) -> bool {
        let additive = self.modifiers.control_key() || self.modifiers.super_key();
        let key = match &event.logical_key {
            Key::Named(NamedKey::Space) => TableSelectionKey::Toggle,
            Key::Named(NamedKey::Escape) => TableSelectionKey::Clear,
            Key::Character(key)
                if key.eq_ignore_ascii_case("a")
                    && additive
                    && !self.modifiers.alt_key()
                    && !self.modifiers.shift_key() =>
            {
                TableSelectionKey::All
            }
            _ => return false,
        };
        let input = self
            .input
            .table_selection_key(&self.scene, key)
            .or_else(|| {
                if key == TableSelectionKey::Toggle && !additive {
                    return None;
                }
                self.input
                    .tree_selection_key(&self.scene, self.layout.as_ref()?, key)
            });
        let Some(input) = input else {
            return false;
        };
        if event.state == ElementState::Pressed && !event.repeat {
            self.apply_control(input);
        }
        true
    }

    fn left_pointer(&mut self, state: ElementState) {
        if state == ElementState::Released
            && let Some(point) = self.input.drag_position()
            && self.finish_drag(point)
        {
            return;
        }
        if state == ElementState::Pressed {
            self.pending_clipboard = None;
            if self.layout.as_ref().is_some_and(|layout| {
                self.input.menu_press(&self.scene, layout)
                    || self.input.choice_press(&self.scene, layout)
            }) {
                self.update_editor();
                return;
            }
            if self.input.pointer_on_stack_slide() {
                return;
            }
            if let Some(action) = self
                .layout
                .as_ref()
                .and_then(|layout| self.input.popup_outside_press(&self.scene, layout))
            {
                self.apply_control(action);
            }
            if let Some(moved) = self
                .layout
                .as_ref()
                .and_then(|layout| self.input.scroll_bar_press(&self.scene, layout))
            {
                if moved {
                    self.relayout_with_caret(false);
                } else {
                    self.refresh_editor(false);
                }
                return;
            }
            if self
                .layout
                .as_ref()
                .is_some_and(|layout| self.input.dialog_pointer_press(layout))
            {
                self.update_editor();
                return;
            }
            if self.input.focused != self.input.hovered {
                self.commit_spin();
            }
            self.input.pointer_press();
            if let Some(point) = self.input.pointer_position() {
                if let Some(layout) = &self.layout {
                    self.input.arm_drag(&self.scene, layout, point);
                }
                self.place_text_caret(point, self.modifiers.shift_key());
            }
            if let Some(layout) = &self.layout {
                self.input
                    .begin_pointer_delay(&self.scene, layout, Instant::now());
                self.input.split_press(&self.scene, layout);
                self.input.spin_press(&self.scene, layout);
            }
            if let Some(input) = self
                .layout
                .as_ref()
                .and_then(|layout| self.input.slider_press(&self.scene, layout))
            {
                self.adjust_range(input);
            }
            self.update_editor();
        } else if self.input.finish_scroll_bar() {
            self.refresh_editor(false);
        } else if self.input.menu_open() {
            if let Some(action) = self
                .layout
                .as_ref()
                .and_then(|layout| self.input.menu_release(&self.scene, layout))
            {
                self.apply_control(action);
            } else {
                self.update_editor();
            }
        } else if self.input.choice_open() {
            if let Some(input) = self
                .layout
                .as_ref()
                .and_then(|layout| self.input.choice_release(&self.scene, layout))
            {
                self.apply_control(input);
            } else {
                self.update_editor();
            }
        } else if let Some(action) = self
            .layout
            .as_ref()
            .and_then(|layout| self.input.dialog_pointer_release(&self.scene, layout))
        {
            self.apply_control(action);
        } else if let Some(input) = self
            .layout
            .as_ref()
            .and_then(|layout| self.input.spin_release(&self.scene, layout))
        {
            self.apply_control(input);
        } else if let Some(input) = self
            .layout
            .as_ref()
            .and_then(|layout| self.input.slider_release(&self.scene, layout))
        {
            self.adjust_range(input);
        } else if self.input.finish_split_resize() || self.input.finish_table_resize() {
            self.layout_changed(true);
        } else if let Some(input) = self
            .layout
            .as_ref()
            .and_then(|layout| self.input.control_part_release(&self.scene, layout))
        {
            self.apply_control(input);
        } else if let Some(input) = self.input.table_header_release(
            &self.scene,
            self.row_selection(false),
            self.modifiers.alt_key(),
        ) {
            self.apply_control(input);
        } else {
            let target = self.input.pointer_release_target();
            let edit_target = if self.row_selection(false) == RowSelection::Replace {
                target.as_ref()
            } else {
                None
            };
            let editing = self.layout.as_ref().is_some_and(|layout| {
                self.input
                    .table_cell_click(&self.scene, layout, edit_target, Instant::now())
            });
            self.activate(target);
            if editing {
                self.update_editor();
            }
        }
    }

    fn begin_direct_scroll(&mut self, contact: ScrollContact, point: Point, now: Instant) {
        let kind = match contact {
            ScrollContact::Touch(..) => ScrollGestureKind::Touch,
            ScrollContact::MousePan(_) => ScrollGestureKind::MousePan,
            ScrollContact::MouseControl(_) => ScrollGestureKind::MouseControl,
        };
        if self.layout.as_ref().is_some_and(|layout| {
            self.input
                .begin_scroll_gesture(&self.scene, layout, point, now, kind)
        }) {
            self.scroll_contact = Some(contact);
            self.pending_clipboard = None;
            self.layout_changed(false);
        }
    }

    fn requested_drag_action(&self) -> Option<DragAction> {
        match (self.modifiers.control_key(), self.modifiers.shift_key()) {
            (false, false) => None,
            (true, false) => Some(DragAction::Copy),
            (false, true) => Some(DragAction::Move),
            (true, true) => Some(DragAction::Link),
        }
    }

    fn dispatch_drag(&mut self, update: DragUpdate) -> bool {
        let mut handled = false;
        for event in update.events {
            handled |= event.apply(&mut self.component, &self.scene);
        }
        handled
    }

    fn apply_drag(&mut self, update: DragUpdate) -> bool {
        let captured = update.captured;
        if self.dispatch_drag(update) {
            self.refresh_scene();
        }
        if captured && let Some(state) = &self.state {
            state.window.set_cursor(self.pointer_cursor());
            state.window.request_redraw();
        }
        captured
    }

    fn move_drag(&mut self, point: Point) -> bool {
        let requested = self.requested_drag_action();
        let Some(layout) = &self.layout else {
            return false;
        };
        let update = self
            .input
            .drag_motion(&self.scene, layout, point, requested);
        self.apply_drag(update)
    }

    fn finish_drag(&mut self, point: Point) -> bool {
        let requested = self.requested_drag_action();
        let update = if let Some(layout) = &self.layout {
            self.input
                .finish_drag(&self.scene, layout, point, requested)
        } else {
            self.input.cancel_drag()
        };
        self.apply_drag(update)
    }

    fn cancel_drag(&mut self) -> bool {
        let update = self.input.cancel_drag();
        self.apply_drag(update)
    }

    fn move_direct_scroll(&mut self, point: Point, now: Instant) {
        if self.layout.as_ref().is_some_and(|layout| {
            self.input
                .move_scroll_gesture(&self.scene, layout, point, now)
        }) {
            self.relayout_with_caret(false);
        }
    }

    fn end_direct_scroll(&mut self, point: Point, now: Instant) {
        let contact = self.scroll_contact.take();
        let Some(layout) = &self.layout else {
            self.input.cancel_scroll_gesture();
            return;
        };
        let outcome = self
            .input
            .end_scroll_gesture(&self.scene, layout, point, now);
        if outcome.scrolled {
            self.relayout_with_caret(false);
        }
        if let Some(control) = outcome.control {
            self.apply_control(control);
        }
        if (outcome.tap
            || matches!(
                contact,
                Some(ScrollContact::MousePan(_) | ScrollContact::MouseControl(_))
            ))
            && let Some(layout) = &self.layout
        {
            self.input.pointer_move(&self.scene, layout, point);
        }
        if outcome.tap {
            self.left_pointer(ElementState::Pressed);
            self.left_pointer(ElementState::Released);
            self.input.pointer_leave();
        }
        self.layout_changed(false);
    }

    fn cancel_direct_scroll(&mut self) {
        self.scroll_contact = None;
        let preview = self.input.tumbler_preview().is_some() || self.input.has_swipe_preview();
        self.input.cancel_scroll_gesture();
        if preview {
            self.relayout_with_caret(false);
        }
    }

    fn touch_event(&mut self, touch: Touch) {
        let Some(state) = &self.state else {
            return;
        };
        let point = touch
            .location
            .to_logical::<f32>(state.window.scale_factor());
        let point = Point {
            x: point.x,
            y: point.y,
        };
        self.update_touch(
            touch.device_id,
            touch.id,
            touch.phase,
            point,
            Instant::now(),
        );
    }

    fn update_touch(
        &mut self,
        device: DeviceId,
        id: u64,
        phase: TouchPhase,
        point: Point,
        now: Instant,
    ) {
        let owned = matches!(
            self.scroll_contact,
            Some(ScrollContact::Touch(current_device, current_id)) if current_device == device && current_id == id
        );
        match phase {
            TouchPhase::Started
                if self.scroll_contact.is_none() && self.input.drag_position().is_none() =>
            {
                self.begin_direct_scroll(ScrollContact::Touch(device, id), point, now);
                if self.scroll_contact.is_some()
                    && let Some(layout) = &self.layout
                {
                    self.input.arm_drag(&self.scene, layout, point);
                }
            }
            TouchPhase::Moved if owned => {
                if !self.move_drag(point) {
                    self.move_direct_scroll(point, now);
                }
            }
            TouchPhase::Ended if owned => {
                if self.finish_drag(point) {
                    self.scroll_contact = None;
                } else {
                    self.end_direct_scroll(point, now);
                }
            }
            TouchPhase::Cancelled if owned => {
                self.cancel_drag();
                self.cancel_direct_scroll();
            }
            _ => {}
        }
    }

    fn tick_delay(&mut self, now: Instant) {
        let pending = self.input.delay_deadline().is_some();
        if let Some(action) = self
            .layout
            .as_ref()
            .and_then(|layout| self.input.advance_delay(&self.scene, layout, now))
        {
            self.apply_control(action);
        } else if pending
            && self.input.delay_deadline().is_none()
            && let Some(state) = &self.state
        {
            state.window.request_redraw();
        }
    }

    fn sync_window_effects(&mut self, now: Instant) {
        fn collect<'a>(
            node: &'a Node,
            layout: &LayoutNode,
            timers: &mut Vec<&'a TimerRegistration>,
            dialogs: &mut Vec<&'a NativeDialogRegistration>,
        ) {
            if !layout.visible {
                return;
            }
            timers.extend(&node.timers);
            if layout.enabled {
                dialogs.extend(&node.native_dialogs);
            }
            for (child, item) in node.children.iter().zip(&layout.children) {
                collect(child, item, timers, dialogs);
            }
        }
        let mut timers = Vec::new();
        let mut dialogs = Vec::new();
        if let Some(layout) = &self.layout {
            collect(&self.scene, layout, &mut timers, &mut dialogs);
        }
        self.timers.sync(now, timers);
        self.native_dialogs.sync(&dialogs);
    }

    fn present_native_dialog(&mut self) {
        let (Some(state), Some(events)) = (&self.state, &self.events) else {
            return;
        };
        let Some(request) = self.native_dialogs.begin_next() else {
            return;
        };
        let window = state.window.clone();
        let events = events.clone();
        let id = request.id;
        if self
            .tasks
            .spawn(async move {
                let result = if request.is_abandoned() {
                    NativeDialogResult::Abandoned
                } else {
                    native_dialog::present(&window, &request.options).await
                };
                events.post(UserEvent::NativeDialogFinished { id, result });
            })
            .is_err()
        {
            self.process_user_event(UserEvent::NativeDialogFinished {
                id,
                result: NativeDialogResult::Failed("The UI executor is closed.".into()),
            });
        }
    }

    fn tick_timers(&mut self, now: Instant) {
        for _ in 0..64 {
            let Some(delivery) = self.timers.next_due(now) else {
                break;
            };
            if delivery.fire() {
                // A callback may stop another due timer or remove its owning subtree.
                self.refresh_after_update(false);
            }
        }
    }

    fn redraw(&mut self) -> Result<(), String> {
        self.tick_delay(Instant::now());
        let animating = self.transition.running(Duration::ZERO);
        let elapsed = self.transition_start.elapsed();
        if self.layout.as_ref().is_some_and(|layout| {
            self.input
                .advance_scroll_motion(&self.scene, layout, Instant::now())
        }) {
            self.prepare_frame(elapsed);
            self.layout_changed(false);
        } else if animating || self.layout.is_none() {
            self.prepare_frame(elapsed);
        }
        let Some(state) = &mut self.state else {
            return Ok(());
        };
        let size = state.window.inner_size();
        if size.width == 0 || size.height == 0 {
            return Ok(());
        }
        let (frame, suboptimal) = match state.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => (frame, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => (frame, true),
            wgpu::CurrentSurfaceTexture::Timeout => {
                state.window.request_redraw();
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Occluded => return Ok(()),
            wgpu::CurrentSurfaceTexture::Outdated => {
                state.resize()?;
                self.relayout();
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                state.surface = state
                    .renderer
                    .instance
                    .create_surface(state.window.clone())
                    .map_err(|e| e.to_string())?;
                state.resize()?;
                self.relayout();
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err("GPU surface validation failed".into());
            }
        };
        self.stack_transitions.advance(Instant::now());
        let layout = self
            .layout
            .as_ref()
            .expect("layout prepared for the window");
        let pointer_changed = self.input.sync_stack_transitions(&self.stack_transitions);
        if pointer_changed {
            self.input.reconcile(&self.scene, layout);
        }
        state.renderer.draw_with_transitions(
            &self.scene,
            layout,
            &frame.texture,
            state.window.scale_factor() as f32,
            &self.input,
            &self.stack_transitions,
        )?;
        if animating {
            if self.transition.running(elapsed) {
                state.window.request_redraw();
            } else {
                self.transition = SceneTransition::default();
            }
        }
        state.window.pre_present_notify();
        state.renderer.queue.present(frame);
        let completion = state.renderer.take_painted_combo_completion();
        self.input
            .set_painted_combo_completion(completion.filter(|_| state.window.has_focus()));
        if state.renderer.needs_redraw()
            || self.input.scroll_motion_active()
            || self.stack_transitions.is_active()
        {
            state.window.request_redraw();
        }
        if suboptimal {
            state.resize()?;
            self.relayout();
        } else if pointer_changed && let Some(state) = &self.state {
            state.window.set_cursor(self.pointer_cursor());
        }
        Ok(())
    }
}

impl<C: Component, M, R: FnMut(&mut C, M)> HostedWindow for Application<C, M, R> {
    fn user_event(&mut self, event: UserEvent) {
        let _scope = self.tasks.enter();
        self.process_user_event(event);
    }

    fn close(&mut self) {
        if let Some(state) = &self.state {
            state.set_visible(false);
        }
        self.close_async();
        self.component.take_window_close_request();
        self.native_dialogs.close();
        self.suspend();
    }

    fn request_close(&mut self) -> bool {
        let _scope = self.tasks.enter();
        self.component.take_window_close_request();
        let accepted = self.component.close_requested();
        // Synchronous requests made during the decision belong to that decision.
        self.component.take_window_close_request();
        if !accepted {
            self.refresh_after_update(false);
        }
        accepted
    }

    fn take_close_request(&mut self) -> bool {
        self.component.take_window_close_request()
    }

    fn window_id(&self) -> Option<WindowId> {
        self.state.as_ref().map(|state| state.window.id())
    }

    fn native_window(&self) -> Option<Arc<Window>> {
        self.state.as_ref().map(|state| state.window.clone())
    }

    fn secondary_windows(&self) -> Option<Rc<[SecondaryWindowRegistration]>> {
        if self.state.is_some() && self.base_scene.boolean("visible", true) {
            self.component.secondary_windows()
        } else {
            None
        }
    }

    fn take_error(&mut self) -> Option<String> {
        self.error.take()
    }

    fn resume(
        &mut self,
        event_loop: &ActiveEventLoop,
        graphics: &mut DesktopGraphics,
        owner: Option<&Window>,
    ) -> Result<WindowId, String> {
        let _scope = self.tasks.enter();
        if let Some(id) = self.window_id() {
            return Ok(id);
        }
        let attrs = Window::default_attributes()
            .with_title(self.base_scene.text("title").unwrap_or("XRack"))
            .with_visible(false)
            .with_inner_size(LogicalSize::new(
                self.base_scene
                    .length("width")
                    .unwrap_or(640.0)
                    .clamp(1.0, 16384.0),
                self.base_scene
                    .length("height")
                    .unwrap_or(480.0)
                    .clamp(1.0, 16384.0),
            ));
        let window = event_loop
            .create_window(window_owner::attributes(attrs, owner)?)
            .map(Arc::new)
            .map_err(|e| e.to_string())?;
        let adapter = self
            .events
            .as_ref()
            .expect("window events")
            .accessibility(event_loop, &window);
        let state = WindowState::new(window, adapter, graphics, owner)?;
        let id = state.window.id();
        state.set_visible(self.base_scene.boolean("visible", true));
        self.state = Some(state);
        self.relayout();
        Ok(id)
    }
    fn suspend(&mut self) {
        self.cancel_direct_scroll();
        self.pending_clipboard = None;
        self.input.dismiss_tooltip();
        self.input.dismiss_choice();
        self.input.dismiss_menu();
        self.input.discard_spin_draft();
        self.input.cancel_press();
        self.input.pointer_leave();
        self.input.clear_file_drag();
        self.layout = None;
        self.timers = TimerQueue::default();
        self.native_dialogs.sync(&[]);
        self.state = None;
    }
    fn tick(&mut self, now: Instant) {
        let _scope = self.tasks.enter();
        self.refresh_dependencies();
        self.tick_delay(now);
        self.tick_timers(now);
    }

    fn refresh_deadline(&mut self, now: Instant) -> Option<Instant> {
        let _scope = self.tasks.enter();
        self.refresh_dependencies();
        self.present_native_dialog();
        self.update_accessibility();
        let deadline = if let Some(state) = &self.state
            && state.window.has_focus()
            && let Some(layout) = &self.layout
        {
            if self.input.update_tooltip(&self.scene, layout, now) {
                state.window.request_redraw();
            }
            self.input.tooltip_deadline()
        } else {
            None
        };
        [
            deadline,
            self.input.delay_deadline(),
            self.timers.deadline(),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    fn window_event(&mut self, id: WindowId, event: WindowEvent) {
        let _scope = self.tasks.enter();
        let Some(state) = self.state.as_mut().filter(|state| state.window.id() == id) else {
            return;
        };
        state.accessibility.process_event(&state.window, &event);
        match event {
            WindowEvent::ThemeChanged(theme) => {
                if state
                    .renderer
                    .set_system_color_scheme(color_scheme(Some(theme)))
                    && self.base_scene.color_scheme() == Some(ColorScheme::System)
                {
                    state.window.request_redraw();
                }
            }
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                if let Err(error) = self.state.as_mut().unwrap().resize() {
                    self.fail(error);
                    return;
                }
                self.relayout();
            }
            WindowEvent::RedrawRequested => {
                if let Err(error) = self.redraw() {
                    self.fail(error);
                }
            }
            WindowEvent::Touch(touch) => self.touch_event(touch),
            WindowEvent::CursorMoved { position, .. } => {
                let state = self.state.as_ref().unwrap();
                let p = position.to_logical::<f32>(state.window.scale_factor());
                let point = Point { x: p.x, y: p.y };
                match self.scroll_contact {
                    Some(ScrollContact::Touch(..)) => return,
                    Some(ScrollContact::MousePan(_) | ScrollContact::MouseControl(_)) => {
                        self.scroll_contact = self.scroll_contact.map(|contact| {
                            if matches!(contact, ScrollContact::MousePan(_)) {
                                ScrollContact::MousePan(point)
                            } else {
                                ScrollContact::MouseControl(point)
                            }
                        });
                        self.move_direct_scroll(point, Instant::now());
                        return;
                    }
                    None => {}
                }
                if self.move_drag(point) {
                    return;
                }
                let state = self.state.as_ref().unwrap();
                if let Some(layout) = &self.layout
                    && self.input.pointer_move(&self.scene, layout, point)
                {
                    state.window.set_cursor(self.pointer_cursor());
                    state.window.request_redraw();
                }
                if let Some(moved) = self
                    .layout
                    .as_ref()
                    .and_then(|layout| self.input.scroll_bar_drag(&self.scene, layout))
                {
                    if moved {
                        self.relayout_with_caret(false);
                    }
                    return;
                }
                if let Some(input) = self
                    .layout
                    .as_ref()
                    .and_then(|layout| self.input.split_drag(&self.scene, layout))
                {
                    self.apply_split_resize(input);
                    return;
                }
                if let Some(input) = self
                    .layout
                    .as_ref()
                    .and_then(|layout| self.input.table_resize_drag(&self.scene, layout))
                {
                    self.apply_control(input);
                    return;
                }
                if self.input.pressed.is_some()
                    && self.input.pressed == self.input.focused
                    && self.input.focused.as_ref().is_some_and(|target| {
                        self.scene
                            .at_path(&target.path)
                            .is_some_and(|node| self.input.is_text_editor(node, target))
                    })
                {
                    self.place_text_caret(point, true);
                    self.update_editor();
                }
                if let Some(input) = self
                    .layout
                    .as_ref()
                    .and_then(|layout| self.input.slider_drag(&self.scene, layout))
                {
                    self.adjust_range(input);
                }
            }
            WindowEvent::CursorLeft { .. } => {
                if matches!(self.scroll_contact, Some(ScrollContact::Touch(..))) {
                    return;
                }
                self.cancel_drag();
                self.input.pointer_leave();
                let window = &self.state.as_ref().unwrap().window;
                window.set_cursor(self.pointer_cursor());
                window.request_redraw();
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if matches!(self.scroll_contact, Some(ScrollContact::Touch(..))) {
                    return;
                }
                self.cancel_direct_scroll();
                if self.input.cancel_delay() {
                    self.update_editor();
                }
                self.dismiss_tooltip();
                let scale = self.state.as_ref().unwrap().window.scale_factor();
                let lines = matches!(delta, MouseScrollDelta::LineDelta(..));
                let delta = match delta {
                    MouseScrollDelta::LineDelta(x, y) => Point {
                        x: -x * 28.0,
                        y: -y * 28.0,
                    },
                    MouseScrollDelta::PixelDelta(position) => Point {
                        x: -(position.x / scale) as f32,
                        y: -(position.y / scale) as f32,
                    },
                };
                let rows = if delta.y == 0.0 {
                    0
                } else {
                    delta.y.signum() as isize * ((delta.y.abs() / 28.0).round() as isize).max(1)
                };
                if self.layout.as_ref().is_some_and(|layout| {
                    self.input.menu_scroll(&self.scene, layout, rows)
                        || self.input.choice_scroll(&self.scene, layout, rows)
                }) {
                    self.update_editor();
                } else if !self.input.pointer_on_stack_slide() {
                    if !self.modifiers.shift_key()
                        && delta.y.abs() > delta.x.abs()
                        && let Some(layout) = &self.layout
                    {
                        let distance = if lines { delta.y / 28.0 } else { delta.y };
                        match self
                            .input
                            .tumbler_scroll(&self.scene, layout, distance, lines)
                        {
                            TumblerScroll::Select(action) => {
                                self.apply_control(action);
                                return;
                            }
                            TumblerScroll::Pending => return,
                            TumblerScroll::Unhandled => {}
                        }
                    }
                    let remaining = self.scroll_text_area(delta);
                    if self.layout.as_ref().is_some_and(|layout| {
                        self.input.scroll_wheel(
                            layout,
                            remaining,
                            self.modifiers.shift_key() && delta.x == 0.0,
                        )
                    }) {
                        self.relayout_with_caret(false);
                    } else if remaining != delta {
                        self.refresh_editor(false);
                    }
                }
            }
            WindowEvent::MouseInput {
                button: MouseButton::Right,
                state: ElementState::Released,
                ..
            } => {
                if self.input.pointer_on_stack_slide()
                    || matches!(self.scroll_contact, Some(ScrollContact::Touch(..)))
                {
                    return;
                }
                self.cancel_direct_scroll();
                self.dismiss_tooltip();
                if self
                    .layout
                    .as_ref()
                    .is_some_and(|layout| self.input.open_context_menu(&self.scene, layout, false))
                {
                    self.pending_clipboard = None;
                    self.update_editor();
                }
            }
            WindowEvent::MouseInput {
                button: MouseButton::Back,
                state: ElementState::Released,
                ..
            } => {
                self.cancel_direct_scroll();
                self.back_in_stack();
            }
            WindowEvent::MouseInput {
                button: MouseButton::Middle,
                state,
                ..
            } => match (state, self.scroll_contact) {
                (ElementState::Pressed, None) if self.input.drag_position().is_none() => {
                    if let Some(point) = self.input.pointer_position() {
                        self.begin_direct_scroll(
                            ScrollContact::MousePan(point),
                            point,
                            Instant::now(),
                        );
                    }
                }
                (ElementState::Released, Some(ScrollContact::MousePan(point))) => {
                    self.end_direct_scroll(point, Instant::now());
                }
                _ => {}
            },
            WindowEvent::MouseInput {
                button: MouseButton::Left,
                state,
                ..
            } => match (state, self.scroll_contact) {
                (_, Some(ScrollContact::Touch(..))) => {}
                (ElementState::Released, Some(ScrollContact::MouseControl(point))) => {
                    self.end_direct_scroll(point, Instant::now());
                }
                (ElementState::Pressed, None) => {
                    if let Some(point) = self.input.pointer_position() {
                        self.begin_direct_scroll(
                            ScrollContact::MouseControl(point),
                            point,
                            Instant::now(),
                        );
                    }
                    if self.scroll_contact.is_none() {
                        self.left_pointer(state);
                    }
                }
                _ => {
                    self.cancel_direct_scroll();
                    self.left_pointer(state);
                }
            },
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
                if self.input.drag_action().is_some()
                    && let Some(point) = self.input.drag_position()
                {
                    self.move_drag(point);
                }
            }
            WindowEvent::KeyboardInput {
                event,
                is_synthetic,
                ..
            } => {
                if event.state == ElementState::Pressed
                    && matches!(
                        event.logical_key,
                        Key::Named(NamedKey::Tab | NamedKey::Escape)
                    )
                {
                    self.input.cancel_shortcut_sequence();
                }
                if event.state == ElementState::Pressed
                    && event.logical_key == Key::Named(NamedKey::Escape)
                    && self.cancel_drag()
                {
                    return;
                }
                if event.state == ElementState::Pressed
                    && event.logical_key == Key::Named(NamedKey::Escape)
                    && self.input.cancel_delay()
                {
                    self.cancel_direct_scroll();
                    self.update_editor();
                    return;
                }
                self.cancel_direct_scroll();
                if event.state == ElementState::Pressed
                    && event.logical_key != Key::Named(NamedKey::Tab)
                {
                    let dismissed = self.dismiss_tooltip();
                    if dismissed && event.logical_key == Key::Named(NamedKey::Escape) {
                        return;
                    }
                }
                if event.logical_key == Key::Named(NamedKey::Tab)
                    && event.state == ElementState::Pressed
                    && !event.repeat
                {
                    self.pending_clipboard = None;
                    self.commit_spin();
                    if self
                        .input
                        .focused
                        .as_ref()
                        .is_some_and(|target| self.input.table_cell_editing(&target.path))
                    {
                        if self
                            .input
                            .editor
                            .as_ref()
                            .is_none_or(|editor| editor.preedit.is_empty())
                        {
                            self.commit_table_edit();
                        } else {
                            self.input.cancel_table_edit();
                        }
                    }
                    self.input.dismiss_choice();
                    self.input.dismiss_menu();
                    if self.layout.as_ref().is_some_and(|layout| {
                        self.input
                            .focus_next(&self.scene, layout, self.modifiers.shift_key())
                    }) {
                        self.relayout();
                    } else {
                        self.update_editor();
                    }
                } else if !self.context_menu_key(&event)
                    && (is_synthetic || !self.shortcut_key(&event))
                    && !self.menu_key(&event)
                    && !self.choice_key(&event)
                    && !self.table_edit_key(&event)
                    && !self.selection_key(&event)
                    && !self.tree_toggle_key(&event)
                    && !self.dialog_key(&event)
                    && !self.spin_key(&event)
                    && !self.text_key(&event)
                    && !self.navigation_key(&event)
                    && !event.repeat
                {
                    let key = match event.logical_key {
                        Key::Named(NamedKey::Enter) => Some(ActivationKey::Enter),
                        Key::Named(NamedKey::Space) => Some(ActivationKey::Space),
                        _ => None,
                    };
                    if let Some(key) = key {
                        if event.state == ElementState::Pressed {
                            self.input.key_press_with_scene(&self.scene, key);
                            if let Some(layout) = &self.layout {
                                self.input.begin_key_delay(
                                    &self.scene,
                                    layout,
                                    key,
                                    Instant::now(),
                                );
                            }
                            self.state.as_ref().unwrap().window.request_redraw();
                        } else {
                            let target = self.input.key_release_with_scene(&self.scene, key);
                            self.activate(target);
                        }
                    }
                }
            }
            WindowEvent::Ime(Ime::Commit(value)) if !self.input.menu_open() => {
                self.edit_text_from(EditAction::Insert(&value), EditSource::Atomic)
            }
            WindowEvent::Ime(Ime::Preedit(value, cursor)) if !self.input.menu_open() => {
                self.pending_clipboard = None;
                if !value.is_empty() {
                    self.input.cancel_shortcut_sequence();
                }
                if !value.is_empty() && self.input.editor.is_some() {
                    self.input.dismiss_choice();
                }
                if let Some(editor) = &mut self.input.editor {
                    editor.set_preedit(value, cursor);
                }
                self.update_editor();
            }
            WindowEvent::Ime(Ime::Disabled) => {
                if let Some(editor) = &mut self.input.editor {
                    editor.set_preedit(String::new(), None);
                }
                self.state.as_ref().unwrap().window.request_redraw();
            }
            WindowEvent::Focused(focused) => {
                if !focused {
                    self.cancel_drag();
                    self.input.cancel_shortcut_sequence();
                    self.input.set_painted_combo_completion(None);
                    self.cancel_direct_scroll();
                    self.dismiss_tooltip();
                    self.pending_clipboard = None;
                    self.commit_spin();
                    self.input.cancel_table_edit();
                    self.input.dismiss_choice();
                    self.input.dismiss_menu();
                    self.input.cancel_press();
                    self.input.pointer_leave();
                    if let Some(editor) = &mut self.input.editor {
                        editor.set_preedit(String::new(), None);
                    }
                }
                self.update_editor();
            }
            WindowEvent::Occluded(occluded) => {
                if occluded {
                    self.cancel_drag();
                    self.input.set_painted_combo_completion(None);
                    self.cancel_direct_scroll();
                } else {
                    self.state.as_ref().unwrap().window.request_redraw();
                }
            }
            _ => {}
        }
    }
}

/// Open a native window and run a generated component on the main thread.
pub fn run<C: Component + 'static>(component: C) -> Result<(), String> {
    run_with_messages(component, |_, _: UiSender<()>| |_: &mut C, _: ()| {})
}

/// Configure a component with a mailbox, then run its reducer on the UI thread.
/// The setup closure can also start local tasks before the first window event.
pub fn run_with_messages<C, M, R>(
    component: C,
    setup: impl FnOnce(&mut C, UiSender<M>) -> R,
) -> Result<(), String>
where
    C: Component + 'static,
    M: Send + 'static,
    R: FnMut(&mut C, M),
{
    desktop::run_window(component, setup)
}
