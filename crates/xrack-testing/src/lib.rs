//! Test XRack components without opening a window.
//!
//! `UiDriver` routes input through XRack's layout and interaction state machine.
//! Both generated Rust components and interpreter instances implement `Component`.
use std::{
    rc::Rc,
    time::{Duration, Instant},
};
pub use xrack_layout::{ActivationKey, EditAction, Point, Size, TableEditCommit, TextMeasurer};
use xrack_layout::{
    ChoiceKey, EditSource, Interaction, LayoutNode, MenuKey, MenuPanel, MenuShortcut,
    NavigationKey, ScrollBarUpdate, ViewportObservers, WindowEffects, prepare_scene_with_input,
};
pub use xrack_runtime::RowKey;
use xrack_runtime::{
    Component, ControlAction, Node, RowSelection, ScrollRequests, TextCommand, TimerQueue,
};

/// Approximate text metrics independent of installed fonts.
#[derive(Debug, Clone, Copy, Default)]
pub struct FixedText;

impl TextMeasurer for FixedText {
    fn measure(&mut self, text: &str, font_size: f32, max_width: f32) -> Size {
        let width = text.chars().count() as f32 * font_size * 0.55;
        Size::new(width.min(max_width), font_size)
    }
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum TargetError {
    #[error("no materialized UI target matches {0}")]
    Missing(String),
    #[error("multiple UI targets match {0}; scope the query to a view and row")]
    Ambiguous(String),
}

/// Drives a component on the test thread with an explicit clock for DSL timers.
pub struct UiDriver<C, T = FixedText> {
    component: C,
    text: T,
    input: Interaction,
    scene: Rc<Node>,
    layout: LayoutNode,
    viewport: Size,
    scroll_requests: ScrollRequests,
    viewport_observers: ViewportObservers,
    focus_requests: xrack_runtime::FocusRequests,
    now: Instant,
    timers: TimerQueue,
}

impl<C: Component> UiDriver<C> {
    /// Use deterministic text metrics and a viewport in logical pixels.
    pub fn headless(component: C, viewport: Size) -> Self {
        Self::new(component, FixedText, viewport)
    }
}

impl<C: Component, T: TextMeasurer> UiDriver<C, T> {
    pub fn new(component: C, mut text: T, viewport: Size) -> Self {
        let mut input = Interaction::default();
        let (scene, layout) = prepare_scene_with_input(
            &component,
            component.render_shared(),
            viewport,
            &mut text,
            &input,
        );
        input.reconcile(&scene, &layout);
        let mut viewport_observers = ViewportObservers::default();
        viewport_observers.sync(&scene, &layout);
        let now = Instant::now();
        viewport_observers.sync_scroll_activity(&scene, &layout, &input, now);
        let mut driver = Self {
            component,
            text,
            input,
            scene,
            layout,
            viewport,
            scroll_requests: ScrollRequests::default(),
            viewport_observers,
            focus_requests: xrack_runtime::FocusRequests::default(),
            now,
            timers: TimerQueue::default(),
        };
        driver.dispatch_loader_events();
        if driver.component.scene_dirty() {
            driver.prepare();
        }
        driver.sync_timers();
        driver
    }

    fn dispatch_loader_events(&mut self) {
        let _focus = self.focus_requests.enter();
        xrack_runtime::dispatch_loader_events(&mut self.component, &self.scene);
        self.input
            .apply_focus_requests(&self.scene, &self.layout, self.focus_requests.drain());
    }

    fn prepare(&mut self) {
        xrack_runtime::update_profile::layout(false);
        (self.scene, self.layout) = prepare_scene_with_input(
            &self.component,
            self.component.render_shared(),
            self.viewport,
            &mut self.text,
            &self.input,
        );
        self.input.reconcile(&self.scene, &self.layout);
        self.sync_timers();
    }

    fn sync_timers(&mut self) {
        let effects = WindowEffects::collect(&self.scene, &self.layout);
        self.timers.sync(self.now, effects.timers);
    }

    /// Advance DSL timers and deliver due callbacks using the runtime's timer queue.
    /// Repeating timers coalesce missed ticks, just as in a desktop window.
    pub fn advance_time(&mut self, elapsed: Duration) {
        self.now += elapsed;
        for _ in 0..64 {
            let Some(delivery) = self.timers.next_due(self.now) else {
                return;
            };
            self.update(|_| {
                delivery.fire();
            });
        }
        assert!(
            self.timers
                .deadline()
                .is_none_or(|deadline| deadline > self.now),
            "DSL timer callbacks did not settle after 64 deliveries"
        );
    }

    fn refresh(&mut self) {
        let _focus = self.focus_requests.enter();
        let _scroll = self.scroll_requests.enter();
        self.prepare();
        self.dispatch_loader_events();
        let scrolled = self.input.take_scroll_events(&self.scene, &self.layout);
        if self
            .input
            .apply_scroll_requests(&self.scene, &self.layout, self.scroll_requests.drain())
        {
            self.prepare();
        }
        self.viewport_observers.sync(&self.scene, &self.layout);
        self.viewport_observers.sync_scroll_activity(
            &self.scene,
            &self.layout,
            &self.input,
            self.now,
        );
        for event in scrolled {
            event.apply(&mut self.component, &self.scene);
        }
        if self.component.scene_dirty() {
            self.prepare();
        }
        self.input
            .apply_focus_requests(&self.scene, &self.layout, self.focus_requests.drain());
    }

    pub fn finish_loader_frame(&mut self) -> bool {
        if !xrack_runtime::finish_loader_frame(&self.scene) {
            return false;
        }
        self.refresh();
        self.dispatch_loader_events();
        true
    }

    pub fn update(&mut self, update: impl FnOnce(&mut C)) {
        let _focus = self.focus_requests.enter();
        let _scroll = self.scroll_requests.enter();
        update(&mut self.component);
        self.refresh();
    }

    pub fn scroll(&mut self, id: &str, delta: Point) -> bool {
        let point = self.point_at(&self.path(id));
        self.scroll_at(point, delta)
    }

    pub fn scroll_at(&mut self, point: Point, delta: Point) -> bool {
        self.input.pointer_move(&self.scene, &self.layout, point);
        let moved = self.input.scroll_wheel(&self.layout, delta, false);
        if moved {
            self.refresh();
        }
        moved
    }

    pub fn scroll_bar(&self, id: &str) -> Option<xrack_layout::ScrollBarGeometry> {
        let path = self.path(id);
        self.layout
            .at_path(&path)?
            .scroll_control_geometry(self.scene.at_path(&path)?)
    }

    pub fn drag_scroll_bar(&mut self, id: &str, fraction: f32, touch: bool) -> bool {
        self.drag_scroll_bar_with(id, fraction, touch, |_| true)
    }

    /// Inspect the committed move before release. Returning false cancels the capture.
    pub fn drag_scroll_bar_with(
        &mut self,
        id: &str,
        fraction: f32,
        touch: bool,
        finish: impl FnOnce(&mut Self) -> bool,
    ) -> bool {
        let Some(bar) = self.scroll_bar(id) else {
            return false;
        };
        let start = Point {
            x: bar.thumb.x + bar.thumb.width * 0.5,
            y: bar.thumb.y + bar.thumb.height * 0.5,
        };
        // Drag past the end stops so rounding the grab point cannot miss an endpoint.
        let overrun = if fraction <= 0.0 {
            -1.0
        } else if fraction >= 1.0 {
            1.0
        } else {
            0.0
        };
        let mut end = start;
        match bar.orientation {
            xrack_runtime::Orientation::Horizontal => {
                end.x = bar.track.x
                    + bar.thumb.width * 0.5
                    + (bar.track.width - bar.thumb.width) * fraction
                    + overrun;
            }
            xrack_runtime::Orientation::Vertical => {
                end.y = bar.track.y
                    + bar.thumb.height * 0.5
                    + (bar.track.height - bar.thumb.height) * fraction
                    + overrun;
            }
        }
        let _focus = self.focus_requests.enter();
        let _scroll = self.scroll_requests.enter();
        let update = if touch {
            if self
                .input
                .touch_scroll_bar_press(&self.scene, &self.layout, start)
                .is_none()
            {
                return false;
            }
            self.input
                .touch_scroll_bar_drag(&self.scene, &self.layout, end)
        } else {
            self.input.pointer_move(&self.scene, &self.layout, start);
            if self
                .input
                .scroll_bar_press(&self.scene, &self.layout)
                .is_none()
            {
                return false;
            }
            self.input.pointer_move(&self.scene, &self.layout, end);
            self.input
                .scroll_bar_drag(&self.scene, &self.layout)
                .unwrap_or(ScrollBarUpdate::Unchanged)
        };
        let moved = self.apply_scroll_bar(update);
        if !finish(self) {
            if touch {
                self.input.finish_touch_scroll_bar();
            } else {
                self.input.finish_scroll_bar();
            }
            return moved;
        }
        let release = if touch {
            self.input
                .release_touch_scroll_bar(&self.scene, &self.layout, end)
        } else {
            self.input.release_scroll_bar(&self.scene, &self.layout)
        };
        moved | release.is_some_and(|update| self.apply_scroll_bar(update))
    }

    pub fn scroll_bar_key(&mut self, key: NavigationKey) -> Option<bool> {
        let _focus = self.focus_requests.enter();
        let _scroll = self.scroll_requests.enter();
        let update = self.input.scroll_bar_key(&self.scene, &self.layout, key)?;
        Some(self.apply_scroll_bar(update))
    }

    fn apply_scroll_bar(&mut self, update: ScrollBarUpdate) -> bool {
        let moved = match update {
            ScrollBarUpdate::Unchanged => false,
            ScrollBarUpdate::ViewportMoved => true,
            ScrollBarUpdate::Range(input) => {
                self.component
                    .adjust_range_at(&self.scene, &input.path, input.action)
            }
        };
        if moved {
            self.refresh();
        }
        moved
    }

    pub fn drag_viewport_from(&mut self, id: &str, delta: Point) -> bool {
        let _focus = self.focus_requests.enter();
        let _scroll = self.scroll_requests.enter();
        let path = self.path(id);
        self.move_to(&path);
        let start = self.point_at(&path);
        let end = Point {
            x: start.x + delta.x,
            y: start.y + delta.y,
        };
        let now = self.now;
        self.input.pointer_press_in_scene(&self.scene, &self.layout);
        self.input.arm_drag(&self.scene, &self.layout, start);
        self.input
            .arm_mouse_viewport_pan(&self.scene, &self.layout, start, now);
        let moved = self.input.move_scroll_gesture(
            &self.scene,
            &self.layout,
            end,
            now + Duration::from_millis(20),
        );
        let claimed = self.input.mouse_viewport_pan_claimed();
        if moved {
            self.refresh();
        }
        if claimed {
            self.input.end_scroll_gesture(
                &self.scene,
                &self.layout,
                end,
                now + Duration::from_millis(160),
            );
        } else {
            self.input.cancel_scroll_gesture();
            self.input.pointer_move(&self.scene, &self.layout, end);
            if let Some(target) = self.input.pointer_release_target() {
                self.component
                    .dispatch_at(&self.scene, &target.path, "clicked");
            }
            self.refresh();
        }
        claimed
    }

    pub fn scene(&self) -> &Node {
        &self.scene
    }
    pub fn component(&self) -> &C {
        &self.component
    }
    pub fn input(&self) -> &Interaction {
        &self.input
    }

    /// Resolve a unique ID among the materialized scene nodes.
    pub fn find_path(&self, id: &str) -> Result<Vec<usize>, TargetError> {
        find_path(&self.scene, Vec::new(), id.to_owned(), |node| {
            node.element_id() == Some(id)
        })
    }

    /// Resolve a materialized row by its stable model key within the named view.
    /// Pass a child ID to select an element within that row.
    pub fn find_row_path(
        &self,
        view: &str,
        key: RowKey,
        child: Option<&str>,
    ) -> Result<Vec<usize>, TargetError> {
        let view_path = self.find_path(view)?;
        let view_node = self
            .scene
            .at_path(&view_path)
            .ok_or_else(|| TargetError::Missing(view.into()))?;
        let row = find_path(
            view_node,
            view_path,
            format!("{view} row {key:?}"),
            |node| node.row_key == Some(key),
        )?;
        match child {
            Some(id) => {
                let node = self
                    .scene
                    .at_path(&row)
                    .ok_or_else(|| TargetError::Missing(id.into()))?;
                find_path(
                    node,
                    row,
                    format!("{view} row {key:?} child {id}"),
                    |node| node.element_id() == Some(id),
                )
            }
            None => Ok(row),
        }
    }

    /// Resolve a materialized table cell by model row key and logical column index.
    /// The column index is the index used by the table's selection API.
    pub fn find_table_cell_path(
        &self,
        view: &str,
        key: RowKey,
        column: usize,
    ) -> Result<Vec<usize>, TargetError> {
        let mut path = self.find_row_path(view, key, None)?;
        if self
            .scene
            .at_path(&path)
            .is_some_and(|node| node.kind == "ListRow")
        {
            path.push(0);
        }
        if self
            .scene
            .at_path(&path)
            .is_some_and(|node| node.kind == "TableRow" && node.child_at(column).is_some())
        {
            path.push(column);
            return Ok(path);
        }
        Err(TargetError::Missing(format!(
            "{view} row {key:?} column {column}"
        )))
    }

    #[track_caller]
    pub fn path(&self, id: &str) -> Vec<usize> {
        self.find_path(id).unwrap_or_else(|error| panic!("{error}"))
    }

    /// Prepared layout for the current frame, usable with the native renderer.
    pub fn layout(&self) -> &LayoutNode {
        &self.layout
    }

    #[track_caller]
    pub fn bounds(&self, id: &str) -> xrack_layout::Rect {
        self.layout
            .at_path(&self.path(id))
            .unwrap_or_else(|| panic!("UI target {id:?} has no layout"))
            .bounds
    }

    pub fn layout_at(&self, path: &[usize]) -> Option<&LayoutNode> {
        self.layout.at_path(path)
    }

    #[track_caller]
    pub fn node(&self, id: &str) -> &Node {
        self.scene
            .at_path(&self.path(id))
            .unwrap_or_else(|| panic!("UI target {id:?} left the scene"))
    }

    pub fn focused(&self) -> Option<&str> {
        self.input.focused.as_ref()?.element_id()
    }

    pub fn can_hit(&mut self, id: &str) -> bool {
        let path = self.path(id);
        self.move_to(&path);
        self.input
            .hovered
            .as_ref()
            .is_some_and(|target| target.path == path)
    }

    #[track_caller]
    fn move_to(&mut self, path: &[usize]) {
        let point = self.point_at(path);
        self.input.pointer_move(&self.scene, &self.layout, point);
    }

    #[track_caller]
    fn point_at(&self, path: &[usize]) -> Point {
        let item = self
            .layout
            .at_path(path)
            .unwrap_or_else(|| panic!("UI target {path:?} has no layout"));
        let node = self
            .scene
            .at_path(path)
            .unwrap_or_else(|| panic!("UI target {path:?} has no scene node"));
        let bounds = if node.group_checkable() {
            item.group_header(node)
        } else {
            item.bounds
        };
        let hit = item.clip.intersect(bounds);
        assert!(
            hit.width > 0.0 && hit.height > 0.0,
            "target is outside the viewport"
        );
        Point {
            x: hit.x + hit.width * 0.5,
            y: hit.y + hit.height * 0.5,
        }
    }

    #[track_caller]
    pub fn click(&mut self, id: &str) {
        self.click_at_path(&self.path(id));
    }

    #[track_caller]
    pub fn click_at_path(&mut self, path: &[usize]) {
        let _focus = self.focus_requests.enter();
        let _scroll = self.scroll_requests.enter();
        self.move_to(path);
        if self.input.menu_press(&self.scene, &self.layout) {
            self.release_menu();
            return;
        }
        if let Some(close) = self.input.popup_outside_press(&self.scene, &self.layout) {
            assert!(
                self.component
                    .control_at(&self.scene, &close.path, close.action)
            );
            self.refresh();
            self.move_to(path);
        }
        if let Some(update) = self.input.scroll_bar_press(&self.scene, &self.layout) {
            self.apply_scroll_bar(update);
            if let Some(update) = self.input.release_scroll_bar(&self.scene, &self.layout) {
                self.apply_scroll_bar(update);
            }
            return;
        }
        self.input.pointer_press_in_scene(&self.scene, &self.layout);
        let target = self
            .input
            .pointer_release_target()
            .unwrap_or_else(|| panic!("click did not reach {path:?}"));
        assert_eq!(target.path, path, "click hit a different control");
        self.activate_target(target);
    }

    fn activate_target(&mut self, mut target: xrack_layout::Target) -> bool {
        if let Some(selection) =
            self.input
                .list_item_delegate_selection(&self.scene, &self.layout, &target)
        {
            let Some(row) = target.row_key else {
                return false;
            };
            assert!(
                self.component
                    .control_at(&self.scene, &selection.path, selection.action)
            );
            self.refresh();
            let Some(resolved) =
                self.input
                    .resolve_item_delegate(&self.scene, &self.layout, &selection.path, row)
            else {
                return false;
            };
            target = resolved;
        }
        if let Some(selection) = self.input.table_delegate_selection(
            &self.scene,
            &self.layout,
            &target,
            RowSelection::Replace,
        ) {
            self.component
                .control_at(&self.scene, &selection.path, selection.action);
            self.refresh();
            if self.scene.row_key_at(&target.path) != target.row_key {
                return true;
            }
        }
        let action = self
            .input
            .dialog_button(&self.scene, &self.layout, &target)
            .or_else(|| {
                self.input.select_group_option_with_selection(
                    &self.scene,
                    &self.layout,
                    &target,
                    RowSelection::Replace,
                )
            });
        let changed = if let Some(action) = action {
            self.component
                .control_at(&self.scene, &action.path, action.action)
        } else {
            self.input
                .open_menu_bar(&self.scene, &self.layout, target.clone())
                || self
                    .input
                    .open_choice_at_pointer(&self.scene, &self.layout, target.clone())
                || self
                    .component
                    .dispatch_at(&self.scene, &target.path, "clicked")
        };
        self.refresh();
        changed
    }

    /// Activate the focused control as with Enter or Space.
    pub fn activate(&mut self, key: ActivationKey) -> bool {
        let _focus = self.focus_requests.enter();
        let _scroll = self.scroll_requests.enter();
        self.input.key_press_with_scene(&self.scene, key);
        let Some(target) = self.input.key_release_with_scene(&self.scene, key) else {
            return false;
        };
        let target = self
            .input
            .current_item_delegate(&self.scene, &self.layout, &target)
            .unwrap_or(target);
        self.activate_target(target)
    }

    pub fn tab(&mut self) {
        self.tab_with_direction(false);
    }

    pub fn begin_table_edit(&mut self) -> bool {
        self.input.begin_table_edit(&self.scene, &self.layout)
    }

    /// Commit an active cell editor using normal validation and model writes.
    pub fn finish_table_edit(&mut self) -> TableEditCommit {
        let _focus = self.focus_requests.enter();
        let _scroll = self.scroll_requests.enter();
        let result = self
            .input
            .finish_table_edit(&mut self.component, &self.scene);
        self.refresh();
        result
    }

    pub fn tab_with_direction(&mut self, backward: bool) {
        let _focus = self.focus_requests.enter();
        let _scroll = self.scroll_requests.enter();
        if let Some(origin) = self.input.tab_edit_origin(&self.scene) {
            use xrack_layout::TableEditCommit;
            let result = self
                .input
                .finish_table_edit(&mut self.component, &self.scene);
            if result == TableEditCommit::Kept {
                return;
            }
            self.refresh();
            if matches!(
                result,
                TableEditCommit::Changed | TableEditCommit::Unchanged
            ) && let Some(target) = self.input.next_table_edit_target(
                &self.component,
                &self.scene,
                &self.layout,
                &origin,
                backward,
            ) {
                if self.input.apply_scroll_requests(
                    &self.scene,
                    &self.layout,
                    [target.scroll_request()],
                ) {
                    self.prepare();
                }
                if self
                    .input
                    .begin_table_edit_target(&self.scene, &self.layout, &target)
                {
                    self.refresh();
                    return;
                }
            }
        }
        if self.input.focus_next(&self.scene, &self.layout, backward) {
            self.refresh();
        }
    }

    pub fn group_key(&mut self, key: NavigationKey) -> bool {
        let _focus = self.focus_requests.enter();
        let _scroll = self.scroll_requests.enter();
        let Some(action) = self.input.group_key(&self.scene, &self.layout, key) else {
            return false;
        };
        let changed = self
            .component
            .control_at(&self.scene, &action.path, action.action);
        self.refresh();
        changed
    }

    pub fn back(&mut self) -> bool {
        let _focus = self.focus_requests.enter();
        let _scroll = self.scroll_requests.enter();
        let Some(action) = self.input.stack_back(&self.scene, &self.layout) else {
            return false;
        };
        let moved = self
            .component
            .control_at(&self.scene, &action.path, action.action);
        self.refresh();
        moved
    }

    pub fn menus(&self) -> Vec<MenuPanel<'_>> {
        self.input.menu_panels(&self.scene, &self.layout)
    }

    pub fn context_menu(&mut self, id: &str) -> bool {
        let path = self.path(id);
        self.move_to(&path);
        self.input
            .open_context_menu(&self.scene, &self.layout, false)
    }

    pub fn keyboard_context_menu(&mut self) -> bool {
        self.input
            .open_context_menu(&self.scene, &self.layout, true)
    }

    pub fn menu_key(&mut self, key: MenuKey) {
        let _focus = self.focus_requests.enter();
        let _scroll = self.scroll_requests.enter();
        if let Some(action) = self.input.menu_key(&self.scene, &self.layout, key) {
            self.apply_menu_action(action);
        }
        self.refresh();
    }

    pub fn menu_shortcut(&mut self, key: MenuShortcut) -> bool {
        let _focus = self.focus_requests.enter();
        let _scroll = self.scroll_requests.enter();
        let Some(action) = self.input.menu_shortcut(&self.scene, &self.layout, key) else {
            return false;
        };
        self.input.close_menu(&self.scene, &self.layout);
        self.apply_menu_action(action);
        self.refresh();
        true
    }

    #[track_caller]
    pub fn click_menu_row(&mut self, label: &str) -> bool {
        let _focus = self.focus_requests.enter();
        let _scroll = self.scroll_requests.enter();
        let rect = {
            let panels = self.menus();
            let panel = panels
                .last()
                .unwrap_or_else(|| panic!("menu is closed while locating {label:?}"));
            let slot = panel
                .rows
                .iter()
                .position(|row| row.label == label)
                .unwrap_or_else(|| panic!("no visible menu row has label {label:?}"));
            panel.row_bounds(slot)
        };
        self.input.pointer_move(
            &self.scene,
            &self.layout,
            Point {
                x: rect.x + rect.width * 0.5,
                y: rect.y + rect.height * 0.5,
            },
        );
        assert!(self.input.menu_press(&self.scene, &self.layout));
        self.release_menu()
    }

    fn release_menu(&mut self) -> bool {
        let activated = self
            .input
            .menu_release(&self.scene, &self.layout)
            .is_some_and(|action| {
                self.apply_menu_action(action);
                true
            });
        self.refresh();
        activated
    }

    fn apply_menu_action(&mut self, input: xrack_layout::ControlInput) {
        match input.action {
            ControlAction::TextCommand(command @ TextCommand::SelectAll) => {
                self.input.apply_text_selection(&self.scene, command);
            }
            ControlAction::TextCommand(command @ (TextCommand::Undo | TextCommand::Redo)) => {
                assert!(self.input.replay_text_edit(
                    &mut self.component,
                    &self.scene,
                    command == TextCommand::Redo
                ));
            }
            action => assert!(self.component.control_at(&self.scene, &input.path, action)),
        }
    }

    pub fn choice_key(&mut self, key: ChoiceKey) {
        let _focus = self.focus_requests.enter();
        let _scroll = self.scroll_requests.enter();
        assert!(self.input.choice_open(), "choice popup is closed");
        if let Some(action) = self.input.choice_key(&self.scene, &self.layout, key) {
            assert!(
                self.component
                    .control_at(&self.scene, &action.path, action.action)
            );
        }
        self.refresh();
    }

    #[track_caller]
    pub fn escape(&mut self) {
        let _focus = self.focus_requests.enter();
        let _scroll = self.scroll_requests.enter();
        let action = self
            .input
            .dialog_escape(&self.scene, &self.layout)
            .unwrap_or_else(|| panic!("there is no dismissible dialog"));
        assert!(
            self.component
                .control_at(&self.scene, &action.path, action.action)
        );
        self.refresh();
    }

    /// Replace all text in the focused editor through the normal edit pipeline.
    #[track_caller]
    pub fn fill_focused(&mut self, text: &str) {
        assert!(
            self.edit_focused(EditAction::ReplaceAll(text)),
            "focused control {:?} did not accept text input",
            self.focused()
        );
    }

    /// Send an edit to the focused control or active table cell editor.
    pub fn edit_focused(&mut self, action: EditAction<'_>) -> bool {
        let _focus = self.focus_requests.enter();
        let _scroll = self.scroll_requests.enter();
        let changed = self.input.edit_text(
            &mut self.component,
            &self.scene,
            action,
            EditSource::Atomic,
            self.now,
        );
        self.refresh();
        changed
    }
}

fn find_path(
    node: &Node,
    mut path: Vec<usize>,
    description: String,
    matches: impl Fn(&Node) -> bool,
) -> Result<Vec<usize>, TargetError> {
    fn visit(
        node: &Node,
        path: &mut Vec<usize>,
        matches: &impl Fn(&Node) -> bool,
        found: &mut Option<Vec<usize>>,
    ) -> bool {
        if matches(node) && found.replace(path.clone()).is_some() {
            return false;
        }
        for (index, child) in node.materialized_children() {
            path.push(index);
            if !visit(child, path, matches, found) {
                return false;
            }
            path.pop();
        }
        true
    }

    let mut found = None;
    if !visit(node, &mut path, &matches, &mut found) {
        return Err(TargetError::Ambiguous(description));
    }
    found.ok_or(TargetError::Missing(description))
}
