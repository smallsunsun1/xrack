//! Native desktop host. The event loop sleeps until input, resize, or OS redraw.
mod clipboard;
mod frame;
mod local_task;
mod messages;
mod queue;
#[cfg(test)]
mod tests;

use clipboard::{
    ClipboardPort, ClipboardValue, EditContext, Operation as ClipboardOperation,
    Reply as ClipboardReply,
};
use frame::FrameCache;
use local_task::LocalExecutor;
pub use local_task::{LocalTask, SpawnLocalError, TaskCancelled, spawn_local};
use messages::Inbox;
pub use messages::UiSender;
use std::{
    borrow::Cow,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalPosition, LogicalSize},
    event::{
        DeviceId, ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, Touch, TouchPhase,
        WindowEvent,
    },
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, ModifiersState, NamedKey},
    window::{CursorIcon, Window, WindowId},
};
use xrack_layout::{
    ActivationKey, ChoiceKey, ControlInput, EditAction, EditSource, Interaction, LayoutNode,
    MenuKey, MenuShortcut, NavigationKey, Point, RangeInput, Rect, ScrollGestureKind, ShortcutKey,
    ShortcutModifiers, Size, SpinGeometry, SplitResize, StackTransitions, TableSelectionKey,
    Target, TextAreaMotion, font_size,
};
use xrack_renderer_femtovg::Renderer;
use xrack_runtime::{
    Component, ControlAction, Node, Orientation, RowSelection, SceneTransition, SpinAction,
    is_multiline_text_input, spin_editable,
};

struct WindowState {
    surface: wgpu::Surface<'static>,
    renderer: Renderer,
    config: wgpu::SurfaceConfiguration,
    window: Arc<Window>,
    frame: FrameCache,
}

impl WindowState {
    fn new(window: Arc<Window>) -> Result<Self, String> {
        let instance = wgpu::Instance::new(
            wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(window.clone())),
        );
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| e.to_string())?;
        let renderer = pollster::block_on(Renderer::new(instance, Some(&surface)))?;
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
        Ok(Self {
            surface,
            renderer,
            config,
            window,
            frame: FrameCache::default(),
        })
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

enum UserEvent {
    PollTasks,
    ApplyMessages,
    Clipboard(ClipboardReply),
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
}

struct Application<C, M, R> {
    component: C,
    base_scene: Rc<Node>,
    scene: Rc<Node>,
    transition: SceneTransition,
    transition_start: Instant,
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
    clipboard: Option<ClipboardPort>,
    pending_clipboard: Option<(u64, EditContext)>,
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
        };
        if processed {
            self.refresh_after_update(false);
        }
    }

    fn close_async(&self) {
        self.inbox.close();
        self.tasks.close();
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: String) {
        if self.error.is_none() {
            self.error = Some(error);
        }
        event_loop.exit();
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
        self.scene = scene;
        self.layout = Some(layout);
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
        if let Some(orientation) = self.input.split_orientation(&self.scene) {
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
            && self.input.open_choice(&self.scene, layout, target.clone())
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
            || layout.dialog_paths().is_empty()
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
            && let Some(action) = self.input.dialog_escape(&self.scene, layout)
        {
            self.apply_control(action);
        }
        true
    }

    fn refresh_scene(&mut self) {
        self.refresh_after_update(true);
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
            state
                .window
                .set_visible(self.base_scene.boolean("visible", true));
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
        self.input.sync_editor(&self.scene);
        let Some(state) = &self.state else {
            return;
        };
        let focused = self.input.focused.clone();
        let field = focused.as_ref().and_then(|target| {
            self.scene
                .at_path(&target.path)
                .filter(|node| self.input.is_text_editor(node, target) || spin_editable(node))
        });
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
            let bounds = if node.kind == "SpinBox" {
                SpinGeometry::new(layout.bounds).text
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
            let caret = self
                .input
                .editor
                .as_ref()
                .map_or(0, |edit| edit.caret(&text));
            let advance = state
                .renderer
                .text
                .shape(&text[..caret], font_size(node))
                .map_or(0.0, |metrics| metrics.width());
            let padding = node.length("padding").unwrap_or(6.0).max(0.0);
            let x = bounds.x + (padding + advance).min(bounds.width);
            state.window.set_ime_cursor_area(
                LogicalPosition::new(x, bounds.y),
                LogicalSize::new(1.0, bounds.height.max(1.0)),
            );
        }
        state.window.request_redraw();
    }

    fn place_text_caret(&mut self, point: Point, selecting: bool) {
        let Some(target) = self.input.focused.as_ref() else {
            return;
        };
        let Some(node) = self
            .scene
            .interactive_at_path(&target.path)
            .filter(|node| self.input.is_text_editor(node, target))
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
        let value = if matches!(node.kind, "TableCell" | "TreeCell") {
            Cow::Owned(self.input.editor_value(node).into_owned())
        } else {
            Cow::Borrowed(node.text("text").unwrap_or(""))
        };
        let text = value.as_ref();
        let position = if is_multiline_text_input(node.kind) {
            let geometry = state
                .renderer
                .text
                .text_area_layout(node, item.bounds, text);
            let scroll = geometry.clamp_scroll(self.input.text_scroll(&target.path));
            geometry.hit(text, point, scroll, |text| {
                state.renderer.text.advance(text, font_size(node))
            })
        } else {
            state.renderer.text.single_line_hit_with_text(
                node,
                item.bounds,
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
                state.renderer.text.advance(text, font_size(node))
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
        if node.kind == "SpinBox" {
            self.input.edit_spin(node, action);
            self.update_editor();
            return;
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
        let Some(target) = &self.input.focused else {
            return;
        };
        let Some(node) = self.scene.interactive_at_path(&target.path) else {
            return;
        };
        if node.boolean("read-only", false) && !matches!(shortcut, ClipboardShortcut::Copy) {
            return;
        }
        let Some(context) = EditContext::capture(&self.input, &self.scene) else {
            return;
        };
        let operation = match shortcut {
            ClipboardShortcut::Paste => ClipboardOperation::Read,
            ClipboardShortcut::Copy | ClipboardShortcut::Cut if !context.selection().is_empty() => {
                ClipboardOperation::Write(context.selection().into())
            }
            _ => return,
        };
        let Some(clipboard) = &mut self.clipboard else {
            return;
        };
        match clipboard.request(operation) {
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
        let spin = node.kind == "SpinBox";
        if event.state != ElementState::Pressed
            || self
                .input
                .editor
                .as_ref()
                .is_some_and(|edit| !edit.preedit.is_empty())
        {
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
                if !event.repeat {
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
        if self
            .component
            .control_at(&self.scene, &input.path, input.action)
        {
            self.refresh_scene();
        } else if self
            .scene
            .at_path(&input.path)
            .is_some_and(|node| node.list.is_some())
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
            .filter(|node| node.kind == "SpinBox")
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

    fn menu_shortcut(&mut self, event: &KeyEvent) -> bool {
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
            _ => return false,
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
                return false;
            }
        }
        let Some(action) = MenuShortcut::new(key, modifiers)
            .and_then(|key| self.input.menu_shortcut(&self.scene, layout, key))
        else {
            return false;
        };
        if !event.repeat {
            self.pending_clipboard = None;
            self.input.close_menu(&self.scene, layout);
            self.apply_control(action);
        }
        true
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
        if event.state != ElementState::Pressed {
            return open;
        }
        let key = match event.logical_key {
            Key::Named(NamedKey::ArrowUp) => ChoiceKey::Up,
            Key::Named(NamedKey::ArrowDown) => ChoiceKey::Down,
            Key::Named(NamedKey::Home) => ChoiceKey::Home,
            Key::Named(NamedKey::End) => ChoiceKey::End,
            Key::Named(NamedKey::PageUp) => ChoiceKey::PageUp,
            Key::Named(NamedKey::PageDown) => ChoiceKey::PageDown,
            Key::Named(NamedKey::Enter | NamedKey::Space) => ChoiceKey::Accept,
            Key::Named(NamedKey::Escape) => ChoiceKey::Escape,
            _ => return open,
        };
        if let Some(input) = self
            .layout
            .as_ref()
            .and_then(|layout| self.input.choice_key(&self.scene, layout, key))
        {
            self.apply_control(input);
            true
        } else if open {
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
                self.place_text_caret(point, self.modifiers.shift_key());
            }
            if let Some(layout) = &self.layout {
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
            .and_then(|layout| self.input.page_marker_release(&self.scene, layout))
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
        if (outcome.tap || matches!(contact, Some(ScrollContact::MousePan(_))))
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
        self.input.cancel_scroll_gesture();
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
            TouchPhase::Started if self.scroll_contact.is_none() => {
                self.begin_direct_scroll(ScrollContact::Touch(device, id), point, now);
            }
            TouchPhase::Moved if owned => self.move_direct_scroll(point, now),
            TouchPhase::Ended if owned => self.end_direct_scroll(point, now),
            TouchPhase::Cancelled if owned => self.cancel_direct_scroll(),
            _ => {}
        }
    }

    fn redraw(&mut self) -> Result<(), String> {
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

impl<C: Component, M, R: FnMut(&mut C, M)> ApplicationHandler<UserEvent> for Application<C, M, R> {
    fn user_event(&mut self, _: &ActiveEventLoop, event: UserEvent) {
        self.process_user_event(event);
    }

    fn exiting(&mut self, _: &ActiveEventLoop) {
        self.close_async();
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title(self.base_scene.text("title").unwrap_or("XRack"))
            .with_visible(self.base_scene.boolean("visible", true))
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
        match event_loop
            .create_window(attrs)
            .map(Arc::new)
            .map_err(|e| e.to_string())
            .and_then(WindowState::new)
        {
            Ok(state) => {
                self.state = Some(state);
                self.relayout();
            }
            Err(error) => self.fail(event_loop, error),
        }
    }
    fn suspended(&mut self, _: &ActiveEventLoop) {
        self.cancel_direct_scroll();
        self.pending_clipboard = None;
        self.input.dismiss_tooltip();
        self.input.dismiss_choice();
        self.input.dismiss_menu();
        self.input.discard_spin_draft();
        self.input.cancel_press();
        self.input.pointer_leave();
        self.layout = None;
        self.state = None;
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let deadline = if let Some(state) = &self.state
            && state.window.has_focus()
            && let Some(layout) = &self.layout
        {
            if self
                .input
                .update_tooltip(&self.scene, layout, Instant::now())
            {
                state.window.request_redraw();
            }
            self.input.tooltip_deadline()
        } else {
            None
        };
        event_loop.set_control_flow(deadline.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if !self.state.as_ref().is_some_and(|s| s.window.id() == id) {
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                if let Err(error) = self.state.as_mut().unwrap().resize() {
                    self.fail(event_loop, error);
                    return;
                }
                self.relayout();
            }
            WindowEvent::RedrawRequested => {
                if let Err(error) = self.redraw() {
                    self.fail(event_loop, error);
                }
            }
            WindowEvent::Touch(touch) => self.touch_event(touch),
            WindowEvent::CursorMoved { position, .. } => {
                let state = self.state.as_ref().unwrap();
                let p = position.to_logical::<f32>(state.window.scale_factor());
                let point = Point { x: p.x, y: p.y };
                match self.scroll_contact {
                    Some(ScrollContact::Touch(..)) => return,
                    Some(ScrollContact::MousePan(_)) => {
                        self.scroll_contact = Some(ScrollContact::MousePan(point));
                        self.move_direct_scroll(point, Instant::now());
                        return;
                    }
                    None => {}
                }
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
                self.dismiss_tooltip();
                let scale = self.state.as_ref().unwrap().window.scale_factor();
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
                (ElementState::Pressed, None) => {
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
            } => {
                if !matches!(self.scroll_contact, Some(ScrollContact::Touch(..))) {
                    self.cancel_direct_scroll();
                    self.left_pointer(state);
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::KeyboardInput {
                event,
                is_synthetic,
                ..
            } => {
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
                    if let Some(layout) = &self.layout {
                        self.input
                            .focus_next(&self.scene, layout, self.modifiers.shift_key());
                    }
                    self.update_editor();
                } else if !self.context_menu_key(&event)
                    && (is_synthetic || !self.menu_shortcut(&event))
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
                            self.input.key_press(key);
                            self.state.as_ref().unwrap().window.request_redraw();
                        } else {
                            let target = self.input.key_release_target(key);
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
    mut component: C,
    setup: impl FnOnce(&mut C, UiSender<M>) -> R,
) -> Result<(), String>
where
    C: Component + 'static,
    M: Send + 'static,
    R: FnMut(&mut C, M),
{
    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .build()
        .map_err(|e| e.to_string())?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let proxy = event_loop.create_proxy();
    let clipboard = ClipboardPort::new(proxy.clone());
    let task_proxy = proxy.clone();
    let tasks = LocalExecutor::new(move || {
        let _ = task_proxy.send_event(UserEvent::PollTasks);
    });
    let _scope = tasks.enter();
    let inbox = Inbox::new(move || {
        let _ = proxy.send_event(UserEvent::ApplyMessages);
    });
    let reduce = setup(&mut component, inbox.sender());
    let mut app = Application::new(component, tasks, inbox, reduce);
    app.clipboard = Some(clipboard);
    let result = event_loop.run_app(&mut app);
    app.close_async();
    result.map_err(|e| e.to_string())?;
    app.error.map_or(Ok(()), Err)
}
