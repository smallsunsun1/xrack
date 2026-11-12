//! Interactive offscreen scenes for editor previews.
pub use crate::preview_pointer::Contact as PointerContact;
use crate::{
    PointerCaret, Renderer, TextSystem,
    preview_pointer::{Captures, Context as PointerContext},
};
use std::{
    borrow::Cow,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
use xrack_layout::{
    ActivationKey, ChoiceKey, ControlInput, DragUpdate, EditAction, EditSource, FileDragInput,
    FocusObservers, HoverObservers, Interaction, LayoutNode, LayoutObservers, MenuKey,
    MenuShortcut, NavigationKey, Point, RangeInput, Rect, ScrollBarUpdate, ScrollGestureKind,
    ShortcutKey, ShortcutModifiers, ShortcutOutcome, Size, SplitResize, StackTransitions,
    TableEditCommit, TableEditTarget, TablePasteDestination, TextAreaMotion, TextEditContext,
    TumblerScroll, WheelAdjustment, WindowEffects,
    scale_rotate_gestures::{self, TouchPhase},
};
use xrack_runtime::{
    Component, ControlAction, DragAction, EventResult, FocusRequests, ImagePlaybackQueue, KeyEvent,
    KeyPhase, NativeDialogPresentation, NativeDialogQueue, NativeDialogResult, Node, PointerButton,
    RowSelection, SceneTransition, ScrollRequests, SpinAction, TablePaste, TextCommand, TimerQueue,
    WheelEvent,
};

#[derive(Clone, Copy)]
enum PointerCapture {
    Split,
    Delay,
    Range,
    ScrollGesture,
    ScrollBar { touch: bool },
    TableSelection,
    TableResize,
    TableReorder,
    TextSelection,
}

struct DragContact {
    id: u64,
    requested: Option<DragAction>,
    captured: bool,
}

pub struct ClipboardRequest {
    pub id: u64,
    /// The text to write. A missing value requests the clipboard contents.
    pub text: Option<String>,
}

enum ClipboardEdit {
    Text {
        context: TextEditContext,
        cut: bool,
    },
    Table {
        destination: TablePasteDestination,
        apply: TablePaste,
    },
}

pub struct PreviewScene {
    component: Box<dyn Component>,
    input: Interaction,
    scroll: ScrollRequests,
    focus: FocusRequests,
    table_tab: Option<(TableEditTarget, bool)>,
    list_item_click: Option<(Vec<usize>, xrack_runtime::RowKey)>,
    table_cell_click: Option<xrack_layout::Target>,
    observers: LayoutObservers,
    focus_observers: FocusObservers,
    hover_observers: HoverObservers,
    hover_pointer: Option<Point>,
    clipboard_request: Option<ClipboardRequest>,
    clipboard_edit: Option<(u64, ClipboardEdit)>,
    pointer_carets: Vec<PointerCaret>,
    ime_area: Option<Rect>,
    reveal_editor_caret: bool,
    frame: Option<(Rc<Node>, LayoutNode)>,
    dimensions: (u32, u32),
    png: Vec<u8>,
    target: Option<Rc<Node>>,
    transition: SceneTransition,
    stack_transitions: StackTransitions,
    image_playback: ImagePlaybackQueue,
    timers: TimerQueue,
    dialogs: NativeDialogQueue,
    window_closed: bool,
    window_active: bool,
    renderer_needs_redraw: bool,
    pointer_capture: Option<PointerCapture>,
    direct_pointers: Captures,
    scale_rotate: scale_rotate_gestures::Gestures<()>,
    drag_contact: Option<DragContact>,
    delay_key: Option<ActivationKey>,
    clock: Instant,
    sampled_at: Duration,
}

impl PreviewScene {
    pub fn new(component: Box<dyn Component>) -> Self {
        Self {
            component,
            input: Interaction::default(),
            scroll: ScrollRequests::default(),
            focus: FocusRequests::default(),
            table_tab: None,
            list_item_click: None,
            table_cell_click: None,
            observers: LayoutObservers::default(),
            focus_observers: FocusObservers::default(),
            hover_observers: HoverObservers::default(),
            hover_pointer: None,
            clipboard_request: None,
            clipboard_edit: None,
            pointer_carets: Vec::new(),
            ime_area: None,
            reveal_editor_caret: false,
            frame: None,
            dimensions: (0, 0),
            png: Vec::new(),
            target: None,
            transition: SceneTransition::default(),
            stack_transitions: StackTransitions::default(),
            image_playback: ImagePlaybackQueue::default(),
            timers: TimerQueue::default(),
            dialogs: NativeDialogQueue::default(),
            window_closed: false,
            window_active: true,
            renderer_needs_redraw: false,
            pointer_capture: None,
            direct_pointers: Captures::default(),
            scale_rotate: Default::default(),
            drag_contact: None,
            delay_key: None,
            clock: Instant::now(),
            sampled_at: Duration::ZERO,
        }
    }

    pub fn keyboard(&mut self, renderer: &mut Renderer, event: &KeyEvent, pressed: bool) -> bool {
        self.flush_pointer_carets(renderer);
        let cancelled = pressed && self.input.cancel_text_composition(None);
        let _scroll = self.scroll.enter();
        let _focus = self.focus.enter();
        let Some((scene, layout)) = &self.frame else {
            return false;
        };
        let tooltip = pressed && event.key != "Tab" && self.input.dismiss_tooltip();
        if tooltip && event.key == "Escape" {
            return true;
        }
        let route = self.input.key_route(scene, layout);
        let (capture, bubble) = if pressed {
            (KeyPhase::CapturePressed, KeyPhase::Pressed)
        } else {
            (KeyPhase::CaptureReleased, KeyPhase::Released)
        };
        let captured = route
            .as_ref()
            .and_then(|route| route.dispatch(&mut self.component, scene, layout, capture, event));
        if captured == Some(EventResult::Accept) {
            self.input.cancel_shortcut_sequence();
            if !pressed {
                if let Some(key) = activation_key(&event.key) {
                    self.cancel_delay_key();
                    self.input.key_release_target(key);
                }
                if let Some(key) = navigation_key(&event.key) {
                    self.input.discard_range_key(key);
                }
            }
            return true;
        }
        if captured.is_some()
            && (self.component.scene_dirty() || !self.focus.is_empty() || !self.scroll.is_empty())
        {
            self.refresh_input_frame(renderer);
        }
        if route
            .as_ref()
            .is_some_and(|route| self.input.focused.as_ref() != Some(route.focused()))
            || self.default_key(event, pressed, Some(&renderer.text))
        {
            return true;
        }
        let bubbled = route.as_ref().and_then(|route| {
            let (scene, layout) = self.frame.as_ref()?;
            route.dispatch(&mut self.component, scene, layout, bubble, event)
        });
        cancelled || tooltip || captured.is_some() || bubbled.is_some()
    }

    fn default_key(&mut self, event: &KeyEvent, pressed: bool, text: Option<&TextSystem>) -> bool {
        let activation = activation_key(&event.key);
        if !pressed {
            return self.selection_key(event, false)
                || self.range_key(&event.key, false)
                || activation.is_some_and(|key| {
                    self.release_delay_key(key) || self.activate_key_target(key, false)
                });
        }
        if matches!(event.key.as_str(), "Tab" | "Escape") {
            self.input.cancel_shortcut_sequence();
        }
        if event.key == "Tab" && !event.repeat {
            if self.table_tab_enabled() {
                self.table_tab(event.modifiers.shift);
                return true;
            }
            let popup = self.input.menu_open() || self.input.choice_open();
            self.input.dismiss_choice();
            self.input.dismiss_menu();
            if let Some((scene, layout)) = &self.frame {
                let before = self.input.focused.clone();
                return self.input.focus_next(scene, layout, event.modifiers.shift)
                    || self.input.focused != before
                    || popup;
            }
        }
        if event.key == "Escape" && self.cancel_pointer_or_delay() {
            return true;
        }
        if (event.key == "ContextMenu" || event.key == "F10" && event.modifiers.shift)
            && !event.repeat
            && self.open_context_menu(None)
        {
            return true;
        }
        if self.shortcut_key(event) || self.menu_key(&event.key) || self.choice_key(event) {
            return true;
        }
        if self.selection_key(event, true) {
            return true;
        }
        if event.key == "Escape" {
            return !event.repeat && self.escape_focused();
        }
        if let Some(key) = activation
            && (self.delay_keyboard_enabled() || self.delay_key.is_some())
        {
            if !event.repeat {
                self.press_delay_key(key);
            }
            return true;
        }
        let shift = event.modifiers.shift;
        if (event.modifiers.control || event.modifiers.meta) && !event.modifiers.alt {
            match event.key.to_ascii_lowercase().as_str() {
                "z" => {
                    self.replay_edit(shift);
                    return true;
                }
                "y" => {
                    self.replay_edit(true);
                    return true;
                }
                key @ ("c" | "x" | "v") => {
                    let Some((scene, layout)) = &self.frame else {
                        return false;
                    };
                    let command = match key {
                        "c" => TextCommand::Copy,
                        "x" => TextCommand::Cut,
                        _ => TextCommand::Paste,
                    };
                    let scene = scene.clone();
                    if command == TextCommand::Copy
                        && !shift
                        && let Some((path, columns)) = self.input.table_copy_target(&scene, layout)
                    {
                        self.clipboard_edit = None;
                        if let Some(text) = self.component.copy_table_at(&scene, &path, &columns) {
                            self.queue_clipboard(Some(text), None);
                        }
                        return true;
                    }
                    if command == TextCommand::Paste
                        && !shift
                        && let Some(destination) = self.input.table_paste_target(&scene, layout)
                        && let Some(apply) = self.component.prepare_table_paste(
                            &scene,
                            &destination.path,
                            destination.row,
                            destination.columns.clone(),
                        )
                    {
                        self.queue_clipboard(
                            None,
                            Some(ClipboardEdit::Table { destination, apply }),
                        );
                        return true;
                    }
                    return self.request_text_clipboard(&scene, command);
                }
                "a" if self.input.editor.is_some() => {
                    self.edit(EditAction::SelectAll);
                    return true;
                }
                _ => {}
            }
        }
        let modifiers = event.modifiers;
        if self.range_key(&event.key, true) || self.spin_key(&event.key) {
            return true;
        }
        if event.key == "ArrowRight"
            && !event.repeat
            && !(modifiers.shift || modifiers.control || modifiers.alt || modifiers.meta)
            && let Some((scene, _)) = &self.frame
            && self
                .input
                .accept_combo_completion(&mut self.component, scene, Instant::now())
        {
            self.clipboard_edit = None;
            return true;
        }
        let shortcut = (modifiers.control || modifiers.meta) && !modifiers.alt;
        if let Some(text) = text
            && let Some(key) = navigation_key(&event.key)
            && let Some(motion) = TextAreaMotion::from_navigation(key, shortcut)
            && let Some((scene, layout)) = &self.frame
            && text.move_editor_caret(&mut self.input, scene, layout, motion, shift)
        {
            self.clipboard_edit = None;
            return true;
        }
        if (event.key == "BrowserBack" || event.key == "ArrowLeft" && event.modifiers.alt)
            && !event.repeat
            && let Some((scene, layout)) = &self.frame
            && let Some(ControlInput { path, action }) = self.input.stack_back(scene, layout)
        {
            self.component.control_at(scene, &path, action);
            return true;
        }
        if modifiers.alt
            && !modifiers.shift
            && !modifiers.control
            && !modifiers.meta
            && let Some(key) = navigation_key(&event.key)
            && let Some((scene, layout)) = &self.frame
            && let Some(ControlInput { path, action }) =
                self.input.table_row_move_key(scene, layout, key)
        {
            self.component.control_at(scene, &path, action);
            return true;
        }
        if navigation_key(&event.key).is_some_and(|key| {
            self.navigation_control_key(key, RowSelection::from_modifiers(modifiers, true))
        }) || (!event.repeat
            && activation.is_some_and(|key| self.activate_key_target(key, true)))
        {
            return true;
        }
        if let Some(
            key @ (NavigationKey::Up
            | NavigationKey::Down
            | NavigationKey::PageUp
            | NavigationKey::PageDown),
        ) = navigation_key(&event.key)
            && (self.tree_navigation_enabled() || self.split_navigation_enabled())
        {
            self.navigate_tree_or_split(key, shift);
            return true;
        }
        if self.input.editor.is_none()
            && !self.focused_accepts_text()
            && !self.split_navigation_enabled()
        {
            return false;
        }
        let action = match event.key.as_str() {
            "Backspace" => EditAction::Backspace,
            "Delete" => EditAction::Delete,
            "ArrowLeft" => EditAction::Left(shift),
            "ArrowRight" => EditAction::Right(shift),
            "Home" => EditAction::Home(shift),
            "End" => EditAction::End(shift),
            "Enter" => EditAction::Insert("\n"),
            _ if !event.text.is_empty() => EditAction::Insert(&event.text),
            _ => return false,
        };
        self.edit(action);
        true
    }

    fn range_key(&mut self, key: &str, pressed: bool) -> bool {
        let Some((scene, layout)) = &self.frame else {
            return false;
        };
        let Some(input) =
            navigation_key(key).and_then(|key| self.input.slider_key(scene, layout, key, pressed))
        else {
            return false;
        };
        self.component
            .adjust_range_at(scene, &input.path, input.action);
        true
    }

    fn spin_key(&mut self, key: &str) -> bool {
        let Some((scene, _)) = &self.frame else {
            return false;
        };
        let Some(target) = self.input.focused.as_ref() else {
            return false;
        };
        let Some(node) = scene
            .interactive_at_path(&target.path)
            .filter(|node| xrack_runtime::is_spin_box(node.kind))
        else {
            return false;
        };
        let action = match key {
            "ArrowUp" => SpinAction::Increase,
            "ArrowDown" => SpinAction::Decrease,
            "Home" if !xrack_runtime::spin_editable(node) => SpinAction::Minimum,
            "End" if !xrack_runtime::spin_editable(node) => SpinAction::Maximum,
            _ => return false,
        };
        if !node.boolean("read-only", false) {
            let scene = scene.clone();
            let path = target.path.clone();
            self.apply_control(
                &scene,
                ControlInput {
                    path,
                    action: ControlAction::Spin(action),
                },
            );
        }
        true
    }

    fn selection_key(&mut self, event: &KeyEvent, pressed: bool) -> bool {
        if self.input.menu_open() || self.input.choice_open() {
            return false;
        }
        let Some((scene, layout)) = &self.frame else {
            return false;
        };
        let Some(ControlInput { path, action }) =
            self.input
                .selection_key_action(scene, layout, &event.key, event.modifiers)
        else {
            return false;
        };
        if pressed && !event.repeat {
            self.component.control_at(scene, &path, action);
        }
        true
    }

    fn navigation_control_key(&mut self, key: NavigationKey, selection: RowSelection) -> bool {
        let Some((scene, layout)) = &self.frame else {
            return false;
        };
        if let Some(update) = self.input.scroll_bar_key(scene, layout, key) {
            apply_scroll_bar_update(&mut self.component, scene, update);
            return true;
        }
        let action = self
            .input
            .group_key(scene, layout, key)
            .or_else(|| self.input.page_indicator_key(scene, layout, key))
            .or_else(|| self.input.calendar_key(scene, layout, key))
            .or_else(|| self.input.tumbler_key(scene, layout, key))
            .or_else(|| self.input.swipe_key(scene, layout, key))
            .or_else(|| self.input.swipe_delegate_key(scene, layout, key))
            .or_else(|| {
                self.input
                    .tree_key_with_selection(scene, layout, key, selection)
            })
            .or_else(|| {
                self.input
                    .list_key_with_selection(scene, layout, key, selection)
            });
        if let Some(ControlInput { path, action }) = action {
            self.component.control_at(scene, &path, action);
            true
        } else {
            false
        }
    }

    fn activate_key_target(&mut self, key: ActivationKey, pressed: bool) -> bool {
        let Some((scene, layout)) = &self.frame else {
            return false;
        };
        let focused = self
            .input
            .focused
            .as_ref()
            .and_then(|target| scene.interactive_at_path(&target.path));
        if !focused.is_some_and(|node| {
            xrack_runtime::is_calendar_control(node.kind)
                || xrack_runtime::is_button(node.kind)
                || xrack_runtime::is_toggle(node.kind)
                || matches!(node.kind, "RadioButton" | "Tab" | "TabButton")
        }) {
            return false;
        }
        if pressed {
            self.input.key_press_with_scene(scene, key);
        } else if let Some(target) = self.input.key_release_with_scene(scene, key) {
            let input = self
                .input
                .dialog_button(scene, layout, &target)
                .or_else(|| self.input.calendar_activate(scene, layout, &target))
                .or_else(|| self.input.select_group_option(scene, layout, &target));
            if let Some(ControlInput { path, action }) = input {
                self.component.control_at(scene, &path, action);
            } else {
                self.component.dispatch_at(scene, &target.path, "clicked");
            }
        }
        true
    }

    pub fn control_navigation_enabled(&self) -> bool {
        self.frame.as_ref().is_some_and(|(scene, _)| {
            self.input
                .focused
                .as_ref()
                .and_then(|target| scene.interactive_at_path(&target.path))
                .is_some_and(|node| {
                    matches!(
                        node.kind,
                        "Slider"
                            | "RangeThumb"
                            | "Dial"
                            | "SpinBox"
                            | "DoubleSpinBox"
                            | "ComboBox"
                            | "SearchField"
                            | "Calendar"
                            | "MonthGrid"
                            | "Tumbler"
                            | "RadioButton"
                            | "Tab"
                            | "TabButton"
                            | "SwipeView"
                            | "SwipeDelegate"
                            | "PageIndicator"
                            | "ScrollBar"
                            | "ListView"
                            | "StandardListView"
                            | "GridView"
                            | "TableView"
                            | "TableCell"
                            | "TextArea"
                            | "TextEdit"
                    )
                })
        })
    }

    fn focused_accepts_text(&self) -> bool {
        let Some((scene, _)) = &self.frame else {
            return false;
        };
        self.input.focused.as_ref().is_some_and(|target| {
            scene.interactive_at_path(&target.path).is_some_and(|node| {
                self.input.is_text_editor(node, target)
                    || xrack_runtime::spin_editable(node)
                    || matches!(node.kind, "TreeView" | "TableView" | "TableCell")
            })
        })
    }

    fn shortcut_key(&mut self, event: &KeyEvent) -> bool {
        let Some((scene, layout)) = &self.frame else {
            return false;
        };
        let modifiers = event.modifiers;
        if matches!(
            event.key.as_str(),
            "Control" | "Alt" | "AltGraph" | "Shift" | "Meta" | "Super"
        ) {
            return false;
        }
        let editing_shortcut = self.input.editor.is_some()
            && !self.input.menu_open()
            && (((modifiers.control || modifiers.meta)
                && !modifiers.alt
                && matches!(
                    event.key.to_ascii_lowercase().as_str(),
                    "a" | "c" | "v" | "x" | "y" | "z"
                ))
                || (modifiers.control && modifiers.alt && !event.text.is_empty()));
        let key = (!editing_shortcut)
            .then(|| {
                let key = if event.key == "Escape" {
                    ShortcutKey::Escape
                } else if let Some(function) =
                    event.key.strip_prefix('F').filter(|_| event.key.len() > 1)
                {
                    ShortcutKey::Function(function.parse().ok()?)
                } else {
                    let mut chars = event.key.chars();
                    ShortcutKey::Character(chars.next().filter(|_| chars.next().is_none())?)
                };
                MenuShortcut::new(
                    key,
                    ShortcutModifiers {
                        ctrl: modifiers.control,
                        alt: modifiers.alt,
                        shift: modifiers.shift,
                        super_key: modifiers.meta,
                    },
                )
            })
            .flatten();
        let Some(key) = key else {
            self.input.cancel_shortcut_sequence();
            return false;
        };
        let outcome = self
            .input
            .shortcut_press(scene, layout, key, event.repeat, Instant::now());
        let scene = scene.clone();
        match outcome {
            ShortcutOutcome::Unmatched => false,
            ShortcutOutcome::Consumed => true,
            ShortcutOutcome::Activate(input) => {
                self.apply_control(&scene, input);
                true
            }
            ShortcutOutcome::Ambiguous(inputs) => {
                for input in inputs {
                    self.apply_control(&scene, input);
                }
                true
            }
        }
    }

    pub fn menu_open(&self) -> bool {
        self.input.menu_open()
    }

    pub fn open_context_menu(&mut self, point: Option<Point>) -> bool {
        let Some((scene, layout)) = &self.frame else {
            return false;
        };
        if let Some(point) = point {
            self.input.pointer_move(scene, layout, point);
        }
        self.input.open_context_menu(scene, layout, point.is_none())
    }

    pub fn hover(&mut self, point: Point) -> bool {
        self.hover_pointer = Some(point);
        let Some((scene, layout)) = &self.frame else {
            return false;
        };
        let deadline = self.input.tooltip_deadline();
        let moved = self.input.pointer_move(scene, layout, point);
        let tooltip = self.input.update_tooltip(scene, layout, Instant::now())
            || self.input.tooltip_deadline() != deadline;
        self.sync_current_hover() || moved || tooltip
    }

    pub fn leave(&mut self) -> bool {
        self.hover_pointer = None;
        let left = self.input.pointer_leave();
        self.sync_current_hover() || left
    }

    fn sync_current_hover(&mut self) -> bool {
        let Some((scene, layout)) = self.frame.take() else {
            return false;
        };
        let changed = self.sync_hover(&scene, &layout);
        self.frame = Some((scene, layout));
        changed
    }

    pub fn cursor_shape(&self) -> xrack_runtime::CursorShape {
        self.hover_observers.cursor_shape()
    }

    fn sync_hover(&mut self, scene: &Node, layout: &LayoutNode) -> bool {
        let _scroll = self.scroll.enter();
        let _focus = self.focus.enter();
        let previous = self.hover_observers.cursor_shape();
        let pointer = self.hover_pointer.filter(|_| self.window_active);
        let events = self
            .hover_observers
            .sync(&self.input, scene, layout, pointer);
        let mut delivered = false;
        for event in events {
            delivered |= event.apply(&mut self.component, scene);
        }
        delivered
            || previous != self.cursor_shape()
            || self.component.scene_dirty()
            || !self.scroll.is_empty()
            || !self.focus.is_empty()
    }

    fn menu_key(&mut self, name: &str) -> bool {
        let Some((scene, layout)) = &self.frame else {
            return false;
        };
        let open = self.input.menu_open();
        let key = match name {
            "ArrowUp" => MenuKey::Up,
            "ArrowDown" => MenuKey::Down,
            "ArrowLeft" => MenuKey::Left,
            "ArrowRight" => MenuKey::Right,
            "Home" => MenuKey::Home,
            "End" => MenuKey::End,
            "Enter" | " " => MenuKey::Accept,
            "Escape" => MenuKey::Escape,
            _ => return open,
        };
        let header = self
            .input
            .focused
            .as_ref()
            .and_then(|target| scene.at_path(&target.path))
            .is_some_and(|node| node.kind == "Menu");
        if !open && (!header || matches!(key, MenuKey::Escape | MenuKey::Home | MenuKey::End)) {
            return false;
        }
        if let Some(input) = self.input.menu_key(scene, layout, key) {
            let scene = scene.clone();
            self.apply_control(&scene, input);
        }
        true
    }

    fn choice_key(&mut self, event: &KeyEvent) -> bool {
        let Some((scene, layout)) = &self.frame else {
            return false;
        };
        let Some(node) = self
            .input
            .focused
            .as_ref()
            .and_then(|target| scene.interactive_at_path(&target.path))
            .filter(|node| matches!(node.kind, "ComboBox" | "SearchField"))
        else {
            return false;
        };
        let search = node.kind == "SearchField";
        let editing = search || xrack_runtime::combo_editable(node);
        let open = self.input.choice_open();
        if search
            && matches!(event.key.as_str(), "Home" | "End")
            && (event.modifiers.shift
                || event.modifiers.control
                || event.modifiers.meta
                || self
                    .input
                    .choice_menu(scene, layout)
                    .is_none_or(|menu| menu.highlighted.is_none()))
        {
            return false;
        }
        let key = match event.key.as_str() {
            "F4" if !event.repeat => ChoiceKey::Open,
            "ArrowDown" if event.modifiers.alt && !event.repeat => ChoiceKey::Open,
            "ArrowUp" => ChoiceKey::Up,
            "ArrowDown" => ChoiceKey::Down,
            "Home" => ChoiceKey::Home,
            "End" => ChoiceKey::End,
            "PageUp" => ChoiceKey::PageUp,
            "PageDown" => ChoiceKey::PageDown,
            "Enter" => ChoiceKey::Accept,
            " " if !editing => ChoiceKey::Accept,
            "Escape" => ChoiceKey::Escape,
            _ => return false,
        };
        if let Some(input) = self.input.choice_key(scene, layout, key) {
            if !event.repeat
                || !matches!(
                    input.action,
                    ControlAction::AcceptComboText | ControlAction::SubmitSearch
                )
            {
                let scene = scene.clone();
                self.apply_control(&scene, input);
            }
            true
        } else {
            open || self.input.choice_open()
        }
    }

    fn apply_control(&mut self, scene: &Node, input: ControlInput) -> bool {
        let search = scene
            .at_path(&input.path)
            .is_some_and(|node| node.kind == "SearchField");
        match input.action {
            ControlAction::TextCommand(command) => {
                if self.input.focused.as_ref().is_some_and(|target| {
                    target.path == input.path && target.row_key == scene.row_key_at(&target.path)
                }) {
                    self.text_command(scene, command);
                    return true;
                }
                false
            }
            ControlAction::Select(index) if search => self.input.select_search_suggestion(
                &mut self.component,
                scene,
                index,
                Instant::now(),
            ),
            ControlAction::ClearSearch if search => self.input.edit_text(
                &mut self.component,
                scene,
                EditAction::ReplaceAll(""),
                EditSource::Atomic,
                Instant::now(),
            ),
            action => {
                let changed = self.component.control_at(scene, &input.path, action);
                if changed
                    && matches!(action, ControlAction::Spin(_))
                    && self
                        .input
                        .focused
                        .as_ref()
                        .is_some_and(|target| target.path == input.path)
                {
                    self.input.discard_spin_draft();
                }
                changed
            }
        }
    }

    pub fn edit(&mut self, action: EditAction<'_>) {
        self.edit_from(action, EditSource::Keyboard);
    }

    pub fn begin_text_composition(&mut self, renderer: &mut Renderer, id: u64) -> bool {
        self.flush_pointer_carets(renderer);
        let Some((scene, _)) = &self.frame else {
            return false;
        };
        let started = self.input.begin_text_composition(scene, id);
        if started {
            self.clipboard_edit = None;
        }
        started
    }

    pub fn update_text_composition(&mut self, id: u64, text: String) -> bool {
        let updated = self
            .frame
            .as_ref()
            .is_some_and(|(scene, _)| self.input.update_text_composition(scene, id, text));
        self.reveal_editor_caret |= updated;
        updated
    }

    pub fn finish_text_composition(&mut self, id: u64, text: &str) -> bool {
        let Some((scene, _)) = &self.frame else {
            return false;
        };
        let (ended, commit) = self.input.finish_text_composition(scene, id);
        if commit && !text.is_empty() {
            self.edit_from(EditAction::Insert(text), EditSource::Atomic);
        }
        ended
    }

    pub fn cancel_text_composition(&mut self, id: u64) -> bool {
        let cancelled = self.input.cancel_text_composition(Some(id));
        self.reveal_editor_caret |= cancelled;
        cancelled
    }

    pub fn ime_area(&self) -> Option<Rect> {
        self.ime_area
    }

    fn edit_from(&mut self, action: EditAction<'_>, source: EditSource) {
        if let Some((scene, layout)) = &self.frame {
            let _scroll = self.scroll.enter();
            let _focus = self.focus.enter();
            if source == EditSource::Keyboard {
                let navigation = match action {
                    EditAction::Left(extend) => Some((NavigationKey::Left, extend)),
                    EditAction::Right(extend) => Some((NavigationKey::Right, extend)),
                    EditAction::Home(extend) => Some((NavigationKey::Home, extend)),
                    EditAction::End(extend) => Some((NavigationKey::End, extend)),
                    _ => None,
                };
                if let Some((key, extend)) = navigation {
                    if let Some(resize) = self.input.split_key(scene, layout, key) {
                        apply_split_resize(&mut self.component, scene, resize);
                        return;
                    }
                    if self.input.tree_has_focus(scene, layout) {
                        if let Some(ControlInput { path, action }) =
                            tree_navigation(&mut self.input, scene, layout, key, extend)
                        {
                            self.component.control_at(scene, &path, action);
                        }
                        return;
                    }
                }
                if matches!(action, EditAction::Insert(" " | "\n"))
                    && let Some(ControlInput { path, action }) =
                        self.input.tree_toggle(scene, layout)
                {
                    self.component.control_at(scene, &path, action);
                    return;
                }
            }
            let enter =
                source == EditSource::Keyboard && matches!(action, EditAction::Insert("\n"));
            if enter {
                if self.table_editing() {
                    self.input.finish_table_edit(&mut self.component, scene);
                    return;
                }
                if self.input.begin_table_edit(scene, layout) {
                    return;
                }
                if let Some(target) = &self.input.focused
                    && let Some(node) = scene.interactive_at_path(&target.path)
                {
                    let action = if xrack_runtime::combo_editable(node) {
                        Some(xrack_runtime::ControlAction::AcceptComboText)
                    } else if node.kind == "SearchField" {
                        Some(xrack_runtime::ControlAction::SubmitSearch)
                    } else {
                        None
                    };
                    if let Some(action) = action {
                        self.component.control_at(scene, &target.path, action);
                        return;
                    }
                    if xrack_runtime::is_text_input(node.kind)
                        && !xrack_runtime::is_multiline_text_input(node.kind)
                    {
                        if node.accepts_text_submission()
                            && !self.component.dispatch_at(scene, &target.path, "accepted")
                            && let Some(ControlInput { path, action }) =
                                self.input.dialog_accept(scene, layout)
                        {
                            self.component.control_at(scene, &path, action);
                        }
                        return;
                    }
                }
            } else if source == EditSource::Keyboard && matches!(action, EditAction::Insert(_)) {
                self.input.begin_table_typing(scene, layout);
            }
            let spin = self
                .input
                .focused
                .as_ref()
                .and_then(|target| scene.interactive_at_path(&target.path))
                .filter(|node| xrack_runtime::is_spin_box(node.kind));
            self.reveal_editor_caret |= self.input.editor.is_some();
            if let Some(node) = spin {
                if enter {
                    if let Some(ControlInput { path, action }) = self.input.commit_spin_draft() {
                        self.component.control_at(scene, &path, action);
                    }
                } else {
                    self.input
                        .edit_spin_from(node, action, source, Instant::now());
                }
            } else {
                self.input
                    .edit_text(&mut self.component, scene, action, source, Instant::now());
            }
        }
    }

    pub fn paste(&mut self, text: &str) {
        if let Some((scene, layout)) = &self.frame
            && let Some(target) = self.input.table_paste_target(scene, layout)
        {
            let _scroll = self.scroll.enter();
            let _focus = self.focus.enter();
            if let Some(apply) =
                self.component
                    .prepare_table_paste(scene, &target.path, target.row, target.columns)
            {
                apply(text);
            }
            return;
        }
        self.edit_from(EditAction::Insert(text), EditSource::Atomic);
    }

    pub fn replay_edit(&mut self, redo: bool) {
        let Some((scene, layout)) = &self.frame else {
            return;
        };
        let _scroll = self.scroll.enter();
        let _focus = self.focus.enter();
        if let Some(path) = self.input.table_edit_history_target(scene, layout) {
            self.component.replay_table_edit_at(scene, path, redo);
        } else {
            self.input
                .replay_text_edit(&mut self.component, scene, redo);
        }
    }

    fn table_editing(&self) -> bool {
        self.input
            .focused
            .as_ref()
            .is_some_and(|target| self.input.table_cell_editing(&target.path))
    }

    pub fn tree_navigation_enabled(&self) -> bool {
        self.frame
            .as_ref()
            .is_some_and(|(scene, layout)| self.input.tree_has_focus(scene, layout))
    }

    pub fn split_navigation_enabled(&self) -> bool {
        self.frame.as_ref().is_some_and(|(scene, _)| {
            self.input
                .focused
                .as_ref()
                .and_then(|target| scene.interactive_at_path(&target.path))
                .is_some_and(|node| node.kind == "SplitHandle")
        })
    }

    pub fn navigate_tree_or_split(&mut self, key: NavigationKey, extend: bool) {
        let Some((scene, layout)) = &self.frame else {
            return;
        };
        let _scroll = self.scroll.enter();
        let _focus = self.focus.enter();
        if let Some(resize) = self.input.split_key(scene, layout, key) {
            apply_split_resize(&mut self.component, scene, resize);
        } else if let Some(ControlInput { path, action }) =
            tree_navigation(&mut self.input, scene, layout, key, extend)
        {
            self.component.control_at(scene, &path, action);
        }
    }

    pub fn table_tab_enabled(&self) -> bool {
        self.frame
            .as_ref()
            .is_some_and(|(scene, _)| self.input.tab_edit_origin(scene).is_some())
    }

    pub fn table_tab(&mut self, backward: bool) {
        let Some((scene, _)) = &self.frame else {
            return;
        };
        let Some(origin) = self.input.tab_edit_origin(scene) else {
            return;
        };
        let _scroll = self.scroll.enter();
        let _focus = self.focus.enter();
        if matches!(
            self.input.finish_table_edit(&mut self.component, scene),
            TableEditCommit::Changed | TableEditCommit::Unchanged
        ) {
            self.table_tab = Some((origin, backward));
        }
    }

    pub fn delay_keyboard_enabled(&self) -> bool {
        self.frame.as_ref().is_some_and(|(scene, _)| {
            self.input
                .focused
                .as_ref()
                .and_then(|target| scene.interactive_at_path(&target.path))
                .is_some_and(|node| node.kind == "DelayButton")
        })
    }

    pub fn press_delay_key(&mut self, key: ActivationKey) -> bool {
        if self.delay_key.is_some()
            || self.pointer_capture.is_some()
            || !self.delay_keyboard_enabled()
        {
            return false;
        }
        let Some((scene, layout)) = &self.frame else {
            return false;
        };
        self.input.key_press_with_scene(scene, key);
        self.input
            .begin_key_delay(scene, layout, key, Instant::now());
        self.delay_key = Some(key);
        true
    }

    pub fn release_delay_key(&mut self, key: ActivationKey) -> bool {
        if self.delay_key != Some(key) {
            return false;
        }
        self.delay_key = None;
        let Some((scene, layout)) = &self.frame else {
            return false;
        };
        let _scroll = self.scroll.enter();
        let _focus = self.focus.enter();
        if let Some(ControlInput { path, action }) =
            self.input.advance_delay(scene, layout, Instant::now())
        {
            self.component.control_at(scene, &path, action);
        }
        if let Some(target) = self.input.key_release_with_scene(scene, key)
            && scene
                .interactive_at_path(&target.path)
                .is_some_and(|node| node.kind == "DelayButton" && node.boolean("checked", false))
        {
            self.component.control_at(
                scene,
                &target.path,
                xrack_runtime::ControlAction::DelayButton(false),
            );
        }
        true
    }

    pub fn cancel_delay_key(&mut self) -> bool {
        if let Some(key) = self.delay_key.take() {
            self.input.key_release_target(key);
            return true;
        }
        false
    }

    pub fn press_contact(&mut self, contact: PointerContact, point: Point, pan: bool) -> bool {
        if self.touch_scale_rotate(contact, point, TouchPhase::Started) {
            return true;
        }
        let mut captured = false;
        if contact.primary {
            if contact.event.is_touch {
                self.leave();
            } else {
                self.hover_pointer = Some(point);
            }
            if pan {
                captured = self.begin_pan(point);
            } else if contact.event.button == PointerButton::Primary {
                captured = self.begin_pointer_for_device(point, contact.event.is_touch);
                if contact.event.is_touch
                    && self.pointer_capture.is_none()
                    && let Some((scene, layout)) = &self.frame
                    && self.input.begin_scroll_gesture(
                        scene,
                        layout,
                        point,
                        Instant::now(),
                        ScrollGestureKind::Touch,
                    )
                {
                    self.pointer_capture = Some(PointerCapture::ScrollGesture);
                    captured = true;
                }
            }
        }

        if matches!(
            self.pointer_capture,
            Some(
                PointerCapture::ScrollBar { .. }
                    | PointerCapture::TableSelection
                    | PointerCapture::TableResize
                    | PointerCapture::TableReorder
            )
        ) {
            return true;
        }
        let Some((scene, layout)) = &self.frame else {
            return captured;
        };
        let _scroll = self.scroll.enter();
        let _focus = self.focus.enter();
        if !contact.event.is_touch {
            self.input.pointer_move(scene, layout, point);
            if contact.primary && contact.event.button == PointerButton::Primary {
                if self.pointer_capture.is_none()
                    && !self.input.menu_open()
                    && !self.input.choice_open()
                    && let Some(caret) = self.input.hovered.as_ref().and_then(|target| {
                        PointerCaret::new(
                            &self.input,
                            scene,
                            layout,
                            target,
                            point,
                            point,
                            contact.event.modifiers.shift,
                        )
                    })
                {
                    self.input.pointer_press_in_scene(scene, layout);
                    self.input.spin_press(scene, layout);
                    self.pointer_carets.clear();
                    self.pointer_carets.push(caret);
                    self.clipboard_edit = None;
                    self.pointer_capture = Some(PointerCapture::TextSelection);
                    return true;
                }
                self.input.arm_drag(scene, layout, point);
                self.drag_contact = self.input.drag_position().map(|_| DragContact {
                    id: contact.id,
                    requested: contact.event.modifiers.drag_action(),
                    captured: false,
                });
                if self.pointer_capture.is_none()
                    && self.input.press_table_drag_selection(
                        scene,
                        layout,
                        contact.event.modifiers.control || contact.event.modifiers.meta,
                    )
                {
                    self.input.spin_press(scene, layout);
                    self.pointer_capture = Some(PointerCapture::TableSelection);
                    return true;
                }
            }
        }
        let mut context = PointerContext {
            input: &mut self.input,
            component: &mut self.component,
            scene,
            layout,
        };
        self.direct_pointers.begin(&mut context, contact, point) || captured
    }

    pub fn move_contact(&mut self, contact: PointerContact, point: Point) -> bool {
        if self.touch_scale_rotate(contact, point, TouchPhase::Moved)
            || self.move_drag_contact(contact, point)
        {
            return true;
        }
        let mut changed = contact.primary && self.move_pointer(point);
        if !contact.event.is_touch {
            changed |= self.hover(point);
        }
        let Some((scene, layout)) = &self.frame else {
            return changed;
        };
        let _scroll = self.scroll.enter();
        let _focus = self.focus.enter();
        let mut context = PointerContext {
            input: &mut self.input,
            component: &mut self.component,
            scene,
            layout,
        };
        self.direct_pointers.advance(&mut context, contact, point) || changed
    }

    pub fn finish_contact(
        &mut self,
        contact: PointerContact,
        point: Point,
        click: bool,
        canceled: bool,
    ) -> bool {
        let phase = if canceled {
            TouchPhase::Cancelled
        } else {
            TouchPhase::Ended
        };
        if self.touch_scale_rotate(contact, point, phase)
            || self.finish_drag_contact(contact, point, canceled)
        {
            self.discard_direct_contact(contact, point);
            return true;
        }
        if !self.direct_pointers.contains(contact.id) {
            if !contact.primary {
                return false;
            }
            if contact.event.button == PointerButton::Primary {
                self.direct_pointers.clear_click();
            }
            return if canceled {
                self.cancel_widget_capture()
            } else {
                self.release_pointer_with_modifiers(
                    point,
                    click && contact.event.button == PointerButton::Primary,
                    contact.event.modifiers,
                )
            };
        }
        if !canceled && contact.primary {
            self.move_pointer(point);
            if !contact.event.is_touch {
                self.hover(point);
            }
        }
        let Some((scene, layout)) = &self.frame else {
            return false;
        };
        let _scroll = self.scroll.enter();
        let _focus = self.focus.enter();
        let mut context = PointerContext {
            input: &mut self.input,
            component: &mut self.component,
            scene,
            layout,
        };
        let Some(release) = self
            .direct_pointers
            .release(&mut context, contact, point, canceled)
        else {
            return false;
        };
        let activate = if contact.primary
            && matches!(self.pointer_capture, Some(PointerCapture::ScrollGesture))
        {
            self.pointer_capture = None;
            if canceled {
                self.input.cancel_table_touch_drag();
                self.input.cancel_scroll_gesture();
                false
            } else {
                let end = self
                    .input
                    .end_scroll_gesture(scene, layout, point, Instant::now());
                if let Some(ControlInput { path, action }) = end.control {
                    self.component.control_at(scene, &path, action);
                }
                end.tap
            }
        } else if !contact.event.is_touch && contact.event.button == PointerButton::Primary {
            self.input.pointer_release_target() == release.target
        } else {
            false
        };
        if activate && let Some(target) = release.target {
            let mut context = PointerContext {
                input: &mut self.input,
                component: &mut self.component,
                scene,
                layout,
            };
            self.direct_pointers
                .click(&mut context, &target, contact, point);
        } else if contact.primary && contact.event.button == PointerButton::Primary {
            self.direct_pointers.clear_click();
        }
        true
    }

    fn cancel_contact_capture(&mut self, contacts: impl IntoIterator<Item = u64>) {
        self.cancel_widget_capture();
        let Some((scene, layout)) = &self.frame else {
            return;
        };
        let mut context = PointerContext {
            input: &mut self.input,
            component: &mut self.component,
            scene,
            layout,
        };
        for id in contacts {
            self.direct_pointers.cancel(&mut context, id);
        }
        self.direct_pointers.clear_click();
    }

    fn discard_direct_contact(&mut self, contact: PointerContact, point: Point) {
        let Some((scene, layout)) = &self.frame else {
            return;
        };
        let mut context = PointerContext {
            input: &mut self.input,
            component: &mut self.component,
            scene,
            layout,
        };
        self.direct_pointers
            .release(&mut context, contact, point, true);
    }

    fn touch_scale_rotate(
        &mut self,
        contact: PointerContact,
        point: Point,
        phase: TouchPhase,
    ) -> bool {
        if !contact.event.is_touch {
            return false;
        }
        let Some((scene, layout)) = &self.frame else {
            return false;
        };
        let scene = scene.clone();
        let _scroll = self.scroll.enter();
        let _focus = self.focus.enter();
        let (consumed, deliveries) = self.scale_rotate.touch(
            &scale_rotate_gestures::Context {
                scene: &scene,
                layout,
                input: &self.input,
            },
            scale_rotate_gestures::ContactId((), contact.id),
            phase,
            point,
        );
        if deliveries
            .iter()
            .flatten()
            .any(|event| event.event == "started")
        {
            let pair = self
                .scale_rotate
                .captured_touch_pair()
                .into_iter()
                .flatten();
            self.cancel_contact_capture(pair.map(|id| id.1));
        }
        for delivery in deliveries.into_iter().flatten() {
            delivery.apply(&mut self.component, &scene);
        }
        consumed
    }

    fn move_drag_contact(&mut self, contact: PointerContact, point: Point) -> bool {
        let Some(drag) = self
            .drag_contact
            .as_mut()
            .filter(|drag| drag.id == contact.id)
        else {
            return false;
        };
        let Some((scene, layout)) = &self.frame else {
            return false;
        };
        let scene = scene.clone();
        drag.requested = contact.event.modifiers.drag_action();
        let update = self
            .input
            .drag_motion(&scene, layout, point, drag.requested);
        let started = update.captured && !drag.captured;
        drag.captured |= update.captured;
        let captured = drag.captured;
        let _scroll = self.scroll.enter();
        let _focus = self.focus.enter();
        if started {
            self.cancel_contact_capture([contact.id]);
        }
        self.dispatch_drag(update, &scene);
        captured
    }

    fn finish_drag_contact(
        &mut self,
        contact: PointerContact,
        point: Point,
        canceled: bool,
    ) -> bool {
        if !self
            .drag_contact
            .as_ref()
            .is_some_and(|drag| drag.id == contact.id)
        {
            return false;
        }
        let drag = self.drag_contact.take().expect("matched drag contact");
        let Some((scene, layout)) = &self.frame else {
            return drag.captured;
        };
        let scene = scene.clone();
        let update = if canceled {
            self.input.cancel_drag()
        } else {
            self.input
                .finish_drag(&scene, layout, point, contact.event.modifiers.drag_action())
        };
        let captured = drag.captured || update.captured;
        let _scroll = self.scroll.enter();
        let _focus = self.focus.enter();
        self.dispatch_drag(update, &scene);
        captured
    }

    fn dispatch_drag(&mut self, update: DragUpdate, scene: &Node) -> bool {
        let mut changed = update.captured;
        for event in update.events {
            changed |= event.apply(&mut self.component, scene);
        }
        changed
    }

    pub fn file_drag(
        &mut self,
        sequence: u64,
        point: Point,
        paths: Vec<String>,
        drop: bool,
    ) -> bool {
        let Some((scene, layout)) = &self.frame else {
            return false;
        };
        let scene = scene.clone();
        let paths: std::sync::Arc<[String]> = paths.into();
        let event = if drop {
            let Some(target) = self
                .input
                .file_drop_scope(layout)
                .target(&scene, layout, point, &paths)
            else {
                return self.leave_file_drag(sequence);
            };
            FileDragInput::Dropped {
                sequence,
                point,
                paths,
                target,
            }
        } else {
            FileDragInput::Moved {
                sequence,
                point,
                paths,
            }
        };
        let update = self.input.file_drag_event(&scene, layout, event);
        let _scroll = self.scroll.enter();
        let _focus = self.focus.enter();
        self.dispatch_drag(update, &scene)
    }

    pub fn leave_file_drag(&mut self, sequence: u64) -> bool {
        let Some((scene, layout)) = &self.frame else {
            return false;
        };
        let scene = scene.clone();
        let update = self
            .input
            .file_drag_event(&scene, layout, FileDragInput::Left { sequence });
        let _scroll = self.scroll.enter();
        let _focus = self.focus.enter();
        self.dispatch_drag(update, &scene)
    }

    pub fn begin_pointer(&mut self, point: Point) -> bool {
        self.begin_pointer_for_device(point, false)
    }

    fn begin_pointer_for_device(&mut self, point: Point, touch: bool) -> bool {
        let cancelled = self.cancel_pointer_captures()
            | self.cancel_delay_key()
            | self.input.cancel_text_composition(None);
        let Some((scene, layout)) = &self.frame else {
            return cancelled;
        };
        let _scroll = self.scroll.enter();
        let _focus = self.focus.enter();
        let scroll_bar = if touch {
            self.input.touch_scroll_bar_press(scene, layout, point)
        } else {
            self.input.pointer_move(scene, layout, point);
            self.input.scroll_bar_press(scene, layout)
        };
        if let Some(update) = scroll_bar {
            apply_scroll_bar_update(&mut self.component, scene, update);
            self.pointer_capture = Some(PointerCapture::ScrollBar { touch });
            return true;
        }
        if !touch && self.input.press_table_selection_handle(scene, layout) {
            self.pointer_capture = Some(PointerCapture::TableSelection);
            return true;
        }
        if !touch && (self.input.table_resize_cursor() || self.input.table_row_resize_cursor()) {
            self.input.pointer_press_in_scene(scene, layout);
            self.pointer_capture = Some(PointerCapture::TableResize);
            return true;
        }
        if !touch && self.input.press_table_reorder(scene, layout) {
            self.pointer_capture = Some(PointerCapture::TableReorder);
            return true;
        }
        if self.begin_split_drag(point) {
            self.pointer_capture = Some(PointerCapture::Split);
            return true;
        }
        let Some((scene, layout)) = &self.frame else {
            return cancelled;
        };
        if self.input.range_pointer_press(scene, layout, point) {
            self.pointer_capture = Some(PointerCapture::Range);
            if let Some(input) = self.input.slider_press(scene, layout) {
                self.component
                    .adjust_range_at(scene, &input.path, input.action);
            }
            return true;
        }
        if self.input.begin_scroll_gesture(
            scene,
            layout,
            point,
            Instant::now(),
            ScrollGestureKind::MouseControl,
        ) {
            self.pointer_capture = Some(PointerCapture::ScrollGesture);
            return true;
        }
        self.input.pointer_move(scene, layout, point);
        if !self
            .input
            .hovered
            .as_ref()
            .and_then(|target| scene.interactive_at_path(&target.path))
            .is_some_and(|node| node.kind == "DelayButton" && !node.boolean("checked", false))
        {
            return cancelled;
        }
        self.input.pointer_press_in_scene(scene, layout);
        self.input
            .begin_pointer_delay(scene, layout, Instant::now());
        if self.input.delay_deadline().is_none() {
            self.input.cancel_press();
            return cancelled;
        }
        self.pointer_capture = Some(PointerCapture::Delay);
        true
    }

    pub fn begin_pan(&mut self, point: Point) -> bool {
        let cancelled = self.cancel_pointer_captures() | self.cancel_delay_key();
        let Some((scene, layout)) = &self.frame else {
            return cancelled;
        };
        if !self.input.begin_scroll_gesture(
            scene,
            layout,
            point,
            Instant::now(),
            ScrollGestureKind::MousePan,
        ) {
            return cancelled;
        }
        self.pointer_capture = Some(PointerCapture::ScrollGesture);
        true
    }

    pub fn move_pointer(&mut self, point: Point) -> bool {
        match self.pointer_capture {
            Some(PointerCapture::Split) => self.move_split_drag(point),
            Some(PointerCapture::ScrollGesture) => {
                self.frame.as_ref().is_some_and(|(scene, layout)| {
                    let moved =
                        self.input
                            .move_scroll_gesture(scene, layout, point, Instant::now());
                    moved || self.input.has_pending_swipe_events()
                })
            }
            Some(PointerCapture::ScrollBar { touch }) => {
                let Some((scene, layout)) = &self.frame else {
                    return false;
                };
                let _scroll = self.scroll.enter();
                let _focus = self.focus.enter();
                let update = if touch {
                    Some(self.input.touch_scroll_bar_drag(scene, layout, point))
                } else {
                    self.input.pointer_move(scene, layout, point);
                    self.input.scroll_bar_drag(scene, layout)
                };
                update.is_some_and(|update| {
                    apply_scroll_bar_update(&mut self.component, scene, update)
                })
            }
            Some(PointerCapture::TableResize) => {
                let Some((scene, layout)) = &self.frame else {
                    return false;
                };
                let _scroll = self.scroll.enter();
                let _focus = self.focus.enter();
                let changed = self.input.pointer_move(scene, layout, point);
                if let Some(ControlInput { path, action }) = self
                    .input
                    .table_resize_drag(scene, layout)
                    .or_else(|| self.input.table_row_resize_drag(scene, layout))
                {
                    self.component.control_at(scene, &path, action) || changed
                } else {
                    changed
                }
            }
            Some(PointerCapture::TextSelection) => {
                let Some((scene, layout)) = &self.frame else {
                    return false;
                };
                if self.input.pointer_position() == Some(point) {
                    return false;
                }
                self.input.pointer_move(scene, layout, point);
                let Some(caret) =
                    PointerCaret::from_pressed(&self.input, scene, layout, point, true)
                else {
                    return false;
                };
                // Preserve a queued press and coalesce later drag positions.
                if self.pointer_carets.len() == 2 {
                    self.pointer_carets.pop();
                }
                self.pointer_carets.push(caret);
                true
            }
            Some(PointerCapture::TableReorder) => {
                let Some((scene, layout)) = &self.frame else {
                    return false;
                };
                let moved = self.input.pointer_move(scene, layout, point);
                let now = Instant::now();
                self.input.table_row_move_drag(layout, now)
                    || self.input.table_column_move_drag(layout, now)
                    || moved
            }
            Some(PointerCapture::TableSelection) => {
                let Some((scene, layout)) = &self.frame else {
                    return false;
                };
                let _focus = self.focus.enter();
                let moved = self.input.pointer_move(scene, layout, point);
                self.input.move_table_drag_selection(layout, Instant::now()) || moved
            }
            Some(PointerCapture::Range) => {
                let Some((scene, layout)) = &self.frame else {
                    return false;
                };
                if self.input.pointer_position() == Some(point) {
                    return false;
                }
                let _scroll = self.scroll.enter();
                let _focus = self.focus.enter();
                self.input.pointer_move(scene, layout, point);
                if let Some(input) = self.input.slider_drag(scene, layout) {
                    self.component
                        .adjust_range_at(scene, &input.path, input.action);
                }
                true
            }
            Some(PointerCapture::Delay) => {
                let Some((scene, layout)) = &self.frame else {
                    return false;
                };
                let _scroll = self.scroll.enter();
                let _focus = self.focus.enter();
                self.input.pointer_move(scene, layout, point);
                if let Some(ControlInput { path, action }) =
                    self.input.advance_delay(scene, layout, Instant::now())
                {
                    self.component.control_at(scene, &path, action);
                }
                true
            }
            None => false,
        }
    }

    pub fn release_pointer(&mut self, point: Point, click: bool) -> bool {
        self.release_pointer_with_modifiers(point, click, Default::default())
    }

    fn release_pointer_with_modifiers(
        &mut self,
        point: Point,
        click: bool,
        modifiers: xrack_runtime::KeyModifiers,
    ) -> bool {
        if matches!(self.pointer_capture, Some(PointerCapture::ScrollGesture)) {
            self.pointer_capture = None;
            let Some((scene, layout)) = &self.frame else {
                return false;
            };
            let _scroll = self.scroll.enter();
            let _focus = self.focus.enter();
            let end = self
                .input
                .end_scroll_gesture(scene, layout, point, Instant::now());
            if let Some(ControlInput { path, action }) = end.control {
                self.component.control_at(scene, &path, action);
            } else if end.tap {
                self.click_with_modifiers(point, modifiers);
            }
            return true;
        }
        if let Some(PointerCapture::ScrollBar { touch }) = self.pointer_capture {
            self.pointer_capture = None;
            let Some((scene, layout)) = &self.frame else {
                return false;
            };
            let _scroll = self.scroll.enter();
            let _focus = self.focus.enter();
            let update = if touch {
                self.input.release_touch_scroll_bar(scene, layout, point)
            } else {
                self.input.pointer_move(scene, layout, point);
                self.input.release_scroll_bar(scene, layout)
            };
            if let Some(update) = update {
                apply_scroll_bar_update(&mut self.component, scene, update);
            }
            return true;
        }
        if let Some(capture @ (PointerCapture::TableSelection | PointerCapture::TableReorder)) =
            self.pointer_capture
        {
            self.move_pointer(point);
            self.pointer_capture = None;
            let committed = match capture {
                PointerCapture::TableReorder => self.finish_table_reorder(),
                _ => self.finish_table_selection(),
            };
            if !committed {
                if click {
                    self.release_pressed_click(modifiers);
                } else {
                    self.input.cancel_press();
                }
            }
            return true;
        }
        if matches!(self.pointer_capture, Some(PointerCapture::TextSelection)) {
            self.move_pointer(point);
            self.pointer_capture = None;
            if click {
                self.release_pressed_click(modifiers);
            } else {
                self.input.cancel_press();
            }
            return true;
        }
        if self.pointer_capture.is_some() {
            self.move_pointer(point);
            if matches!(self.pointer_capture, Some(PointerCapture::Range))
                && let Some((scene, layout)) = &self.frame
                && let Some(input) = self.input.slider_release(scene, layout)
            {
                let _scroll = self.scroll.enter();
                let _focus = self.focus.enter();
                self.component
                    .adjust_range_at(scene, &input.path, input.action);
            }
            self.cancel_pointer();
            return true;
        }
        if click {
            self.click_with_modifiers(point, modifiers);
        }
        click
    }

    fn finish_table_reorder(&mut self) -> bool {
        let row = self.input.table_row_move_claimed();
        if !row && !self.input.table_column_move_claimed() {
            return false;
        }
        let _scroll = self.scroll.enter();
        let _focus = self.focus.enter();
        if let Some((scene, layout)) = &self.frame {
            let action = if row {
                self.input.finish_table_row_move(scene, layout)
            } else {
                self.input.finish_table_column_move(scene, layout)
            };
            if let Some(ControlInput { path, action }) = action {
                self.component.control_at(scene, &path, action);
            }
        } else {
            self.input.cancel_press();
        }
        true
    }

    fn finish_table_selection(&mut self) -> bool {
        if !self.input.table_drag_selection_claimed() {
            return false;
        }
        let _scroll = self.scroll.enter();
        let _focus = self.focus.enter();
        if let Some((scene, layout)) = &self.frame
            && let Some(ControlInput { path, action }) =
                self.input.finish_table_drag_selection(scene, layout)
        {
            self.component.control_at(scene, &path, action);
        } else {
            self.input.cancel_table_drag_selection();
        }
        true
    }

    pub fn cancel_pointer(&mut self) -> bool {
        let canceled = self.cancel_pointer_captures();
        let scale_rotate = self.scale_rotate.cancel();
        let drag = self.input.cancel_drag();
        let Some((scene, _)) = &self.frame else {
            return canceled;
        };
        let scene = scene.clone();
        let _scroll = self.scroll.enter();
        let _focus = self.focus.enter();
        let mut changed = canceled || scale_rotate.is_some();
        if let Some(event) = scale_rotate {
            event.apply(&mut self.component, &scene);
        }
        changed |= self.dispatch_drag(drag, &scene);
        changed
    }

    fn cancel_pointer_captures(&mut self) -> bool {
        let widgets = self.cancel_widget_capture();
        let Some((scene, layout)) = &self.frame else {
            return widgets;
        };
        let _scroll = self.scroll.enter();
        let _focus = self.focus.enter();
        let mut context = PointerContext {
            input: &mut self.input,
            component: &mut self.component,
            scene,
            layout,
        };
        self.direct_pointers.cancel_all(&mut context) || widgets
    }

    fn cancel_widget_capture(&mut self) -> bool {
        match self.pointer_capture.take() {
            Some(PointerCapture::Split) => self.finish_split_drag(),
            Some(PointerCapture::ScrollGesture) => {
                self.input.cancel_table_touch_drag();
                self.input.cancel_scroll_gesture();
                true
            }
            Some(PointerCapture::TextSelection) => {
                self.pointer_carets.clear();
                self.input.cancel_press();
                true
            }
            Some(PointerCapture::ScrollBar { touch: true }) => self.input.finish_touch_scroll_bar(),
            Some(PointerCapture::ScrollBar { touch: false }) => self.input.finish_scroll_bar(),
            Some(
                PointerCapture::Delay
                | PointerCapture::Range
                | PointerCapture::TableSelection
                | PointerCapture::TableResize
                | PointerCapture::TableReorder,
            ) => {
                self.input.cancel_press();
                true
            }
            None => false,
        }
    }

    pub fn begin_split_drag(&mut self, point: Point) -> bool {
        self.frame
            .as_ref()
            .is_some_and(|(scene, layout)| self.input.touch_split_press(scene, layout, point))
    }

    pub fn move_split_drag(&mut self, point: Point) -> bool {
        let Some((scene, layout)) = &self.frame else {
            return false;
        };
        if !self.input.split_resize_active() {
            return false;
        }
        let _scroll = self.scroll.enter();
        let _focus = self.focus.enter();
        self.input.pointer_move(scene, layout, point);
        if let Some(resize) = self.input.split_drag(scene, layout) {
            apply_split_resize(&mut self.component, scene, resize);
        }
        true
    }

    pub fn finish_split_drag(&mut self) -> bool {
        self.input.finish_split_resize()
    }

    pub fn wheel(&mut self, point: Point, delta: Point, shift: bool) {
        self.wheel_event(
            point,
            WheelEvent {
                delta_x: xrack_runtime::Length(f64::from(delta.x)),
                delta_y: xrack_runtime::Length(f64::from(delta.y)),
                modifiers: xrack_runtime::KeyModifiers {
                    shift,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
    }

    pub fn wheel_event(&mut self, point: Point, event: WheelEvent) {
        self.wheel_input(point, event, None);
    }

    pub fn wheel_event_with_renderer(
        &mut self,
        renderer: &mut Renderer,
        point: Point,
        event: WheelEvent,
    ) {
        self.wheel_input(point, event, Some(renderer));
    }

    fn wheel_input(
        &mut self,
        point: Point,
        event: WheelEvent,
        mut renderer: Option<&mut Renderer>,
    ) {
        let Some((scene, layout)) = &self.frame else {
            return;
        };
        let _scroll = self.scroll.enter();
        let _focus = self.focus.enter();
        self.input.pointer_move(scene, layout, point);
        self.input.dismiss_tooltip();
        let delta = Point {
            x: event.delta_x.0 as f32,
            y: event.delta_y.0 as f32,
        };
        let rows = if delta.y == 0.0 {
            0
        } else {
            delta.y.signum() as isize * ((delta.y.abs() / 28.0).round() as isize).max(1)
        };
        if self.input.menu_scroll(scene, layout, rows)
            || self.input.choice_scroll(scene, layout, rows)
        {
            self.input.reset_wheel_adjustment();
            return;
        }
        let handled = self
            .input
            .wheel_route(scene, layout, event)
            .map(|route| route.dispatch(&mut self.component, scene));
        if handled == Some(EventResult::Accept) {
            self.input.reset_wheel_adjustment();
            return;
        }
        if handled.is_some()
            && (self.component.scene_dirty() || !self.focus.is_empty() || !self.scroll.is_empty())
            && let Some(renderer) = renderer.as_deref_mut()
        {
            self.refresh_input_frame(renderer);
        }
        let Some((scene, layout)) = &self.frame else {
            return;
        };
        let changed = match self
            .input
            .wheel_adjustment(scene, layout, event, Instant::now())
        {
            Some(WheelAdjustment::Pending) => true,
            Some(WheelAdjustment::Range(input)) => {
                self.component
                    .adjust_range_at(scene, &input.path, input.action)
            }
            Some(WheelAdjustment::Spin(input)) => {
                let scene = scene.clone();
                self.apply_control(&scene, input)
            }
            None => false,
        };
        if changed {
            return;
        }
        if !event.modifiers.shift
            && delta.y.abs() > delta.x.abs()
            && let Some((scene, layout)) = &self.frame
        {
            let distance = if event.is_line {
                event.lines_y as f32
            } else {
                delta.y
            };
            match self
                .input
                .tumbler_scroll(scene, layout, distance, event.is_line)
            {
                TumblerScroll::Select(ControlInput { path, action }) => {
                    self.component.control_at(scene, &path, action);
                    return;
                }
                TumblerScroll::Pending => return,
                TumblerScroll::Unhandled => {}
            }
        }
        if let Some((scene, layout)) = &self.frame {
            let remaining = renderer.as_deref().map_or(delta, |renderer| {
                renderer
                    .text
                    .scroll_editor_area(&mut self.input, scene, layout, delta)
            });
            self.input
                .scroll_wheel(layout, remaining, event.modifiers.shift && delta.x == 0.0);
        }
    }

    pub fn escape(&mut self) {
        if self.input.cancel_text_composition(None) || self.input.dismiss_tooltip() {
            return;
        }
        let _scroll = self.scroll.enter();
        let _focus = self.focus.enter();
        self.default_key(
            &KeyEvent {
                key: "Escape".into(),
                ..KeyEvent::default()
            },
            true,
            None,
        );
    }

    fn cancel_pointer_or_delay(&mut self) -> bool {
        if matches!(self.pointer_capture, Some(PointerCapture::Delay)) {
            self.input.cancel_delay();
            return true;
        }
        self.cancel_pointer() || self.cancel_delay_key()
    }

    fn escape_focused(&mut self) -> bool {
        let Some((scene, layout)) = &self.frame else {
            return false;
        };
        if self.input.cancel_table_edit() {
            return true;
        }
        let was_open = self.input.menu_open() || self.input.choice_open();
        let action = if self.input.menu_open() {
            self.input
                .menu_key(scene, layout, xrack_layout::MenuKey::Escape)
        } else if self.input.choice_open() {
            self.input
                .choice_key(scene, layout, xrack_layout::ChoiceKey::Escape)
        } else {
            self.input
                .dismiss_focused_swipe(scene, layout)
                .or_else(|| self.input.dialog_escape(scene, layout))
        };
        if let Some(ControlInput { path, action }) = action {
            self.component.control_at(scene, &path, action);
            return true;
        }
        was_open
    }

    pub fn click(&mut self, point: Point) {
        self.click_with_modifiers(point, Default::default());
    }

    fn click_with_modifiers(&mut self, point: Point, modifiers: xrack_runtime::KeyModifiers) {
        self.input.cancel_text_composition(None);
        let Some((scene, layout)) = &self.frame else {
            return;
        };
        let _scroll = self.scroll.enter();
        let _focus = self.focus.enter();
        self.input.pointer_move(scene, layout, point);
        if !self.input.menu_open()
            && !self.input.choice_open()
            && let Some(ControlInput { path, action }) =
                self.input.popup_outside_press(scene, layout)
        {
            self.component.control_at(scene, &path, action);
        }
        if let Some(update) = self.input.scroll_bar_press(scene, layout) {
            for update in [Some(update), self.input.release_scroll_bar(scene, layout)]
                .into_iter()
                .flatten()
            {
                if let ScrollBarUpdate::Range(RangeInput { path, action }) = update {
                    self.component.adjust_range_at(scene, &path, action);
                }
            }
            return;
        }
        let action = if self.input.menu_press(scene, layout) {
            self.input.menu_release(scene, layout)
        } else if self.input.choice_open() {
            self.input.choice_release(scene, layout)
        } else if self.input.dialog_pointer_press(layout) {
            self.input.dialog_pointer_release(scene, layout)
        } else {
            self.input.pointer_press_in_scene(scene, layout);
            let pressed = self.input.slider_press(scene, layout);
            if let Some(RangeInput { path, action }) = &pressed {
                self.component.adjust_range_at(scene, path, *action);
            }
            let released = self.input.slider_release(scene, layout);
            if let Some(RangeInput { path, action }) = &released {
                self.component.adjust_range_at(scene, path, *action);
            }
            if pressed.is_some() || released.is_some() {
                self.input.pointer_release_target();
                return;
            }
            if let Some(caret) =
                PointerCaret::from_pressed(&self.input, scene, layout, point, modifiers.shift)
            {
                self.pointer_carets.clear();
                self.pointer_carets.push(caret);
                self.clipboard_edit = None;
            }
            self.input.spin_press(scene, layout);
            self.release_pressed_click(modifiers);
            return;
        };
        if let Some(input) = action {
            let scene = scene.clone();
            self.apply_control(&scene, input);
        }
    }

    fn release_pressed_click(&mut self, modifiers: xrack_runtime::KeyModifiers) {
        let Some((scene, layout)) = &self.frame else {
            return;
        };
        let _scroll = self.scroll.enter();
        let _focus = self.focus.enter();
        let selection = RowSelection::from_modifiers(modifiers, false);
        let action = self
            .input
            .spin_release(scene, layout)
            .or_else(|| self.input.control_part_release(scene, layout))
            .or_else(|| {
                self.input
                    .table_header_release(scene, selection, modifiers.alt)
            })
            .or_else(|| {
                let target = self.input.pointer_release_target()?;
                if let Some(node) = scene.interactive_at_path(&target.path)
                    && node.kind == "DelayButton"
                {
                    return node.boolean("checked", false).then_some(ControlInput {
                        path: target.path,
                        action: xrack_runtime::ControlAction::DelayButton(false),
                    });
                }
                if let Some(selection) = self
                    .input
                    .list_item_delegate_selection(scene, layout, &target)
                    && let Some(row) = target.row_key
                    && self
                        .component
                        .control_at(scene, &selection.path, selection.action)
                {
                    self.list_item_click = Some((selection.path, row));
                    return None;
                }
                if let Some(update) = self
                    .input
                    .table_delegate_selection(scene, layout, &target, selection)
                {
                    self.component
                        .control_at(scene, &update.path, update.action);
                    self.table_cell_click = Some(target);
                    return None;
                }
                self.input
                    .table_cell_click(scene, layout, Some(&target), Instant::now());
                let action = self
                    .input
                    .dialog_button(scene, layout, &target)
                    .or_else(|| self.input.tree_pointer_action(scene, layout, &target))
                    .or_else(|| {
                        self.input
                            .select_group_option_with_selection(scene, layout, &target, selection)
                    })
                    .or_else(|| self.input.text_link_activation(scene, layout, &target));
                if action.is_none()
                    && !self.input.open_menu_bar(scene, layout, target.clone())
                    && !self
                        .input
                        .open_choice_at_pointer(scene, layout, target.clone())
                {
                    self.component.dispatch_at(scene, &target.path, "clicked");
                }
                action
            });
        if let Some(input) = action {
            let scene = scene.clone();
            self.apply_control(&scene, input);
        }
    }

    pub fn render(&mut self, renderer: &mut Renderer) -> Result<&[u8], String> {
        self.prepare_snapshot(renderer, false);
        self.draw_prepared(renderer)?;
        self.renderer_needs_redraw = false;
        Ok(&self.png)
    }

    pub fn render_animated(&mut self, renderer: &mut Renderer) -> Result<&[u8], String> {
        self.advance(renderer);
        self.draw_prepared(renderer)
    }

    #[doc(hidden)]
    pub fn advance(&mut self, renderer: &mut Renderer) {
        let now = Instant::now();
        self.sampled_at = now.duration_since(self.clock);
        self.image_playback.advance(now);
        {
            let _scroll = self.scroll.enter();
            let _focus = self.focus.enter();
            for _ in 0..64 {
                let Some(delivery) = self.image_playback.next_finished() else {
                    break;
                };
                delivery.finish();
            }
        }
        self.prepare_snapshot(renderer, true);
    }

    #[doc(hidden)]
    pub fn draw_prepared(&mut self, renderer: &mut Renderer) -> Result<&[u8], String> {
        let (scene, layout) = self
            .frame
            .take()
            .ok_or("preview window has no prepared scene")?;
        let (width, height) = self.dimensions;
        let snapshot = renderer.snapshot_prepared_with_transitions(
            &scene,
            layout,
            [width, height],
            1.0,
            &self.input,
            &self.stack_transitions,
        )?;
        self.renderer_needs_redraw = renderer.needs_redraw();
        self.png.clear();
        snapshot.write_png(&mut self.png)?;
        self.frame = Some((scene, snapshot.layout));
        Ok(&self.png)
    }

    #[doc(hidden)]
    pub fn title(&self) -> Option<&str> {
        self.frame
            .as_ref()?
            .0
            .text("title")
            .filter(|title| !title.is_empty())
    }

    #[doc(hidden)]
    pub fn secondary_windows(&self) -> Option<Rc<[xrack_runtime::SecondaryWindowRegistration]>> {
        self.component.secondary_windows()
    }

    pub fn next_frame_delay(&self) -> Option<Duration> {
        if self.window_closed {
            return None;
        }
        // Use the sampled time so an expensive render still requests the final frame.
        let deadline = self
            .transition
            .next_frame(self.sampled_at)
            .map(|deadline| self.clock + deadline);
        let now = Instant::now();
        let hold = self
            .input
            .delay_deadline()
            .map(|deadline| deadline.min(now + Duration::from_millis(16)));
        deadline
            .into_iter()
            .chain(self.image_playback.deadline())
            .chain(self.timers.deadline())
            .chain(self.input.tooltip_deadline())
            .chain(self.direct_pointers.deadline())
            .chain(self.input.scroll_motion_deadline())
            .chain(self.input.table_touch_hold_deadline())
            .chain(self.frame.as_ref().and_then(|(scene, layout)| {
                self.input.table_drag_selection_deadline(scene, layout)
            }))
            .chain(
                self.frame.as_ref().and_then(|(scene, layout)| {
                    self.input.table_column_move_deadline(scene, layout)
                }),
            )
            .chain(
                self.frame
                    .as_ref()
                    .and_then(|(scene, layout)| self.input.table_row_move_deadline(scene, layout)),
            )
            .chain(self.input.has_pending_swipe_events().then_some(now))
            .chain(hold)
            .chain(
                (self.renderer_needs_redraw || self.stack_transitions.is_active())
                    .then_some(now + Duration::from_millis(16)),
            )
            .min()
            .map(|deadline| {
                deadline
                    .saturating_duration_since(now)
                    .max(Duration::from_millis(16))
            })
    }

    fn scene(&mut self, animated: bool) -> Rc<Node> {
        let target = self.component.render_shared();
        if !animated {
            return target;
        }
        if let Some(previous) = &self.target
            && !Rc::ptr_eq(previous, &target)
        {
            self.transition.retarget(previous, &target, self.sampled_at);
        }
        self.target = Some(target.clone());
        match self.transition.frame(&target, self.sampled_at) {
            Cow::Borrowed(_) => target,
            Cow::Owned(scene) => Rc::new(scene),
        }
    }

    fn prepare_frame(
        &mut self,
        renderer: &mut Renderer,
        animated: bool,
    ) -> (Rc<Node>, LayoutNode, u32, u32) {
        let scene = self.scene(animated);
        let dimension = |name, default| {
            scene
                .length(name)
                .filter(|value| value.is_finite() && *value > 0.0)
                .unwrap_or(default)
                .ceil()
                .clamp(1.0, 2048.0) as u32
        };
        let (width, height) = (dimension("width", 640.0), dimension("height", 480.0));
        let size = Size::new(width as f32, height as f32);
        let (scene, layout) = renderer.prepare_scene(&self.component, scene, size, &self.input);
        (scene, layout, width, height)
    }

    fn settle_frame(
        &mut self,
        renderer: &mut Renderer,
        animated: bool,
    ) -> (Rc<Node>, LayoutNode, u32, u32) {
        let (mut scene, mut layout, mut width, mut height) = self.prepare_frame(renderer, animated);
        self.input.reconcile(&scene, &layout);
        let deliveries = self.input.take_scroll_events(&scene, &layout);
        // Layout reads can invalidate bindings. Bound the settling loop for
        // scenes that use their own measured size to drive their constraints.
        let mut previous = self.frame.as_ref().map(|(scene, _)| scene.clone());
        for pass in 0..8 {
            if self
                .input
                .apply_scroll_requests(&scene, &layout, self.scroll.drain())
            {
                (scene, layout, width, height) = self.prepare_frame(renderer, animated);
                self.input.reconcile(&scene, &layout);
            }
            self.observers.sync(&scene, &layout);
            let mut changed = animated && self.sync_window_effects(&scene, &layout);
            if animated {
                self.input.reconcile_swipe(&scene, &layout);
                for event in self.input.take_swipe_events() {
                    let target = if event.matches(&scene) {
                        &scene
                    } else {
                        previous.as_ref().unwrap_or(&scene)
                    };
                    event.apply(&mut self.component, target);
                    changed = true;
                }
                let context = scale_rotate_gestures::Context {
                    scene: &scene,
                    layout: &layout,
                    input: &self.input,
                };
                if let Some(event) = self.scale_rotate.reconcile(&context) {
                    event.apply(&mut self.component, previous.as_ref().unwrap_or(&scene));
                    changed = true;
                }
                let requested = self.drag_contact.as_ref().and_then(|drag| drag.requested);
                let update = self.input.reconcile_drag(&scene, &layout, requested);
                let files = self.input.reconcile_file_drag(&scene, &layout);
                for event in update.events.into_iter().chain(files.events) {
                    changed |= event.apply(&mut self.component, &scene);
                }
            }
            if pass == 0 {
                for event in &deliveries {
                    changed |= event.apply(&mut self.component, &scene);
                }
            }
            if !changed && !self.component.scene_dirty() && self.scroll.is_empty() {
                break;
            }
            previous = Some(scene.clone());
            (scene, layout, width, height) = self.prepare_frame(renderer, animated);
            self.input.reconcile(&scene, &layout);
        }
        (scene, layout, width, height)
    }

    fn prepare_snapshot(&mut self, renderer: &mut Renderer, animated: bool) {
        let _scroll = self.scroll.enter();
        let _focus = self.focus.enter();
        let (mut scene, mut layout, mut width, mut height) = self.settle_frame(renderer, animated);
        if let Some((view, row)) = self.list_item_click.take()
            && let Some(target) = self
                .input
                .resolve_item_delegate(&scene, &layout, &view, row)
        {
            self.component.dispatch_at(&scene, &target.path, "clicked");
            (scene, layout, width, height) = self.settle_frame(renderer, animated);
        }
        if let Some(target) = self.table_cell_click.take() {
            if let Some(action) = self.input.text_link_activation(&scene, &layout, &target) {
                self.apply_control(&scene, action);
            } else if target.text_link_index().is_none()
                && self
                    .input
                    .accessibility_target(&scene, &layout, &target.path)
                    .as_ref()
                    == Some(&target)
            {
                self.component.dispatch_at(&scene, &target.path, "clicked");
            }
            (scene, layout, width, height) = self.settle_frame(renderer, animated);
        }
        if animated {
            let now = self.clock + self.sampled_at;
            if self.input.advance_table_touch_hold(&scene, &layout, now) {
                (scene, layout, width, height) = self.settle_frame(renderer, animated);
            }
            if self.input.advance_table_column_move(&scene, &layout, now)
                || self.input.advance_table_row_move(&scene, &layout, now)
                || self
                    .input
                    .advance_table_drag_selection(&scene, &layout, now)
            {
                (scene, layout, width, height) = self.settle_frame(renderer, animated);
            }
            if self.input.advance_scroll_motion(&scene, &layout, now) {
                (scene, layout, width, height) = self.settle_frame(renderer, animated);
            }
            if let Some(ControlInput { path, action }) =
                self.input.advance_delay(&scene, &layout, now)
            {
                self.component.control_at(&scene, &path, action);
                (scene, layout, width, height) = self.settle_frame(renderer, animated);
            }
            for _ in 0..64 {
                let Some(delivery) = self.timers.next_due(now) else {
                    break;
                };
                if delivery.fire() {
                    // Reconcile visibility and registrations before delivering another due timer.
                    (scene, layout, width, height) = self.settle_frame(renderer, animated);
                }
            }
        }
        if let Some((origin, backward)) = self.table_tab.take() {
            if let Some(target) = self.input.next_table_edit_target(
                &self.component,
                &scene,
                &layout,
                &origin,
                backward,
            ) {
                if self
                    .input
                    .apply_scroll_requests(&scene, &layout, [target.scroll_request()])
                {
                    (scene, layout, width, height) = self.prepare_frame(renderer, animated);
                    self.input.reconcile(&scene, &layout);
                    self.observers.sync(&scene, &layout);
                    if animated {
                        self.sync_window_effects(&scene, &layout);
                    }
                }
                self.input.begin_table_edit_target(&scene, &layout, &target);
            } else {
                self.input.focus_next(&scene, &layout, backward);
            }
        }
        let (mut scene, mut layout, mut width, mut height) =
            self.settle_focus(renderer, animated, (scene, layout, width, height));
        if animated {
            for _ in 0..8 {
                let mut context = PointerContext {
                    input: &mut self.input,
                    component: &mut self.component,
                    scene: &scene,
                    layout: &layout,
                };
                let pointer = self.direct_pointers.sync(&mut context, Instant::now());
                if !self.sync_hover(&scene, &layout) && !pointer {
                    break;
                }
                let frame = self.settle_frame(renderer, animated);
                (scene, layout, width, height) = self.settle_focus(renderer, animated, frame);
            }
        }
        if animated && self.component.take_window_close_request() {
            self.window_closed = self.component.close_requested();
            self.component.take_window_close_request();
            if self.window_closed {
                self.dialogs.close();
            } else {
                let frame = self.settle_frame(renderer, animated);
                (scene, layout, width, height) = self.settle_focus(renderer, animated, frame);
            }
        }
        self.update_stack_transitions(&scene, &layout, animated);
        self.input.sync_editor(&scene);
        if animated && self.window_active {
            self.input.update_tooltip(&scene, &layout, Instant::now());
        }
        self.renderer_needs_redraw = false;
        self.dimensions = (width, height);
        self.frame = Some((scene, layout));
        self.flush_pointer_carets(renderer);
        let (scene, layout) = self.frame.as_ref().unwrap();
        self.ime_area = renderer.text.editor_ime_area(
            &mut self.input,
            scene,
            layout,
            std::mem::take(&mut self.reveal_editor_caret),
        );
    }

    fn flush_pointer_carets(&mut self, renderer: &Renderer) {
        let Some((scene, layout)) = &self.frame else {
            self.pointer_carets.clear();
            return;
        };
        for caret in self.pointer_carets.drain(..) {
            if caret.place(&renderer.text, &mut self.input, scene, layout) {
                self.clipboard_edit = None;
            }
        }
    }

    fn refresh_input_frame(&mut self, renderer: &mut Renderer) {
        let frame = self.settle_frame(renderer, true);
        let (scene, layout, width, height) = self.settle_focus(renderer, true, frame);
        self.update_stack_transitions(&scene, &layout, true);
        self.dimensions = (width, height);
        self.frame = Some((scene, layout));
    }

    fn update_stack_transitions(&mut self, scene: &Node, layout: &LayoutNode, animated: bool) {
        if animated {
            let now = Instant::now();
            self.stack_transitions.advance(now);
            if let Some((previous_scene, previous_layout)) = &self.frame {
                self.stack_transitions.update(
                    (previous_scene, previous_layout),
                    (scene, layout),
                    now,
                );
            }
        } else {
            self.stack_transitions = StackTransitions::default();
        }
        if self.input.sync_stack_transitions(&self.stack_transitions) {
            self.input.reconcile(scene, layout);
        }
    }

    fn settle_focus(
        &mut self,
        renderer: &mut Renderer,
        animated: bool,
        mut frame: (Rc<Node>, LayoutNode, u32, u32),
    ) -> (Rc<Node>, LayoutNode, u32, u32) {
        for _ in 0..8 {
            for request in self.focus.drain() {
                if let Some(command) = self.input.apply_focus_request(&frame.0, &frame.1, request) {
                    self.input.sync_editor(&frame.0);
                    self.text_command(&frame.0, command);
                    if self.component.scene_dirty() || !self.scroll.is_empty() {
                        frame = self.settle_frame(renderer, animated);
                    }
                }
            }
            for event in
                self.focus_observers
                    .sync(&self.input, &frame.0, &frame.1, self.window_active)
            {
                event.apply(&mut self.component, &frame.0);
            }
            if !self.component.scene_dirty() && self.focus.is_empty() && self.scroll.is_empty() {
                break;
            }
            frame = self.settle_frame(renderer, animated);
        }
        frame
    }

    fn text_command(&mut self, scene: &Node, command: TextCommand) {
        match command {
            TextCommand::SelectAll
            | TextCommand::ClearSelection
            | TextCommand::SetSelection { .. } => {
                self.input.apply_text_selection(scene, command);
            }
            TextCommand::Undo | TextCommand::Redo => {
                self.input.replay_text_edit(
                    &mut self.component,
                    scene,
                    command == TextCommand::Redo,
                );
            }
            TextCommand::Cut | TextCommand::Copy | TextCommand::Paste => {
                self.request_text_clipboard(scene, command);
            }
        }
    }

    fn request_text_clipboard(&mut self, scene: &Node, command: TextCommand) -> bool {
        self.clipboard_edit = None;
        self.input.break_text_edit_group();
        let Some(context) = TextEditContext::for_clipboard(&self.input, scene, command) else {
            return false;
        };
        let text = (command != TextCommand::Paste).then(|| context.selection().to_owned());
        let edit = (command != TextCommand::Copy).then_some(ClipboardEdit::Text {
            context,
            cut: command == TextCommand::Cut,
        });
        self.queue_clipboard(text, edit);
        true
    }

    fn queue_clipboard(&mut self, text: Option<String>, edit: Option<ClipboardEdit>) {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        self.clipboard_request = Some(ClipboardRequest { id, text });
        self.clipboard_edit = edit.map(|edit| (id, edit));
    }

    pub fn take_clipboard_request(&mut self) -> Option<ClipboardRequest> {
        self.clipboard_request.take()
    }

    pub fn finish_clipboard(&mut self, id: u64, success: bool, text: &str) -> bool {
        let Some((_, edit)) = self.clipboard_edit.take_if(|(pending, _)| *pending == id) else {
            return false;
        };
        let Some((scene, layout)) = self.frame.as_ref().filter(|_| success) else {
            return false;
        };
        match edit {
            ClipboardEdit::Text { context, cut } if context.is_current(&self.input, scene) => {
                let action = if cut {
                    EditAction::Delete
                } else if !text.is_empty() {
                    EditAction::Insert(text)
                } else {
                    return false;
                };
                self.edit_from(action, EditSource::Atomic);
                true
            }
            ClipboardEdit::Table { destination, apply }
                if self.input.table_paste_target(scene, layout).as_ref() == Some(&destination) =>
            {
                let _scroll = self.scroll.enter();
                let _focus = self.focus.enter();
                apply(text)
            }
            _ => false,
        }
    }

    #[doc(hidden)]
    pub fn set_window_active(&mut self, active: bool) {
        self.window_active = active;
        self.component
            .publish_window_status(xrack_runtime::WindowStatus {
                active,
                ..Default::default()
            });
        if !active {
            self.input.cancel_text_composition(None);
            self.cancel_pointer();
            self.direct_pointers.clear();
            self.scale_rotate.reset();
            self.drag_contact = None;
            self.cancel_delay_key();
            self.leave();
            self.input.dismiss_tooltip();
        }
    }

    pub fn window_closed(&self) -> bool {
        self.window_closed
    }

    pub fn begin_native_dialog(&mut self) -> Option<NativeDialogPresentation> {
        if self.window_closed {
            return None;
        }
        self.dialogs.begin_next()
    }

    pub fn finish_native_dialog(&mut self, id: u64, result: NativeDialogResult) -> bool {
        let _scroll = self.scroll.enter();
        let _focus = self.focus.enter();
        self.dialogs.complete(id, result)
    }

    fn sync_window_effects(&mut self, scene: &Node, layout: &LayoutNode) -> bool {
        let effects = WindowEffects::collect(scene, layout);
        self.timers.sync(Instant::now(), effects.timers);
        self.dialogs.sync(&effects.dialogs);
        self.image_playback
            .sync(self.clock + self.sampled_at, effects.playback)
    }
}

impl Drop for PreviewScene {
    fn drop(&mut self) {
        self.cancel_pointer();
        self.hover_observers.clear();
        self.component.take_window_close_request();
        self.component.take_window_commands();
        self.dialogs.close();
    }
}

fn navigation_key(key: &str) -> Option<NavigationKey> {
    Some(match key {
        "ArrowLeft" => NavigationKey::Left,
        "ArrowRight" => NavigationKey::Right,
        "ArrowUp" => NavigationKey::Up,
        "ArrowDown" => NavigationKey::Down,
        "Home" => NavigationKey::Home,
        "End" => NavigationKey::End,
        "PageUp" => NavigationKey::PageUp,
        "PageDown" => NavigationKey::PageDown,
        _ => return None,
    })
}

fn activation_key(key: &str) -> Option<ActivationKey> {
    match key {
        "Enter" => Some(ActivationKey::Enter),
        " " => Some(ActivationKey::Space),
        _ => None,
    }
}

fn apply_split_resize(component: &mut impl Component, scene: &Node, resize: SplitResize) {
    if let Some(sizes) = &resize.sizes {
        component.resize_split_sizes_at(scene, &resize.path, sizes);
    } else {
        component.resize_split_at(scene, &resize.path, &resize.weights);
    }
}

fn apply_scroll_bar_update(
    component: &mut impl Component,
    scene: &Node,
    update: ScrollBarUpdate,
) -> bool {
    match update {
        ScrollBarUpdate::ViewportMoved => true,
        ScrollBarUpdate::Range(RangeInput { path, action }) => {
            component.adjust_range_at(scene, &path, action)
        }
        ScrollBarUpdate::Unchanged => false,
    }
}

fn tree_navigation(
    input: &mut Interaction,
    scene: &Node,
    layout: &LayoutNode,
    key: NavigationKey,
    extend: bool,
) -> Option<ControlInput> {
    if !input.tree_has_focus(scene, layout) {
        return None;
    }
    let operation = if extend {
        RowSelection::Extend
    } else {
        RowSelection::Replace
    };
    if matches!(key, NavigationKey::Left | NavigationKey::Right) {
        input.tree_key_with_selection(scene, layout, key, operation)
    } else {
        input.list_key_with_selection(scene, layout, key, operation)
    }
}
