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
mod scale_rotate;
mod system_tray;
#[cfg(target_os = "linux")]
mod system_tray_linux;
#[cfg(target_os = "macos")]
mod system_tray_macos;
#[cfg(target_os = "windows")]
mod system_tray_windows;
#[cfg(test)]
mod tests;
mod window_chrome;
mod window_owner;
mod window_presentation;

use accessibility::{Projection, Route};
use accesskit::{Action as AccessibleAction, ActionData, ActionRequest, TreeId};
use clipboard::{ClipboardPort, ClipboardValue, EditContext, Reply as ClipboardReply};
pub use desktop::{Desktop, DesktopTray, DesktopWindow, run_desktop};
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
use window_chrome::{WindowChrome, resize_direction};
use window_owner::WindowOwner;
use window_presentation::WindowPresentation;
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
    FocusObservers, Interaction, LayoutNode, MenuKey, MenuShortcut, NavigationKey, Point,
    RangeInput, Rect, ScrollBarUpdate, ScrollGestureKind, ShortcutKey, ShortcutModifiers,
    ShortcutOutcome, Size, SpinGeometry, SplitResize, StackTransitions, TablePasteDestination,
    TableSelectionKey, Target, TextAreaMotion, TumblerScroll, ViewportObservers,
};
use xrack_renderer_femtovg::Renderer;
use xrack_runtime::{
    ColorScheme, Component, ControlAction, DragAction, EventResult, FocusRequests,
    ImagePlaybackQueue, ImagePlaybackRegistration, KeyPhase, NativeDialogQueue,
    NativeDialogRegistration, NativeDialogResult, Node, Orientation, RangeAction, RowSelection,
    SceneTransition, ScrollRequests, SecondaryWindowRegistration, SpinAction, SpinDecimal,
    TablePaste, TextCommand, TimerQueue, TimerRegistration, combo_editable, decimal_separator,
    is_multiline_text_input, is_spin_box, spin_editable, text_input_value,
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
    #[cfg(target_os = "macos")]
    resize_drag: Option<window_chrome::ResizeDrag>,
    owner: WindowOwner,
    window: Arc<Window>,
    frame: FrameCache,
    presentation: WindowPresentation,
}

fn activation_key_value(key: &Key) -> Option<ActivationKey> {
    match key {
        Key::Named(NamedKey::Enter) => Some(ActivationKey::Enter),
        Key::Named(NamedKey::Space) => Some(ActivationKey::Space),
        _ => None,
    }
}

fn navigation_key_value(key: &Key) -> Option<NavigationKey> {
    Some(match key {
        Key::Named(NamedKey::ArrowLeft) => NavigationKey::Left,
        Key::Named(NamedKey::ArrowRight) => NavigationKey::Right,
        Key::Named(NamedKey::ArrowUp) => NavigationKey::Up,
        Key::Named(NamedKey::ArrowDown) => NavigationKey::Down,
        Key::Named(NamedKey::Home) => NavigationKey::Home,
        Key::Named(NamedKey::End) => NavigationKey::End,
        Key::Named(NamedKey::PageUp) => NavigationKey::PageUp,
        Key::Named(NamedKey::PageDown) => NavigationKey::PageDown,
        _ => return None,
    })
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
            #[cfg(target_os = "macos")]
            resize_drag: None,
            owner,
            window,
            frame: FrameCache::default(),
            presentation: WindowPresentation::default(),
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

    fn begin_client_resize(&mut self, edge: xrack_layout::WindowResizeEdge) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.resize_drag = window_chrome::ResizeDrag::start(&self.window, edge);
            self.resize_drag.is_some()
        }
        #[cfg(not(target_os = "macos"))]
        {
            self.window
                .drag_resize_window(resize_direction(edge))
                .is_ok()
        }
    }

    #[cfg(target_os = "macos")]
    fn update_client_resize(&mut self, allowed: bool) -> bool {
        if !allowed {
            self.finish_client_resize();
        }
        if let Some(drag) = &self.resize_drag {
            drag.update();
            true
        } else {
            false
        }
    }

    fn finish_client_resize(&mut self) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.resize_drag.take().is_some()
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }

    fn sample_window_status(&mut self) -> xrack_runtime::WindowStatus {
        #[cfg(target_os = "macos")]
        if self.resize_drag.is_some() {
            return self.presentation.observed();
        }
        self.presentation.sample(&self.window)
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

enum ClipboardContext {
    Edit(EditContext),
    Table {
        destination: TablePasteDestination,
        apply: TablePaste,
    },
}

#[derive(Clone, Copy)]
enum ScrollContact {
    Touch(DeviceId, u64),
    MousePan(Point),
    MouseControl(Point),
    MouseViewport(Point),
}

struct Application<C, M, R> {
    component: C,
    base_scene: Rc<Node>,
    scene: Rc<Node>,
    transition: SceneTransition,
    transition_epoch: Instant,
    animation_wake: Option<Instant>,
    timers: TimerQueue,
    image_playback: ImagePlaybackQueue,
    native_dialogs: NativeDialogQueue,
    stack_transitions: StackTransitions,
    layout: Option<Rc<LayoutNode>>,
    input: Interaction,
    scale_rotate: scale_rotate::Gestures,
    focus_requests: FocusRequests,
    focus_observers: FocusObservers,
    scroll_requests: ScrollRequests,
    viewport_observers: ViewportObservers,
    focus_active: bool,
    scroll_contact: Option<ScrollContact>,
    modifiers: ModifiersState,
    state: Option<WindowState>,
    error: Option<String>,
    tasks: Rc<LocalExecutor>,
    inbox: Inbox<M>,
    reduce: R,
    clipboard: Option<Rc<RefCell<ClipboardPort>>>,
    pending_clipboard: Option<(u64, ClipboardContext)>,
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
            transition_epoch: Instant::now(),
            animation_wake: None,
            timers: TimerQueue::default(),
            image_playback: ImagePlaybackQueue::default(),
            native_dialogs: NativeDialogQueue::default(),
            stack_transitions: StackTransitions::default(),
            layout: None,
            input: Interaction::default(),
            scale_rotate: scale_rotate::Gestures::default(),
            focus_requests: FocusRequests::default(),
            focus_observers: FocusObservers::default(),
            scroll_requests: ScrollRequests::default(),
            viewport_observers: ViewportObservers::default(),
            focus_active: false,
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
            Route::TextLink(target) => {
                let Some(action) = self.input.text_link_activation(&self.scene, layout, target)
                else {
                    return;
                };
                match request.action {
                    AccessibleAction::Focus => {
                        self.input.focus_target(&self.scene, layout, target.clone());
                        self.update_editor();
                    }
                    AccessibleAction::Click => {
                        self.input.focus_target(&self.scene, layout, target.clone());
                        self.apply_control(action);
                    }
                    _ => {}
                }
                return;
            }
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
            Route::MenuPanel {
                trigger,
                path,
                session,
            } => {
                if request.action == AccessibleAction::Collapse {
                    self.input.accessibility_close_menu(
                        &self.scene,
                        layout,
                        trigger,
                        path,
                        *session,
                    );
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
                    && menu.row_enabled(offset)
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
                    self.input.focus_target(&self.scene, layout, target);
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
                        && self
                            .input
                            .supports_context_menu(&self.scene, layout, &target) =>
                {
                    self.input.accessibility_focus(target);
                    self.input.open_context_menu(&self.scene, layout, true);
                    self.pending_clipboard = None;
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
                | AccessibleAction::SetScrollOffset
                | AccessibleAction::SetValue
                | AccessibleAction::Increment
                | AccessibleAction::Decrement => {
                    if kind == "ScrollBar" && node.scroll_target.is_none() {
                        if let Some(action) = accessibility::requested_standalone_scroll(
                            request.action,
                            request.data.as_ref(),
                            node,
                        ) {
                            self.adjust_range(RangeInput {
                                path: target.path,
                                action,
                            });
                        }
                        return;
                    }
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
        let elastic_input = self
            .input
            .has_overscroll()
            .then(|| (base.clone(), self.input.overscroll_frame()));
        let (mut scene, mut layout) = state.frame.prepare(base, viewport, &self.input, |base| {
            state
                .renderer
                .prepare_scene(&self.component, base, viewport, &self.input)
        });
        if self
            .layout
            .as_ref()
            .is_some_and(|previous| Rc::ptr_eq(previous, &layout))
        {
            let cancelled = self.scale_rotate.reconcile(&scale_rotate::Context {
                scene: &scene,
                layout: &layout,
                input: &self.input,
            });
            let previous = std::mem::replace(&mut self.scene, scene);
            let swiped = self.flush_swipe_events(Some(&previous));
            if let Some(event) = cancelled {
                self.dispatch_scale_rotate(&previous, event);
                return true;
            }
            return swiped;
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
        if let Some((base, offsets)) = elastic_input
            && offsets != self.input.overscroll_frame()
        {
            let state = self.state.as_mut().unwrap();
            (scene, layout) = state.frame.prepare(base, viewport, &self.input, |base| {
                state
                    .renderer
                    .prepare_scene(&self.component, base, viewport, &self.input)
            });
            self.input.reconcile(&scene, &layout);
        }
        let requested = self.requested_drag_action();
        let drag = self.input.reconcile_drag(&scene, &layout, requested);
        let files = self.input.reconcile_file_drag(&scene, &layout);
        let cancelled_gesture = self
            .scale_rotate
            .reconcile(&scale_rotate::Context {
                scene: &scene,
                layout: &layout,
                input: &self.input,
            })
            .map(|event| (self.scene.clone(), event));
        if self.scroll_requests.is_empty() {
            self.viewport_observers.sync(&scene, &layout);
        }
        let previous = std::mem::replace(&mut self.scene, scene);
        self.layout = Some(layout);
        self.flush_swipe_events(Some(&previous));
        #[cfg(target_os = "macos")]
        self.install_file_drop();
        self.dispatch_drag(drag);
        self.dispatch_drag(files);
        if let Some((previous, event)) = cancelled_gesture {
            self.dispatch_scale_rotate(&previous, event);
        }
        self.sync_window_effects(Instant::now());
        true
    }

    fn relayout(&mut self) {
        self.relayout_with_caret(true);
    }

    fn relayout_with_caret(&mut self, follow_caret: bool) {
        self.prepare_frame(self.transition_epoch.elapsed());
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
        } else if self.input.table_row_resize_cursor() {
            CursorIcon::RowResize
        } else if self.input.table_resize_cursor() {
            CursorIcon::ColResize
        } else if let Some(edge) = self.window_resize_cursor() {
            resize_direction(edge).into()
        } else if self.input.hovered.is_some() {
            CursorIcon::Pointer
        } else {
            CursorIcon::Default
        }
    }

    fn window_resize_cursor(&self) -> Option<xrack_layout::WindowResizeEdge> {
        let state = self.state.as_ref()?;
        #[cfg(target_os = "macos")]
        if let Some(drag) = &state.resize_drag {
            return Some(drag.edge());
        }
        if state.presentation.observed().maximized
            || state.presentation.observed().full_screen
            || state.window.fullscreen().is_some()
            || state.presentation.is_switching_full_screen()
        {
            return None;
        }
        self.input
            .window_resize_edge(&self.scene, self.layout.as_ref()?)
    }

    fn dismiss_tooltip(&mut self) -> bool {
        let visible = self.input.dismiss_tooltip();
        if visible && let Some(state) = &self.state {
            state.window.request_redraw();
        }
        visible
    }
    fn activate(&mut self, target: Option<Target>) {
        if let (Some(target), Some(layout)) = (target.as_ref(), self.layout.as_ref())
            && let Some(selection) = self.input.table_delegate_selection(
                &self.scene,
                layout,
                target,
                self.row_selection(false),
            )
        {
            self.apply_control(selection);
            // A selection handler can sort, replace, or remove the clicked row.
            if self.scene.row_key_at(&target.path) != target.row_key {
                return;
            }
        }
        if let Some(target) = target
            .as_ref()
            .filter(|target| target.text_link_index().is_some())
        {
            if let Some(action) = self
                .layout
                .as_ref()
                .and_then(|layout| self.input.text_link_activation(&self.scene, layout, target))
            {
                self.apply_control(action);
            }
            return;
        }
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
            || self.input.has_pending_swipe_events()
            || !self.focus_requests.is_empty()
            || !self.scroll_requests.is_empty()
            || self
                .state
                .as_ref()
                .is_some_and(|state| state.frame.is_dirty())
        {
            self.refresh_after_update(false);
        }
    }

    fn refresh_after_update(&mut self, redraw_input: bool) {
        self.flush_swipe_events(None);
        if self.sync_base_scene() {
            self.relayout();
        } else if self.prepare_frame(self.transition_epoch.elapsed()) {
            self.layout_changed(true);
        } else if redraw_input
            || !self.focus_requests.is_empty()
            || !self.scroll_requests.is_empty()
        {
            self.update_editor();
        }
    }

    fn sync_base_scene(&mut self) -> bool {
        let target = self.component.render_shared();
        if Rc::ptr_eq(&self.base_scene, &target) {
            return false;
        }
        let previous_width = self.base_scene.length("width");
        let previous_height = self.base_scene.length("height");
        let was_visible = self.base_scene.boolean("visible", true);
        let previous_chrome = WindowChrome::read(&self.base_scene);
        self.transition
            .retarget(&self.base_scene, &target, self.transition_epoch.elapsed());
        self.animation_wake = None;
        self.base_scene = target;
        if let Some(state) = &mut self.state {
            state
                .window
                .set_title(self.base_scene.text("title").unwrap_or("XRack"));
            let visible = self.base_scene.boolean("visible", true);
            if visible != was_visible {
                state.set_visible(visible);
            }
            let chrome = WindowChrome::read(&self.base_scene);
            let width = self.base_scene.length("width");
            let height = self.base_scene.length("height");
            let size_changed = width != previous_width || height != previous_height;
            if chrome != previous_chrome || size_changed || !visible {
                state.finish_client_resize();
                self.input.cancel_window_move();
            }
            chrome.apply(previous_chrome, &state.window);
            let mut size = state
                .window
                .inner_size()
                .to_logical::<f64>(state.window.scale_factor());
            if size_changed {
                if width != previous_width
                    && let Some(width) = width.filter(|width| width.is_finite())
                {
                    size.width = f64::from(width);
                }
                if height != previous_height
                    && let Some(height) = height.filter(|height| height.is_finite())
                {
                    size.height = f64::from(height);
                }
                let _ = state.window.request_inner_size(chrome.constrain(size));
            }
        }
        true
    }

    fn apply_window_presentation(&mut self, now: Instant) -> Option<Instant> {
        let state = self.state.as_mut()?;
        let commands = self.component.take_window_commands();
        #[cfg(target_os = "macos")]
        if commands.is_empty() && state.resize_drag.is_some() {
            return None;
        }
        if !commands.is_empty() {
            state.finish_client_resize();
            self.input.cancel_window_move();
        }
        let status = state.presentation.apply(&state.window, commands, now);
        self.component.publish_window_status(status);
        state.presentation.deadline(now)
    }

    fn update_editor(&mut self) {
        self.refresh_editor(true);
    }

    fn publish_focus(&mut self) {
        let Some(layout) = &self.layout else {
            return;
        };
        let events = self
            .focus_observers
            .sync(&self.input, &self.scene, layout, self.focus_active);
        if events.is_empty() {
            return;
        }
        for event in events {
            event.apply(&mut self.component, &self.scene);
        }
        if (self.component.scene_dirty()
            || !self.focus_requests.is_empty()
            || !self.scroll_requests.is_empty())
            && let Some(state) = &self.state
        {
            state.window.request_redraw();
        }
    }

    fn refresh_editor(&mut self, follow_caret: bool) {
        self.flush_swipe_events(None);
        // Capture the result of input before applying queued programmatic moves.
        let scrolled = self.layout.as_ref().map_or_else(Vec::new, |layout| {
            self.input.take_scroll_events(&self.scene, layout)
        });
        let scroll = self.scroll_requests.drain();
        if !scroll.is_empty()
            && let Some(layout) = &self.layout
        {
            if self
                .input
                .apply_scroll_requests(&self.scene, layout, scroll)
            {
                self.prepare_frame(self.transition_epoch.elapsed());
            } else {
                self.viewport_observers.sync(&self.scene, layout);
            }
        }
        self.apply_focus_and_text_requests();
        if let Some(layout) = &self.layout {
            self.input.remember_focus_scope(&self.scene, layout);
        }
        self.publish_focus();
        for event in scrolled {
            event.apply(&mut self.component, &self.scene);
        }
        #[cfg(target_os = "macos")]
        self.sync_file_drop();
        self.input.sync_editor(&self.scene);
        if let Some(layout) = &self.layout {
            self.input.reconcile_text_menu(&self.scene, layout);
        }
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
                    let caret = layout.window_bounds(Rect::new(
                        area.x + (caret.x - scroll.x).clamp(0.0, area.width),
                        area.y
                            + (caret.y - scroll.y)
                                .clamp(0.0, (area.height - geometry.line_height).max(0.0)),
                        1.0,
                        geometry.line_height,
                    ));
                    state.window.set_ime_cursor_area(
                        LogicalPosition::new(caret.x, caret.y),
                        LogicalSize::new(caret.width, caret.height),
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
            let caret = layout.window_bounds(Rect::new(x, bounds.y, 1.0, bounds.height.max(1.0)));
            state.window.set_ime_cursor_area(
                LogicalPosition::new(caret.x, caret.y),
                LogicalSize::new(caret.width, caret.height),
            );
        }
        state.window.request_redraw();
    }

    fn apply_focus_and_text_requests(&mut self) {
        for request in self.focus_requests.drain() {
            let Some(layout) = &self.layout else {
                break;
            };
            let focused = self.input.focused.clone();
            let command = self.input.apply_focus_request(&self.scene, layout, request);
            if self.input.focused != focused {
                self.pending_clipboard = None;
            }
            if command.is_some_and(|command| self.run_text_command(command))
                && self.sync_base_scene()
            {
                // Later requests from this callback see the committed value and layout.
                self.prepare_frame(self.transition_epoch.elapsed());
            }
        }
    }

    fn run_text_command(&mut self, command: TextCommand) -> bool {
        self.pending_clipboard = None;
        match command {
            TextCommand::SelectAll
            | TextCommand::ClearSelection
            | TextCommand::SetSelection { .. } => {
                self.input.apply_text_selection(&self.scene, command);
            }
            TextCommand::Undo | TextCommand::Redo => {
                return self.replay_text_edit(command == TextCommand::Redo);
            }
            TextCommand::Cut => self.request_clipboard(ClipboardShortcut::Cut),
            TextCommand::Copy => self.request_clipboard(ClipboardShortcut::Copy),
            TextCommand::Paste => self.request_clipboard(ClipboardShortcut::Paste),
        }
        false
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
        if self.input.pressed.as_ref() != Some(target) {
            return;
        }
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
        let origin = self.input.pointer_press_origin().unwrap_or(point);
        let Some(origin) = item.map_from_window(origin) else {
            return;
        };
        let Some(point) = item.map_from_window(point) else {
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
            && !bounds.contains(origin)
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

    fn move_text_caret(&mut self, motion: TextAreaMotion, selecting: bool) -> bool {
        let (Some(target), Some(state)) = (self.input.focused.as_ref(), &self.state) else {
            return false;
        };
        let Some(node) = self.scene.interactive_at_path(&target.path) else {
            return false;
        };
        if !self.input.is_text_editor(node, target) || node.password_masked() {
            return false;
        }
        let bounds = self
            .layout
            .as_ref()
            .and_then(|layout| layout.at_path(&target.path))
            .filter(|item| item.visible && item.enabled)
            .map(|item| item.bounds);
        let Some(bounds) = bounds else {
            return false;
        };
        let text = if matches!(node.kind, "TableCell" | "TreeCell") {
            Cow::Owned(self.input.editor_value(node).into_owned())
        } else {
            Cow::Borrowed(text_input_value(node))
        };
        let Some(editor) = &mut self.input.editor else {
            return false;
        };
        if is_multiline_text_input(node.kind) {
            let layout = state.renderer.text.text_area_layout(node, bounds, &text);
            layout.move_caret(&text, editor, motion, selecting, |text| {
                state.renderer.text.advance_node(node, text)
            });
        } else {
            let visual = state.renderer.text.single_line_layout(node, &text);
            let position = match motion {
                TextAreaMotion::Left | TextAreaMotion::Right => {
                    let right = matches!(motion, TextAreaMotion::Right);
                    let edge = (!selecting)
                        .then(|| visual.selection_edge(editor.selection(&text), right))
                        .flatten();
                    edge.or_else(|| visual.step(editor.position(&text), right))
                }
                TextAreaMotion::Home | TextAreaMotion::End => {
                    Some(visual.edge(matches!(motion, TextAreaMotion::End) != visual.is_rtl()))
                }
                _ => return false,
            };
            if let Some(position) = position {
                editor.place_caret(&text, position, selecting);
            }
        }
        self.pending_clipboard = None;
        self.input.break_text_edit_group();
        self.update_editor();
        true
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
            self.input
                .edit_spin_from(node, action, source, Instant::now());
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
                self.pending_clipboard = Some((id, ClipboardContext::Edit(context)))
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
        if self.component.scene_dirty() {
            self.refresh_scene();
        }
        match context {
            ClipboardContext::Edit(context) if context.is_current(&self.input, &self.scene) => {
                match result {
                    ClipboardValue::Text(text) if !text.is_empty() => {
                        self.edit_text_from(EditAction::Insert(&text), EditSource::Atomic)
                    }
                    ClipboardValue::Text(_) => {}
                    ClipboardValue::Written => {
                        self.edit_text_from(EditAction::Delete, EditSource::Atomic)
                    }
                }
            }
            ClipboardContext::Table { destination, apply } => {
                if let ClipboardValue::Text(text) = result
                    && self.layout.as_ref().is_some_and(|layout| {
                        self.input.table_paste_target(&self.scene, layout).as_ref()
                            == Some(&destination)
                    })
                    && apply(&text)
                {
                    self.refresh_scene();
                }
            }
            ClipboardContext::Edit(_) => {}
        }
    }

    fn table_clipboard_key(&mut self, event: &KeyEvent) -> bool {
        if !(self.modifiers.control_key() || self.modifiers.super_key()) || self.modifiers.alt_key()
        {
            return false;
        }
        let Key::Character(key) = &event.logical_key else {
            return false;
        };
        let pressed = event.state == ElementState::Pressed && !event.repeat;
        let shift = self.modifiers.shift_key();
        let undo_key = key.eq_ignore_ascii_case("z");
        let redo_key = !shift && key.eq_ignore_ascii_case("y");
        if undo_key || redo_key {
            return self.table_history_key(pressed, shift || redo_key);
        }
        if shift {
            return false;
        }
        if key.eq_ignore_ascii_case("v") {
            return self.table_paste_key(pressed);
        }
        if !key.eq_ignore_ascii_case("c") {
            return false;
        }
        let Some((path, columns)) = self
            .layout
            .as_ref()
            .and_then(|layout| self.input.table_copy_target(&self.scene, layout))
        else {
            return false;
        };
        if pressed {
            self.pending_clipboard = None;
            if let (Some(clipboard), Some(events)) = (&self.clipboard, &self.events)
                && let Some(text) = self.component.copy_table_at(&self.scene, &path, &columns)
                && let Err(error) = clipboard
                    .borrow_mut()
                    .request(events, clipboard::Operation::Write(text))
            {
                eprintln!("XRack clipboard: {error}");
            }
        }
        true
    }

    fn table_history_key(&mut self, pressed: bool, redo: bool) -> bool {
        let Some(path) = self
            .layout
            .as_ref()
            .and_then(|layout| self.input.table_paste_history_target(&self.scene, layout))
        else {
            return false;
        };
        if pressed {
            self.pending_clipboard = None;
            if self
                .component
                .replay_table_paste_at(&self.scene, path, redo)
            {
                self.refresh_scene();
            }
        }
        true
    }

    fn table_paste_key(&mut self, pressed: bool) -> bool {
        let Some(destination) = self
            .layout
            .as_ref()
            .and_then(|layout| self.input.table_paste_target(&self.scene, layout))
        else {
            return false;
        };
        if pressed {
            self.pending_clipboard = None;
            if let (Some(clipboard), Some(events)) = (&self.clipboard, &self.events)
                && let Some(apply) = self.component.prepare_table_paste(
                    &self.scene,
                    &destination.path,
                    destination.row,
                    destination.columns.clone(),
                )
            {
                match clipboard
                    .borrow_mut()
                    .request(events, clipboard::Operation::Read)
                {
                    Ok(id) => {
                        self.pending_clipboard =
                            Some((id, ClipboardContext::Table { destination, apply }))
                    }
                    Err(error) => eprintln!("XRack clipboard: {error}"),
                }
            }
        }
        true
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
            let redo = shift || key.eq_ignore_ascii_case("y");
            if self.replay_text_edit(redo) {
                self.refresh_scene();
            }
            return true;
        }
        false
    }

    fn replay_text_edit(&mut self, redo: bool) -> bool {
        self.pending_clipboard = None;
        self.input
            .replay_text_edit(&mut self.component, &self.scene, redo)
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
        if self
            .input
            .editor
            .as_ref()
            .is_some_and(|edit| !edit.preedit.is_empty())
        {
            return true;
        }
        if event.state != ElementState::Pressed {
            return false;
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
        if let Some(motion) = match event.logical_key {
            Key::Named(NamedKey::ArrowLeft) if !shortcut => Some(TextAreaMotion::Left),
            Key::Named(NamedKey::ArrowRight) if !shortcut => Some(TextAreaMotion::Right),
            Key::Named(NamedKey::ArrowUp) if multiline => Some(TextAreaMotion::Up),
            Key::Named(NamedKey::ArrowDown) if multiline => Some(TextAreaMotion::Down),
            Key::Named(NamedKey::Home) if !shortcut => Some(TextAreaMotion::Home),
            Key::Named(NamedKey::End) if !shortcut => Some(TextAreaMotion::End),
            Key::Named(NamedKey::PageUp) if multiline => Some(TextAreaMotion::PageUp),
            Key::Named(NamedKey::PageDown) if multiline => Some(TextAreaMotion::PageDown),
            _ => None,
        } && self.move_text_caret(motion, shift)
        {
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
                return true;
            }
            Key::Named(NamedKey::Enter) if spin => {
                self.commit_spin();
                self.update_editor();
                return true;
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
                return true;
            }
            _ if !shortcut
                && !matches!(event.logical_key, Key::Named(key) if key != NamedKey::Space) =>
            {
                event.text.as_deref().map(EditAction::Insert)
            }
            _ => None,
        };
        if let Some(action) = action {
            self.edit_text(action);
            true
        } else {
            false
        }
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
            ControlAction::TextCommand(command) => {
                self.input.focused.as_ref().is_some_and(|target| {
                    target.path == input.path
                        && target.row_key == self.scene.row_key_at(&target.path)
                }) && self.run_text_command(command)
            }
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
        if !editing
            && focused.is_some_and(|node| node.kind == "ComboBox")
            && !(self.modifiers.control_key()
                || self.modifiers.super_key()
                || self.modifiers.alt_key())
            && let Key::Character(text) = &event.logical_key
            && let Some(layout) = &self.layout
        {
            if let Some(action) =
                self.input
                    .choice_typeahead(&self.scene, layout, text, Instant::now())
            {
                self.apply_control(action);
            } else if open {
                self.update_editor();
            }
            return true;
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

    fn apply_scroll_bar(&mut self, update: ScrollBarUpdate, refresh_focus: bool) {
        match update {
            ScrollBarUpdate::ViewportMoved => self.relayout_with_caret(false),
            ScrollBarUpdate::Range(input) => self.adjust_range(input),
            ScrollBarUpdate::Unchanged if refresh_focus => self.refresh_editor(false),
            ScrollBarUpdate::Unchanged => {}
        }
    }

    fn keyboard_input(&mut self, event: &KeyEvent, is_synthetic: bool) {
        let route = (!is_synthetic)
            .then(|| self.input.key_route(&self.scene, self.layout.as_deref()?))
            .flatten();
        let Some(route) = route else {
            self.default_key_input(event, is_synthetic);
            return;
        };
        let (capture, bubble) = if event.state == ElementState::Pressed {
            (KeyPhase::CapturePressed, KeyPhase::Pressed)
        } else {
            (KeyPhase::CaptureReleased, KeyPhase::Released)
        };
        let key = xrack_runtime::KeyEvent {
            key: match &event.logical_key {
                Key::Character(text) => text.to_string(),
                Key::Named(key) => format!("{key:?}"),
                Key::Dead(_) => "Dead".into(),
                Key::Unidentified(_) => "Unidentified".into(),
            },
            text: event.text.as_deref().unwrap_or_default().to_owned(),
            repeat: event.repeat,
            modifiers: xrack_runtime::KeyModifiers {
                control: self.modifiers.control_key(),
                shift: self.modifiers.shift_key(),
                alt: self.modifiers.alt_key(),
                meta: self.modifiers.super_key(),
                primary: if cfg!(target_os = "macos") {
                    self.modifiers.super_key()
                } else {
                    self.modifiers.control_key()
                },
            },
        };
        if let Some(result) = self.layout.as_ref().and_then(|layout| {
            route.dispatch(&mut self.component, &self.scene, layout, capture, &key)
        }) {
            if result == EventResult::Accept {
                self.input.cancel_shortcut_sequence();
                if event.state == ElementState::Released {
                    if let Some(key) = activation_key_value(&event.logical_key) {
                        self.input.key_release_target(key);
                    }
                    if let Some(key) = navigation_key_value(&event.logical_key) {
                        self.input.discard_range_key(key);
                    }
                }
                self.refresh_scene();
                return;
            }
            self.refresh_after_update(false);
        }
        if self.input.focused.as_ref() != Some(route.focused())
            || self.default_key_input(event, is_synthetic)
            || self.input.focused.as_ref() != Some(route.focused())
        {
            return;
        }
        if self
            .layout
            .as_ref()
            .and_then(|layout| {
                route.dispatch(&mut self.component, &self.scene, layout, bubble, &key)
            })
            .is_some()
        {
            self.refresh_after_update(false);
        }
    }

    fn default_key_input(&mut self, event: &KeyEvent, is_synthetic: bool) -> bool {
        if event.state == ElementState::Pressed
            && matches!(
                event.logical_key,
                Key::Named(NamedKey::Tab | NamedKey::Escape)
            )
        {
            self.input.cancel_shortcut_sequence();
        }
        if event.state == ElementState::Pressed && event.logical_key == Key::Named(NamedKey::Escape)
        {
            if self.cancel_drag() {
                return true;
            }
            if self.input.cancel_table_drag_selection() || self.input.cancel_delay() {
                self.cancel_direct_scroll();
                self.update_editor();
                return true;
            }
        }
        self.cancel_direct_scroll();
        if event.state == ElementState::Pressed && event.logical_key != Key::Named(NamedKey::Tab) {
            let dismissed = self.dismiss_tooltip();
            if dismissed && event.logical_key == Key::Named(NamedKey::Escape) {
                return true;
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
            return true;
        }
        if self.context_menu_key(event)
            || !is_synthetic && self.shortcut_key(event)
            || self.menu_key(event)
            || self.choice_key(event)
            || self.table_edit_key(event)
            || self.selection_key(event)
            || self.tree_toggle_key(event)
            || self.dialog_key(event)
            || self.spin_key(event)
            || self.text_key(event)
            || self.table_clipboard_key(event)
            || self.navigation_key(event)
        {
            return true;
        }
        let Some(key) = activation_key_value(&event.logical_key).filter(|_| !event.repeat) else {
            return false;
        };
        if !self.input.focused.as_ref().is_some_and(|target| {
            self.scene.at_path(&target.path).is_some_and(|node| {
                node.kind != "FocusScope"
                    && !self.input.is_text_editor(node, target)
                    && !spin_editable(node)
            })
        }) {
            return false;
        }
        if event.state == ElementState::Pressed {
            self.input.key_press_with_scene(&self.scene, key);
            if let Some(layout) = &self.layout {
                self.input
                    .begin_key_delay(&self.scene, layout, key, Instant::now());
            }
            if let Some(state) = &self.state {
                state.window.request_redraw();
            }
        } else {
            let target = self.input.key_release_with_scene(&self.scene, key);
            self.activate(target);
        }
        true
    }

    fn navigation_key(&mut self, event: &KeyEvent) -> bool {
        if event.logical_key == Key::Named(NamedKey::BrowserBack)
            || self.modifiers.alt_key() && event.logical_key == Key::Named(NamedKey::ArrowLeft)
        {
            return event.state == ElementState::Pressed && !event.repeat && self.back_in_stack();
        }
        let Some(key) = navigation_key_value(&event.logical_key) else {
            return false;
        };
        if event.state == ElementState::Pressed
            && let Some(update) = self
                .layout
                .as_ref()
                .and_then(|layout| self.input.scroll_bar_key(&self.scene, layout, key))
        {
            self.apply_scroll_bar(update, false);
            return true;
        }
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

    fn window_chrome_press(&mut self) -> bool {
        let edge = self.window_resize_cursor();
        let (Some(layout), Some(state)) = (&self.layout, &mut self.state) else {
            return false;
        };
        if state.presentation.observed().full_screen
            || state.window.fullscreen().is_some()
            || state.presentation.is_switching_full_screen()
        {
            return false;
        }
        if edge.is_some_and(|edge| state.begin_client_resize(edge)) {
            self.input.cancel_press();
        } else if !self.input.arm_window_move(&self.scene, layout) {
            return false;
        }
        self.pending_clipboard = None;
        self.input.cancel_shortcut_sequence();
        self.dismiss_tooltip();
        true
    }

    fn left_pointer(&mut self, state: ElementState) {
        if state == ElementState::Released && self.input.table_drag_selection_claimed() {
            let action = self
                .layout
                .as_ref()
                .and_then(|layout| self.input.finish_table_drag_selection(&self.scene, layout));
            if let Some(action) = action {
                self.apply_control(action);
            } else {
                self.input.cancel_table_drag_selection();
                self.update_editor();
            }
            return;
        }
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
            if let Some(update) = self
                .layout
                .as_ref()
                .and_then(|layout| self.input.scroll_bar_press(&self.scene, layout))
            {
                self.apply_scroll_bar(update, true);
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
            if self.input.focused != self.input.hovered && !self.input.table_row_resize_cursor() {
                self.commit_spin();
            }
            if let Some(layout) = &self.layout {
                self.input.pointer_press_in_scene(&self.scene, layout);
            } else {
                self.input.pointer_press();
            }
            if let Some(point) = self.input.pointer_position() {
                if let Some(layout) = &self.layout {
                    self.input.arm_drag(&self.scene, layout, point);
                    self.input.arm_table_drag_selection(
                        &self.scene,
                        layout,
                        self.modifiers.control_key() || self.modifiers.super_key(),
                    );
                }
                if !self.input.table_row_resize_cursor() {
                    self.place_text_caret(point, self.modifiers.shift_key());
                }
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
        } else if self.input.scroll_bar_captured() {
            let update = if let Some(layout) = &self.layout {
                self.input
                    .release_scroll_bar(&self.scene, layout)
                    .unwrap_or(ScrollBarUpdate::Unchanged)
            } else {
                self.input.finish_scroll_bar();
                ScrollBarUpdate::Unchanged
            };
            self.apply_scroll_bar(update, true);
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
        } else if self.input.table_column_move_claimed() {
            let action = self
                .layout
                .as_ref()
                .and_then(|layout| self.input.finish_table_column_move(&self.scene, layout));
            if let Some(action) = action {
                self.apply_control(action);
            } else {
                self.input.cancel_press();
                self.update_editor();
            }
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
            ScrollContact::MouseViewport(_) => {
                unreachable!("a viewport pan starts with an ordinary press")
            }
        };
        let overscroll = self.input.overscroll_frame();
        if self.layout.as_ref().is_some_and(|layout| {
            self.input
                .begin_scroll_gesture(&self.scene, layout, point, now, kind)
        }) {
            self.scroll_contact = Some(contact);
            self.pending_clipboard = None;
            if overscroll != self.input.overscroll_frame() {
                self.relayout_with_caret(false);
            } else {
                self.layout_changed(false);
            }
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

    fn flush_swipe_events(&mut self, previous: Option<&Node>) -> bool {
        let mut delivered = false;
        loop {
            if let Some(layout) = &self.layout {
                self.input.reconcile_swipe(&self.scene, layout);
            }
            let events = self.input.take_swipe_events();
            if events.is_empty() {
                return delivered;
            }
            delivered = true;
            for event in events {
                let scene = if event.matches(&self.scene) {
                    &*self.scene
                } else {
                    previous.unwrap_or(&self.scene)
                };
                event.apply(&mut self.component, scene);
            }
        }
    }

    fn move_direct_scroll(&mut self, point: Point, now: Instant) -> bool {
        let was_claimed = self.input.mouse_viewport_pan_claimed();
        let moved = self.layout.as_ref().is_some_and(|layout| {
            self.input
                .move_scroll_gesture(&self.scene, layout, point, now)
        });
        let claimed = self.input.mouse_viewport_pan_claimed();
        if self.input.has_pending_swipe_events() {
            self.refresh_after_update(false);
        } else if moved {
            self.relayout_after_scroll();
        } else if claimed && !was_claimed {
            self.update_editor();
        }
        claimed
    }

    fn relayout_after_scroll(&mut self) {
        if self.component.scene_dirty() {
            self.sync_base_scene();
        }
        self.relayout_with_caret(false);
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
        if self.input.has_pending_swipe_events() {
            self.refresh_after_update(false);
        }
        if outcome.scrolled {
            self.relayout_after_scroll();
        }
        if let Some(control) = outcome.control {
            self.apply_control(control);
        }
        if (outcome.tap
            || matches!(
                contact,
                Some(
                    ScrollContact::MousePan(_)
                        | ScrollContact::MouseControl(_)
                        | ScrollContact::MouseViewport(_)
                )
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
        if matches!(self.scroll_contact, Some(ScrollContact::Touch(..))) {
            self.input.finish_touch_scroll_bar();
        }
        self.scroll_contact = None;
        let preview = self.input.tumbler_preview().is_some()
            || self.input.has_swipe_preview()
            || self.input.has_overscroll();
        self.input.cancel_scroll_gesture();
        if self.input.has_pending_swipe_events() {
            self.refresh_after_update(false);
        } else if preview {
            self.relayout_after_scroll();
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
        if let Some(layout) = &self.layout {
            let (consumed, events) = self.scale_rotate.touch(
                &scale_rotate::Context {
                    scene: &self.scene,
                    layout,
                    input: &self.input,
                },
                scale_rotate::ContactId(device, id),
                phase,
                point,
            );
            self.apply_scale_rotate_batch(events);
            if consumed {
                return;
            }
        }
        let owned = matches!(
            self.scroll_contact,
            Some(ScrollContact::Touch(current_device, current_id)) if current_device == device && current_id == id
        );
        match phase {
            TouchPhase::Started
                if self.scroll_contact.is_none()
                    && self.input.drag_position().is_none()
                    && !self.input.scroll_bar_captured() =>
            {
                if let Some(update) = self.layout.as_ref().and_then(|layout| {
                    self.input
                        .touch_scroll_bar_press(&self.scene, layout, point)
                }) {
                    self.scroll_contact = Some(ScrollContact::Touch(device, id));
                    self.apply_scroll_bar(update, true);
                    return;
                }
                self.begin_direct_scroll(ScrollContact::Touch(device, id), point, now);
                if self.scroll_contact.is_some()
                    && let Some(layout) = &self.layout
                {
                    self.input.arm_drag(&self.scene, layout, point);
                }
            }
            TouchPhase::Moved if owned && self.input.scroll_bar_captured() => {
                if let Some(update) = self
                    .layout
                    .as_ref()
                    .map(|layout| self.input.touch_scroll_bar_drag(&self.scene, layout, point))
                {
                    self.apply_scroll_bar(update, false);
                }
            }
            TouchPhase::Moved if owned => {
                if !self.move_drag(point) {
                    self.move_direct_scroll(point, now);
                }
            }
            TouchPhase::Ended if owned && self.input.scroll_bar_captured() => {
                self.scroll_contact = None;
                let update = if let Some(layout) = &self.layout {
                    self.input
                        .release_touch_scroll_bar(&self.scene, layout, point)
                        .unwrap_or(ScrollBarUpdate::Unchanged)
                } else {
                    self.input.finish_touch_scroll_bar();
                    ScrollBarUpdate::Unchanged
                };
                self.apply_scroll_bar(update, true);
            }
            TouchPhase::Cancelled if owned && self.input.finish_touch_scroll_bar() => {
                self.scroll_contact = None;
                if let Some(state) = &self.state {
                    state.window.request_redraw();
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

    fn dispatch_scale_rotate(&mut self, scene: &Node, event: scale_rotate::Delivery) {
        event.target.handle.publish(event.sample);
        if scene
            .at_path(&event.target.path)
            .and_then(|node| node.scale_rotate_handle.as_ref())
            == Some(&event.target.handle)
        {
            self.component
                .dispatch_at(scene, &event.target.path, event.event);
        }
    }

    fn apply_scale_rotate(&mut self, event: scale_rotate::Delivery) {
        let scene = self.scene.clone();
        self.apply_scale_rotate_in(&scene, event);
    }

    fn apply_scale_rotate_in(&mut self, scene: &Node, event: scale_rotate::Delivery) {
        if event.event == "started" {
            self.scroll_contact = None;
            self.pending_clipboard = None;
            self.input.cancel_press();
        }
        self.dispatch_scale_rotate(scene, event);
        self.refresh_after_update(false);
    }

    fn apply_scale_rotate_batch(&mut self, events: [Option<scale_rotate::Delivery>; 2]) {
        if events.iter().all(Option::is_none) {
            return;
        }
        let starting = events[0]
            .as_ref()
            .is_some_and(|event| event.event == "started");
        let scene = self.scene.clone();
        for event in events.into_iter().flatten() {
            if !starting || event.event != "updated" || self.scale_rotate.is_active() {
                self.apply_scale_rotate_in(&scene, event);
            }
        }
    }

    fn cancel_scale_rotate(&mut self) {
        if let Some(event) = self.scale_rotate.reset() {
            self.apply_scale_rotate(event);
        }
    }

    fn update_trackpad_gesture(
        &mut self,
        device: DeviceId,
        phase: TouchPhase,
        delta: scale_rotate::TrackpadDelta,
    ) {
        let Some(layout) = &self.layout else {
            return;
        };
        if self.scroll_contact.is_some() && !self.scale_rotate.is_active() {
            return;
        }
        let deliveries = self.scale_rotate.trackpad(
            &scale_rotate::Context {
                scene: &self.scene,
                layout,
                input: &self.input,
            },
            device,
            phase,
            delta,
        );
        self.apply_scale_rotate_batch(deliveries);
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
            playback: &mut Vec<(&'a Rc<ImagePlaybackRegistration>, bool)>,
        ) {
            if layout.visible {
                timers.extend(&node.timers);
                if layout.enabled {
                    dialogs.extend(&node.native_dialogs);
                }
            }
            if let Some(image) = &node.image_playback {
                let clip = layout.clip_rect(layout.bounds);
                playback.push((
                    image,
                    layout.visible && clip.width > 0.0 && clip.height > 0.0,
                ));
            }
            for (child, item) in node.children.iter().zip(&layout.children) {
                collect(child, item, timers, dialogs, playback);
            }
        }
        let mut timers = Vec::new();
        let mut dialogs = Vec::new();
        let mut playback = Vec::new();
        if let Some(layout) = &self.layout {
            collect(
                &self.scene,
                layout,
                &mut timers,
                &mut dialogs,
                &mut playback,
            );
        }
        self.timers.sync(now, timers);
        self.native_dialogs.sync(&dialogs);
        self.image_playback.sync(now, playback);
    }

    fn tick_image_playback(&mut self, now: Instant) {
        if self.image_playback.advance(now) {
            self.refresh_after_update(false);
        }
        for _ in 0..64 {
            let Some(delivery) = self.image_playback.next_finished() else {
                break;
            };
            if delivery.finish() {
                self.refresh_after_update(false);
            }
        }
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
        let now = Instant::now();
        self.tick_delay(now);
        let animating = self.transition.running(Duration::ZERO);
        let elapsed = self.transition_epoch.elapsed();
        if self.layout.as_ref().is_some_and(|layout| {
            self.input.advance_scroll_motion(&self.scene, layout, now)
                || self
                    .input
                    .advance_table_column_move(&self.scene, layout, now)
                || self
                    .input
                    .advance_table_drag_selection(&self.scene, layout, now)
        }) {
            self.relayout_after_scroll();
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
            self.animation_wake = match self.transition.next_frame(elapsed) {
                Some(next) if next > elapsed => self.transition_epoch.checked_add(next),
                Some(_) => {
                    state.window.request_redraw();
                    None
                }
                None => {
                    self.transition = SceneTransition::default();
                    None
                }
            };
        }
        state.window.pre_present_notify();
        state.renderer.queue.present(frame);
        let completion = state.renderer.take_painted_combo_completion();
        self.input
            .set_painted_combo_completion(completion.filter(|_| state.window.has_focus()));
        if state.renderer.needs_redraw() || self.stack_transitions.is_active() {
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
        let _focus = self.focus_requests.enter();
        let _scroll = self.scroll_requests.enter();
        self.process_user_event(event);
    }

    fn close(&mut self) {
        if let Some(state) = &self.state {
            state.set_visible(false);
        }
        self.close_async();
        self.component.take_window_close_request();
        self.component.take_window_commands();
        self.native_dialogs.close();
        self.suspend();
    }

    fn request_close(&mut self) -> bool {
        let _scope = self.tasks.enter();
        let _focus = self.focus_requests.enter();
        let _scroll = self.scroll_requests.enter();
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
        let _focus = self.focus_requests.enter();
        let _scroll = self.scroll_requests.enter();
        if let Some(id) = self.window_id() {
            return Ok(id);
        }
        if self.base_scene.kind == "SystemTrayIcon" {
            return Err("use Desktop::register_tray for a SystemTrayIcon component".into());
        }
        let attrs = Window::default_attributes()
            .with_title(self.base_scene.text("title").unwrap_or("XRack"))
            .with_visible(false);
        let attrs = WindowChrome::read(&self.base_scene).attributes(attrs, &self.base_scene);
        let window = event_loop
            .create_window(window_owner::attributes(attrs, owner)?)
            .map(Arc::new)
            .map_err(|e| e.to_string())?;
        let adapter = self
            .events
            .as_ref()
            .expect("window events")
            .accessibility(event_loop, &window);
        let mut state = WindowState::new(window, adapter, graphics, owner)?;
        let id = state.window.id();
        state.set_visible(self.base_scene.boolean("visible", true));
        self.focus_active = state.window.has_focus();
        self.component
            .publish_window_status(state.sample_window_status());
        self.state = Some(state);
        self.sync_base_scene();
        self.relayout();
        Ok(id)
    }
    fn suspend(&mut self) {
        if let Some(event) = self.scale_rotate.reset() {
            let scene = self.scene.clone();
            self.dispatch_scale_rotate(&scene, event);
        }
        self.focus_requests.drain();
        self.focus_observers.clear();
        self.scroll_requests.drain();
        self.viewport_observers.clear();
        self.input.clear_scroll_events();
        self.focus_active = false;
        self.animation_wake = None;
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
        self.image_playback.sync(Instant::now(), []);
        self.native_dialogs.sync(&[]);
        self.state = None;
        self.component
            .publish_window_status(xrack_runtime::WindowStatus::default());
    }
    fn tick(&mut self, now: Instant) {
        let _scope = self.tasks.enter();
        let _focus = self.focus_requests.enter();
        let _scroll = self.scroll_requests.enter();
        if let Some(state) = &mut self.state {
            self.component
                .publish_window_status(state.sample_window_status());
        }
        self.refresh_dependencies();
        self.tick_delay(now);
        self.tick_timers(now);
        self.tick_image_playback(now);
    }

    fn refresh_deadline(&mut self, now: Instant) -> Option<Instant> {
        let _scope = self.tasks.enter();
        let _focus = self.focus_requests.enter();
        let _scroll = self.scroll_requests.enter();
        let window_deadline = self.apply_window_presentation(now);
        self.refresh_dependencies();
        self.present_native_dialog();
        self.publish_focus();
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
        if self.animation_wake.is_some_and(|wake| now >= wake) {
            self.animation_wake = None;
            if let Some(state) = &self.state {
                state.window.request_redraw();
            }
        }
        let scroll_wake = self.state.as_ref().and_then(|state| {
            let wake =
                self.input
                    .scroll_motion_deadline()
                    .into_iter()
                    .chain(self.layout.as_ref().and_then(|layout| {
                        self.input.table_column_move_deadline(&self.scene, layout)
                    }))
                    .chain(self.layout.as_ref().and_then(|layout| {
                        self.input
                            .table_drag_selection_deadline(&self.scene, layout)
                    }))
                    .min()?;
            if now >= wake {
                state.window.request_redraw();
                return None;
            }
            Some(wake)
        });
        [
            deadline,
            window_deadline,
            (self.state.is_some() && self.component.has_window_commands()).then_some(now),
            self.animation_wake,
            scroll_wake,
            self.input.delay_deadline(),
            self.timers.deadline(),
            self.image_playback.deadline(),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    fn window_event(&mut self, id: WindowId, event: WindowEvent) {
        let _scope = self.tasks.enter();
        let _focus = self.focus_requests.enter();
        let _scroll = self.scroll_requests.enter();
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
            change @ (WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. }) => {
                if matches!(change, WindowEvent::ScaleFactorChanged { .. }) {
                    self.cancel_scale_rotate();
                }
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
            WindowEvent::PinchGesture {
                device_id,
                delta,
                phase,
            } => {
                self.update_trackpad_gesture(
                    device_id,
                    phase,
                    scale_rotate::TrackpadDelta::Scale(delta),
                );
            }
            WindowEvent::RotationGesture {
                device_id,
                delta,
                phase,
            } => {
                self.update_trackpad_gesture(
                    device_id,
                    phase,
                    scale_rotate::TrackpadDelta::Rotation(f64::from(delta)),
                );
            }
            WindowEvent::CursorMoved { position, .. } => {
                #[cfg(target_os = "macos")]
                if state.resize_drag.is_some()
                    && state.update_client_resize(self.layout.as_ref().is_some_and(|layout| {
                        self.input.window_gestures_allowed(&self.scene, layout)
                    }))
                {
                    return;
                }
                let state = self.state.as_ref().unwrap();
                let p = position.to_logical::<f32>(state.window.scale_factor());
                let point = Point { x: p.x, y: p.y };
                if let Some(layout) = &self.layout
                    && self.input.take_window_move(&self.scene, layout, point)
                {
                    let _ = state.window.drag_window();
                    return;
                }
                match &mut self.scroll_contact {
                    Some(ScrollContact::Touch(..)) => return,
                    Some(
                        ScrollContact::MousePan(last)
                        | ScrollContact::MouseControl(last)
                        | ScrollContact::MouseViewport(last),
                    ) => {
                        *last = point;
                        self.move_direct_scroll(point, Instant::now());
                        return;
                    }
                    None => {
                        if self.move_direct_scroll(point, Instant::now()) {
                            self.scroll_contact = Some(ScrollContact::MouseViewport(point));
                            return;
                        }
                    }
                }
                if self.move_drag(point) {
                    return;
                }
                let state = self.state.as_ref().unwrap();
                if let Some(layout) = &self.layout {
                    let before_resize = self.window_resize_cursor();
                    let changed = self.input.pointer_move(&self.scene, layout, point);
                    let drag_was_claimed = self.input.table_drag_selection_claimed();
                    if self.input.move_table_drag_selection(layout, Instant::now()) {
                        if drag_was_claimed {
                            state.window.request_redraw();
                        } else {
                            self.update_editor();
                        }
                        return;
                    }
                    if changed || before_resize != self.window_resize_cursor() {
                        state.window.set_cursor(self.pointer_cursor());
                        state.window.request_redraw();
                    }
                }
                if let Some(update) = self
                    .layout
                    .as_ref()
                    .and_then(|layout| self.input.scroll_bar_drag(&self.scene, layout))
                {
                    self.apply_scroll_bar(update, false);
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
                if let Some(input) = self.layout.as_ref().and_then(|layout| {
                    self.input
                        .table_resize_drag(&self.scene, layout)
                        .or_else(|| self.input.table_row_resize_drag(&self.scene, layout))
                }) {
                    self.apply_control(input);
                    return;
                }
                if self
                    .layout
                    .as_ref()
                    .is_some_and(|layout| self.input.table_column_move_drag(layout, Instant::now()))
                {
                    self.state.as_ref().unwrap().window.request_redraw();
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
                if self.layout.as_ref().is_some_and(|layout| {
                    self.input
                        .pointer_context_menu_changes_focus(&self.scene, layout)
                }) {
                    self.commit_spin();
                }
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
                (
                    ElementState::Released,
                    Some(ScrollContact::MouseControl(point) | ScrollContact::MouseViewport(point)),
                ) => {
                    self.end_direct_scroll(point, Instant::now());
                }
                (ElementState::Pressed, None) => {
                    if self.window_chrome_press() {
                        return;
                    }
                    if let Some(point) = self.input.pointer_position() {
                        self.begin_direct_scroll(
                            ScrollContact::MouseControl(point),
                            point,
                            Instant::now(),
                        );
                    }
                    if self.scroll_contact.is_none() {
                        self.left_pointer(state);
                        if !self.input.table_drag_selection_armed()
                            && let (Some(layout), Some(point)) =
                                (&self.layout, self.input.pointer_position())
                        {
                            self.input.arm_mouse_viewport_pan(
                                &self.scene,
                                layout,
                                point,
                                Instant::now(),
                            );
                        }
                    }
                }
                _ => {
                    if state == ElementState::Released
                        && (self.input.cancel_window_move()
                            || self
                                .state
                                .as_mut()
                                .is_some_and(WindowState::finish_client_resize))
                    {
                        self.layout_changed(false);
                        return;
                    }
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
                self.keyboard_input(&event, is_synthetic);
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
                self.focus_active = focused;
                if !focused {
                    self.cancel_scale_rotate();
                    self.state.as_mut().unwrap().finish_client_resize();
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
                    self.cancel_scale_rotate();
                    self.state.as_mut().unwrap().finish_client_resize();
                    self.input.cancel_window_move();
                    self.cancel_drag();
                    self.input.set_painted_combo_completion(None);
                    self.cancel_direct_scroll();
                    self.input.finish_scroll_bar();
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
